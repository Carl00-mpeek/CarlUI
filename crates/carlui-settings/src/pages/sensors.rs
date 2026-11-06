//! Bu modül "PawnIO, CarlUI'den sonra kurulunca CPU sıcaklığı nasıl açılır?"
//! sorununu çözüyor. Görevi normalde kurucu kaydeder; o an sürücü yoksa bir
//! daha kaydeden olmaz. Buradaki satır durumu gösterir, gerekirse yönetici
//! izniyle `carlui-sensors.exe --install` çalıştırır.

use crate::{
    ui::{Flow, P},
    Act, App,
};
use carlui_sys::temp;
use carlui_ui::paint::R;
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Threading::WaitForSingleObject,
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_HIDE,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Live,
    NotIntel,
    NoDriver,
    Ready,
}

fn state(live: bool, intel: bool, driver: bool) -> State {
    match (live, intel, driver) {
        (true, ..) => State::Live,
        (_, false, _) => State::NotIntel,
        (_, _, false) => State::NoDriver,
        _ => State::Ready,
    }
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let st = state(temp::read().is_some(), temp::intel(), temp::driver_installed());
    let label = t!("CPU sıcaklığı okuyucusu", "CPU temperature reader");
    let text = match st {
        State::Live => t!("Çalışıyor.", "Running."),
        State::NotIntel => t!("Şimdilik yalnızca Intel işlemcilerde çalışır.", "Only works on Intel CPUs for now."),
        State::NoDriver => t!("PawnIO sürücüsü gerekir; kurduktan sonra buradan aç.", "Needs the PawnIO driver; turn this on here after installing it."),
        State::Ready => t!("PawnIO kurulu, okuyucu çalışmıyor. Bir kez yönetici izni ister.", "PawnIO is installed but the reader isn't running. Asks for admin rights once."),
    };
    // Canlıysa eski bir hata iletisi kalmasın.
    let desc = app.sensor_note.as_deref().filter(|_| st != State::Live).unwrap_or(text);
    let button = match st {
        State::Ready => Some(('\u{E7EF}', t!("Kur", "Set up"), Act::SensorsSetup)),
        State::NoDriver => Some(('\u{E8A7}', t!("PawnIO", "PawnIO"), Act::SensorsDriver)),
        _ => None,
    };
    let r = f.row(p, label, desc, if button.is_some() { 120.0 } else { 0.0 });
    if let Some((icon, name, act)) = button {
        p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some(icon), name, act, true, false);
    }
}

/// Görevi kurar; sıcaklık gelmezse gösterilecek iletiyi döndürür.
pub fn setup(owner: HWND) -> Option<String> {
    let file = HSTRING::from(carlui_core::paths::sibling_exe("carlui-sensors.exe").as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: owner,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: w!("--install"),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            return Some(if e.code() == ERROR_CANCELLED.to_hresult() { t!("Yönetici izni verilmedi.", "Admin rights were declined.").to_string() } else { e.message().to_string() });
        }
        if !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, 30_000);
            let _ = CloseHandle(info.hProcess);
        }
    }
    // Görev başlayıp ilk ölçümü yazana kadar kısa bir süre tanı.
    let end = Instant::now() + Duration::from_secs(4);
    while Instant::now() < end {
        if temp::read().is_some() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Some(t!("Görev kuruldu ama sıcaklık gelmedi; sensors günlüğüne bakın.", "The task was set up but no temperature arrived; check the sensors log.").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_follows_what_is_missing() {
        assert_eq!(state(true, false, false), State::Live);
        assert_eq!(state(false, false, true), State::NotIntel);
        assert_eq!(state(false, true, false), State::NoDriver);
        assert_eq!(state(false, true, true), State::Ready);
    }
}
