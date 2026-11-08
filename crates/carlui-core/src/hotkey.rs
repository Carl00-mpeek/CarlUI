//! Genel kısayolların ortak kaydı. Klavye kısayolları `RegisterHotKey` ile
//! alınır; bu API fare tuşu kabul etmez. Fare kısayolları için süreçte tek bir
//! `WH_MOUSE_LL` kancası kurulur ve eşleşen basış, klavyedekiyle aynı
//! `WM_HOTKEY` iletisine çevrilir; çağıranın ileti işleyicisi değişmez.
//!
//! Kanca kendi iş parçacığında çalışır: arayüz meşgulken fare gecikmez. Yalnızca
//! kayıtlı bir fare kısayolu varken kuruludur. DLL enjekte etmez, oyunlara
//! dokunmaz. Eşleşen basış ve bırakışı yutar; o tuş başka uygulamaya gitmez.
//! Yutulan tuş `GetAsyncKeyState`'te görünmez, basılı tutma denetimi için `held`.

use crate::keys::Combo;
use std::sync::{
    atomic::{AtomicIsize, AtomicU32, AtomicU8, Ordering},
    Mutex, Once,
};
use windows::{
    core::{Result, HSTRING},
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, Threading::*},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

#[derive(Clone, Copy)]
struct Bind {
    hwnd: isize,
    /// Pencere yoksa iletinin gideceği iş parçacığı (`RegisterHotKey(None, …)` gibi).
    thread: u32,
    id: i32,
    mods: u8,
    vk: u32,
    msg: u32,
}

static BINDS: Mutex<Vec<Bind>> = Mutex::new(Vec::new());
static STARTED: Once = Once::new();
static THREAD: AtomicU32 = AtomicU32::new(0);
/// Basışı yutulan, bırakışı beklenen tuşlar (bit: sanal tuş kodu).
static SWALLOWED: AtomicU8 = AtomicU8::new(0);
/// Tuş başına "basılı" olayları (sanal tuş koduyla dizinli).
static HELD: [AtomicIsize; 7] = [const { AtomicIsize::new(0) }; 7];
const MOUSE_VKS: [u32; 4] = [0x02, 0x04, 0x05, 0x06];
/// Kanca iş parçacığına: kayıt listesi değişti.
const WM_SYNC: u32 = WM_APP + 1;

fn event_name(vk: u32) -> HSTRING {
    HSTRING::from(format!("Local\\CarlUI.MouseHeld.{vk}"))
}

/// Fare iletisini (sanal tuş, basış mı) çiftine çevirir; sol tuş ve hareket `None`.
fn button(msg: u32, data: u32) -> Option<(u32, bool)> {
    let x = || if data >> 16 == 1 { 0x05 } else { 0x06 };
    match msg {
        WM_RBUTTONDOWN => Some((0x02, true)),
        WM_RBUTTONUP => Some((0x02, false)),
        WM_MBUTTONDOWN => Some((0x04, true)),
        WM_MBUTTONUP => Some((0x04, false)),
        WM_XBUTTONDOWN => Some((x(), true)),
        WM_XBUTTONUP => Some((x(), false)),
        _ => None,
    }
}

fn mod_bits(c: &Combo) -> u8 {
    c.mods.ctrl as u8 | (c.mods.alt as u8) << 1 | (c.mods.shift as u8) << 2 | (c.mods.win as u8) << 3
}

fn mods_now() -> u8 {
    let down = |k: VIRTUAL_KEY| unsafe { GetAsyncKeyState(k.0 as i32) } as u16 & 0x8000 != 0;
    down(VK_CONTROL) as u8 | (down(VK_MENU) as u8) << 1 | (down(VK_SHIFT) as u8) << 2 | ((down(VK_LWIN) || down(VK_RWIN)) as u8) << 3
}

fn mark(vk: u32, on: bool) {
    let h = HANDLE(HELD[vk as usize].load(Ordering::Relaxed) as _);
    if !h.is_invalid() && !h.0.is_null() {
        unsafe {
            let _ = if on { SetEvent(h) } else { ResetEvent(h) };
        }
    }
}

/// Win+fare kısayolundan sonra Win bırakılınca Başlat ya da Win menüsü açılmasın:
/// araya atanmamış bir tuş girer, Windows ve dock Win'in kullanıldığını anlar.
unsafe fn mask_win() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xFF), dwFlags: flags, ..Default::default() } },
    };
    SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
}

/// Bu olay yutulsun mu? Basışta eşleşen kısayolu tetikler.
unsafe fn handle(vk: u32, down: bool) -> bool {
    let bit = 1u8 << vk;
    if !down {
        let ours = SWALLOWED.fetch_and(!bit, Ordering::SeqCst) & bit != 0;
        if ours {
            mark(vk, false);
        }
        return ours;
    }
    let mods = mods_now();
    let Ok(binds) = BINDS.lock() else { return false };
    let Some(b) = binds.iter().find(|b| b.vk == vk && b.mods == mods).copied() else { return false };
    drop(binds);
    SWALLOWED.fetch_or(bit, Ordering::SeqCst);
    // Önce durum: kısayolu alan süreç tuşun basılı olduğunu hemen görebilsin.
    mark(vk, true);
    if b.mods & 8 != 0 {
        mask_win();
    }
    if b.hwnd != 0 {
        let _ = PostMessageW(HWND(b.hwnd as _), b.msg, WPARAM(b.id as usize), LPARAM(0));
    } else {
        let _ = PostThreadMessageW(b.thread, b.msg, WPARAM(b.id as usize), LPARAM(0));
    }
    true
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
        if let Some((vk, down)) = button(wp.0 as u32, m.mouseData) {
            if handle(vk, down) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe fn run() {
    let mut msg = MSG::default();
    // İleti kuyruğu oluşsun; ancak ondan sonra `THREAD` görünür olur.
    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    for vk in MOUSE_VKS {
        if let Ok(h) = CreateEventW(None, true, false, &event_name(vk)) {
            HELD[vk as usize].store(h.0 as isize, Ordering::Relaxed);
        }
    }
    THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
    let mut hook = HHOOK::default();
    loop {
        let want = BINDS.lock().is_ok_and(|b| !b.is_empty());
        if want && hook.is_invalid() {
            match SetWindowsHookExW(WH_MOUSE_LL, Some(proc), GetModuleHandleW(None).unwrap_or_default(), 0) {
                Ok(h) => hook = h,
                Err(e) => crate::warn!("fare kısayolları dinlenemiyor: {e}"),
            }
        } else if !want && !hook.is_invalid() {
            let _ = UnhookWindowsHookEx(hook);
            hook = HHOOK::default();
            SWALLOWED.store(0, Ordering::SeqCst);
            MOUSE_VKS.iter().for_each(|vk| mark(*vk, false));
        }
        if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
            return;
        }
    }
}

/// Liste değişti: kanca gerekiyorsa kurulsun, gerekmiyorsa kalksın. İş parçacığı
/// henüz açılıyorsa listeyi zaten kendisi okuyacak.
fn sync() {
    STARTED.call_once(|| {
        std::thread::spawn(|| unsafe { run() });
    });
    let t = THREAD.load(Ordering::SeqCst);
    if t != 0 {
        unsafe {
            let _ = PostThreadMessageW(t, WM_SYNC, WPARAM(0), LPARAM(0));
        }
    }
}

fn owner(hwnd: HWND) -> (isize, u32) {
    if hwnd.0.is_null() {
        (0, unsafe { GetCurrentThreadId() })
    } else {
        (hwnd.0 as isize, 0)
    }
}

/// Fare kısayolunu `WM_HOTKEY` yerine verilen iletiyle bildirir (wParam: `id`).
pub fn register_as(hwnd: HWND, id: i32, c: Combo, msg: u32) {
    let (w, thread) = owner(hwnd);
    if let Ok(mut b) = BINDS.lock() {
        b.retain(|b| !(b.hwnd == w && b.thread == thread && b.id == id));
        b.push(Bind { hwnd: w, thread, id, mods: mod_bits(&c), vk: c.vk, msg });
    }
    sync();
}

/// Kısayolu kaydeder; ileti `RegisterHotKey`'deki gibi `WM_HOTKEY` olarak gelir.
/// Pencere boşsa çağıran iş parçacığının kuyruğuna gider.
pub fn register(hwnd: HWND, id: i32, c: Combo) -> Result<()> {
    if c.is_mouse() {
        register_as(hwnd, id, c, WM_HOTKEY);
        return Ok(());
    }
    let mut mods = MOD_NOREPEAT;
    for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
        if on {
            mods |= m;
        }
    }
    unsafe { RegisterHotKey(hwnd, id, mods, c.vk) }
}

/// Kısayolu kaldırır (klavye ya da fare). Kayıtlı değilse bir şey yapmaz.
pub fn unregister(hwnd: HWND, id: i32) {
    unsafe {
        let _ = UnregisterHotKey(hwnd, id);
    }
    let (w, thread) = owner(hwnd);
    let Ok(mut b) = BINDS.lock() else { return };
    let before = b.len();
    b.retain(|b| !(b.hwnd == w && b.thread == thread && b.id == id));
    let changed = b.len() != before;
    drop(b);
    if changed {
        sync();
    }
}

/// Kısayolun ana tuşu şu an basılı mı? Başka süreçten de sorulabilir.
pub fn held(vk: u32) -> bool {
    if vk == 0 {
        return false;
    }
    if !MOUSE_VKS.contains(&vk) {
        return unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    }
    unsafe {
        let Ok(h) = OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &event_name(vk)) else { return false };
        let on = WaitForSingleObject(h, 0) == WAIT_OBJECT_0;
        let _ = CloseHandle(h);
        on
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons() {
        assert_eq!(button(WM_RBUTTONDOWN, 0), Some((0x02, true)));
        assert_eq!(button(WM_MBUTTONUP, 0), Some((0x04, false)));
        assert_eq!(button(WM_XBUTTONDOWN, 1 << 16), Some((0x05, true)));
        assert_eq!(button(WM_XBUTTONUP, 2 << 16), Some((0x06, false)));
        // Sol tuş asla kısayol olmaz: kanca onu görmezden gelir.
        assert_eq!(button(WM_LBUTTONDOWN, 0), None);
        assert_eq!(button(WM_LBUTTONUP, 0), None);
        assert_eq!(button(WM_MOUSEMOVE, 0), None);
    }

    /// Kancayı gerçek bir olayla dener: uçtan uca kayıt, yutma ve basılı durumu.
    #[test]
    #[ignore = "sisteme fare olayı gönderir"]
    fn live() {
        let send = |flags| unsafe {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 { mi: MOUSEINPUT { mouseData: 2, dwFlags: flags, ..Default::default() } },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            std::thread::sleep(std::time::Duration::from_millis(150));
        };
        let mut msg = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        }
        let none = HWND::default();
        register(none, 7, crate::keys::parse_combo("Mouse5").unwrap()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        send(MOUSEEVENTF_XDOWN);
        let on = held(0x06);
        let got = unsafe { PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }.as_bool();
        send(MOUSEEVENTF_XUP);
        let off = !held(0x06);
        unregister(none, 7);
        assert!(got && msg.wParam.0 == 7, "WM_HOTKEY gelmedi");
        assert!(on && off, "basılı durumu yanlış: {on} {off}");
    }
}
