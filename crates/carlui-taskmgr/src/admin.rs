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

/// Tek kopya kilidi (`Local\CarlUI.TaskMgr`), sahibi ana iş parçacığı.
static INSTANCE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Tek kopya denetimi; `false` dönerse zaten açık olan kopya öne getirilmeli,
/// bu kopya kapanmalı. `replace` (yönetici olarak yeniden açılış): eski kopyanın
/// kilidi bırakması 20 saniyeye kadar beklenir; yine bırakmazsa bu kopya yine de
/// açılır. Eskiden 5 saniyede vazgeçip sessizce kapanıyordu: eski kopya da
/// kapandığı için görev yöneticisi ortadan kayboluyordu.
pub fn single_instance(replace: bool) -> bool {
    use windows::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::Threading::{CreateMutexW, WaitForSingleObject},
    };
    unsafe {
        let m = CreateMutexW(None, true, w!("Local\\CarlUI.TaskMgr"));
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        let Ok(m) = m else { return true };
        INSTANCE.store(m.0 as isize, std::sync::atomic::Ordering::SeqCst);
        if !exists {
            return true;
        }
        if !replace {
            return false;
        }
        let r = WaitForSingleObject(m, 20_000);
        if r != WAIT_OBJECT_0 && r != WAIT_ABANDONED {
            carlui_core::warn!("eski kopya kilidi bırakmadı; yine de açılıyor");
        }
        true
    }
}

/// Kilidi bırakır (yönetici kopyası açılmadan hemen önce, ana iş parçacığında).
pub fn release_instance() {
    let h = INSTANCE.swap(0, std::sync::atomic::Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let h = HANDLE(h as _);
            let _ = windows::Win32::System::Threading::ReleaseMutex(h);
            let _ = CloseHandle(h);
        }
    }
}
