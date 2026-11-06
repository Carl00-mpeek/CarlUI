//! Bu modül "asistan tuşunu bırakınca neden saniyelerce bekliyorum?" sorununu
//! çözüyor. Whisper'ı açmak yazıya dökmekten kat kat uzun sürer; bu yüzden
//! `--assist-warm` süreci CarlUI ile başlar ve modeli hazır tutar. Kısayol kısa
//! ömürlü `--assist` sürecini açar, o da buraya haber verip çıkar.
//! Asistan kapalıyken, model yokken ya da tam ekran oyunda model bırakılır; bellek
//! oyuna kalır. Mikrofon yalnızca kısayoldan sonra açılır. Arama açılamazsa iş
//! `--assist --cold` ile eski küçük karta devredilir.

use crate::{
    ai,
    assist::{self, Loader},
    assist_panel::{Link, Panel},
    progress::Shared,
};
use carlui_ai::{
    catalog::Model,
    whisper::{self, Whisper},
};
use carlui_core::{config::Config, info, warn, win};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::JoinHandle,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Threading::*},
        UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY},
    },
};

const ONE: PCWSTR = w!("Local\\CarlUI.AssistWarm");
const GO: PCWSTR = w!("Local\\CarlUI.AssistGo");
/// Ayar, oyun ve model durumuna bu aralıkla bakılır (ms).
const TICK: u32 = 2000;
/// Oyun bu kadar yoklama sürerse model bırakılır, oyun bitince bu kadar yoklama sonra geri yüklenir.
const GAME_TICKS: u32 = 2;
const CLEAR_TICKS: u32 = 15;

pub(crate) type Keep = Arc<Mutex<Option<Whisper>>>;

/// Hazır bekleyen modelin bir konuşmaya verilmesi.
pub(crate) struct Warm {
    pub loader: Loader,
    pub keep: Keep,
}

/// Konuşma nasıl biterse bitsin (iptal, sessizlik) modeli yerine koyar.
pub(crate) struct Held {
    pub loader: Option<Loader>,
    pub keep: Option<Keep>,
    pub halt: Arc<AtomicBool>,
}

impl Held {
    pub fn take(&mut self) -> std::result::Result<Whisper, String> {
        self.loader.take().ok_or_else(|| "model yok".to_string())?.join().unwrap_or_else(|_| Err("panik".into()))
    }

    pub fn warm(&self) -> bool {
        self.keep.is_some()
    }

    pub fn give_back(&self, wh: Whisper) {
        if let Some(k) = &self.keep {
            *k.lock().unwrap_or_else(|e| e.into_inner()) = Some(wh);
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.halt.store(true, Ordering::SeqCst);
        if self.keep.is_some() && self.loader.is_some() {
            if let Ok(wh) = self.take() {
                self.give_back(wh);
            }
        }
    }
}

/// Bekleyen süreç varsa işi ona verir. Asistan açtığı uygulamayı öne getirebilsin
/// diye ön plan hakkı da devredilir.
pub fn hand_over() -> bool {
    unsafe {
        let Ok(go) = OpenEventW(EVENT_MODIFY_STATE, false, GO) else { return false };
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let sent = SetEvent(go).is_ok();
        let _ = CloseHandle(go);
        sent
    }
}

fn key(m: &Model, cfg: &Config) -> String {
    format!("{}:{}", m.id, cfg.ai.gpu)
}

/// Modeli düşük öncelikle açar, boş bir geçişle ilk kullanımın gecikmesini de öder.
fn preload(m: &'static Model, lang: String) -> Loader {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        let mut wh = assist::open(m)?;
        let _ = wh.transcribe(&vec![0.0; whisper::SR], Some(&lang), false, &AtomicBool::new(false), &|_| {});
        Ok(wh)
    })
}

struct Warmer {
    link: Link,
    stop: isize,
    keep: Keep,
    /// `keep` içindeki modelin anahtarı.
    have: String,
    loading: Option<(String, Loader)>,
    /// Açılamayan model her yoklamada yeniden denenmez; kısayol yeniden dener.
    failed: String,
    talk: Option<JoinHandle<()>>,
    game: u32,
    clear: u32,
    resting: bool,
}

impl Warmer {
    fn slot(&self) -> MutexGuard<'_, Option<Whisper>> {
        self.keep.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn watch_game(&mut self) {
        if win::game_in_front() {
            self.clear = 0;
            self.game += 1;
            self.resting |= self.game >= GAME_TICKS;
        } else {
            self.game = 0;
            self.clear += 1;
            self.resting &= self.clear < CLEAR_TICKS;
        }
    }

    fn collect(&mut self) {
        if !self.loading.as_ref().is_some_and(|(_, h)| h.is_finished()) {
            return;
        }
        let Some((k, h)) = self.loading.take() else { return };
        match h.join().unwrap_or_else(|_| Err("panik".into())) {
            Ok(wh) => {
                *self.slot() = Some(wh);
                info!("asistan: model hazır ({k})");
                self.have = k;
            }
            Err(e) => {
                warn!("asistan: model hazırlanamadı: {e}");
                self.failed = k;
            }
        }
    }

    fn tick(&mut self) {
        if self.talk.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        self.watch_game();
        self.collect();
        let cfg = carlui_core::config::load();
        let m = assist::model(&cfg);
        let k = key(m, &cfg);
        let wanted = cfg.ai.assistant && !self.resting && ai::missing_bytes(m) == 0;
        if (!wanted || self.have != k) && self.slot().take().is_some() {
            info!("asistan: model bırakıldı");
            self.have.clear();
        }
        if wanted && self.slot().is_none() && self.loading.is_none() && self.failed != k {
            self.loading = Some((k, preload(m, assist::opts(&cfg, m, false).lang)));
        }
    }

    /// Konuşmaya modeli verir: hazırsa onu, yükleniyorsa yüklemenin kendisini.
    fn loader(&mut self, m: &'static Model, k: &str) -> Loader {
        self.collect();
        let ready = self.slot().take().filter(|_| self.have == k);
        self.have = k.to_string();
        self.failed.clear();
        if let Some(wh) = ready {
            return std::thread::spawn(move || Ok(wh));
        }
        match self.loading.take() {
            Some((lk, h)) if lk == k => h,
            _ => assist::load(m),
        }
    }

    fn press(&mut self) {
        if let Some(t) = self.talk.take() {
            let _ = t.join();
        }
        unsafe {
            let busy = match assist::busy_mutex() {
                Ok(b) => b,
                Err(e) => return warn!("asistan: kilit alınamadı: {e}"),
            };
            let _ = ResetEvent(HANDLE(self.stop as _));
            let cfg = carlui_core::config::load();
            let m = assist::model(&cfg);
            let panel = if ai::missing_bytes(m) == 0 { Panel::on(&self.link) } else { None };
            let Some(p) = panel else { return cold(busy) };
            let k = key(m, &cfg);
            let loader = self.loader(m, &k);
            let (o, keep, stop, busy) = (assist::opts(&cfg, m, false), self.keep.clone(), self.stop, busy.0 as isize);
            self.talk = Some(std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                assist::work(&Shared::new(), &o, HANDLE(stop as _), Some(&p), Some(Warm { loader, keep }));
                drop(p);
                let _ = CloseHandle(HANDLE(busy as _));
            }));
        }
    }
}

/// Arama açılamadı ya da model inecek: işi kendi kartı olan sürece bırakır.
fn cold(busy: HANDLE) {
    unsafe {
        let _ = CloseHandle(busy);
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["--assist", "--cold"]).spawn()) {
        Ok(_) => info!("asistan: kart kipine geçildi"),
        Err(e) => warn!("asistan: başlatılamadı: {e}"),
    }
}

pub fn run() -> Result<()> {
    unsafe {
        let _one = CreateMutexW(None, false, ONE)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let go = CreateEventW(None, false, false, GO)?;
        let stop = assist::stop_event()?;
        let mut me = Warmer {
            link: Link::connect(),
            stop: stop.0 as isize,
            keep: Arc::default(),
            have: String::new(),
            loading: None,
            failed: String::new(),
            talk: None,
            game: 0,
            clear: CLEAR_TICKS,
            resting: false,
        };
        loop {
            me.tick();
            if WaitForSingleObject(go, TICK) == WAIT_OBJECT_0 {
                me.press();
            }
        }
    }
}
