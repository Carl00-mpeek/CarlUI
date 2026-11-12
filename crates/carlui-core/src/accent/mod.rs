//! Bu modül "kullanıcı istediği rengi seçsin ama yazı her zaman okunsun"
//! sorununu çözüyor: özel ya da duvar kâğıdından gelen tek bir renkten vurgu
//! degradesinin iki ucunu türetir, aşırı uçları (neredeyse siyah/beyaz)
//! okunaklı bir aralığa çeker. Açık temadaki koyulaştırma `theme::light_rgba`
//! ile aynı yoldan geçer.

pub mod extract;

use crate::theme::{contrast, Rgba};

/// Koyu camın rengi (bkz. `Theme::from_config`); vurgu bunun üstünde görünmeli.
const DARK_GLASS: u32 = 0x0E101C;
/// Koyu cam üstünde vurgunun en az kontrastı.
const MIN_ON_GLASS: f32 = 3.0;
/// Açıklık sınırları: altı camda kaybolur, üstü beyazla karışır.
const L_MIN: f32 = 0.42;
const L_MAX: f32 = 0.72;
/// Bundan az doygun renk gri sayılır (Graphite'ın serin grisi ~0,11).
const GRAY_S: f32 = 0.12;
/// Degradenin ikinci ucu için ton kayması (derece).
const HUE_SHIFT: f32 = 25.0;
/// Vurgu üstündeki koyu yazının rengi.
pub const INK: u32 = 0x111318;

/// "#RRGGBB", "RRGGBB" ya da "#RGB" yazısını 0xRRGGBB'ye çevirir.
pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim();
    let h = s.strip_prefix('#').unwrap_or(s);
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match h.len() {
        6 => u32::from_str_radix(h, 16).ok(),
        3 => {
            let v = u32::from_str_radix(h, 16).ok()?;
            let (r, g, b) = (v >> 8 & 0xF, v >> 4 & 0xF, v & 0xF);
            Some((r * 17) << 16 | (g * 17) << 8 | b * 17)
        }
        _ => None,
    }
}

/// 0xRRGGBB → "#RRGGBB".
pub fn to_hex(c: u32) -> String {
    format!("#{:06X}", c & 0xFF_FFFF)
}

pub fn to_rgba(c: u32) -> Rgba {
    Rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255)
}

pub fn from_rgba(c: Rgba) -> u32 {
    (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32
}

/// 0xRRGGBB → (ton 0..360, doygunluk 0..1, açıklık 0..1).
pub fn rgb_to_hsl(c: u32) -> (f32, f32, f32) {
    let r = (c >> 16 & 0xFF) as f32 / 255.0;
    let g = (c >> 8 & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s.clamp(0.0, 1.0), l)
}

/// (ton, doygunluk, açıklık) → 0xRRGGBB.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> u32 {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let q = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    q(r) << 16 | q(g) << 8 | q(b)
}

/// Rengi koyu cam üstünde okunur olana dek açar (ton ve doygunluk korunur).
fn lift(h: f32, s: f32, mut l: f32) -> u32 {
    let mut c = hsl_to_rgb(h, s, l);
    while contrast(c, DARK_GLASS) < MIN_ON_GLASS && l < L_MAX {
        l = (l + 0.02).min(L_MAX);
        c = hsl_to_rgb(h, s, l);
    }
    c
}

/// Okunaklılık sınırına çekilmiş ana renk (degradenin ilk ucu).
pub fn clamp(c: u32) -> u32 {
    let (h, s, l) = rgb_to_hsl(c);
    lift(h, s, l.clamp(L_MIN, L_MAX))
}

/// Tek renkten koyu temanın vurgu degradesi. Gri renkler için ikinci uç
/// biraz daha koyu bir gri (Graphite paleti gibi); renkliler için ton
/// biraz kaydırılır, böylece degrade palet degradeleri gibi canlı durur.
pub fn derive(c: u32) -> (Rgba, Rgba) {
    let (h, s, l) = rgb_to_hsl(c);
    let l = l.clamp(L_MIN, L_MAX);
    if s < GRAY_S {
        let a = lift(h, s, l.max(0.55));
        let (_, _, la) = rgb_to_hsl(a);
        return (to_rgba(a), to_rgba(lift(h, s, la - 0.15)));
    }
    let a = lift(h, s, l);
    let (_, _, la) = rgb_to_hsl(a);
    let b = lift(h + HUE_SHIFT, s, (la + 0.04).min(L_MAX));
    (to_rgba(a), to_rgba(b))
}

/// Vurgu üstündeki yazının rengi: beyaz ya da koyu mürekkep, hangisi daha okunursa.
pub fn on_accent(c: u32) -> u32 {
    if contrast(c, 0xFFFFFF) >= contrast(c, INK) {
        0xFFFFFF
    } else {
        INK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::light_rgba;

    #[test]
    fn hex_parse_and_roundtrip() {
        assert_eq!(parse_hex("#4F8BFF"), Some(0x4F8BFF));
        assert_eq!(parse_hex("4f8bff"), Some(0x4F8BFF));
        assert_eq!(parse_hex("  #abc "), Some(0xAABBCC));
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#GG0000"), None);
        assert_eq!(parse_hex("#+12345"), None);
        for c in [0x000000, 0xFFFFFF, 0x4F8BFF, 0x0A0B0C] {
            assert_eq!(parse_hex(&to_hex(c)), Some(c));
        }
        assert_eq!(to_hex(0x0a0b0c), "#0A0B0C");
    }

    #[test]
    fn hsl_roundtrip() {
        for c in [0x4F8BFF, 0x10B981, 0xFF0000, 0x808080, 0xC084FC] {
            let (h, s, l) = rgb_to_hsl(c);
            let d = hsl_to_rgb(h, s, l);
            for sh in [16, 8, 0] {
                let (x, y) = ((c >> sh & 0xFF) as i32, (d >> sh & 0xFF) as i32);
                assert!((x - y).abs() <= 1, "{c:06X} → {d:06X}");
            }
        }
    }

    /// Siyah, beyaz, koyu ve soluk renkler okunur bir aralığa çekilmeli:
    /// koyu camda görünür, açık temada beyaz yazı okunur.
    #[test]
    fn extremes_are_clamped_and_legible() {
        for c in [0x000000, 0xFFFFFF, 0x050510, 0xFAFAFF, 0x200000, 0xFFFFE0, 0x0000FF, 0x4F8BFF, 0x808080] {
            let (a, b) = derive(c);
            for x in [a, b] {
                let v = from_rgba(x);
                assert!(contrast(v, DARK_GLASS) >= MIN_ON_GLASS - 0.05, "{c:06X} → {v:06X} koyu camda okunmuyor");
                let (_, _, l) = rgb_to_hsl(v);
                assert!(l <= L_MAX + 0.01, "{c:06X} → {v:06X} fazla açık");
                // Açık temada `light_rgba` beyaz yazıya yetecek kadar koyulaştırır.
                let lv = from_rgba(light_rgba(x));
                assert!(contrast(lv, 0xFFFFFF) >= 4.5, "{c:06X} → {lv:06X} açık temada okunmuyor");
            }
        }
    }

    #[test]
    fn derived_shades_keep_hue() {
        let (a, b) = derive(0x10B981);
        let (ha, _, _) = rgb_to_hsl(from_rgba(a));
        let (hb, _, _) = rgb_to_hsl(from_rgba(b));
        assert!((ha - 160.0).abs() < 4.0, "ton {ha}");
        assert!(((hb - ha).rem_euclid(360.0) - HUE_SHIFT).abs() < 4.0, "kayma {ha} → {hb}");
        // Gri girdi gri kalır, ikinci uç daha koyu.
        let (a, b) = derive(0x9CA3AF);
        let (_, sa, la) = rgb_to_hsl(from_rgba(a));
        let (_, _, lb) = rgb_to_hsl(from_rgba(b));
        assert!(sa < 0.15 && lb < la);
    }

    #[test]
    fn text_on_accent() {
        assert_eq!(on_accent(0x1E3A8A), 0xFFFFFF);
        assert_eq!(on_accent(0xFDE68A), INK);
    }
}
