//! Bu modül "vurgu rengi duvar kâğıdından gelsin" sorununu çözüyor. Renk
//! yalnızca burada, ana süreçte (carlui.exe) ve arka plan iş parçacığında
//! hesaplanır; sonuç `theme.wallpaper_accent` olarak ayar dosyasına yazılır,
//! diğer modüller onu zaten izledikleri ayar dosyasından okur.
//!
//! Ne zaman bakılır:
//! - Windows duvar kâğıdı değişti diye yayın yapınca (WM_SETTINGCHANGE /
//!   SPI_SETDESKWALLPAPER) ve ekran düzeni değişince (birincil ekran değişmiş olabilir);
//! - ayarlarda "duvar kâğıdından" seçilince;
//! - seyrek bir yedek kontrolle (slayt gösterisi her geçişte yayın yapmaz):
//!   bu yalnızca dosya yolu, boyutu ve değişme zamanına bakar; resim yalnızca
//!   bunlar değişince çözülür.
//!
//! Okuma (WIC, IDesktopWallpaper) `read.rs`'de.

mod read;

use carlui_core::{
    accent,
    config::{self, AccentSource, ThemeConfig},
    info, warn,
};
use read::Source;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, RecvTimeoutError, Sender},
        Mutex, OnceLock,
    },
    time::Duration,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Yedek kontrol aralığı (slayt gösterisi için).
const POLL: Duration = Duration::from_secs(60);
/// Art arda gelen uyarılar (yayınlar birkaç kez gelir, dosya biraz geç
/// yazılır) bu kadar bekleyip tek seferde işlenir.
const SETTLE: Duration = Duration::from_millis(600);

enum Msg {
    /// Duvar kâğıdı değişmiş olabilir: imza değiştiyse yeniden hesapla.
    Wake,
    /// İmzaya bakmadan yeniden hesapla (mod yeni açıldı).
    Force,
}

/// Ayarda "duvar kâğıdından" seçili mi?
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TX: OnceLock<Sender<Msg>> = OnceLock::new();
/// En son hesaplanıp yazılan değer (ayar dosyasındakiyle karşılaştırmak için).
static LAST: Mutex<Option<String>> = Mutex::new(None);

fn send(m: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(m);
    }
}

fn wanted(t: &ThemeConfig) -> bool {
    t.accent_source == AccentSource::Wallpaper
}

/// İşçi iş parçacığını başlatır (bir kez).
pub fn start(theme: &ThemeConfig) {
    let (tx, rx) = channel();
    if TX.set(tx).is_err() {
        return;
    }
    ACTIVE.store(wanted(theme), Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("wallpaper-accent".into()).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut seen: Option<Source> = None;
        // Açılışta bir kez bak: kapalıyken duvar kâğıdı değişmiş olabilir.
        let mut force = true;
        loop {
            if ACTIVE.load(Ordering::Relaxed) {
                refresh(&mut seen, force);
            }
            force = match rx.recv_timeout(POLL) {
                Ok(m) => {
                    let mut f = matches!(m, Msg::Force);
                    // Kısa süre içinde gelen diğer uyarıları topla.
                    loop {
                        match rx.recv_timeout(SETTLE) {
                            Ok(m) => f |= matches!(m, Msg::Force),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    f
                }
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    });
    if let Err(e) = spawned {
        warn!("duvar kâğıdı iş parçacığı açılamadı: {e}");
    }
}

/// Duvar kâğıdı ya da ekran düzeni değişti (WM_SETTINGCHANGE / WM_DISPLAYCHANGE).
pub fn wake() {
    if ACTIVE.load(Ordering::Relaxed) {
        send(Msg::Wake);
    }
}

/// Ayar değişti: mod açıldıysa hemen hesapla; ayar dosyasındaki değer son
/// hesaplanandan farklıysa (ör. dışarıdan silindi) yeniden yaz.
pub fn sync(theme: &ThemeConfig) {
    let on = wanted(theme);
    let was = ACTIVE.swap(on, Ordering::Relaxed);
    if !on {
        return;
    }
    let stale = LAST.lock().ok().and_then(|l| l.clone()).is_some_and(|l| l != theme.wallpaper_accent);
    if !was || stale {
        send(Msg::Force);
    }
}

/// İmza değiştiyse (ya da `force`) rengi yeniden hesaplar ve gerekirse yazar.
fn refresh(seen: &mut Option<Source>, force: bool) {
    let src = read::current();
    if !force && seen.as_ref() == Some(&src) {
        return;
    }
    let color = match &src {
        Source::Image { path, .. } => read::decode(path).and_then(|(px, w, h)| accent::extract::from_pixels(&px, w, h)),
        // Düz renk arka plan: tek piksellik resim gibi; griyse palete düşülür.
        Source::Solid(c) => {
            let px = [(*c & 0xFF) as u8, (*c >> 8 & 0xFF) as u8, (*c >> 16 & 0xFF) as u8, 255];
            accent::extract::from_pixels(&px, 1, 1)
        }
        Source::Unknown => None,
    };
    let hex = color.map(accent::to_hex).unwrap_or_default();
    info!("duvar kâğıdı rengi: {} ({src:?})", if hex.is_empty() { "yok, palet" } else { &hex });
    *seen = Some(src);
    store(hex);
}

/// Değer ayar dosyasındakinden farklıysa yazar (gereksiz yazma tüm modülleri uyandırır).
fn store(hex: String) {
    if let Ok(mut l) = LAST.lock() {
        *l = Some(hex.clone());
    }
    let Some(cur) = config::load_checked() else { return };
    if cur.theme.wallpaper_accent == hex {
        return;
    }
    let _ = config::update(|c| c.theme.wallpaper_accent = hex);
}
