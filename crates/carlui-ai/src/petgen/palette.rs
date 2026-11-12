//! Renk paleti: fotoğraftan gelen yüzlerce tonu medyan kesimle birkaç renge
//! indirir (piksel sanatı görünümü). Küçültme renkleri soldurduğu için
//! önce doygunluk ve karşıtlık biraz artırılır.

use super::sprite::{parts, rgb, Sprite, CLEAR};

/// Renkleri canlandırır: doygunluk ×`sat`, orta griden uzaklaşma ×`con`.
pub fn enhance(s: &mut Sprite, sat: f32, con: f32) {
    for c in s.px.iter_mut().filter(|c| **c != CLEAR) {
        let (r, g, b) = parts(*c);
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let l = 0.299 * r + 0.587 * g + 0.114 * b;
        let f = |v: f32| ((l + (v - l) * sat - 128.0) * con + 128.0).round().clamp(0.0, 255.0) as u8;
        *c = rgb(f(r), f(g), f(b));
    }
}

/// Bir kutudaki renkler (her biri kaç kez geçtiğiyle).
struct Bucket(Vec<(u32, u32)>);

impl Bucket {
    /// En geniş kanal (0 r, 1 g, 2 b) ve genişliği.
    fn widest(&self) -> (usize, u8) {
        let mut best = (0, 0u8);
        for ch in 0..3 {
            let v = self.0.iter().map(|&(c, _)| chan(c, ch));
            let (lo, hi) = v.fold((255u8, 0u8), |(lo, hi), x| (lo.min(x), hi.max(x)));
            let span = hi.saturating_sub(lo);
            if span > best.1 {
                best = (ch, span);
            }
        }
        best
    }

    fn weight(&self) -> u64 {
        self.0.iter().map(|&(_, n)| n as u64).sum()
    }

    fn mean(&self) -> u32 {
        let n = self.weight().max(1);
        let s = |ch| (self.0.iter().map(|&(c, k)| chan(c, ch) as u64 * k as u64).sum::<u64>() / n) as u8;
        rgb(s(0), s(1), s(2))
    }
}

fn chan(c: u32, ch: usize) -> u8 {
    let (r, g, b) = parts(c);
    [r, g, b][ch]
}

/// Algıya yakın uzaklık (yeşile duyarlı).
fn dist(a: u32, b: u32) -> u32 {
    let (ar, ag, ab) = parts(a);
    let (br, bg, bb) = parts(b);
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2) as u32;
    2 * d(ar, br) + 4 * d(ag, bg) + 3 * d(ab, bb)
}

/// Medyan kesim: en çok renk taşıyan ve en geniş kutu ortadan ikiye bölünür.
/// Sonuç en fazla `k` renk (ve sprite'taki farklı renk sayısı).
pub fn median_cut(s: &Sprite, k: usize) -> Vec<u32> {
    let mut hist: Vec<(u32, u32)> = Vec::new();
    let mut sorted: Vec<u32> = s.px.iter().copied().filter(|&c| c != CLEAR).collect();
    sorted.sort_unstable();
    for c in sorted {
        match hist.last_mut() {
            Some((last, n)) if *last == c => *n += 1,
            _ => hist.push((c, 1)),
        }
    }
    if hist.is_empty() || k == 0 {
        return Vec::new();
    }
    let mut buckets = vec![Bucket(hist)];
    while buckets.len() < k {
        // Bölünecek kutu: ağırlık × genişlik en büyük olan.
        let Some((i, ch)) = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.0.len() > 1)
            .map(|(i, b)| (i, b.widest(), b.weight()))
            .filter(|(_, (_, span), _)| *span > 0)
            .max_by_key(|(_, (_, span), w)| *span as u64 * w)
            .map(|(i, (ch, _), _)| (i, ch))
        else {
            break;
        };
        let mut b = buckets.swap_remove(i).0;
        b.sort_unstable_by_key(|&(c, _)| chan(c, ch));
        let half = b.iter().map(|&(_, n)| n as u64).sum::<u64>() / 2;
        let mut acc = 0u64;
        let mut cut = 1;
        for (j, &(_, n)) in b.iter().enumerate() {
            acc += n as u64;
            if acc >= half {
                cut = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(cut);
        buckets.push(Bucket(b));
        buckets.push(Bucket(rest));
    }
    let mut pal: Vec<u32> = buckets.iter().map(Bucket::mean).collect();
    pal.sort_unstable();
    pal.dedup();
    pal
}

/// Her opak pikseli paletteki en yakın renge çeker.
pub fn remap(s: &mut Sprite, pal: &[u32]) {
    if pal.is_empty() {
        return;
    }
    for c in s.px.iter_mut().filter(|c| **c != CLEAR) {
        *c = *pal.iter().min_by_key(|&&p| dist(p, *c)).unwrap();
    }
}

/// Sprite'ı en fazla `k` renge indirir.
pub fn quantise(s: &mut Sprite, k: usize) {
    let pal = median_cut(s, k);
    remap(s, &pal);
}

/// Sprite'taki farklı opak renkler.
pub fn colours(s: &Sprite) -> Vec<u32> {
    let mut v: Vec<u32> = s.px.iter().copied().filter(|&c| c != CLEAR).collect();
    v.sort_unstable();
    v.dedup();
    v
}
