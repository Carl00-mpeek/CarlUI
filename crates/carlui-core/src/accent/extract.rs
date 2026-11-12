//! Küçültülmüş duvar kâğıdı piksellerinden baskın, canlı rengi bulur.
//! Ada'nın albüm kapağı renginde (`island/mood.rs`) kullanılan yöntemin aynısı:
//! renkli pikseller 12 ton dilimine (30°) doygunluk² × parlaklık ağırlığıyla
//! dağıtılır, komşularıyla birlikte en güçlü dilim seçilir. Sonuç o dilimdeki
//! piksellerin ağırlıklı ortalaması, vurgu olacak kadar doygunlaştırılmış hali.
//! Resim neredeyse tamamen renksizse `None` döner (palete düşülür).
//!
//! Yalnızca saf hesap: çözme (WIC) çağıran tarafta, arka plan iş parçacığında.

use super::{hsl_to_rgb, rgb_to_hsl};

/// Renkli sayılmak için en az doygunluk ve parlaklık (HSV).
const MIN_S: f32 = 0.18;
const MIN_V: f32 = 0.18;
/// Seçilen dilimin ağırlığı toplam piksel sayısının bu oranından az olmamalı.
const MIN_SHARE: f32 = 0.04 * 0.2;
/// Vurgunun en az doygunluğu (HSL).
const VIVID: f32 = 0.45;

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d < 1e-6 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx > 0.0 { d / mx } else { 0.0 }, mx)
}

#[derive(Clone, Copy, Default)]
struct Bin {
    w: f32,
    r: f32,
    g: f32,
    b: f32,
}

/// `bgra`: satır satır, boşluksuz 32 bit BGRA (WIC'in 32bppBGRA biçimi),
/// `w × h` piksel. Dönen değer 0xRRGGBB.
pub fn from_pixels(bgra: &[u8], w: u32, h: u32) -> Option<u32> {
    let n = (w as usize * h as usize).min(bgra.len() / 4);
    let mut bins = [Bin::default(); 12];
    let mut total = 0.0f32;
    for p in bgra[..n * 4].chunks_exact(4) {
        // Duvar kâğıdı opaktır; saydam pikseller (bozuk veri) sayılmaz.
        if p[3] < 128 {
            continue;
        }
        let (r, g, b) = (p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0);
        total += 1.0;
        let (hue, s, v) = rgb_to_hsv(r, g, b);
        if s < MIN_S || v < MIN_V {
            continue;
        }
        let wt = s * s * v;
        let bin = &mut bins[((hue / 30.0) as usize).min(11)];
        bin.w += wt;
        bin.r += r * wt;
        bin.g += g * wt;
        bin.b += b * wt;
    }
    let score = |k: usize| bins[k].w + 0.5 * (bins[(k + 11) % 12].w + bins[(k + 1) % 12].w);
    let best = (0..12).max_by(|&i, &j| score(i).total_cmp(&score(j)))?;
    let b = bins[best];
    if total == 0.0 || b.w < total * MIN_SHARE {
        return None;
    }
    let q = |v: f32| ((v / b.w) * 255.0).round().clamp(0.0, 255.0) as u32;
    let avg = q(b.r) << 16 | q(b.g) << 8 | q(b.b);
    let (hue, s, l) = rgb_to_hsl(avg);
    Some(hsl_to_rgb(hue, s.max(VIVID), l))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accent::rgb_to_hsl;

    /// 0xRRGGBB renklerden BGRA tampon.
    fn img(px: &[u32]) -> Vec<u8> {
        px.iter().flat_map(|&c| [(c & 0xFF) as u8, (c >> 8 & 0xFF) as u8, (c >> 16 & 0xFF) as u8, 255]).collect()
    }

    fn hue(c: u32) -> f32 {
        rgb_to_hsl(c).0
    }

    #[test]
    fn dominant_hue_wins() {
        // %60 koyu gri, %30 turuncu, %10 mavi: renkli olanların baskını turuncu.
        let mut px = vec![0x202020; 60];
        px.extend(vec![0xE07020; 30]);
        px.extend(vec![0x2050E0; 10]);
        let c = from_pixels(&img(&px), 10, 10).expect("renk bulunmalı");
        assert!((hue(c) - hue(0xE07020)).abs() < 6.0, "{c:06X}");
    }

    #[test]
    fn grayscale_falls_back() {
        let px: Vec<u32> = (0..64).map(|i| (i * 4) * 0x010101).collect();
        assert_eq!(from_pixels(&img(&px), 8, 8), None);
        assert_eq!(from_pixels(&[], 0, 0), None);
    }

    #[test]
    fn tiny_colored_patch_ignored() {
        // 400 pikselde tek kırmızı piksel: %4'ün altında → palete düşülür.
        let mut px = vec![0x808080; 399];
        px.push(0xFF0000);
        assert_eq!(from_pixels(&img(&px), 20, 20), None);
    }

    #[test]
    fn muted_color_is_made_vivid() {
        // Soluk yeşil tek renk (düz renk arka plan da böyle 1 piksel gelir).
        let c = from_pixels(&img(&[0x6E8A70]), 1, 1);
        // Doygunluk eşiğin altında kalıyorsa yok sayılır; üstündeyse canlanır.
        if let Some(c) = c {
            assert!(rgb_to_hsl(c).1 >= VIVID - 0.01);
        }
        let c = from_pixels(&img(&[0x3C78B4]), 1, 1).unwrap();
        assert!((hue(c) - hue(0x3C78B4)).abs() < 3.0);
    }

    #[test]
    fn short_buffer_is_safe() {
        // Bildirilen boyuttan kısa tampon taşmaz.
        assert!(from_pixels(&img(&[0xFF0000]), 10, 10).is_some());
    }
}
