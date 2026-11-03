//! Bu modül "menü Windows tuşuyla açılınca odağı alamıyor; dışarı tıklanınca
//! WM_ACTIVATE gelmediği için açık kalıyor" sorununu çözüyor. Yöntem: menü
//! açıkken düşük seviyeli fare kancası kurulur; bu süreçe ait olmayan bir
//! pencereye (masaüstü, başka uygulama, dock...) basılınca menüye kapanma
//! mesajı gönderilir. Menü kapanınca kanca kaldırılır, boşta maliyeti yoktur.

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
    UI::WindowsAndMessaging::*,
};

/// Dışarı tıklandı: menü kapanmalı.
pub const WM_APP_OUTSIDE: u32 = WM_APP + 3;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);

/// Kancayı kurar (zaten kuruluysa bir şey yapmaz).
pub fn arm(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    if HOOK.load(Ordering::Relaxed) != 0 {
        return;
    }
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(proc), module, 0) {
            HOOK.store(h.0 as isize, Ordering::Relaxed);
        }
    }
}

pub fn disarm() {
    let h = HOOK.swap(0, Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as _));
        }
    }
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let down = matches!(wp.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN);
    if code >= 0 && down {
        let pt = (*(lp.0 as *const MSLLHOOKSTRUCT)).pt;
        // Kendi pencerelerimiz (menü, sağ tık menüsü) "dışarı" sayılmaz.
        let root = GetAncestor(WindowFromPoint(pt), GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(root, Some(&mut pid));
        if pid != GetCurrentProcessId() {
            let _ = PostMessageW(HWND(TARGET.load(Ordering::Relaxed) as _), WM_APP_OUTSIDE, WPARAM(0), LPARAM(0));
        }
    }
    CallNextHookEx(None, code, wp, lp)
}
