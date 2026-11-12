//! Dock'taki evcil hayvana küçük işaretler (bkz. dock `live/react.rs`).
//!
//! Bus'a yalnızca olayın adı gider: bildirimin uygulaması, başlığı ya da
//! metni, çalan parçanın adı ASLA gönderilmez. Müzik için tek bir bool
//! (çalıyor mu) gider ve yalnızca değiştiğinde.

use carlui_core::bus::Event;

/// Ekranda gösterilen yeni bildirim.
pub const NOTE: &str = "pet-note";
/// Medya çalmaya başladı / durdu (`data.playing`).
pub const MUSIC: &str = "pet-music";

/// Bildirim işareti: içerik yok.
pub fn note() -> Event {
    Event::Custom { name: NOTE.into(), data: serde_json::Value::Null }
}

/// Çalma durumu değiştiyse gönderilecek olay; değişmediyse `None`.
/// `last` son gönderilen durumu tutar.
pub fn music(last: &mut Option<bool>, playing: bool) -> Option<Event> {
    if *last == Some(playing) {
        return None;
    }
    *last = Some(playing);
    Some(Event::Custom { name: MUSIC.into(), data: serde_json::json!({ "playing": playing }) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bildirim_icerik_tasimaz() {
        let s = serde_json::to_string(&note()).unwrap();
        assert!(s.contains(NOTE));
        let Event::Custom { data, .. } = note() else { panic!() };
        assert!(data.is_null());
    }

    #[test]
    fn muzik_yalniz_degisince() {
        let mut last = None;
        assert!(music(&mut last, false).is_some());
        assert!(music(&mut last, false).is_none());
        let Some(Event::Custom { data, .. }) = music(&mut last, true) else { panic!() };
        assert_eq!(data, serde_json::json!({ "playing": true }));
        assert!(music(&mut last, true).is_none());
    }
}
