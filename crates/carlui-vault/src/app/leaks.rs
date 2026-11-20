//! Güvenlik sayfasındaki sızıntı denetimi: kasadaki gizli alanlar (parolalar)
//! çözülür, ayrı iş parçacığında `carlui_vault::leak` ile denetlenir; sonuç
//! `WM_APP_WORK` ile arayüze döner. Kasa bu arada kilitlenirse sonuç atılır.

use super::{App, WM_APP_WORK};
use carlui_core::{info, t};
use carlui_vault::{
    entry::FieldKind,
    leak::{self, Candidate, Report},
};
use std::{sync::Mutex, time::Instant};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct LeakState {
    pub busy: bool,
    pub report: Option<(Report, Instant)>,
}

static DONE: Mutex<Option<Report>> = Mutex::new(None);

impl App {
    pub(super) fn leak_start(&mut self) {
        if self.leak.busy {
            return;
        }
        let Some(v) = self.vault.as_ref().filter(|v| v.is_open()) else { return };
        let mut list = Vec::new();
        for it in v.items() {
            let Ok(body) = v.decrypt(&it.id) else { continue };
            for f in body.fields.iter().filter(|f| f.kind == FieldKind::Secret && !f.value.is_empty()) {
                list.push(Candidate { id: it.id.clone(), name: it.meta.name.clone(), password: Zeroizing::new(f.value.clone()) });
            }
        }
        if list.is_empty() {
            self.toast(t!("Denetlenecek parola yok", "No passwords to check"));
            return;
        }
        self.leak.busy = true;
        let h = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let r = leak::check(list);
            if let Ok(mut d) = DONE.lock() {
                *d = Some(r);
            }
            unsafe {
                let _ = PostMessageW(HWND(h as _), WM_APP_WORK, WPARAM(0), LPARAM(0));
            }
        });
    }

    pub fn leak_done(&mut self) {
        let Some(r) = DONE.lock().ok().and_then(|mut d| d.take()) else { return };
        self.leak.busy = false;
        if !self.is_open() {
            return;
        }
        info!("sızıntı denetimi: {} parola, {} sızmış, {} denetlenemedi", r.checked, r.found.len(), r.failed);
        self.leak.report = Some((r, Instant::now()));
    }
}
