//! Kısayol kaydedici. Kayıt sürerken düşük seviyeli klavye kancası kurulur ve
//! basılan tuşlar yutulur: böylece Win+V gibi bir kısayolu kaydederken
//! Windows'un ya da dock'un kendi kısayolu tetiklenmez. Yalnızca klavye
//! dinlenir; fare tuşları hiçbir zaman kısayol olamaz (kayıt sırasında
//! tıklamak kaydı iptal eder). Sonuç pencereye `WM_APP_KEY` ile gelir:
//! wParam = değiştiriciler (1 Ctrl, 2 Alt, 4 Shift, 8 Win), lParam = sanal tuş
//! (0: yalnızca değiştiriciler basılıp bırakıldı).

use carlui_core::keys::Mods;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use windows::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const WM_APP_KEY: u32 = WM_APP + 5;
/// Kayıt bitti ve tüm tuşlar bırakıldı: kanca kaldırılabilir.
pub const WM_APP_UNHOOK: u32 = WM_APP + 6;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static RECORDING: AtomicBool = AtomicBool::new(false);
static DOWN: AtomicU8 = AtomicU8::new(0);
static PEAK: AtomicU8 = AtomicU8::new(0);

fn mod_bit(vk: VIRTUAL_KEY) -> u8 {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => 1,
        VK_MENU | VK_LMENU | VK_RMENU => 2,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => 4,
        VK_LWIN | VK_RWIN => 8,
        _ => 0,
    }
}

fn post(msg: u32, w: usize, l: isize) {
    unsafe {
        let _ = PostMessageW(HWND(TARGET.load(Ordering::SeqCst) as _), msg, WPARAM(w), LPARAM(l));
    }
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wp, lp);
    }
    let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
    let vk = VIRTUAL_KEY(kb.vkCode as u16);
    let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let bit = mod_bit(vk);
    let rec = RECORDING.load(Ordering::SeqCst);
    if bit != 0 {
        if down {
            if !rec {
                return CallNextHookEx(None, code, wp, lp);
            }
            let d = DOWN.fetch_or(bit, Ordering::SeqCst) | bit;
            PEAK.fetch_or(d, Ordering::SeqCst);
        } else {
            let was = DOWN.fetch_and(!bit, Ordering::SeqCst);
            if was & bit == 0 && !rec {
                return CallNextHookEx(None, code, wp, lp);
            }
            if was & !bit == 0 {
                let peak = PEAK.swap(0, Ordering::SeqCst);
                if rec && peak != 0 {
                    post(WM_APP_KEY, peak as usize, 0);
                }
                if !rec {
                    post(WM_APP_UNHOOK, 0, 0);
                }
            }
        }
        return LRESULT(1);
    }
    if !rec {
        return CallNextHookEx(None, code, wp, lp);
    }
    if down {
        // Ana tuş geldi: bırakılınca ayrıca "yalnız değiştirici" sonucu üretilmesin.
        PEAK.store(0, Ordering::SeqCst);
        post(WM_APP_KEY, DOWN.load(Ordering::SeqCst) as usize, vk.0 as isize);
    }
    LRESULT(1)
}

pub fn start(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::SeqCst);
    DOWN.store(0, Ordering::SeqCst);
    PEAK.store(0, Ordering::SeqCst);
    RECORDING.store(true, Ordering::SeqCst);
    if HOOK.load(Ordering::SeqCst) == 0 {
        unsafe {
            if let Ok(h) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), GetModuleHandleW(None).unwrap_or_default(), 0) {
                HOOK.store(h.0 as isize, Ordering::SeqCst);
            }
        }
    }
}

/// Kaydı bitirir; basılı tuş kalmadıysa kancayı hemen kaldırır (yoksa
/// bırakılınca `WM_APP_UNHOOK` gelir, o ana kadar bırakmalar yutulur).
pub fn stop() {
    RECORDING.store(false, Ordering::SeqCst);
    PEAK.store(0, Ordering::SeqCst);
    if DOWN.load(Ordering::SeqCst) == 0 {
        unhook();
    }
}

pub fn unhook() {
    if RECORDING.load(Ordering::SeqCst) {
        return;
    }
    let h = HOOK.swap(0, Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as _));
        }
    }
}

pub fn mods_of(bits: usize) -> Mods {
    Mods { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0, win: bits & 8 != 0 }
}

/// Değiştiricileri ayar metnine çevirir: "Win+Ctrl+Alt+Shift" sırasıyla.
pub fn mods_text(m: Mods) -> String {
    let mut v = Vec::new();
    if m.win {
        v.push("Win");
    }
    if m.ctrl {
        v.push("Ctrl");
    }
    if m.alt {
        v.push("Alt");
    }
    if m.shift {
        v.push("Shift");
    }
    v.join("+")
}

/// Sanal tuşun ayar metnindeki adı (`keys::parse_combo`'nun tanıdığı adlar).
pub fn vk_name(vk: u32) -> Option<String> {
    Some(match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk)?.to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".into(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x08 => "Backspace".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x26 => "Up".into(),
        0x28 => "Down".into(),
        0x13 => "Pause".into(),
        0x2C => "PrintScreen".into(),
        0xC0 => "`".into(),
        0xBD => "-".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        _ => return None,
    })
}
