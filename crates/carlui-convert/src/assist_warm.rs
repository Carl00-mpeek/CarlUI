//! Bu modül "asistan tuşuna basınca arama neden hemen açılmıyor?" sorununu
//! çözüyor. `--assist-warm` süreci CarlUI ile başlar ve adaya bağlı bekler;
//! kısayol kısa ömürlü `--assist` sürecini açar, o da buraya haber verip çıkar.
//! Burada hiçbir model hazır beklemez: Whisper tuşa basınca açılır, yazıya
//! dökünce kapanır; dil modeli de yalnızca o komut için açılır (bkz. `assist`).
//! Mikrofon yalnızca kısayoldan sonra açılır. Arama açılamazsa iş
//! `--assist --cold` ile eski küçük karta devredilir.

use crate::{
    ai, assist,
    assist_panel::{Link, Panel},
    progress::Shared,
};
use carlui_core::{info, warn};
use std::thread::JoinHandle;
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

struct Waiter {
    link: Link,
    stop: isize,
    talk: Option<JoinHandle<()>>,
}

impl Waiter {
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
            let (o, stop, busy) = (assist::opts(&cfg, m, false), self.stop, busy.0 as isize);
            self.talk = Some(std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                assist::work(&Shared::new(), &o, HANDLE(stop as _), Some(&p), true);
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
        let mut me = Waiter { link: Link::connect(), stop: stop.0 as isize, talk: None };
        loop {
            if WaitForSingleObject(go, INFINITE) == WAIT_OBJECT_0 {
                me.press();
            }
        }
    }
}
