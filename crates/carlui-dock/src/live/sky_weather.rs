//! Gerçek hava → sahne. Elle seçilen sahneyi (ve saat kipini) geçersiz kılar.
//! Saf işlev; ağ ve dosya burada yok (bkz. `weather_poll.rs`, `carlui_core::weather::state`).

use super::{
    mix::Mix,
    particles::Kind,
    sky::Resolved,
};
use carlui_core::weather::{Sky, Weather};

/// Şiddet yüzdesi, 5'lik adımlara yuvarlı: küçük dalgalanmalar sahneyi
/// yeniden kurdurmasın (kurulum değişiklikte 2 sn'lik geçişle olur).
fn level(intensity: f32) -> u8 {
    let i = if intensity.is_finite() { intensity.clamp(0.0, 1.0) } else { 0.5 };
    ((i * 20.0).round() as u8) * 5
}

/// `day_night` kapalıysa `is_day` yok sayılır (hep gündüz).
pub fn resolve_weather(w: Weather, day_night: bool) -> Option<Resolved> {
    let night = day_night && !w.is_day;
    let lv = Some(level(w.intensity));
    let r = |kind, tint, mix| Some(Resolved { kind, tint: Some(tint), mix });
    match w.sky {
        // Açık gökte gündüz parçacık yok; gece ay, yıldız ve ateşböcekleri.
        Sky::Clear => night.then_some(Resolved { kind: Kind::Night, tint: Some((24, 32, 80)), mix: Mix::default() }),
        Sky::Cloudy => {
            let tint = if night { (24, 32, 80) } else { (175, 185, 200) };
            r(Kind::Clouds, tint, Mix { level: lv, night, droplets: false })
        }
        Sky::Rain => {
            let tint = if night { (50, 65, 100) } else { (90, 110, 140) };
            // Gündüz yağmuru camdaki damlalar; gece düşen çizgiler.
            r(Kind::Rain, tint, Mix { level: lv, night, droplets: !night })
        }
        Sky::Snow => {
            let tint = if night { (70, 85, 130) } else { (200, 215, 235) };
            r(Kind::Snow, tint, Mix { level: lv, night, droplets: false })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(sky: Sky, intensity: f32, is_day: bool) -> Weather {
        Weather { sky, intensity, is_day, thunder: false }
    }

    #[test]
    fn gokyuzu_sahneye() {
        let r = resolve_weather(w(Sky::Rain, 0.5, true), true).unwrap();
        assert_eq!(r.kind, Kind::Rain);
        assert!(r.mix.droplets && !r.mix.night);
        let r = resolve_weather(w(Sky::Rain, 0.5, false), true).unwrap();
        assert!(!r.mix.droplets && r.mix.night, "gece: çizgi yağmur + ay");
        // Gece/gündüz kapalıyken gece de gündüz sayılır.
        let r = resolve_weather(w(Sky::Rain, 0.5, false), false).unwrap();
        assert!(r.mix.droplets && !r.mix.night);
        assert_eq!(resolve_weather(w(Sky::Snow, 0.5, true), true).unwrap().kind, Kind::Snow);
        assert_eq!(resolve_weather(w(Sky::Cloudy, 0.5, true), true).unwrap().kind, Kind::Clouds);
    }

    #[test]
    fn acik_gok() {
        assert_eq!(resolve_weather(w(Sky::Clear, 0.1, true), true), None);
        assert_eq!(resolve_weather(w(Sky::Clear, 0.1, false), false), None);
        assert_eq!(resolve_weather(w(Sky::Clear, 0.1, false), true).unwrap().kind, Kind::Night);
    }

    #[test]
    fn seviye_adimli() {
        assert_eq!(level(0.2), 20);
        assert_eq!(level(0.23), 25);
        assert_eq!(level(1.5), 100);
        assert_eq!(level(f32::NAN), 50);
        // Küçük dalgalanma aynı çözümü verir.
        assert_eq!(resolve_weather(w(Sky::Rain, 0.61, true), true), resolve_weather(w(Sky::Rain, 0.62, true), true));
    }
}
