//! WMO hava kodu ve ölçümlerden `Weather` üretir (saf eşleme, ağ yok).

use super::{Sky, Weather};

/// Karekök ekseninde parçalı doğrusal eşleme; (miktar, şiddet) noktalarından geçer,
/// 0'dan ilk noktaya ve son noktadan sonra 1.0'a sabit gider.
fn curve(x: f32, pts: &[(f32, f32)]) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let s = x.sqrt();
    let (mut ps, mut pv) = (0.0f32, 0.0f32);
    for &(px, v) in pts {
        let ns = px.sqrt();
        if s <= ns {
            return (pv + (v - pv) * (s - ps) / (ns - ps)).clamp(0.0, 1.0);
        }
        (ps, pv) = (ns, v);
    }
    1.0
}

/// Saatlik yağmur miktarı (mm) -> 0..1: 0.1 -> 0.15, 2.5 -> 0.5, 7.6+ -> 1.0.
fn rain_curve(mm: f32) -> f32 {
    curve(mm, &[(0.1, 0.15), (2.5, 0.5), (7.6, 1.0)])
}

/// Saatlik kar (cm) -> 0..1: 0.1 -> 0.2, 1.0 -> 0.55, 3.0+ -> 1.0.
fn snow_curve(cm: f32) -> f32 {
    curve(cm, &[(0.1, 0.2), (1.0, 0.55), (3.0, 1.0)])
}

/// Kod ailesine göre yağış taban şiddeti ve üst sınırı.
fn rain_family(code: u32) -> (f32, f32) {
    match code {
        51..=57 => (0.15, 0.35),
        61 | 80 => (0.25, 1.0),
        63 | 81 | 66 => (0.5, 1.0),
        65 | 82 | 67 => (0.85, 1.0),
        95..=99 => (0.5, 1.0),
        _ => (0.25, 1.0),
    }
}

fn snow_family(code: u32) -> f32 {
    match code {
        71 | 77 | 85 => 0.25,
        73 => 0.5,
        75 | 86 => 0.85,
        _ => 0.25,
    }
}

pub fn from_readings(code: u32, precip_mm: f32, snowfall_cm: f32, cloud_pct: f32, is_day: bool) -> Weather {
    let precip = if precip_mm.is_finite() { precip_mm.max(0.0) } else { 0.0 };
    let snow = if snowfall_cm.is_finite() { snowfall_cm.max(0.0) } else { 0.0 };
    let cloud = if cloud_pct.is_finite() { (cloud_pct / 100.0).clamp(0.0, 1.0) } else { 0.0 };
    let mk = |sky, intensity: f32, thunder| Weather { sky, intensity: intensity.clamp(0.0, 1.0), is_day, thunder };

    match code {
        51..=57 | 61..=67 | 80..=82 | 95..=99 => {
            let (floor, cap) = rain_family(code);
            let i = if precip > 0.0 { rain_curve(precip).max(0.1) } else { floor };
            mk(Sky::Rain, i.min(cap), (95..=99).contains(&code))
        }
        71..=77 | 85 | 86 => {
            let i = if snow > 0.0 { snow_curve(snow) } else { snow_family(code) };
            mk(Sky::Snow, i, false)
        }
        _ => {
            if snow > 0.0 {
                mk(Sky::Snow, snow_curve(snow), false)
            } else if precip > 0.0 {
                mk(Sky::Rain, rain_curve(precip).max(0.1), false)
            } else {
                let cloudy = matches!(code, 3 | 45 | 48) || (matches!(code, 1 | 2) && cloud_pct >= 60.0);
                mk(if cloudy { Sky::Cloudy } else { Sky::Clear }, cloud, false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sky(code: u32) -> (Sky, bool) {
        let w = from_readings(code, 0.0, 0.0, 0.0, true);
        (w.sky, w.thunder)
    }

    #[test]
    fn code_table() {
        assert_eq!(sky(0), (Sky::Clear, false));
        assert_eq!(sky(1), (Sky::Clear, false));
        assert_eq!(sky(2), (Sky::Clear, false));
        assert_eq!(sky(3), (Sky::Cloudy, false));
        assert_eq!(sky(45), (Sky::Cloudy, false));
        assert_eq!(sky(48), (Sky::Cloudy, false));
        for c in [51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 80, 81, 82] {
            assert_eq!(sky(c), (Sky::Rain, false), "kod {c}");
        }
        for c in [71, 73, 75, 77, 85, 86] {
            assert_eq!(sky(c), (Sky::Snow, false), "kod {c}");
        }
        for c in [95, 96, 99] {
            assert_eq!(sky(c), (Sky::Rain, true), "kod {c}");
        }
    }

    #[test]
    fn partly_cloudy_by_cover() {
        assert_eq!(from_readings(2, 0.0, 0.0, 59.0, true).sky, Sky::Clear);
        assert_eq!(from_readings(2, 0.0, 0.0, 60.0, true).sky, Sky::Cloudy);
    }

    #[test]
    fn dry_code_with_precip() {
        assert_eq!(from_readings(3, 0.0, 0.5, 90.0, false).sky, Sky::Snow);
        assert_eq!(from_readings(3, 0.4, 0.0, 90.0, false).sky, Sky::Rain);
        assert!(!from_readings(3, 0.4, 0.0, 90.0, false).is_day);
    }

    #[test]
    fn rain_curve_points_and_monotonic() {
        let at = |mm| from_readings(63, mm, 0.0, 0.0, true).intensity;
        assert!((at(0.1) - 0.15).abs() < 0.02);
        assert!((at(2.5) - 0.5).abs() < 0.02);
        assert_eq!(at(7.6), 1.0);
        assert_eq!(at(50.0), 1.0);
        let mut prev = 0.0;
        for i in 1..100 {
            let v = at(i as f32 * 0.1);
            assert!(v >= prev && (0.0..=1.0).contains(&v));
            prev = v;
        }
    }

    #[test]
    fn rain_curve_light() {
        let w = from_readings(61, 0.1, 0.0, 0.0, true);
        assert!((w.intensity - 0.15).abs() < 0.03, "{}", w.intensity);
    }

    #[test]
    fn floors_and_drizzle_cap() {
        assert_eq!(from_readings(53, 0.0, 0.0, 0.0, true).intensity, 0.15);
        assert_eq!(from_readings(61, 0.0, 0.0, 0.0, true).intensity, 0.25);
        assert_eq!(from_readings(63, 0.0, 0.0, 0.0, true).intensity, 0.5);
        assert_eq!(from_readings(65, 0.0, 0.0, 0.0, true).intensity, 0.85);
        assert_eq!(from_readings(53, 20.0, 0.0, 0.0, true).intensity, 0.35);
    }

    #[test]
    fn snow_curve_points() {
        let at = |cm| from_readings(73, 0.0, cm, 0.0, true).intensity;
        assert!((at(0.1) - 0.2).abs() < 0.03);
        assert!((at(1.0) - 0.55).abs() < 0.02);
        assert_eq!(at(3.0), 1.0);
        assert_eq!(at(9.0), 1.0);
        assert!(at(0.5) > at(0.2) && at(1.0) > at(0.5));
        assert_eq!(from_readings(75, 0.0, 0.0, 0.0, true).intensity, 0.85);
    }

    #[test]
    fn clear_cloudy_use_cover_and_clamp() {
        assert_eq!(from_readings(3, 0.0, 0.0, 80.0, true).intensity, 0.8);
        assert_eq!(from_readings(0, 0.0, 0.0, 250.0, true).intensity, 1.0);
        assert_eq!(from_readings(0, f32::NAN, f32::NAN, f32::NAN, true).intensity, 0.0);
    }
}
