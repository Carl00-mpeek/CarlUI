//! Open-Meteo yanıtlarını ayrıştırır; bozuk girdide panik yok, `None`/boş döner.

use super::{map::from_readings, Place, Weather};
use serde_json::Value;

pub fn parse_forecast(json: &str) -> Option<Weather> {
    let v: Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current")?;
    let code = cur.get("weather_code")?.as_f64()?;
    if !(0.0..=1000.0).contains(&code) {
        return None;
    }
    let num = |k: &str| cur.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let is_day = cur.get("is_day").and_then(Value::as_f64).map(|d| d >= 0.5).unwrap_or(true);
    Some(from_readings(code as u32, num("precipitation"), num("snowfall"), num("cloud_cover"), is_day))
}

pub fn parse_places(json: &str) -> Vec<Place> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let Some(list) = v.get("results").and_then(Value::as_array) else { return Vec::new() };
    let text = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    list.iter()
        .filter_map(|r| {
            let lat = r.get("latitude")?.as_f64()?;
            let lon = r.get("longitude")?.as_f64()?;
            let name = text(r, "name");
            if name.is_empty() || !lat.is_finite() || !lon.is_finite() {
                return None;
            }
            Some(Place { name, region: text(r, "admin1"), country: text(r, "country"), lat, lon })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::Sky;

    const FORECAST: &str = r#"{"latitude":51.5,"longitude":-0.12,"generationtime_ms":0.05,"utc_offset_seconds":0,
      "timezone":"Europe/London","timezone_abbreviation":"GMT","elevation":23.0,
      "current_units":{"time":"iso8601","interval":"seconds","weather_code":"wmo code","precipitation":"mm","snowfall":"cm","cloud_cover":"%","is_day":""},
      "current":{"time":"2026-11-12T10:00","interval":900,"weather_code":63,"precipitation":2.5,"snowfall":0.0,"cloud_cover":100,"is_day":0}}"#;

    const PLACES: &str = r#"{"results":[
      {"id":2643743,"name":"London","latitude":51.50853,"longitude":-0.12574,"country":"United Kingdom","admin1":"England"},
      {"id":1,"name":"Atlantis","latitude":10.5,"longitude":-20.25,"country":"Nowhere"},
      {"id":2,"name":"Bad","latitude":"x","longitude":1.0}]}"#;

    #[test]
    fn forecast_sample() {
        let w = parse_forecast(FORECAST).unwrap();
        assert_eq!(w.sky, Sky::Rain);
        assert!(!w.is_day && !w.thunder);
        assert!((w.intensity - 0.5).abs() < 0.02);
    }

    #[test]
    fn places_sample() {
        let p = parse_places(PLACES);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "London");
        assert_eq!(p[0].region, "England");
        assert_eq!(p[0].country, "United Kingdom");
        assert_eq!(p[1].region, "");
        assert_eq!(p[1].lat, 10.5);
    }

    #[test]
    fn garbage() {
        for s in ["", "{", "null", "[]", "{\"current\":5}", "{\"current\":{}}", "{\"current\":{\"weather_code\":\"x\"}}", "\u{0}\u{1}"] {
            assert!(parse_forecast(s).is_none(), "{s:?}");
            assert!(parse_places(s).is_empty(), "{s:?}");
        }
        assert!(parse_places("{\"results\":5}").is_empty());
        assert!(parse_places("{}").is_empty());
    }
}
