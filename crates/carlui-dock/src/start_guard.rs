//! Başlat bekçisi: yönetici yetkisiyle çalışan bir pencere öndeyken Windows,
//! klavye kancamıza tuş göstermez; Win tuşu doğrudan Windows'a gider ve
//! menümüz yerine Windows'un Başlat'ı açılır. Bekçi öne gelen pencereyi izler:
//! Başlat, kancanın göremediği bir Win basışıyla bir yönetici penceresinin
//! üstünde açıldıysa kapatıp yerine menümüzü açar.
//!
//! Oyunlara dokunmaz: tam ekranda, özel tam ekranın hemen ardından ve önceki
//! pencere oyun gibiyse hiçbir şey yapılmaz (kısayol tetiklenmez, tuş gönderilmez).

use crate::input;
use carlui_core::info;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::{
    Foundation::*,
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};

/// Başlat'tan önce öndeki (Başlat olmayan) pencere.
static PREV: AtomicIsize = AtomicIsize::new(0);
/// Kanca bu kadar ms tuş görmediyse kör sayılır. Başlat, Win bırakılınca açılır;
/// kanca Win'i görseydi bu süre içinde görürdü.
const QUIET_MS: u32 = 700;
/// Windows Başlat'ı kapandıktan sonra menümüzü açmak için bekleme.
const TIMER_OPEN: usize = 1;

fn is_windows_start(hwnd: HWND) -> bool {
    let exe = carlui_core::win::window_exe(hwnd);
    let name = exe.rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
    name == "startmenuexperiencehost.exe"
}

/// Pencerenin süreci yönetici yetkisiyle mi çalışıyor? Belirtecini okuyamıyorsak
/// (yönetici süreçlerinde normal kullanıcıya erişim verilmez) evet sayılır.
fn elevated(hwnd: HWND) -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return true };
        let mut tok = HANDLE::default();
        let opened = OpenProcessToken(proc, TOKEN_QUERY, &mut tok).is_ok();
        let _ = CloseHandle(proc);
        if !opened {
            return true;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(tok, TokenElevation, Some(&mut e as *mut _ as _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
        let _ = CloseHandle(tok);
        ok && e.TokenIsElevated != 0
    }
}

/// İmleç görev çubuğunun üstünde mi? Oradaki Başlat düğmesine fareyle
/// bilerek tıklanmış olabilir; ona karışılmaz.
fn over_taskbar() -> bool {
    unsafe {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            return false;
        }
        let root = GetAncestor(WindowFromPoint(p), GA_ROOT);
        matches!(carlui_core::win::class_name(root).as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
    }
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    if hwnd.is_invalid() {
        return;
    }
    if !is_windows_start(hwnd) {
        PREV.store(hwnd.0 as isize, Ordering::Relaxed);
        return;
    }
    let prev = HWND(PREV.load(Ordering::Relaxed) as _);
    if prev.is_invalid() || !IsWindow(prev).as_bool() || !input::start_redirect_ok(QUIET_MS) {
        return;
    }
    if carlui_core::win::window_like_game(prev) || !elevated(prev) || over_taskbar() {
        return;
    }
    info!("Windows Başlat'ı yönetici penceresinin üstünde açıldı; yerine Win menüsü açılıyor");
    // Esc Başlat'ı kapatır (Başlat'ın süreci yönetici değil, tuş ona ulaşır).
    let key = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_ESCAPE, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    };
    SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32);
    // Başlat'ın kapanma animasyonu odağı geri verdikten sonra.
    SetTimer(None, TIMER_OPEN, 150, Some(open_menu));
}

unsafe extern "system" fn open_menu(_: HWND, _: u32, id: usize, _: u32) {
    let _ = KillTimer(None, id);
    if !carlui_core::win::toggle_start() {
        let _ = carlui_core::win::toggle_search();
    }
}

/// Bekçiyi kendi iş parçacığında başlatır (olaylar süreç dışından gelir; dock'un
/// ana iş parçacığını meşgul etmez).
pub fn start() {
    let _ = std::thread::Builder::new().name("start-guard".into()).spawn(|| unsafe {
        let hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        if hook.is_invalid() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWinEvent(hook);
    });
}
