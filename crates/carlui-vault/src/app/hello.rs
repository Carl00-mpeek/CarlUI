//! Pencere tarafında Windows Hello: imza ayrı iş parçacığında beklenir (Hello
//! penceresi açıkken arayüz donmasın), bitince `WM_APP_WORK` ile arayüz
//! iş parçacığına dönülür ve kasa açılır ya da Hello sarmalı kaydedilir.
//! Kullanıcı vazgeçerse sessizce kalınır.

use super::{App, View, WM_APP_WORK};
use carlui_core::{info, t, warn};
use carlui_vault::hello::{self, HelloError};
use std::sync::Mutex;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};
use zeroize::Zeroizing;

struct Job {
    enroll: bool,
    salt: Vec<u8>,
    result: Result<Zeroizing<Vec<u8>>, HelloError>,
}

static DONE: Mutex<Option<Job>> = Mutex::new(None);

fn post(h: isize) {
    unsafe {
        let _ = PostMessageW(HWND(h as _), WM_APP_WORK, WPARAM(0), LPARAM(0));
    }
}

impl App {
    /// Ayarlarda izin var ve bu bilgisayarda Hello kurulu mu (ilk sorguda arka planda yoklanır).
    pub fn hello_supported(&self) -> bool {
        if !self.cfg.vault.windows_hello {
            return false;
        }
        let h = self.hwnd.0 as isize;
        hello::probe(move || post(h));
        hello::available() == Some(true)
    }

    /// Kilit ekranında "Windows Hello ile aç" gösterilsin mi.
    pub fn hello_ready(&self) -> bool {
        self.vault.as_ref().is_some_and(|v| v.has_hello()) && self.hello_supported()
    }

    pub(super) fn hello_start(&mut self, enroll: bool) {
        if self.hello_busy {
            return;
        }
        let Some(v) = self.vault.as_ref() else { return };
        let (salt, challenge) = if enroll {
            let Ok(s) = carlui_vault::crypto::random::<16>() else { return };
            (s.to_vec(), carlui_vault::vault::hello_challenge(&s))
        } else {
            match v.hello_challenge() {
                Some(c) => (Vec::new(), c),
                None => return,
            }
        };
        self.hello_busy = true;
        self.error = None;
        let h = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let result = hello::sign(&challenge, enroll, HWND(h as _)).map(Zeroizing::new);
            if let Ok(mut d) = DONE.lock() {
                *d = Some(Job { enroll, salt, result });
            }
            post(h);
        });
    }

    /// Hello sonucu geldi (ya da destek yoklaması bitti; o zaman yalnızca yeniden çizilir).
    pub fn hello_done(&mut self) {
        let Some(job) = DONE.lock().ok().and_then(|mut d| d.take()) else { return };
        self.hello_busy = false;
        let sig = match job.result {
            Ok(s) => s,
            Err(HelloError::Canceled) => return,
            Err(e) => {
                warn!("windows hello: {e}");
                if job.enroll {
                    self.toast(e.to_string());
                } else {
                    self.error = Some(e.to_string());
                }
                return;
            }
        };
        let Some(v) = self.vault.as_mut() else { return };
        if job.enroll {
            match v.enable_hello(&job.salt, &sig) {
                Ok(()) => {
                    info!("windows hello ile açma etkin");
                    self.toast(t!("Windows Hello ile açma etkin", "Windows Hello unlock is on"));
                }
                Err(e) => self.toast(e.to_string()),
            }
        } else if !v.is_open() {
            match v.unlock_hello(&sig) {
                Ok(()) => {
                    info!("kasa windows hello ile açıldı");
                    self.unlocked();
                }
                Err(e) => {
                    warn!("hello sarmalı açılamadı: {e}");
                    self.error = Some(t!("Windows Hello ile açılamadı; ana parolayı kullanın.", "Couldn't unlock with Windows Hello; use the master password.").into());
                }
            }
        }
    }

    pub(super) fn hello_toggle(&mut self) {
        let Some(v) = self.vault.as_mut() else { return };
        if v.has_hello() {
            match v.disable_hello() {
                Ok(()) => {
                    hello::forget();
                    self.toast(t!("Windows Hello ile açma kapatıldı", "Windows Hello unlock is off"));
                }
                Err(e) => self.toast(e.to_string()),
            }
        } else {
            self.hello_start(true);
        }
    }

    /// Kasa açıldıktan sonra (parola ya da Hello): listeye geç, bekleyenleri işle.
    pub(super) fn unlocked(&mut self) {
        self.throttle.reset(&self.dir);
        self.clear_forms();
        self.go(View::List);
        self.move_mail_logins();
        info!("kasa açıldı ({} kayıt)", self.vault.as_ref().map(|v| v.items().len()).unwrap_or(0));
        self.return_to_browser();
        self.app_unlocked();
    }
}
