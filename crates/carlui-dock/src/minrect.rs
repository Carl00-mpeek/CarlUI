//! Bu modül "pencere küçülürken/büyürken animasyon nereye gitsin?" sorununu
//! şu yöntemle çözüyor: Windows küçültme animasyonunun hedefini kabuk kancası
//! pencerelerine `HSHELL_GETMINRECT` ile sorar; yanıt vermeyen olursa gizli
//! Windows görev çubuğundaki düğmeye gider. Dock kendini kabuk kancası olarak
//! kaydeder ve pencere dock'taki bir ikona aitse o ikonun dikdörtgenini verir.

use std::sync::atomic::{AtomicU32, Ordering};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        UI::WindowsAndMessaging::{DeregisterShellHookWindow, RegisterShellHookWindow,RegisterWindowMessageW, HSHELL_GETMINRECT, SHELLHOOKINFO},
    },
};

static MSG: AtomicU32 = AtomicU32::new(0);

/// Dock penceresini kabuk kancası olarak (yeniden) kaydeder. Yeniden kayıt
/// dock'u listenin sonuna alır: explorer yeniden başlayınca onun yanıtı
/// bizimkini ezmesin diye.
pub fn register(hwnd: HWND) {
    unsafe {
        MSG.store(RegisterWindowMessageW(w!("SHELLHOOK")), Ordering::Relaxed);
        let _ = DeregisterShellHookWindow(hwnd);
        if !RegisterShellHookWindow(hwnd).as_bool() {
            carlui_core::warn!("kabuk kancası kaydedilemedi: küçültme animasyonu görev çubuğuna gider");
        }
    }
}

/// Bu mesaj bir küçültme hedefi sorusuysa sorulan pencereyi verir.
pub fn asked(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<HWND> {
    let m = MSG.load(Ordering::Relaxed);
    if m == 0 || msg != m || wp.0 as u32 != HSHELL_GETMINRECT || lp.0 == 0 {
        return None;
    }
    Some(unsafe { (*(lp.0 as *const SHELLHOOKINFO)).hwnd })
}

/// Soruyu yanıtlar: animasyon `rc`'ye gider.
pub fn answer(lp: LPARAM, rc: RECT) {
    unsafe { (*(lp.0 as *mut SHELLHOOKINFO)).rc = rc };
}
