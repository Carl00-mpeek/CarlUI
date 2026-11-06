//! Bu modül "ekranın yalnızca bir bölümünü videoya almak istiyorum" sorununu
//! çözüyor. Ekran görüntüsü aracında alan seçilip kayıt düğmesine basılınca
//! `carlui-replay.exe --area x,y,g,y` ayrı bir süreç olarak açılır; anlık
//! tekrardan bağımsızdır, ikisi aynı anda çalışabilir.
//!
//! - `grab.rs`: alanı imleçle birlikte okur, ekran kartında MP4'e kodlar, sesi ekler.
//! - `frame.rs`: alanın çevresindeki kırmızı çerçeve ve durdurma kısayolu.
//! - `pill.rs`: süre, boyut, Durdur; bitince Aç / Klasörde göster / GIF yap.
//!
//! Çerçeve ve hap kayda girmez (`WDA_EXCLUDEFROMCAPTURE`), odak almaz. Aynı anda
//! tek alan kaydı olur. Özel tam ekran oyunlar GDI ile okunamaz; onlar için
//! anlık tekrar var.

mod frame;
mod grab;
mod pill;

use carlui_core::{config, log};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex,
    },
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Media::MediaFoundation::*,
        System::{SystemInformation::GetLocalTime, Threading::CreateMutexW},
        UI::{HiDpi::*, WindowsAndMessaging::PostMessageW},
    },
};

/// Kaydedilecek alan: sanal masaüstünde fiziksel piksel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Kodlayıcıların sorunsuz kabul ettiği en küçük kenar.
const MIN_SIDE: u32 = 64;
/// Bundan büyük alan (birkaç ekrana yayılmış) küçültülerek kaydedilir.
const MAX_SIDE: u32 = 4096;

impl Area {
    /// `x,y,g,y` biçimini okur; boyut kodlayıcı için çift sayıya indirilir.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split(',').map(|p| p.trim().parse::<i64>().ok());
        let (x, y, w, h) = (it.next()??, it.next()??, it.next()??, it.next()??);
        if it.next().is_some() || w <= 0 || h <= 0 {
            return None;
        }
        Some(Self { x: i32::try_from(x).ok()?, y: i32::try_from(y).ok()?, w: u32::try_from(w).ok()? & !1, h: u32::try_from(h).ok()? & !1 })
    }

    pub fn big_enough(&self) -> bool {
        self.w >= MIN_SIDE && self.h >= MIN_SIDE
    }

    /// Kayıt boyutu: en uzun kenar sınırı aşıyorsa oran korunarak küçülür.
    pub fn out_size(&self) -> (u32, u32) {
        let long = self.w.max(self.h);
        if long <= MAX_SIDE {
            return (self.w, self.h);
        }
        let fit = |v: u32| ((v as u64 * MAX_SIDE as u64 / long as u64) as u32 & !1).max(2);
        (fit(self.w), fit(self.h))
    }
}

/// Kaydın durumu; işçi yazar, hap gösterir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Recording,
    Saving,
    Done,
    Failed(String),
}

pub struct Shared {
    pub path: PathBuf,
    pub stop: AtomicBool,
    /// Sistem sesi kayda alınamıyor.
    pub silent: AtomicBool,
    phase: Mutex<Phase>,
    pill: AtomicIsize,
}

impl Shared {
    fn new(path: PathBuf) -> Self {
        Self { path, stop: AtomicBool::new(false), silent: AtomicBool::new(false), phase: Mutex::new(Phase::Recording), pill: AtomicIsize::new(0) }
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, p: Phase) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = p;
        self.poke();
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.poke();
    }

    /// Hapı yeniden çizdirir.
    pub fn poke(&self) {
        let h = self.pill.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(HWND(h as _), carlui_ui::view::WM_APP_TICK, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn target(folder: &Path) -> PathBuf {
    let t = unsafe { GetLocalTime() };
    let name = format!("{} {:04}-{:02}-{:02} {:02}.{:02}.{:02}.mp4", t!("Alan kaydı", "Area recording"), t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    folder.join(name)
}

/// `--area` modu. Kayıt bitip hap kapanana dek döner.
pub fn run(spec: &str) -> Result<()> {
    log::init("record");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let Some(area) = Area::parse(spec) else {
        warn!("geçersiz kayıt alanı: {spec}");
        return Ok(());
    };
    if !area.big_enough() {
        crate::note(t!("Alan çok küçük", "Area too small"), &t!(format!("Kayıt için en az {MIN_SIDE}×{MIN_SIDE} piksel seçin."), format!("Select at least {MIN_SIDE}×{MIN_SIDE} pixels to record.")));
        return Ok(());
    }
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\CarlUI.AreaRecord"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            crate::note(t!("Kayıt zaten sürüyor", "Already recording"), t!("Önce açık alan kaydını durdurun.", "Stop the current area recording first."));
            return Ok(());
        }
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let cfg = config::load();
        carlui_ui::tone::follow(&cfg.theme);
        let shared = Arc::new(Shared::new(target(&cfg.replay.folder())));
        info!("alan kaydı başladı: {area:?} → {}", shared.path.display());

        let s = shared.clone();
        let worker = std::thread::Builder::new().name("record-video".into()).spawn(move || grab::run(area, s)).map_err(|_| Error::from(E_FAIL))?;
        let s = shared.clone();
        let combo = cfg.shortcuts.record_stop_combo();
        let ring = std::thread::Builder::new().name("record-frame".into()).spawn(move || frame::run(area, combo, s)).map_err(|_| Error::from(E_FAIL))?;

        let r = pill::run(area, cfg.shortcuts.record_stop.clone(), shared.clone());
        // Hap beklenmedik kapansa da dosya düzgün kapansın.
        shared.stop();
        let _ = worker.join();
        let _ = ring.join();
        let _ = MFShutdown();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_is_parsed_and_made_even() {
        assert_eq!(Area::parse("-1920,10,501,303"), Some(Area { x: -1920, y: 10, w: 500, h: 302 }));
        for bad in ["", "1,2,3", "1,2,3,4,5", "a,2,3,4", "0,0,0,10", "0,0,10,-4"] {
            assert_eq!(Area::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn small_areas_are_refused() {
        assert!(Area { x: 0, y: 0, w: 64, h: 64 }.big_enough());
        assert!(!Area { x: 0, y: 0, w: 62, h: 400 }.big_enough());
    }

    #[test]
    fn huge_areas_keep_their_shape() {
        assert_eq!(Area { x: 0, y: 0, w: 1920, h: 1080 }.out_size(), (1920, 1080));
        let (w, h) = Area { x: 0, y: 0, w: 7680, h: 2160 }.out_size();
        assert_eq!((w, h), (4096, 1152));
        assert_eq!((w % 2, h % 2), (0, 0));
    }
}
