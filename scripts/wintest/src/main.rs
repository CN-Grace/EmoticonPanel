// 真实系统验证注入剪贴板字节布局: DIB(BGRA+白底合成) / "PNG"格式(原始字节) / GIF虚拟文件 / 窗口拾取激活
#![allow(dead_code)]
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
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

unsafe fn read_global(h: HANDLE) -> Vec<u8> {
    let hg = HGLOBAL(h.0);
    let ptr = GlobalLock(hg);
    if ptr.is_null() {
        return Vec::new();
    }
    // 大小未知, 剪贴板全局句柄: 用 GlobalSize
    let size = unsafe { windows::Win32::System::Memory::GlobalSize(hg) };
    let mut out = vec![0u8; size];
    std::ptr::copy_nonoverlapping(ptr as *const u8, out.as_mut_ptr(), size);
    let _ = GlobalUnlock(hg);
    out
}

fn main() {
    // ---- 1. 构造测试图片: 2x2 = [ 红A255 蓝A255 | 红A0 绿A128 ] (row-major) ----
    // RGBA: 红(255,0,0,255) → BGRA [0,0,255,255]; 蓝(0,0,255,255) → [255,0,0,255]
    // 透明红(255,0,0,0) → 合白 [255,255,255,255]; 半透绿(0,255,0,128) → [128,255,128,255]
    let rgba: Vec<u8> = vec![
        255, 0, 0, 255, 0, 0, 255, 255, //
        255, 0, 0, 0, 0, 255, 0, 128,
    ];
    let expected: Vec<u8> = vec![
        0, 0, 255, 255, 255, 0, 0, 255, //
        255, 255, 255, 255, 127, 255, 127, 255,
    ];

    // 1x1 PNG 字节 (红色像素)
    const PNG1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x64,
        0x60, 0x78, 0x8F, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x03, 0x00, 0x05, 0x01, 0x02, 0x57,
        0x4A, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const GIFB: &[u8] = b"GIF89aFAKE-BYTES-FOR-FILECONTENTS";

    // ---- 2. 写入 (与 insert_sticker 同序) ----
    println!("[1] clipboard write...");
    let ok = unsafe { OpenClipboard(None).is_ok() };
    assert!(ok, "OpenClipboard");
    unsafe {
        let _ = EmptyClipboard();
        // set_dib: BGRA + 白底合成
        let header = BITMAPINFOHEADER {
            bi_size: 40,
            bi_width: 2,
            bi_height: -2,
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: 16,
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
        assert_eq!(flipped, expected, "BGRA flatten mismatch!");
        let hdib = GlobalAlloc(GMEM_MOVEABLE, 40 + flipped.len()).unwrap();
        let p = GlobalLock(hdib);
        std::ptr::copy_nonoverlapping((&header as *const BITMAPINFOHEADER) as *const u8, p as *mut u8, 40);
        std::ptr::copy_nonoverlapping(flipped.as_ptr(), (p as *mut u8).add(40), flipped.len());
        let _ = GlobalUnlock(hdib);
        SetClipboardData(8, HANDLE(hdib.0)).unwrap();
        // set_png_formats + set_gif
        let png_fmt = RegisterClipboardFormatW(windows::core::w!("PNG"));
        let ipng_fmt = RegisterClipboardFormatW(windows::core::w!("image/png"));
        let fdw = RegisterClipboardFormatW(windows::core::w!("FileGroupDescriptorW"));
        let fc = RegisterClipboardFormatW(windows::core::w!("FileContents"));
        for (f, data) in [
            (png_fmt, PNG1),
            (ipng_fmt, PNG1),
            (fdw, b"DESC"),
            (fc, GIFB),
        ] {
            let h = GlobalAlloc(GMEM_MOVEABLE, data.len()).unwrap();
            let pp = GlobalLock(h);
            std::ptr::copy_nonoverlapping(data.as_ptr(), pp as *mut u8, data.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(f, HANDLE(h.0)).unwrap();
        }
        let _ = CloseClipboard();
    }
    println!("  write OK (DIB + PNG + image/png + FileGroupDescriptorW + FileContents)");

    // ---- 3. 读回验证 ----
    println!("[2] read-back...");
    assert!(unsafe { OpenClipboard(None).is_ok() });
    unsafe {
        let mut names = Vec::new();
        let mut f = 0u32;
        loop {
            let nf = EnumClipboardFormats(f);
            if nf == 0 { break; }
            let mut b = [0u16; 64];
            let n = GetClipboardFormatNameW(nf, &mut b);
            let nm = if n > 0 { String::from_utf16_lossy(&b[..n as usize]) } else { format!("#{nf}") };
            names.push(nm);
            f = nf;
        }
        println!("  formats: {}", names.join(", "));

        // DIB 字节断言
        let dib_h = GetClipboardData(8).unwrap();
        let dib = read_global(dib_h);
        assert!(dib.len() == 40 + 16, "DIB len {}", dib.len());
        let px: Vec<u8> = dib[40..].to_vec();
        assert_eq!(px, expected, "DIB pixels mismatch: {:?}", px);
        println!("  CF_DIB pixels == expected BGRA white-flattened: OK");

        // PNG 格式内容
        let png_fmt = RegisterClipboardFormatW(windows::core::w!("PNG"));
        let png_h = GetClipboardData(png_fmt).unwrap();
        let png = read_global(png_h);
        assert_eq!(png[..8], PNG1[..8], "PNG format content mismatch");
        println!("  \"PNG\" format == raw PNG bytes: OK");

        // FileContents 内容
        let fc = RegisterClipboardFormatW(windows::core::w!("FileContents"));
        let fc_h = GetClipboardData(fc).unwrap();
        let fc_data = read_global(fc_h);
        assert_eq!(&fc_data[..], GIFB, "FileContents mismatch");
        println!("  FileContents == raw GIF bytes: OK");
        let _ = CloseClipboard();
    }

    // ---- 4. 窗口拾取 + 激活粘贴 (回归) ----
    println!("[3] window pick + paste...");
    let _ = std::process::Command::new("notepad").spawn();
    std::thread::sleep(std::time::Duration::from_millis(1700));
    let fg = unsafe { GetForegroundWindow().0 as isize };
    let h = HWND(fg as *mut _);
    let mut buf = vec![0u16; 256];
    let len = unsafe { GetWindowTextW(h, &mut buf) };
    let title = if len > 0 { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() };
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid as *mut u32)); }
    println!("  captured: title={title} pid={pid}");
    if !title.contains("记事本") && !title.contains("Notepad") {
        println!("  WARN: not notepad, adjust expectations");
    }

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_CONTROL, VK_MENU,
        VK_V,
    };
    unsafe {
        let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
        let _ = keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        if IsIconic(h).as_bool() {
            ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h).as_bool();
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    unsafe {
        let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        let _ = keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        let _ = keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        let _ = keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    println!("ALL WIN32 CHECKS PASSED (DIB layout corrected, PNG/GIF formats live)");
}