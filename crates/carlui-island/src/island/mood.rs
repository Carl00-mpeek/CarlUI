//! Bu modül "şarkı çalarken ada kapağın rengini alsın" sorununu çözüyor: kapak
//! 24×24'e küçültülüp renk tonlarına göre oylanır, baskın canlı ton seçilir;
//! cam bu tonla hafifçe boyanır ve vurgu çubukları aynı tonu alır.
//! Ayar: `[island] media_tint`.

use super::*;

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

pub(super) fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

/// Kapağın baskın, canlı rengi (HSV). Görsel 24×24'e küçültülür; renk tonları
/// 12 dilime ayrılır, doygun ve parlak pikseller daha çok oy verir. Gri/siyah
/// kapaklarda None döner (ada nötr kalır).
fn dominant_color(gfx: &Gfx, bytes: &[u8]) -> Option<(f32, f32, f32)> {
    let (conv, _, _) = decode(gfx, bytes)?;
    const N: u32 = 24;
    let mut px = vec![0u8; (N * N * 4) as usize];
    unsafe {
        let scaler = gfx.wic.CreateBitmapScaler().ok()?;
        scaler.Initialize(&conv, N, N, WICBitmapInterpolationModeFant).ok()?;
        scaler.CopyPixels(std::ptr::null(), N * 4, &mut px).ok()?;
    }
    let mut bins = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 12]; // ağırlık, ton x, ton y, s
    let mut total = 0.0;
    for p in px.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        if a < 0.5 {
            continue;
        }
        let (h, s, v) = rgb_to_hsv(p[2] as f32 / 255.0 / a, p[1] as f32 / 255.0 / a, p[0] as f32 / 255.0 / a);
        total += 1.0;
        if s < 0.18 || v < 0.18 {
            continue;
        }
        let w = s * s * v;
        let b = &mut bins[((h / 30.0) as usize).min(11)];
        let rad = h.to_radians();
        b.0 += w;
        b.1 += rad.cos() * w;
        b.2 += rad.sin() * w;
        b.3 += s * w;
    }
    // Komşu dilimlerle birlikte en güçlü bölge.
    let best = (0..12).max_by(|&i, &j| {
        let sc = |k: usize| bins[k].0 + 0.5 * (bins[(k + 11) % 12].0 + bins[(k + 1) % 12].0);
        sc(i).total_cmp(&sc(j))
    })?;
    let b = bins[best];
    // Kapağın en az ~%4'ü renkli olmalı.
    if total == 0.0 || b.0 < total * 0.04 * 0.2 {
        return None;
    }
    let h = b.2.atan2(b.1).to_degrees().rem_euclid(360.0);
    Some((h, (b.3 / b.0).clamp(0.0, 1.0), 1.0))
}

impl Island {
    /// Çalan şarkının kapağına göre adanın rengini ayarlar; `None` nötre döner.
    /// Dönen değer: renk değişti mi (içerik yeniden çizilmeli mi)?
    pub fn set_mood(&mut self, gfx: &Gfx, art: Option<&std::sync::Arc<Vec<u8>>>) -> bool {
        let key = art.map(|a| std::sync::Arc::as_ptr(a) as usize).unwrap_or(0);
        if self.mood.map(|m| m.0) == Some(key) || (key == 0 && self.mood.is_none()) {
            return false;
        }
        let hs = art.and_then(|a| dominant_color(gfx, a)).map(|(h, s, _)| (h, s));
        let old = self.mood.and_then(|m| m.1);
        self.mood = if key == 0 { None } else { Some((key, hs)) };
        let tint = hs.map(|(h, s)| {
            let (r, g, b) = hsv_to_rgb(h, s.clamp(0.45, 0.8), 0.62);
            Rgba(r, g, b, 0x66)
        });
        let _ = self.glass.set_mood(&self.comp, tint, 600);
        old != hs
    }
}
