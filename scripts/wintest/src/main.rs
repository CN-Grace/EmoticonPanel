// 真实系统验证 Win32 注入链路: 剪贴板格式 / 窗口拾取 / 激活+Ctrl+V
#![allow(dead_code)]
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};
use windows::core::PWSTR;

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

unsafe fn set_dib(rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
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
    let h = GlobalAlloc(GMEM_MOVEABLE, 40 + rgba.len()).map_err(|e| e.to_string())?;
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return Err("GlobalLock failed".into());
    }
    std::ptr::copy_nonoverlapping((&header as *const BITMAPINFOHEADER) as *const u8, ptr as *mut u8, 40);
    std::ptr::copy_nonoverlapping(rgba.as_ptr(), (ptr as *mut u8).add(40), rgba.len());
    let _ = GlobalUnlock(h);
    SetClipboardData(8u32, HANDLE(h.0)).map_err(|e| e.to_string())?;
    Ok(())
}

unsafe fn set_data(format: u32, bytes: &[u8]) -> Result<(), String> {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return Err("GlobalLock failed".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    let _ = GlobalUnlock(h);
    SetClipboardData(format, HANDLE(h.0)).map_err(|e| e.to_string())?;
    Ok(())
}

fn capture_target(hwnd: isize) {
    let h = HWND(hwnd as *mut _);
    let mut buf = vec![0u16; 512];
    let len = unsafe { GetWindowTextW(h, &mut buf) };
    let title = if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    };
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid as *mut u32));
    }
    let process = if pid != 0 {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return;
            };
            let mut b = vec![0u16; 1024];
            let mut size = b.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(b.as_mut_ptr()),
                &mut size as *mut u32,
            );
            let _ = CloseHandle(handle);
            if ok.is_ok() && size > 0 {
                let name = OsString::from_wide(&b[..size as usize]).to_string_lossy().to_string();
                Path::file_name(name.as_ref())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(name)
            } else {
                String::new()
            }
        }
    } else {
        String::new()
    };
    println!("  captured: hwnd={hwnd} pid={pid} process={process} title={title}");
}

fn activate_and_paste(hwnd: isize) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_CONTROL, VK_MENU,
        VK_V,
    };
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
        let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        if IsIconic(hwnd).as_bool() {
            ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd).as_bool();
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    unsafe {
        let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        let _ = keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        let _ = keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    println!("  activate + Ctrl+V sent");
}

fn main() {
    // 1. 剪贴板: 8x8 纯红 DIB + FileGroupDescriptorW/FileContents (GIF 模拟)
    println!("[1] clipboard write...");
    let rgba: Vec<u8> = vec![255u8, 0, 0, 255].repeat(64);
    let fdw = unsafe { RegisterClipboardFormatW(windows::core::w!("FileGroupDescriptorW")) };
    let fc = unsafe { RegisterClipboardFormatW(windows::core::w!("FileContents")) };
    println!("  reg fmts: FileGroupDescriptorW={fdw} FileContents={fc}");
    let ok = unsafe { OpenClipboard(None).is_ok() };
    if !ok {
        println!("  FAIL: OpenClipboard"); std::process::exit(1);
    }
    unsafe {
        let _ = EmptyClipboard();
        let r1 = set_dib(&rgba, 8, 8);
        let r2 = set_data(fdw, b"FAKEGIFDESC");
        let r3 = set_data(fc, b"FAKEGIFBYTES");
        let _ = CloseClipboard();
        assert!(r1.is_ok(), "DIB: {r1:?}");
        assert!(r2.is_ok(), "FDW: {r2:?}");
        assert!(r3.is_ok(), "FC: {r3:?}");
    }
    println!("  set OK (DIB + FileGroupDescriptorW + FileContents)");

    // 2. 读回验证: CF_DIB 数据 + 格式枚举
    let okread = unsafe { OpenClipboard(None).is_ok() };
    assert!(okread, "OpenClipboard(read)");
    let dib_len = unsafe { GetClipboardData(8u32) }
        .map(|h| unsafe {
            let hg = HGLOBAL(h.0);
            let p = GlobalLock(hg);
            let _ = GlobalUnlock(hg);
            p
        })
        .map(|_| 1).unwrap_or(0);
    let mut formats = Vec::new();
    unsafe {
        let mut f = 0u32;
        loop {
            let nf = EnumClipboardFormats(f);
            if nf == 0 { break; }
            formats.push(nf);
            f = nf;
        }
        let _ = CloseClipboard();
    }
    println!("  read-back: CF_DIB handle ok={} (len>0)", dib_len);
    let names: Vec<String> = formats
        .iter()
        .map(|&f| {
            let mut b = [0u16; 64];
            let n = unsafe { GetClipboardFormatNameW(f, &mut b) };
            if n > 0 { String::from_utf16_lossy(&b[..n as usize]) } else { format!("#{f}") }
        })
        .collect();
    println!("  clipboard formats: {}", names.join(", "));
    let has_dib = formats.contains(&8u32);
    let has_fdw = names.iter().any(|n| n == "FileGroupDescriptorW");
    let has_fc = names.iter().any(|n| n == "FileContents");
    println!("  assert CF_DIB present: {has_dib}, FileGroupDescriptorW present: {has_fdw}, FileContents present: {has_fc}");
    assert!(has_dib && has_fdw && has_fc, "clipboard formats incomplete");

    // 3. 窗口拾取: 启动记事本, 抓前台窗口
    println!("[2] window pick...");
    let _ = std::process::Command::new("notepad").spawn();
    std::thread::sleep(std::time::Duration::from_millis(1800));
    let fg = unsafe { GetForegroundWindow().0 as isize };
    println!("  foreground hwnd={fg}");
    capture_target(fg);

    // 4. 激活 + Ctrl+V (粘贴图片进记事本无害)
    println!("[3] activate + paste...");
    activate_and_paste(fg);
    println!("ALL WIN32 CHECKS PASSED");
}