//! Bu modül "hücreleri ve metni başka programlarla kopyala/yapıştır" sorununu
//! çözüyor: Windows panosuna Unicode düz metin (CF_UNICODETEXT) yazılır ve
//! okunur. Tablolar Excel gibi sekme/satır sonu ile ayrılır.

use windows::Win32::{
    Foundation::*,
    System::{
        DataExchange::*,
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
    },
};

const CF_UNICODETEXT: u32 = 13;

unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..8 {
        if OpenClipboard(hwnd).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

pub fn copy(hwnd: HWND, text: &str) -> bool {
    unsafe {
        if !open(hwnd) {
            return false;
        }
        let _ = EmptyClipboard();
        let bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect();
        let mut ok = false;
        if let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
            let p = GlobalLock(g) as *mut u8;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
                let _ = GlobalUnlock(g);
                ok = SetClipboardData(CF_UNICODETEXT, HANDLE(g.0)).is_ok();
            }
            if !ok {
                let _ = GlobalFree(g);
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

pub fn paste(hwnd: HWND) -> Option<String> {
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let out = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 && n < 16 * 1024 * 1024 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}
