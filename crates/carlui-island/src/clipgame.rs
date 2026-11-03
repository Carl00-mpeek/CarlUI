//! Bu modül "tam ekran oyunda (ör. CS2'de Shift+Tab sohbetinde) pano geçmişi
//! açılınca oyun simge durumuna küçülüyor" sorununu odak çalmayan pasif
//! açılışla çözüyor: pano paneli etkinleştirilmeden en üste konur, oyun ön
//! planda kalır. Klavye düşük düzeyli bir kancayla yalnızca panel açıkken
//! dinlenir; tuşlar oyuna gitmesin diye yutulur ve panele mesaj olarak
//! iletilir. Seçilen kayıt panoya yazılınca Ctrl+V doğrudan oyuna (sohbet
//! kutusuna) gider. Win basılıyken tuşlar geçer: Win+V paneli yine kapatır.

use crate::*;
use std::sync::atomic::{AtomicBool, AtomicIsize};

pub(crate) const WM_APP_CLIP_KEY: u32 = WM_APP + 11;
pub(crate) const WM_APP_CLIP_CHAR: u32 = WM_APP + 12;

static ON: AtomicBool = AtomicBool::new(false);
static POP: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

const MODIFIERS: [VIRTUAL_KEY; 9] = [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU];

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && ON.load(Ordering::Relaxed) {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let injected = k.flags.0 & LLKHF_INJECTED.0 != 0;
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        let win = down(VK_LWIN) || down(VK_RWIN) || vk == VK_LWIN || vk == VK_RWIN;
        if !injected && !win && !MODIFIERS.contains(&vk) {
            let msg = wp.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let pop = HWND(POP.load(Ordering::Relaxed) as _);
                let _ = PostMessageW(pop, WM_APP_CLIP_KEY, WPARAM(k.vkCode as usize), LPARAM(0));
                let mut state = [0u8; 256];
                for m in MODIFIERS {
                    if down(m) {
                        state[m.0 as usize] = 0x80;
                    }
                }
                if GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 {
                    state[VK_CAPITAL.0 as usize] = 1;
                }
                let mut buf = [0u16; 4];
                let layout = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), None));
                let n = ToUnicodeEx(k.vkCode, k.scanCode, &state, &mut buf, 4, layout);
                // Denetim karakterleri (Enter, Esc, Geri) tuş olarak işlendi.
                if n > 0 && !(down(VK_CONTROL) && !down(VK_MENU)) {
                    for u in &buf[..n as usize] {
                        if *u >= 0x20 {
                            let _ = PostMessageW(pop, WM_APP_CLIP_CHAR, WPARAM(*u as usize), LPARAM(0));
                        }
                    }
                }
            }
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Pasif modu açar: klavye kancası kurulur (ada iş parçacığından çağrılır).
pub(crate) fn start(pop: HWND) -> bool {
    if ON.load(Ordering::Relaxed) {
        return true;
    }
    POP.store(pop.0 as isize, Ordering::Relaxed);
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), inst, 0) {
            Ok(h) => {
                HOOK.store(h.0 as isize, Ordering::Relaxed);
                ON.store(true, Ordering::Relaxed);
                true
            }
            Err(e) => {
                carlui_core::warn!("pano (oyun modu) klavye kancası kurulamadı: {e}");
                false
            }
        }
    }
}

pub(crate) fn stop() {
    ON.store(false, Ordering::Relaxed);
    let h = HOOK.swap(0, Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as _));
        }
    }
}

pub(crate) fn active() -> bool {
    ON.load(Ordering::Relaxed)
}
