//! Kartın gerçek hava sahnesi. Ada ağa ÇIKMAZ: hava dock'un yazdığı küçük
//! dosyadan okunur (`carlui_core::weather::state`). Dock'taki
//! `live/sky_weather.rs` ile aynı eşleme, ama kart küçük olduğu için gece
//! ay/yıldız katmanı yağışın altına eklenmez: geceleyin yağış kendi
//! koyulaşmış tonuyla, açık/bulutlu gece ise gece sahnesiyle çizilir.

use super::sim::Kind;
use carlui_core::weather::{Sky, Weather};

pub fn resolve(w: Weather, day_night: bool) -> Option<(Kind, (u8, u8, u8))> {
    let night = day_night && !w.is_day;
    Some(match w.sky {
        Sky::Clear if night => (Kind::Night, (24, 32, 80)),
        Sky::Clear => return None,
        Sky::Cloudy if night => (Kind::Night, (24, 32, 80)),
        Sky::Cloudy => (Kind::Clouds, (175, 185, 200)),
        Sky::Rain => (Kind::Rain, if night { (50, 65, 100) } else { (90, 110, 140) }),
        Sky::Snow => (Kind::Snow, if night { (70, 85, 130) } else { (200, 215, 235) }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(sky: Sky, is_day: bool) -> Weather {
        Weather { sky, intensity: 0.5, is_day, thunder: false }
    }

    #[test]
    fn eslesme() {
        assert_eq!(resolve(w(Sky::Rain, true), true).unwrap().0, Kind::Rain);
        assert_eq!(resolve(w(Sky::Snow, false), true).unwrap().0, Kind::Snow);
        assert_eq!(resolve(w(Sky::Cloudy, true), true).unwrap().0, Kind::Clouds);
        assert_eq!(resolve(w(Sky::Cloudy, false), true).unwrap().0, Kind::Night);
        assert_eq!(resolve(w(Sky::Cloudy, false), false).unwrap().0, Kind::Clouds, "gece/gündüz kapalı: hep gündüz");
        assert!(resolve(w(Sky::Clear, true), true).is_none());
        assert_eq!(resolve(w(Sky::Clear, false), true).unwrap().0, Kind::Night);
    }
}
