//! CPU yönlendirme:
//! - Güç modu: Windows'un Ayarlar > Güç'teki "En iyi güç verimliliği /
//!   Dengeli / En iyi performans" seçeneği (powrprof "overlay" şeması). Bu,
//!   zamanlayıcının işleri P mi E çekirdeklerine mi koyacağını ve frekans
//!   tavanını belirler.
//! - Süreç başına çekirdek ataması: Windows CPU Set'leri. Bir sürece yalnızca
//!   P ya da yalnızca E çekirdekleri verilebilir (yönetici gerekmez, kendi
//!   süreçlerimiz için).
//! - Verimlilik modu (EcoQoS): süreç düşük öncelikli, E çekirdeğine yatkın ve
//!   düşük frekansta çalışır; Windows Görev Yöneticisi'ndeki yaprak simgesi.

use carlui_sys::cpu::{cpu_sets, Kind};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Threading::*,
        },
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Saver,
    Balanced,
    Performance,
}

const SAVER: GUID = GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a);
const PERF: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);

type GetOverlay = unsafe extern "system" fn(*mut GUID) -> u32;
type SetOverlay = unsafe extern "system" fn(GUID) -> u32;

fn powrprof(name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe { GetProcAddress(LoadLibraryW(w!("powrprof.dll")).ok()?, name) }
}

pub fn mode() -> Mode {
    let Some(f) = powrprof(s!("PowerGetEffectiveOverlayScheme")) else { return Mode::Balanced };
    let f: GetOverlay = unsafe { std::mem::transmute(f) };
    let mut g = GUID::zeroed();
    unsafe { f(&mut g) };
    match g {
        SAVER => Mode::Saver,
        PERF => Mode::Performance,
        _ => Mode::Balanced,
    }
}

pub fn set_mode(m: Mode) -> bool {
    let Some(f) = powrprof(s!("PowerSetActiveOverlayScheme")) else { return false };
    let f: SetOverlay = unsafe { std::mem::transmute(f) };
    let g = match m {
        Mode::Saver => SAVER,
        Mode::Balanced => GUID::zeroed(),
        Mode::Performance => PERF,
    };
    unsafe { f(g) == 0 }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    All,
    Performance,
    Efficient,
    Custom,
    /// Okunamadı (erişim yok).
    Unknown,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Route::All => t!("Tümü", "All"),
            Route::Performance => t!("P çekirdekleri", "P-cores"),
            Route::Efficient => t!("E çekirdekleri", "E-cores"),
            Route::Custom => t!("Özel", "Custom"),
            Route::Unknown => "—",
        }
    }
}

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<HANDLE> {
    unsafe { OpenProcess(access, false, pid).ok() }
}

pub fn route(pid: u32) -> Route {
    let Some(h) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else { return Route::Unknown };
    let mut ids = [0u32; 256];
    let mut n = 0u32;
    let ok = unsafe { GetProcessDefaultCpuSets(h, Some(&mut ids), &mut n) }.as_bool();
    unsafe {
        let _ = CloseHandle(h);
    }
    if !ok {
        return Route::Unknown;
    }
    if n == 0 {
        return Route::All;
    }
    let sets = cpu_sets();
    let mine: Vec<Kind> = ids[..n as usize].iter().filter_map(|id| sets.iter().find(|s| s.id == *id).map(|s| s.kind)).collect();
    let p = sets.iter().filter(|s| s.kind == Kind::Performance).count();
    let e = sets.len() - p;
    if mine.len() == sets.len() {
        Route::All
    } else if mine.iter().all(|k| *k == Kind::Performance) && mine.len() == p {
        Route::Performance
    } else if mine.iter().all(|k| *k == Kind::Efficient) && mine.len() == e {
        Route::Efficient
    } else {
        Route::Custom
    }
}

pub fn set_route(pid: u32, r: Route) -> bool {
    let sets = cpu_sets();
    let ids: Vec<u32> = match r {
        Route::Performance => sets.iter().filter(|s| s.kind == Kind::Performance).map(|s| s.id).collect(),
        Route::Efficient => sets.iter().filter(|s| s.kind == Kind::Efficient).map(|s| s.id).collect(),
        _ => Vec::new(),
    };
    if r != Route::All && ids.is_empty() {
        return false;
    }
    let Some(h) = open(pid, PROCESS_SET_LIMITED_INFORMATION) else { return false };
    let ok = unsafe { SetProcessDefaultCpuSets(h, if ids.is_empty() { None } else { Some(&ids) }) }.as_bool();
    unsafe {
        let _ = CloseHandle(h);
    }
    ok
}

pub fn eco(pid: u32) -> Option<bool> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut st = PROCESS_POWER_THROTTLING_STATE { Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION, ..Default::default() };
    let ok = unsafe { GetProcessInformation(h, ProcessPowerThrottling, &mut st as *mut _ as *mut _, std::mem::size_of_val(&st) as u32) }.is_ok();
    unsafe {
        let _ = CloseHandle(h);
    }
    ok.then_some(st.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0 && st.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0)
}

pub fn set_eco(pid: u32, on: bool) -> bool {
    let Some(h) = open(pid, PROCESS_SET_INFORMATION) else { return false };
    let st = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
    };
    let ok = unsafe { SetProcessInformation(h, ProcessPowerThrottling, &st as *const _ as *const _, std::mem::size_of_val(&st) as u32) }.is_ok();
    // Verimlilik modu Windows'ta düşük öncelikle birlikte gelir.
    unsafe {
        let _ = SetPriorityClass(h, if on { IDLE_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS });
        let _ = CloseHandle(h);
    }
    ok
}
