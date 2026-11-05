//! Stüdyonun arka plan işinin (dinleme, kaydetme, kayıt, yazıya dökme)
//! durumu. İş parçacıkları buraya yazar ve pencereyi uyandırır; pencere her
//! çizimde okur.

use crate::{ai, progress};
use carlui_ai::{catalog::Model, whisper::Segment};
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Arc, Mutex,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Work {
    #[default]
    Idle,
    Preparing,
    Playing,
    Saving,
    Recording,
    Transcribing,
}

#[derive(Default)]
pub struct State {
    pub work: Work,
    pub note: String,
    pub failed: bool,
    pub frac: f32,
    /// Mikrofon seviyesi (0–1) ve kayıt süresi (sn).
    pub level: f32,
    pub seconds: f32,
    /// Kaydedilen dosya ("Klasörde göster" için).
    pub saved: Option<String>,
    /// Yazıya dökme sonucu; pencere alıp metin alanına koyar.
    pub result: Option<(String, Vec<Segment>)>,
}

pub struct Job {
    pub state: Mutex<State>,
    /// İşi iptal eder.
    pub stop: Arc<AtomicBool>,
    /// Mikrofon kaydını bitirir (yazıya dökme sürer).
    pub end_rec: AtomicBool,
    hwnd: AtomicIsize,
}

impl Job {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { state: Mutex::default(), stop: Arc::new(AtomicBool::new(false)), end_rec: AtomicBool::new(false), hwnd: AtomicIsize::new(0) })
    }

    pub fn set_hwnd(&self, h: HWND) {
        self.hwnd.store(h.0 as isize, Ordering::SeqCst);
    }

    /// Pencereyi yeniden çizmeye uyandırır.
    pub fn poke(&self) {
        let h = self.hwnd.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(HWND(h as _), carlui_ui::view::WM_APP_TICK, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn update(&self, f: impl FnOnce(&mut State)) {
        if let Ok(mut s) = self.state.lock() {
            f(&mut s);
        }
        self.poke();
    }

    pub fn work(&self) -> Work {
        self.state.lock().map(|s| s.work).unwrap_or_default()
    }

    /// Yeni iş başlatır; başka iş sürüyorsa false.
    pub fn begin(&self, w: Work, note: String) -> bool {
        let Ok(mut s) = self.state.lock() else { return false };
        if s.work != Work::Idle {
            return false;
        }
        self.stop.store(false, Ordering::SeqCst);
        self.end_rec.store(false, Ordering::SeqCst);
        *s = State { work: w, note, ..Default::default() };
        true
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn fail(&self, e: String) {
        let stopped = self.stopped();
        self.update(|s| {
            s.work = Work::Idle;
            s.failed = !stopped;
            s.note = if stopped { t!("Durduruldu", "Stopped").into() } else { e };
            s.frac = 0.0;
        });
    }

    pub fn finish(&self, note: String, saved: Option<String>) {
        self.update(|s| {
            s.work = Work::Idle;
            s.note = note;
            s.frac = 1.0;
            s.saved = saved;
        });
    }

    /// Model (ve motor) eksikse indirir; ilerleme bu işin durumunda görünür.
    /// Onay önceden (pencerede) alınmış olmalı.
    pub fn prepare(&self, m: &'static Model) -> Result<(), String> {
        if ai::missing_bytes(m) == 0 {
            return Ok(());
        }
        let before = self.work();
        self.update(|s| s.work = Work::Preparing);
        let ps = progress::Shared::new();
        let worker = ps.clone();
        let h = std::thread::spawn(move || ai::ensure(&worker, m));
        while !h.is_finished() {
            if self.stopped() {
                ps.cancel.store(true, Ordering::SeqCst);
            }
            if let Ok(p) = ps.state.lock() {
                let (title, frac) = (p.title.clone(), p.frac);
                self.update(|s| {
                    s.note = title;
                    s.frac = frac;
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        let ok = h.join().unwrap_or(false);
        if !ok || ai::missing_bytes(m) > 0 {
            let e = ps.state.lock().map(|p| p.title.clone()).unwrap_or_default();
            return Err(if e.is_empty() { t!("Model indirilemedi", "Couldn't download the model").into() } else { e });
        }
        self.update(|s| {
            s.work = before;
            s.frac = 0.0;
        });
        Ok(())
    }
}
