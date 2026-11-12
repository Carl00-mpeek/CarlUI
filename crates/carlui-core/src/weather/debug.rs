//! Hata ayıklama: `CARLUI_LIVE_WEATHER` ile havayı elle belirleme.
//! Biçim: `<gök>[:<şiddet>][,day|night][,thunder]` — örn. `rain:0.8,day`, `clear,night`.

use super::{Sky, Weather};

pub fn debug_override() -> Option<Weather> {
    parse_override(&std::env::var("CARLUI_LIVE_WEATHER").ok()?)
}

pub fn parse_override(s: &str) -> Option<Weather> {
    let mut parts = s.split(',').map(|p| p.trim().to_ascii_lowercase());
    let head = parts.next()?;
    let (name, level) = match head.split_once(':') {
        Some((n, l)) => (n.to_string(), Some(l.trim().parse::<f32>().ok().filter(|v| v.is_finite())?)),
        None => (head, None),
    };
    let sky = match name.as_str() {
        "clear" => Sky::Clear,
        "cloudy" => Sky::Cloudy,
        "rain" => Sky::Rain,
        "snow" => Sky::Snow,
        _ => return None,
    };
    let mut w = Weather { sky, intensity: level.unwrap_or(0.5).clamp(0.0, 1.0), is_day: true, thunder: false };
    for p in parts {
        match p.as_str() {
            "day" => w.is_day = true,
            "night" => w.is_day = false,
            "thunder" => w.thunder = true,
            _ => return None,
        }
    }
    Some(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(sky: Sky, intensity: f32, is_day: bool, thunder: bool) -> Option<Weather> {
        Some(Weather { sky, intensity, is_day, thunder })
    }

    #[test]
    fn examples() {
        assert_eq!(parse_override("rain:0.8,day"), w(Sky::Rain, 0.8, true, false));
        assert_eq!(parse_override("snow:0.3,night"), w(Sky::Snow, 0.3, false, false));
        assert_eq!(parse_override("cloudy"), w(Sky::Cloudy, 0.5, true, false));
        assert_eq!(parse_override("clear,night"), w(Sky::Clear, 0.5, false, false));
        assert_eq!(parse_override("Rain:1,thunder,night"), w(Sky::Rain, 1.0, false, true));
        assert_eq!(parse_override("rain:5"), w(Sky::Rain, 1.0, true, false));
    }

    #[test]
    fn invalid() {
        for s in ["", "fog", "rain:abc", "rain:", "rain,sunny", "rain:NaN", ",day", "rain:0.5:1"] {
            assert_eq!(parse_override(s), None, "{s:?}");
        }
    }
}
