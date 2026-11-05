//! Bu modül "sık yazılan metinleri her seferinde elle yazmak" sorununu
//! çözüyor: metin genişletici. `config.toml`'daki `[tools] snippets`
//! listesindeki bir kısaltma (`;adres` gibi) herhangi bir uygulamada
//! yazılınca silinir ve yerine açılım metni yazılır.
//!
//! Düşük seviye klavye kancası yalnızca modül açıkken ve geçerli kısaltma
//! varken kurulur. Kanca işini hızlı bitirir: karakteri tampona ekler,
//! eşleşme varsa işi ayrı bir gönderici iş parçacığına bırakır. Kendi
//! gönderdiğimiz (enjekte) tuşlar sayılmaz; ön plan penceresi değişince,
//! Ctrl/Alt/Win kısayollarında ya da yön tuşlarında tampon sıfırlanır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod matcher;
mod send;

use carlui_core::{
    config::{self, Config},
    log,
};
use matcher::Buffer;
use std::{
    cell::RefCell,
    sync::{mpsc, Mutex},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, SystemInformation::GetLocalTime, Threading::*},
        UI::{Accessibility::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_APP_RELOAD: u32 = WM_APP + 1;

/// Etkin kısaltmalar: (anahtar karakterleri, açılım metni).
struct Active {
    keys: Vec<Vec<char>>,
    texts: Vec<String>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static PENDING: Mutex<Option<Config>> = Mutex::new(None);
static SENDER: Mutex<Option<mpsc::Sender<(usize, String)>>> = Mutex::new(None);

thread_local! {
    static BUF: RefCell<Buffer> = RefCell::new(Buffer::default());
    static HOOK: RefCell<HHOOK> = const { RefCell::new(HHOOK(std::ptr::null_mut())) };
}

fn down(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) }) < 0
}

/// Basılan tuşun ürettiği karakter (ön plandaki pencerenin klavye düzeniyle).
unsafe fn char_of(k: &KBDLLHOOKSTRUCT) -> Option<char> {
    let mut state = [0u8; 256];
    for v in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU] {
        if down(v) {
            state[v.0 as usize] = 0x80;
        }
    }
    if GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 {
        state[VK_CAPITAL.0 as usize] = 1;
    }
    let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
    let layout = GetKeyboardLayout(tid);
    let mut out = [0u16; 8];
    // 0x4: tuş durumunu değiştirme — ölü tuşlar (^, ¨) kullanıcı için bozulmasın.
    let n = ToUnicodeEx(k.vkCode, k.scanCode, &state, &mut out, 0x4, layout);
    if n <= 0 {
        return None;
    }
    char::decode_utf16(out[..n as usize].iter().copied()).next()?.ok()
}

/// Başka programların enjekte ettiği tuşlar normalde sayılmaz (uzak masaüstü
/// araçları, makrolar). Yalnızca debug derlemede, otomatik test için açılabilir.
fn accept_injected() -> bool {
    cfg!(debug_assertions) && std::env::var_os("CARLUI_EXPAND_INJECTED").is_some()
}

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN) {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if k.dwExtraInfo != send::MARK && (k.flags.0 & LLKHF_INJECTED.0 == 0 || accept_injected()) && on_key(k) {
            // Kısaltmanın son harfi uygulamaya hiç ulaşmaz; yerine açılım gider.
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Tuşu işler; kısaltma tamamlandıysa açılımı başlatıp `true` döner (tuş yutulur).
unsafe fn on_key(k: &KBDLLHOOKSTRUCT) -> bool {
    let vk = VIRTUAL_KEY(k.vkCode as u16);
    if matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU | VK_CAPITAL) {
        return false;
    }
    if vk == VK_BACK {
        BUF.with(|b| b.borrow_mut().backspace());
        return false;
    }
    let ctrl = down(VK_CONTROL);
    let alt = down(VK_MENU);
    // AltGr = Ctrl+Alt; tek başına Ctrl ya da Alt bir kısayoldur.
    let shortcut = (ctrl != alt) || down(VK_LWIN) || down(VK_RWIN);
    let c = if shortcut { None } else { char_of(k) };
    let Some(c) = c.filter(|c| !c.is_control()) else {
        BUF.with(|b| b.borrow_mut().clear());
        return false;
    };
    let hit = BUF.with(|b| {
        let mut b = b.borrow_mut();
        b.push(c);
        let a = ACTIVE.lock().ok()?;
        let a = a.as_ref()?;
        let i = b.find(&a.keys)?;
        b.clear();
        Some((a.keys[i].len(), a.texts[i].clone()))
    });
    let Some((len, text)) = hit else { return false };
    // Son harf yutulduğu için bir eksik silinir.
    let erase = len - 1;
    let slow = text.contains("{pano}") || text.contains("{clipboard}") || text.chars().count() > 150;
    if slow {
        // Pano okuması ya da uzun metin kancayı bekletmesin.
        if let Some(tx) = SENDER.lock().ok().and_then(|s| s.clone()) {
            let _ = tx.send((erase, text));
        }
    } else {
        // Kancanın içinden hemen gönderilir: hızlı yazımda sonraki harfler
        // açılımın önüne geçmesin.
        let (d, t) = now();
        let out = matcher::expand(&text, &d, &t, || None);
        send::send(erase, &out);
    }
    true
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    BUF.with(|b| b.borrow_mut().clear());
}

fn now() -> (String, String) {
    let t = unsafe { GetLocalTime() };
    let date = if carlui_core::lang::is_tr() { format!("{:02}.{:02}.{}", t.wDay, t.wMonth, t.wYear) } else { format!("{}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay) };
    (date, format!("{:02}:{:02}", t.wHour, t.wMinute))
}

fn sender() -> mpsc::Sender<(usize, String)> {
    let (tx, rx) = mpsc::channel::<(usize, String)>();
    std::thread::Builder::new()
        .name("expand-send".into())
        .spawn(move || {
            for (erase, text) in rx {
                let (d, t) = now();
                let out = matcher::expand(&text, &d, &t, carlui_clip::capture::plain_text);
                send::send(erase, &out);
                info!("uzun kısaltma açıldı ({} karakter)", out.chars().count());
            }
        })
        .ok();
    tx
}

/// Ayarlara göre kancayı kurar ya da kaldırır.
unsafe fn apply(cfg: &Config) {
    let snippets: Vec<_> = cfg.tools.valid_snippets().cloned().collect();
    let on = cfg.tools.expander && !snippets.is_empty();
    *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = on.then(|| Active { keys: matcher::keys(&snippets), texts: snippets.into_iter().map(|s| s.text).collect() });
    BUF.with(|b| b.borrow_mut().clear());
    HOOK.with(|h| {
        let mut h = h.borrow_mut();
        if on && h.0.is_null() {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), GetModuleHandleW(None).unwrap_or_default(), 0) {
                Ok(k) => {
                    *h = k;
                    info!("metin genişletici açık");
                }
                Err(e) => warn!("metin genişletici klavye kancası kurulamadı: {e}"),
            }
        } else if !on && !h.0.is_null() {
            let _ = UnhookWindowsHookEx(*h);
            *h = HHOOK::default();
            info!("metin genişletici kapalı");
        }
    });
}

fn main() -> Result<()> {
    log::init("expand");
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\CarlUI.Expand"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        *SENDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender());
        apply(&config::load());

        let tid = GetCurrentThreadId();
        config::watch(move |c| {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
            let _ = PostThreadMessageW(tid, WM_APP_RELOAD, WPARAM(0), LPARAM(0));
        });
        let fg = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.hwnd.0.is_null() && msg.message == WM_APP_RELOAD {
                if let Some(c) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    apply(&c);
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWinEvent(fg);
    }
    Ok(())
}
