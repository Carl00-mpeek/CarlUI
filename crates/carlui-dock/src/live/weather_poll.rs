//! Gerçek hava için arka plan yoklayıcısı. YALNIZCA dock'ta, yalnızca özellik
//! açıkken ve yer seçiliyken çalışır; ada hiç ağa çıkmaz (dosyadan okur).
//!
//! İlk istek hemen (kayıt yeniyse kalan süre sonra), sonra 20 dakikada bir;
//! hata olursa eski kayıt kalır ve 5 dakika sonra yeniden denenir. Sonuç
//! `weather.json`'a yazılır ve dock penceresine `WM_APP_WEATHER` gönderilir;
//! UI iş parçacığı hiç beklemez. `Poller` bırakılınca (özellik kapandı ya da
//! yer değişti) kanal kapanır ve iş parçacığı yeni istek atmadan çıkar.

use carlui_core::weather::{self, state};
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    time::Duration,
};
use windows::Win32::{
    Foundation::*,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

/// Yeni hava kaydedildi (wParam/lParam kullanılmaz).
pub const WM_APP_WEATHER: u32 = WM_APP + 31;

pub const OK_EVERY: Duration = Duration::from_secs(20 * 60);
pub const RETRY: Duration = Duration::from_secs(5 * 60);

/// İlk beklemeden önce: kayıt yeniyse gereksiz istek atma.
pub fn first_wait(saved_age_secs: Option<u64>) -> Duration {
    match saved_age_secs {
        Some(a) if a < OK_EVERY.as_secs() => OK_EVERY - Duration::from_secs(a),
        _ => Duration::ZERO,
    }
}

/// Yoklayıcının sahibi; bırakılınca iş parçacığı durur.
pub struct Poller {
    _stop: mpsc::Sender<()>,
}

pub fn start(lat: f64, lon: f64, hwnd: HWND) -> Poller {
    let (tx, rx) = mpsc::channel::<()>();
    let raw = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("hava".into()).spawn(move || {
        let mut wait = first_wait(state::load_for(lat, lon).map(|(_, age)| age));
        loop {
            // Zaman aşımı = sıra yoklamada; kanal kapandıysa (ya da veri geldiyse) çık.
            if !matches!(rx.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
                return;
            }
            let got = weather::fetch(lat, lon);
            // İstek sürerken özellik kapanmış ya da yer değişmiş olabilir: yazma.
            if !matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                return;
            }
            wait = match got {
                Some(w) if state::save(&w, lat, lon) => {
                    unsafe {
                        let _ = PostMessageW(HWND(raw as _), WM_APP_WEATHER, WPARAM(0), LPARAM(0));
                    }
                    OK_EVERY
                }
                _ => RETRY,
            };
        }
    });
    Poller { _stop: tx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ilk_bekleme() {
        assert_eq!(first_wait(None), Duration::ZERO);
        assert_eq!(first_wait(Some(0)), OK_EVERY);
        assert_eq!(first_wait(Some(300)), Duration::from_secs(900));
        assert_eq!(first_wait(Some(OK_EVERY.as_secs())), Duration::ZERO);
        assert_eq!(first_wait(Some(10 * 3600)), Duration::ZERO);
    }
}
