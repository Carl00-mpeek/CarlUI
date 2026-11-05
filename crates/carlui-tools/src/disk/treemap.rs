//! Bu modül "binlerce klasörün boyutunu tek bakışta göstermek" sorununu
//! çözüyor: kare oranlı (squarified) ağaç haritası. Her öğe boyutuyla
//! orantılı bir dikdörtgen alır; satırlar en kötü en/boy oranını en küçük
//! tutacak şekilde dizilir, böylece ince uzun şeritler yerine okunabilir
//! kutular çıkar. Saf geometri; test edilir.

use carlui_ui::paint::R;

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (mx, mn) = row.iter().fold((0f64, f64::MAX), |(a, b), &v| (a.max(v), b.min(v)));
    let s2 = side * side;
    let sum2 = sum * sum;
    (s2 * mx / sum2).max(sum2 / (s2 * mn))
}

/// `sizes` büyükten küçüğe sıralı olmalı. Sıfır boyutlular boş dikdörtgen alır.
pub fn layout(sizes: &[u64], r: R) -> Vec<R> {
    let mut out = vec![R::new(r.x, r.y, 0.0, 0.0); sizes.len()];
    let total: u64 = sizes.iter().sum();
    if total == 0 || r.w <= 0.0 || r.h <= 0.0 {
        return out;
    }
    let scale = (r.w as f64 * r.h as f64) / total as f64;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();
    let (mut x, mut y, mut w, mut h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    let mut i = 0;
    let n = areas.iter().position(|&a| a <= 0.0).unwrap_or(areas.len());
    while i < n {
        let side = w.min(h);
        let mut j = i + 1;
        while j < n && worst(&areas[i..=j], side) <= worst(&areas[i..j], side) {
            j += 1;
        }
        let row = &areas[i..j];
        let sum: f64 = row.iter().sum();
        if w >= h {
            // Sol kenara dikey sütun.
            let cw = if h > 0.0 { sum / h } else { 0.0 };
            let mut cy = y;
            for (k, a) in row.iter().enumerate() {
                let ch = if cw > 0.0 { a / cw } else { 0.0 };
                out[i + k] = R::new(x as f32, cy as f32, cw as f32, ch as f32);
                cy += ch;
            }
            x += cw;
            w -= cw;
        } else {
            // Üst kenara yatay satır.
            let rh = if w > 0.0 { sum / w } else { 0.0 };
            let mut cx = x;
            for (k, a) in row.iter().enumerate() {
                let cw = if rh > 0.0 { a / rh } else { 0.0 };
                out[i + k] = R::new(cx as f32, y as f32, cw as f32, rh as f32);
                cx += cw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_and_inside() {
        let sizes = [600u64, 300, 200, 100, 50, 25, 0];
        let r = R::new(10.0, 20.0, 400.0, 300.0);
        let rects = layout(&sizes, r);
        let total: u64 = sizes.iter().sum();
        for (s, q) in sizes.iter().zip(&rects) {
            let want = *s as f32 / total as f32 * r.w * r.h;
            assert!((q.w * q.h - want).abs() < 1.0, "{s}: {} vs {want}", q.w * q.h);
            if *s > 0 {
                assert!(q.x >= r.x - 0.01 && q.y >= r.y - 0.01 && q.right() <= r.right() + 0.01 && q.bottom() <= r.bottom() + 0.01);
            }
        }
        // Kare oranlı: en büyük kutu çok ince olmamalı.
        let q = rects[0];
        assert!(q.w.max(q.h) / q.w.min(q.h) < 3.0);
        assert!(layout(&[], r).is_empty());
        assert_eq!(layout(&[0, 0], r)[0].w, 0.0);
    }
}
