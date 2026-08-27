// 表情面板后端: 表情包扫描 / 读取 / 商店安装 / 分组删除 + attach 目标窗口后点击表情插入输入框
use base64::Engine;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

const ROOT_DIR: &str = "stickers";
const SHOP_DIR: &str = "shop";

fn image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "gif" | "jpg" | "jpeg" | "webp" | "bmp"
    )
}

fn is_gif(path: &Path) -> bool {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StickerInfo {
    url: String,
    name: String,
    is_gif: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageInfo {
    name: String,
    cover: Option<String>,
    count: usize,
    gif_count: usize,
    shop: bool,
}

/// 表情包根目录: 环境变量 EMOTICON_STICKERS_DIR > 持久化配置 > appdata
fn root_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(p) = std::env::var("EMOTICON_STICKERS_DIR") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    if let Ok(p) = std::fs::read_to_string(settings_file(app)) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&p) {
            if let Some(dir) = v.get("stickersDir").and_then(|d| d.as_str()) {
                let d = PathBuf::from(dir);
                if d.is_dir() {
                    return d;
                }
            }
        }
    }
    app.path()
        .app_data_dir()
        .expect("no app data dir")
        .join(ROOT_DIR)
}

fn settings_file(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .expect("no app config dir")
        .join("settings.json")
}

/// 选择/设置表情包根目录 (持久化到 settings.json)
#[tauri::command]
fn set_stickers_dir(app: tauri::AppHandle, path: String) -> Result<String, String> {
    if path.trim().is_empty() {
        return Err("路径为空".into());
    }
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        fs::create_dir_all(&p).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let file = settings_file(&app);
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut v = read_settings(&app);
    v["stickersDir"] = serde_json::json!(p.to_string_lossy());
    write_settings(&app, &v)?;
    Ok(p.to_string_lossy().to_string())
}

fn read_settings(app: &tauri::AppHandle) -> serde_json::Value {
    std::fs::read_to_string(settings_file(app))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings(app: &tauri::AppHandle, v: &serde_json::Value) -> Result<(), String> {
    let file = settings_file(app);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&file, serde_json::to_string_pretty(v).unwrap_or_default())
        .map_err(|e| format!("保存配置失败: {e}"))
}

/// 始终置顶 状态
#[tauri::command]
fn get_always_on_top(app: tauri::AppHandle) -> bool {
    read_settings(&app)
        .get("alwaysOnTop")
        .and_then(|a| a.as_bool())
        .unwrap_or(false)
}

#[tauri::command]
fn set_always_on_top(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let mut v = read_settings(&app);
    v["alwaysOnTop"] = serde_json::json!(on);
    write_settings(&app, &v)
}

fn valid_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(':')
        || name.contains('*')
        || name.contains('?')
        || name.contains('"')
        || name.contains('<')
        || name.contains('>')
        || name.contains('|')
    {
        return Err("非法的表情包名称".into());
    }
    Ok(name.to_string())
}

fn scan_dir_packages(root: &Path, shop: bool) -> Vec<PackageInfo> {
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
        // 读取全部图片 (含 cover), cover 只做封面不计入网格
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
        out.push(PackageInfo {
            name,
            cover: cover.map(|c| c.to_string_lossy().to_string()),
            count: files.len(),
            gif_count: files.iter().filter(|f| is_gif(f)).count(),
            shop,
        });
    }
    out
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in rd {
        let e = e.map_err(|e| e.to_string())?;
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------- 表情包命令 ----------

#[tauri::command]
fn get_root(app: tauri::AppHandle) -> String {
    root_dir(&app).to_string_lossy().to_string()
}

#[tauri::command]
fn list_packages(app: tauri::AppHandle) -> Vec<PackageInfo> {
    scan_dir_packages(&root_dir(&app), false)
}

#[tauri::command]
fn list_stickers(app: tauri::AppHandle, package: String) -> Result<Vec<StickerInfo>, String> {
    let name = valid_name(&package)?;
    let dir = root_dir(&app).join(name);
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
        .map(|f| StickerInfo {
            url: f.to_string_lossy().to_string(),
            name: f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            is_gif: is_gif(&f),
        })
        .collect())
}

fn read_file_safe(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let p = PathBuf::from(path);
    let ok = p.is_absolute()
        && fs::canonicalize(root)
            .map(|cr| fs::canonicalize(&p).map(|cp| cp.starts_with(&cr)).unwrap_or(false))
            .unwrap_or(false);
    if !ok {
        return Err("非法的文件路径".into());
    }
    let bytes = fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("空文件".into());
    }
    Ok(bytes)
}

#[tauri::command]
fn read_sticker(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let root = root_dir(&app);
    let bytes = read_file_safe(&root, &path)?;
    let p = PathBuf::from(&path);
    let mime = match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("gif") => "image/gif",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    };
    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn shop_list(app: tauri::AppHandle) -> Vec<PackageInfo> {
    scan_dir_packages(&root_dir(&app).join(SHOP_DIR), true)
}

#[tauri::command]
fn install_package(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let n = valid_name(&name)?;
    let root = root_dir(&app);
    let from = root.join(SHOP_DIR).join(&n);
    let to = root.join(&n);
    if !from.is_dir() {
        return Err("商店里没有这个表情包".into());
    }
    if to.exists() {
        return Err(format!("「{n}」已下载过了"));
    }
    copy_dir_recursive(&from, &to)?;
    Ok(())
}

#[tauri::command]
fn delete_package(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let n = valid_name(&name)?;
    let root = root_dir(&app);
    let dir = root.join(&n);
    if !dir.is_dir() {
        return Err("表情包不存在".into());
    }
    fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reveal_root(app: tauri::AppHandle) -> Result<(), String> {
    let root = root_dir(&app);
    let _ = fs::create_dir_all(&root);
    tauri_plugin_opener::reveal_item_in_dir(&root).map_err(|e| e.to_string())
}

// ---------- attach 目标窗口 + 插入 ----------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetInfo {
    hwnd: isize,
    title: String,
    process: String,
    pid: u32,
}

struct AppState {
    target: Arc<Mutex<Option<TargetInfo>>>,
    picking: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            target: Arc::new(Mutex::new(None)),
            picking: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::TargetInfo;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use tauri::Manager;
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    use windows::core::PWSTR;

    pub fn self_hwnd(app: &tauri::AppHandle) -> isize {
        app.get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize)
            .unwrap_or(0)
    }

    pub fn foreground_hwnd() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn capture_target(hwnd: isize) -> Option<TargetInfo> {
        let h = HWND(hwnd as *mut _);
        // 标题
        let mut buf = vec![0u16; 512];
        let len = unsafe { GetWindowTextW(h, &mut buf) };
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        // pid + 进程名
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid as *mut u32)); }
        let process = process_name(pid);
        if title.trim().is_empty() && process.is_empty() {
            return None;
        }
        Some(TargetInfo {
            hwnd,
            title,
            process,
            pid,
        })
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

    /// 激活目标窗口并发送 Ctrl+V
    pub fn activate_and_paste(hwnd: isize) -> Result<(), String> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_CONTROL,
            VK_MENU, VK_V,
        };
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            // Alt 空点一次, 解除其他进程的前台锁定
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

    /// “PNG”/“image/png” 注册格式 ← 原始文件字节。
    /// PNG: 保透明; GIF: 微信/QQ 等从 image/png 名义读取后按实际字节(GIF 头)解码为动图。
    pub unsafe fn set_png_formats(bytes: &[u8]) -> Result<(), String> {
        set_data(RegisterClipboardFormatW(windows::core::w!("PNG")), bytes)?;
        set_data(RegisterClipboardFormatW(windows::core::w!("image/png")), bytes)?;
        Ok(())
    }

    /// CF_HDROP(真实文件路径, UTF-16) —— 微信/QQ“复制图片文件+Ctrl+V”会插入原始文件(保透明/动图)
    pub unsafe fn set_hdrop(path: &str) -> Result<(), String> {
        #[repr(C)]
        struct DROPFILES {
            p_files: u32, // 距结构起头的文件列表偏移
            pt: [i32; 2],
            f_nc: u32,
            f_wide: u32, // 1 = UTF-16
        }
        let wide: Vec<u16> = path.encode_utf16().collect();
        let struct_bytes = std::mem::size_of::<DROPFILES>();
        let payload_len = struct_bytes + (wide.len() + 1 + 1) * 2; // 路径 + 双 null
        let h = GlobalAlloc(GMEM_MOVEABLE, payload_len).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        unsafe {
            let base = ptr as *mut u8;
            let header = DROPFILES {
                p_files: struct_bytes as u32,
                pt: [0, 0],
                f_nc: 0,
                f_wide: 1,
            };
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

    // ---------- 剪贴板 ----------
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

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

    /// 把 RGBA 像素写为 CF_DIB (32bpp, **BGRA 字节序**, 透明区域合成到白底)
    /// 注意: DIB 低位字节是 B; 直接拷贝 RGBA 会导致 R/B 互换 (此前实测“主体变色”) 且透明像素 RGB=0 会被显示为黑底。
    pub unsafe fn set_dib(rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
        let header = BITMAPINFOHEADER {
            bi_size: 40,
            bi_width: w as i32,
            bi_height: -(h as i32), // 顶-底
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0, // BI_RGB
            bi_size_image: w * h * 4,
            bi_xpels_per_meter: 0,
            bi_ypels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        // RGBA -> BGRA 且 alpha 合成到白底 (聊天背景) 避免黑/脏边
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

    /// GIF: FileGroupDescriptorW + FileContents (资源管理器"复制文件"格式)
    /// 普通剪贴板客户端看 CF_DIB, 微信/QQ 等识别虚拟文件可保留动画
    pub unsafe fn set_gif(bytes: &[u8], filename: &str) -> Result<(), String> {
        let fdw = RegisterClipboardFormatW(windows::core::w!("FileGroupDescriptorW"));
        let fc = RegisterClipboardFormatW(windows::core::w!("FileContents"));
        let mut name = [0u16; 260];
        for (i, c) in filename.encode_utf16().take(259).enumerate() {
            name[i] = c;
        }
        let desc = FILEGROUPDESCRIPTORW {
            c_items: 1,
            fgd: [FILEDESCRIPTORW {
                dw_flags: 0x4 /*FD_ATTRIBUTES*/ | 0x40 /*FD_FILESIZE*/,
                clsid: [0; 16],
                sizel: (0, 0),
                pointl: (0, 0),
                dw_file_attributes: 0x80 /*FILE_ATTRIBUTE_NORMAL*/,
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

    /// 打开剪贴板会话 (带重试), 设置回调内容
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

    // ---- 跟随窗口: 面板随目标进程 显示/隐藏/移动 (与 egui-lite 同步) ----
    pub unsafe fn panel_size(panel: isize) -> (i32, i32) {
        use windows::Win32::Foundation::RECT;
        let mut r = RECT::default();
        if GetWindowRect(HWND(panel as *mut _), &mut r).is_ok() {
            (r.right - r.left, r.bottom - r.top)
        } else {
            (0, 0)
        }
    }

    pub unsafe fn follow_pos(hwnd: isize, panel_w: i32, panel_h: i32) -> Option<(i32, i32)> {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
        let hwnd = HWND(hwnd as *mut _);
        if hwnd.0.is_null() {
            return None;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return None;
        }
        let mut cr = RECT::default();
        let mut client_bottom = r.bottom;
        if GetClientRect(hwnd, &mut cr).is_ok() {
            let mut pt = POINT { x: 0, y: cr.bottom };
            if ClientToScreen(hwnd, &mut pt).as_bool() {
                client_bottom = pt.y;
            }
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let work = if GetMonitorInfoW(mon, &mut mi).as_bool() { mi.rcWork } else { RECT { left: 0, top: 0, right: 1920, bottom: 1080 } };
        let w = r.right - r.left;
        let gap = 0;
        let mut x = r.left + w + gap;
        let mut y = client_bottom - panel_h;
        if x + panel_w > work.right {
            x = r.left - gap - panel_w;
        }
        x = x.max(work.left).min((work.right - panel_w).max(work.left));
        y = y.max(work.top).min((work.bottom - panel_h).max(work.top));
        Some((x, y))
    }

    pub unsafe fn target_visible(hwnd: isize) -> bool {
        IsWindowVisible(HWND(hwnd as *mut _)).as_bool() && !IsIconic(HWND(hwnd as *mut _)).as_bool()
    }

    pub unsafe fn place_panel(panel: isize, target: isize, x: i32, y: i32) {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOACTIVATE, SWP_ASYNCWINDOWPOS};
        let _ = SetWindowPos(
            HWND(panel as *mut _),
            HWND(target as *mut _),
            x, y, 0, 0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }

    pub unsafe fn panel_hwnd_from(app: &tauri::AppHandle) -> isize {
        use tauri::Manager;
        match app.get_webview_window("main") {
            Some(w) => match w.hwnd() {
                Ok(h) => h.0 as isize,
                Err(_) => 0,
            },
            None => 0,
        }
    }
}

#[tauri::command]
fn get_target(state: tauri::State<AppState>) -> Option<TargetInfo> {
    state.target.lock().unwrap().clone()
}

#[tauri::command]
fn is_picking(state: tauri::State<AppState>) -> bool {
    state.picking.load(Ordering::SeqCst)
}

#[tauri::command]
fn begin_pick(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let picking = state.picking.clone();
    if picking.swap(true, Ordering::SeqCst) {
        return Err("已在拾取中".into());
    }
    let target = state.target.clone();
    let self_hwnd = win::self_hwnd(&app);
    std::thread::spawn(move || {
        let timer = std::time::Instant::now();
        let mut last = 0isize;
        let timeout = std::time::Duration::from_secs(15);
        while picking.load(Ordering::SeqCst) && timer.elapsed() < timeout {
            let fg = win::foreground_hwnd();
            if fg != 0 && fg != self_hwnd && fg != last {
                if let Some(info) = win::capture_target(fg) {
                    *target.lock().unwrap() = Some(info.clone());
                    #[cfg(target_os = "windows")]
                    if follow::enabled() {
                        follow::set_target(info.hwnd);
                    }
                    break;
                }
            }
            last = fg;
            std::thread::sleep(std::time::Duration::from_millis(180));
        }
        picking.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn cancel_pick(state: tauri::State<AppState>) {
    state.picking.store(false, Ordering::SeqCst);
}

/// 点击表情: 写入剪贴板并 Ctrl+V 到目标窗口的输入框
#[tauri::command]
fn insert_sticker(app: tauri::AppHandle, state: tauri::State<AppState>, path: String) -> Result<(), String> {
    let target = state
        .target
        .lock()
        .unwrap()
        .clone()
        .ok_or("请先选择目标窗口")?;
    let root = root_dir(&app);
    let bytes = read_file_safe(&root, &path)?;
    let p = PathBuf::from(&path);
    let filename = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sticker")
        .to_string();
    let is_gif_file = is_gif(&p);
    // 原图为 PNG 时塞“PNG”格式原始字节保透明
    let ext_lower = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let is_png = ext_lower == "png";

    // 解码为 RGBA (gif 取第一帧)
    let img = image::load_from_memory(&bytes).map_err(|e| format!("图片解码失败: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());

    #[cfg(target_os = "windows")]
    unsafe {
        win::set_clipboard(|| {
            // 1) CF_HDROP 真实文件路径: 微信/QQ 插入原始文件 (透明 PNG / 动图 GIF 全保留)
            win::set_hdrop(&p.to_string_lossy())?;
            // 2) FileGroupDescriptorW + FileContents 虚拟文件
            win::set_gif(&bytes, &filename)?;
            // 3) “PNG” 格式: PNG 原始字节 (保透明) / GIF 原始字节 (部分 app 按动图解码)
            if is_png || is_gif_file {
                win::set_png_formats(&bytes)?;
            }
            // 4) 绝不写 CF_DIB(白底合成位图): 微信等优先读位图导致透明 PNG 变白底
            Ok(())
        })?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (w, h, is_gif_file, filename);
        return Err("当前平台不支持插入".into());
    }

    win::activate_and_paste(target.hwnd)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(target_os = "windows")]
mod follow {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering as A};
    use std::sync::OnceLock;
    use super::win;
    use tauri::Manager;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static TARGET: AtomicIsize = AtomicIsize::new(0);
    static PANEL: AtomicIsize = AtomicIsize::new(0);
    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    static LAST_VIS: AtomicIsize = AtomicIsize::new(1);

    pub fn enabled() -> bool { ENABLED.load(A::SeqCst) }
    pub fn set_target(hwnd: isize) { TARGET.store(hwnd, A::SeqCst); }

    pub fn start(app: tauri::AppHandle) {
        ENABLED.store(true, A::SeqCst);
        let _ = APP.set(app.clone());
        if PANEL.load(A::SeqCst) == 0 {
            PANEL.store(unsafe { win::panel_hwnd_from(&app) }, A::SeqCst);
        }
        start_hook();
    }

    pub fn stop() { ENABLED.store(false, A::SeqCst); }

    fn start_hook() {
        use std::sync::Once;
        use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, TranslateMessage, MSG, EVENT_OBJECT_LOCATIONCHANGE,
            WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, OBJID_CLIENT, OBJID_WINDOW,
        };
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            std::thread::spawn(move || {
                unsafe extern "system" fn proc(
                    _h: HWINEVENTHOOK, _e: u32, hwnd: windows::Win32::Foundation::HWND,
                    idobj: i32, _ic: i32, _t: u32, _tm: u32,
                ) {
                    if !ENABLED.load(A::SeqCst) { return; }
                    if hwnd.0 as isize != TARGET.load(A::SeqCst) { return; }
                    if idobj != OBJID_WINDOW.0 && idobj != OBJID_CLIENT.0 { return; }
                    let mut panel = PANEL.load(A::SeqCst);
                    if panel == 0 {
                        if let Some(a) = APP.get() {
                            panel = win::panel_hwnd_from(a);
                            PANEL.store(panel, A::SeqCst);
                        }
                    }
                    if panel != 0 {
                        let (pw, ph) = win::panel_size(panel);
                        if pw > 0 && ph > 0 {
                            if let Some((x, y)) = win::follow_pos(hwnd.0 as isize, pw, ph) {
                                win::place_panel(panel, hwnd.0 as isize, x, y);
                            }
                        }
                    }
                    let vis = win::target_visible(hwnd.0 as isize) as isize;
                    let prev = LAST_VIS.swap(vis, A::SeqCst);
                    if prev != vis {
                        let panel = PANEL.load(A::SeqCst);
                        if panel != 0 {
                            // 纯 Win32 最小化/恢复 (跨线程安全, 不触发 tauri 线程限制)
                            use windows::Win32::UI::WindowsAndMessaging::{
                                ShowWindow, SW_RESTORE, SW_MINIMIZE,
                            };
                            let _ = ShowWindow(
                                windows::Win32::Foundation::HWND(panel as *mut _),
                                if vis == 1 { SW_RESTORE } else { SW_MINIMIZE },
                            );
                        }
                    }
                }
                let hook = unsafe {
                    SetWinEventHook(
                        EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None,
                        Some(proc), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                    )
                };
                let mut msg = MSG::default();
                unsafe {
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    if !hook.0.is_null() { let _ = UnhookWinEvent(hook); }
                }
            });
        });
    }
}

/// 跟随窗口 (面板随目标进程 显示/隐藏/移动)
#[tauri::command]
fn get_follow_window(app: tauri::AppHandle) -> bool {
    // 默认开启 (与 egui-lite 一致); 未配置过时视为 true
    read_settings(&app)
        .get("followWindow")
        .and_then(|a| a.as_bool())
        .unwrap_or(true)
}

#[tauri::command]
fn set_follow_window(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let mut v = read_settings(&app);
    v["followWindow"] = serde_json::json!(on);
    write_settings(&app, &v)?;
    if on {
        follow::start(app);
    } else {
        follow::stop();
    }
    Ok(())
}


pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if get_follow_window(app.handle().clone()) {
                let _ = set_follow_window(app.handle().clone(), true);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_root,
            set_stickers_dir,
            get_follow_window,
            set_follow_window,
            list_packages,
            list_stickers,
            read_sticker,
            shop_list,
            install_package,
            delete_package,
            reveal_root,
            get_target,
            is_picking,
            begin_pick,
            cancel_pick,
            insert_sticker
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_src(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emoji_rs_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_packages_with_gif_and_cover() {
        let root = tmp_src("scan").join("stickers");
        let pkg = root.join("元气团子");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("01.png"), b"pngdata").unwrap();
        fs::write(pkg.join("02.gif"), b"gifdata").unwrap();
        fs::write(pkg.join("cover.png"), b"coverdata").unwrap();
        let info = scan_dir_packages(&root, false);
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].name, "元气团子");
        assert_eq!(info[0].count, 2);
        assert_eq!(info[0].gif_count, 1);
        let cover = info[0].cover.as_deref().unwrap_or("");
        assert!(cover.ends_with("cover.png"));
    }

    #[test]
    fn empty_root_returns_empty() {
        let root = tmp_src("empty").join("empty");
        fs::create_dir_all(&root).unwrap();
        assert!(scan_dir_packages(&root, false).is_empty());
        assert!(scan_dir_packages(&root.join("shop"), true).is_empty());
    }

    #[test]
    fn valid_name_rejects_unsafe() {
        for bad in ["a/b", "a\\b", "..", "a..b", "a:", "a*b", "a?b"] {
            assert!(valid_name(bad).is_err(), "should reject {bad}");
        }
        assert_eq!(valid_name("我的表情").unwrap(), "我的表情");
        assert_eq!(valid_name("basic-1").unwrap(), "basic-1");
    }

    #[test]
    fn install_and_delete_package() {
        let root = tmp_src("install").join("stickers");
        let shop = root.join(SHOP_DIR);
        let pkg = shop.join("柴犬日常");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("01.gif"), b"gif").unwrap();
        fs::write(pkg.join("cover.png"), b"png").unwrap();

        copy_dir_recursive(&pkg, &root.join("柴犬日常")).unwrap();
        assert!(root.join("柴犬日常/01.gif").is_file());
        assert!(root.join("柴犬日常/cover.png").is_file());

        let err = install_package_in(root.clone(), "柴犬日常".to_string()).unwrap_err();
        assert!(err.contains("已下载"), "err={err}");
        assert!(install_package_in(root.clone(), "不存在包".to_string()).is_err());

        delete_package_in(root.clone(), "柴犬日常".to_string()).unwrap();
        assert!(!root.join("柴犬日常").exists());
    }

    #[test]
    fn read_sticker_mime_and_safety() {
        let root = tmp_src("read");
        let pkg = root.join("stickers").join("包");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("a.png"), [137, 80, 78, 71]).unwrap();
        fs::write(pkg.join("b.gif"), [71, 73, 70, 56]).unwrap();
        fs::write(root.join("evil.png"), b"x").unwrap();

        let outside_name = format!("emoji_outside_{}", std::process::id());
        let outside = std::env::temp_dir().join(&outside_name);
        fs::write(&outside, b"x").unwrap();
        let evasive = root.join("..").join(&outside_name);

        let inside_png = pkg.join("a.png").to_string_lossy().to_string();
        assert_eq!(
            read_file_safe(&root, &inside_png).unwrap().starts_with(&[137, 80, 78, 71]),
            true
        );
        // 非绝对路径 / 越界(含 .. 绕过)必须拒绝; root 内文件放行
        assert!(read_file_safe(&root, "rel/path.png").is_err());
        assert!(read_file_safe(&root, &evasive.to_string_lossy().to_string()).is_err());
        let evil = root.join("evil.png").to_string_lossy().to_string();
        assert!(read_file_safe(&root, &evil).is_ok());
        let _ = fs::remove_file(&outside);
    }

    fn install_package_in(root: PathBuf, name: String) -> Result<(), String> {
        let n = valid_name(&name)?;
        let from = root.join(SHOP_DIR).join(&n);
        let to = root.join(&n);
        if !from.is_dir() {
            return Err("商店里没有这个表情包".into());
        }
        if to.exists() {
            return Err(format!("「{n}」已下载过了"));
        }
        copy_dir_recursive(&from, &to)?;
        Ok(())
    }

    fn delete_package_in(root: PathBuf, name: String) -> Result<(), String> {
        let n = valid_name(&name)?;
        let dir = root.join(&n);
        if !dir.is_dir() {
            return Err("表情包不存在".into());
        }
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(())
    }
}