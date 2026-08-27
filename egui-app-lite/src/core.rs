// 核心逻辑: 表情包扫描 / 剪贴板注入 / 窗口拾取 (与 Tauri 版同款, 无 GUI 耦合)
#![allow(dead_code)]
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const GAME_APP_DIR: &str = "EmoticonPanel-egui";

pub fn image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "gif" | "jpg" | "jpeg" | "webp" | "bmp"
    )
}

pub fn is_gif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gif"))
        .unwrap_or(false)
}

fn is_cover(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cover"))
        .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub name: String,
    pub cover: Option<PathBuf>,
    pub count: usize,
    pub gif_count: usize,
}

#[derive(Clone, Debug)]
pub struct Sticker {
    pub path: PathBuf,
    pub name: String,
    pub is_gif: bool,
}

fn settings_file() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(GAME_APP_DIR).join("settings.json")
}

/// 表情包根目录: 环境变量 > settings.json > APPDATA/EmoticonPanel-egui/stickers
pub fn root_dir() -> PathBuf {
    if let Ok(p) = std::env::var("EMOTICON_STICKERS_DIR") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    if let Ok(p) = fs::read_to_string(settings_file()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&p) {
            if let Some(dir) = v.get("stickersDir").and_then(|d| d.as_str()) {
                let d = PathBuf::from(dir);
                if d.is_dir() {
                    return d;
                }
            }
        }
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(GAME_APP_DIR).join("stickers")
}

/// 设置表情包根目录并持久化
fn read_settings() -> serde_json::Value {
    fs::read_to_string(settings_file())
        .ok()
        .and_then(|p| serde_json::from_str(&p).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings(v: &serde_json::Value) -> Result<(), String> {
    let file = settings_file();
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&file, serde_json::to_string_pretty(v).unwrap_or_default())
        .map_err(|e| format!("保存配置失败: {e}"))
}

pub fn set_stickers_dir(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("路径为空".into());
    }
    let p = PathBuf::from(path);
    if !p.is_dir() {
        fs::create_dir_all(&p).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let mut v = read_settings();
    v["stickersDir"] = serde_json::json!(p.to_string_lossy());
    write_settings(&v)?;
    Ok(p)
}

/// 跟随窗口 (面板随目标进程 显示/隐藏/移动)
pub fn get_follow_window() -> bool {
    read_settings()
        .get("followWindow")
        .and_then(|a| a.as_bool())
        .unwrap_or(false)
}

pub fn set_follow_window(v: bool) -> Result<(), String> {
    let mut s = read_settings();
    s["followWindow"] = serde_json::json!(v);
    write_settings(&s)
}

pub fn list_packages(root: &Path) -> Vec<Package> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| {
        a.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
    });
    for d in dirs {
        let name = d
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let mut all: Vec<PathBuf> = fs::read_dir(&d)
            .map(|rd2| {
                rd2.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).map(image_ext).unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default();
        all.sort_by(|a, b| {
            a.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
        });
        let cover = all.iter().find(|p| is_cover(p)).or_else(|| all.first()).cloned();
        let files: Vec<PathBuf> = all.into_iter().filter(|p| !is_cover(p)).collect();
        if files.is_empty() {
            continue;
        }
        out.push(Package {
            name,
            cover,
            count: files.len(),
            gif_count: files.iter().filter(|f| is_gif(f)).count(),
        });
    }
    out
}

pub fn list_stickers(root: &Path, package: &str) -> Result<Vec<Sticker>, String> {
    if package.is_empty() || package.contains('/') || package.contains('\\') || package.contains("..") {
        return Err("非法的表情包名称".into());
    }
    let dir = root.join(package);
    if !dir.is_dir() {
        return Err("表情包不存在".into());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && !is_cover(p) && p.extension().and_then(|e| e.to_str()).map(image_ext).unwrap_or(false))
                .collect()
        })
        .map_err(|e| e.to_string())?;
    files.sort_by(|a, b| {
        a.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase())
    });
    Ok(files
        .into_iter()
        .map(|f| Sticker {
            name: f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            is_gif: is_gif(&f),
            path: f,
        })
        .collect())
}

pub fn delete_package(root: &Path, name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("非法的表情包名称".into());
    }
    let dir = root.join(name);
    if !dir.is_dir() {
        return Err("表情包不存在".into());
    }
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}

// ---------- attach 目标窗口 ----------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub hwnd: isize,
    pub title: String,
    pub process: String,
    pub pid: u32,
}

pub struct Attach {
    pub target: Arc<Mutex<Option<TargetInfo>>>,
    pub picking: Arc<AtomicBool>,
}

impl Default for Attach {
    fn default() -> Self {
        Self {
            target: Arc::new(Mutex::new(None)),
            picking: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(target_os = "windows")]
pub mod win {
    use super::TargetInfo;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    use windows::core::PWSTR;

    pub fn foreground_hwnd() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn self_pid() -> u32 {
        unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
    }

    pub fn window_pid(hwnd: isize) -> u32 {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid as *mut u32));
        }
        pid
    }

    pub fn capture_target(hwnd: isize) -> Option<TargetInfo> {
        let h = HWND(hwnd as *mut _);
        let mut buf = vec![0u16; 512];
        let len = unsafe { GetWindowTextW(h, &mut buf) };
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid as *mut u32)); }
        let process = process_name(pid);
        if title.trim().is_empty() && process.is_empty() {
            return None;
        }
        Some(TargetInfo { hwnd, title, process, pid })
    }

    fn process_name(pid: u32) -> String {
        if pid == 0 {
            return String::new();
        }
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut buf = vec![0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut size as *mut u32,
            );
            let _ = CloseHandle(handle);
            if ok.is_ok() && size > 0 {
                let name = OsString::from_wide(&buf[..size as usize]).to_string_lossy().to_string();
                return Path::file_name(name.as_ref())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(name);
            }
            String::new()
        }
    }

    // ---------- 剪贴板 ----------

    unsafe fn set_data(format: u32, bytes: &[u8]) -> Result<(), String> {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(h);
        SetClipboardData(format, HANDLE(h.0)).map_err(|e| format!("SetClipboardData: {e}"))?;
        Ok(())
    }

    /// CF_HDROP(真实文件路径, UTF-16) —— 微信/QQ 插入原始文件(保透明/动图)
    pub unsafe fn set_hdrop(path: &str) -> Result<(), String> {
        #[repr(C)]
        struct DROPFILES {
            p_files: u32,
            pt: [i32; 2],
            f_nc: u32,
            f_wide: u32,
        }
        let wide: Vec<u16> = path.encode_utf16().collect();
        let struct_bytes = std::mem::size_of::<DROPFILES>();
        let payload_len = struct_bytes + (wide.len() + 2) * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE, payload_len).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        unsafe {
            let base = ptr as *mut u8;
            let header = DROPFILES { p_files: struct_bytes as u32, pt: [0, 0], f_nc: 0, f_wide: 1 };
            std::ptr::copy_nonoverlapping((&header as *const DROPFILES) as *const u8, base, struct_bytes);
            let dst = base.add(struct_bytes) as *mut u16;
            for (i, c) in wide.iter().enumerate() {
                *dst.add(i) = *c;
            }
            *dst.add(wide.len()) = 0;
            *dst.add(wide.len() + 1) = 0;
            let _ = GlobalUnlock(h);
        }
        SetClipboardData(15 /*CF_HDROP*/, HANDLE(h.0)).map_err(|e| format!("SetClipboardData CF_HDROP: {e}"))?;
        Ok(())
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_xpels_per_meter: i32,
        bi_ypels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    /// CF_DIB: BGRA 字节序 + 透明合成白底 (兜底位图)
    pub unsafe fn set_dib(rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
        let header = BITMAPINFOHEADER {
            bi_size: 40,
            bi_width: w as i32,
            bi_height: -(h as i32),
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: w * h * 4,
            bi_xpels_per_meter: 0,
            bi_ypels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        let mut flipped = Vec::with_capacity(rgba.len());
        for c in rgba.chunks_exact(4) {
            let (r, g, b) = (c[0] as u32, c[1] as u32, c[2] as u32);
            let a = c[3] as u32;
            let blend = |v: u32| ((v * a + 255 * (255 - a)) / 255) as u8;
            flipped.extend_from_slice(&[blend(b), blend(g), blend(r), 255]);
        }
        let hb = unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, 40 + flipped.len()).map_err(|e| format!("GlobalAlloc: {e}"))?;
            let ptr = GlobalLock(h);
            if ptr.is_null() {
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping((&header as *const BITMAPINFOHEADER) as *const u8, ptr as *mut u8, 40);
            std::ptr::copy_nonoverlapping(flipped.as_ptr(), (ptr as *mut u8).add(40), flipped.len());
            let _ = GlobalUnlock(h);
            h
        };
        SetClipboardData(8 /*CF_DIB*/, HANDLE(hb.0)).map_err(|e| format!("SetClipboardData CF_DIB: {e}"))?;
        Ok(())
    }

    /// “PNG”/“image/png” 原始字节
    pub unsafe fn set_png_formats(bytes: &[u8]) -> Result<(), String> {
        set_data(RegisterClipboardFormatW(windows::core::w!("PNG")), bytes)?;
        set_data(RegisterClipboardFormatW(windows::core::w!("image/png")), bytes)?;
        Ok(())
    }

    /// FileGroupDescriptorW + FileContents 虚拟文件
    pub unsafe fn set_virtual_file(bytes: &[u8], filename: &str) -> Result<(), String> {
        let fdw = RegisterClipboardFormatW(windows::core::w!("FileGroupDescriptorW"));
        let fc = RegisterClipboardFormatW(windows::core::w!("FileContents"));

        #[repr(C)]
        struct FILEDESCRIPTORW {
            dw_flags: u32,
            clsid: [u8; 16],
            sizel: (i32, i32),
            pointl: (i32, i32),
            dw_file_attributes: u32,
            ft_creation_time: [u64; 1],
            ft_last_access_time: [u64; 1],
            ft_last_write_time: [u64; 1],
            n_file_size_high: u32,
            n_file_size_low: u32,
            c_file_name: [u16; 260],
        }
        #[repr(C)]
        struct FILEGROUPDESCRIPTORW {
            c_items: u32,
            fgd: [FILEDESCRIPTORW; 1],
        }
        let mut name = [0u16; 260];
        for (i, c) in filename.encode_utf16().take(259).enumerate() {
            name[i] = c;
        }
        let desc = FILEGROUPDESCRIPTORW {
            c_items: 1,
            fgd: [FILEDESCRIPTORW {
                dw_flags: 0x4 | 0x40,
                clsid: [0; 16],
                sizel: (0, 0),
                pointl: (0, 0),
                dw_file_attributes: 0x80,
                ft_creation_time: [0; 1],
                ft_last_access_time: [0; 1],
                ft_last_write_time: [0; 1],
                n_file_size_high: (bytes.len() >> 32) as u32,
                n_file_size_low: bytes.len() as u32,
                c_file_name: name,
            }],
        };
        let desc_bytes = unsafe {
            std::slice::from_raw_parts(
                (&desc as *const FILEGROUPDESCRIPTORW) as *const u8,
                std::mem::size_of::<FILEGROUPDESCRIPTORW>(),
            )
        };
        set_data(fdw, desc_bytes)?;
        set_data(fc, bytes)?;
        Ok(())
    }

    pub unsafe fn set_clipboard(f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        for attempt in 0..8 {
            if OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                let r = f();
                let _ = CloseClipboard();
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1)));
        }
        Err("无法打开剪贴板 (可能被占用)".into())
    }

    /// 激活目标窗口并发送 Ctrl+V
    pub fn activate_and_paste(hwnd: isize) -> Result<(), String> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_CONTROL,
            VK_MENU, VK_V,
        };
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE).as_bool();
            }
            let _ = SetForegroundWindow(hwnd).as_bool();
        }
        std::thread::sleep(std::time::Duration::from_millis(140));
        unsafe {
            let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            let _ = keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            let _ = keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        Ok(())
    }
    /// 目标窗口跟随信息: (面板x, y, 目标可见?) — 贴右(+8), 超界放左, 夹工作区
    /// 面板自身窗口句柄 (按进程+尺寸定位)
    pub unsafe fn self_panel_hwnd() -> isize {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        use windows::Win32::Foundation::{BOOL, LPARAM};
        static FOUND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
        unsafe extern "system" fn cb(h: HWND, _l: LPARAM) -> BOOL {
            let mut pid = 0u32;
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == std::process::id() {
                let mut r = windows::Win32::Foundation::RECT::default();
                if GetWindowRect(h, &mut r).is_ok() {
                    let w = r.right - r.left;
                    let hh = r.bottom - r.top;
                    if (280..=420).contains(&w) && (380..=520).contains(&hh) {
                        FOUND.store(h.0 as isize, std::sync::atomic::Ordering::SeqCst);
                        return BOOL(0); // 停止
                    }
                }
            }
            BOOL(1)
        }
        FOUND.store(0, std::sync::atomic::Ordering::SeqCst);
        let _ = EnumWindows(Some(cb), LPARAM(0));
        FOUND.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 目标窗口跟随位置 (贴右+8, 超界放左, 夹工作区)
    pub unsafe fn follow_pos(hwnd: isize, panel_w: i32, panel_h: i32) -> Option<(i32, i32)> {
        use windows::Win32::Foundation::RECT;
        let hwnd = HWND(hwnd as *mut _);
        if hwnd.0.is_null() {
            return None;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return None;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(mon, &mut mi).as_bool() {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        };
        let w = r.right - r.left;
        // 右下角优先: 面板底对齐目标底, 贴右 +8
        let mut x = r.left + w + 8;
        let mut y = r.bottom - panel_h;
        if x + panel_w > work.right {
            x = r.left - 8 - panel_w; // 放不下 -> 左下角
        }
        x = x.max(work.left).min((work.right - panel_w).max(work.left));
        y = y.max(work.top).min((work.bottom - panel_h).max(work.top));
        Some((x, y))
    }

    /// 目标窗口是否可见 (可见且未被最小化)
    pub unsafe fn target_visible(hwnd: isize) -> bool {
        let hwnd = HWND(hwnd as *mut _);
        IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()
    }

    /// 放置面板: 位置 + 同层绘制 (插到目标窗口正下方, 随目标 Z 层级)
    pub unsafe fn place_panel(panel: isize, target: isize, x: i32, y: i32) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOSIZE, SWP_NOACTIVATE, SWP_ASYNCWINDOWPOS,
        };
        let _ = SetWindowPos(
            HWND(panel as *mut _),
            HWND(target as *mut _), // 目标下方 = 与目标同一绘制层级
            x, y, 0, 0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }

    /// 目标窗口跟随信息: (x, y, 可见?) — 兼容旧接口
    pub unsafe fn follow_target(hwnd: isize, panel_w: i32, panel_h: i32) -> Option<(i32, i32, bool)> {
        let (x, y) = follow_pos(hwnd, panel_w, panel_h)?;
        Some((x, y, target_visible(hwnd)))
    }
}

/// 开始拾取目标窗口 (后台线程, 轮流检测前台窗口变化, 排除自身进程)
pub fn begin_pick(attach: &Attach) {
    if attach.picking.swap(true, Ordering::SeqCst) {
        return;
    }
    let picking = attach.picking.clone();
    let target = attach.target.clone();
    std::thread::spawn(move || {
        let timer = std::time::Instant::now();
        let mut last = 0isize;
        let self_pid = win::self_pid();
        let timeout = std::time::Duration::from_secs(15);
        while picking.load(Ordering::SeqCst) && timer.elapsed() < timeout {
            let fg = win::foreground_hwnd();
            let fg_pid = if fg == 0 { 0 } else { win::window_pid(fg) };
            if fg != 0 && fg_pid != 0 && fg_pid != self_pid && fg != last {
                if let Some(info) = win::capture_target(fg) {
                    *target.lock().unwrap() = Some(info);
                    break;
                }
            }
            last = fg;
            std::thread::sleep(std::time::Duration::from_millis(180));
        }
        picking.store(false, Ordering::SeqCst);
    });
}

pub fn cancel_pick(attach: &Attach) {
    attach.picking.store(false, Ordering::SeqCst);
}

/// 插入表情: 写剪贴板 (HDROP + 虚拟文件 + PNG + DIB) 并 Ctrl+V 到目标
pub fn insert_sticker(attach: &Attach, root: &Path, path: &Path) -> Result<(), String> {
    let target = attach
        .target
        .lock()
        .unwrap()
        .clone()
        .ok_or("请先选择目标窗口")?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sticker")
        .to_string();
    let is_gif_file = is_gif(path);
    let ext_lower = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let is_png = ext_lower == "png";
    let _ = root; // 路径已由 list_stickers 提供
    let _ = &bytes;

    #[cfg(target_os = "windows")]
    unsafe {
        // 只写"文件"类格式: CF_HDROP + 虚拟文件 + PNG 原始字节
        // 绝不写 CF_DIB(白底合成位图) —— 微信等会优先读位图导致透明 PNG 变白底
        win::set_clipboard(|| {
            win::set_hdrop(&path.to_string_lossy())?;
            win::set_virtual_file(&bytes, &filename)?;
            if is_png || is_gif_file {
                win::set_png_formats(&bytes)?;
            }
            Ok(())
        })?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("当前平台不支持插入".into());
    }

    if is_gif_file {
        let _ = (is_gif_file, is_png);
    }
    win::activate_and_paste(target.hwnd)?;
    Ok(())
}

/// 后台全量扫描: 返回所有分组 + 每组贴图列表 (一次性完成, 不在 UI 线程执行)
pub struct ScanResult {
    pub packages: Vec<Package>,
    pub entries: std::collections::HashMap<String, Vec<Sticker>>,
}

pub fn scan_all(root: &Path) -> ScanResult {
    let packages = list_packages(root);
    let mut entries = std::collections::HashMap::new();
    for p in &packages {
        if let Ok(ss) = list_stickers(root, &p.name) {
            entries.insert(p.name.clone(), ss);
        }
    }
    ScanResult { packages, entries }
}
