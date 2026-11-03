//! "Yönetici olarak çalıştır": bazı süreçler (hizmetler, başka kullanıcıların
//! programları) ancak yönetici yetkisiyle kapatılır ya da değiştirilir. Kenar
//! çubuğundaki düğme görev yöneticisini UAC sorusuyla yeniden açar; kullanıcı
//! onaylarsa yeni (yönetici) kopya açık sekmeyle başlar, bu kopya kapanır.

use windows::{
    core::*,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

/// Bu süreç yönetici yetkisiyle mi çalışıyor.
pub fn is_elevated() -> bool {
    unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(tok, TokenElevation, Some(&mut e as *mut _ as _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
        let _ = CloseHandle(tok);
        ok && e.TokenIsElevated != 0
    }
}

/// Kendini yönetici olarak yeniden açar (`--replace`: eski kopyanın kapanmasını bekler).
/// UAC reddedilirse `false`.
pub fn relaunch(tab: usize) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let (exe_w, params) = (HSTRING::from(exe.as_os_str()), HSTRING::from(format!("--replace --tab {tab}")));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info).is_ok() }
}
