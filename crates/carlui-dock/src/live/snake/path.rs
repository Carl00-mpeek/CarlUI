//! Yılanın yolu ve boncukları (saf geometri, sanat pikseli biriminde).
//!
//! Yerel çerçeve: yılan hep +x'e (sağa) gider, +y aşağı, y = 0 dock'un ilk
//! satırı. Sola giden yılan ekranda aynalanır. Baş bir "iz" üzerinde ilerler
//! (`s` = yay uzunluğu); gövde izin son `LEN` pikselidir ve `STEP` aralıkla
//! boncuklara bölünür. İz düz bir kayma çizgisidir; sarılırken içine dock'un
//! üst kenarını saran bir helezon eklenir. Helezon dünyada sabittir: yılan
//! içinden vida gibi geçer.

use std::f32::consts::{PI, TAU};

/// Boncuk aralığı (yay uzunluğu, px).
pub const STEP: f32 = 2.0;
/// Boncuk sayısı (kuyruk ucu dahil).
pub const BEADS: usize = 80;
/// Gövde uzunluğu (kuyruktan başa).
pub const LEN: f32 = (BEADS - 1) as f32 * STEP;
/// Kayarken gövde merkezinin y'si: alt çizgi y = -1'de, dock'un hemen üstünde.
pub const GLIDE_Y: f32 = -5.0;
/// Helezonun bir turda ilerlediği x.
pub const PITCH: f32 = 21.0;
/// Kayma dalgası: dünyada sabit, bu dalga boyunda ve en çok bu kadar yukarı.
const WAVE_LEN: f32 = 30.0;
const WAVE_AMP: f32 = 5.0;
/// Dalga baş/kuyruk ve helezon kavşağı yakınında bu mesafede söner.
const WAVE_FADE: f32 = 24.0;
/// Kuyruk incelmesi (bkz. `art.rs`): yarıçaplar ve karına doğru kayma.
pub const TAPER_OFF: [f32; 5] = [1.0, 1.0, 2.0, 2.0, 3.0];
const BAND_PERIOD: usize = 7;
const BAND_ON: [usize; 2] = [3, 4];
/// Bakarken yerinden kalkan boyun boncukları.
pub const NECK: usize = 14;
/// Baş kalkınca en çok bu kadar yükselir.
pub const RAISE: f32 = 22.0;

/// Dock'un üst kenarını saran helezon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wrap {
    /// Helezonun başladığı iz konumu.
    pub at: f32,
    /// Tur sayısı (2-3).
    pub loops: f32,
    /// Ray yarıçapı (dikeyde).
    pub rr: f32,
}

impl Wrap {
    fn c(&self) -> f32 {
        (self.rr * self.rr + (PITCH / TAU).powi(2)).sqrt()
    }

    /// Helezonun yay uzunluğu.
    pub fn arc(&self) -> f32 {
        self.loops * TAU * self.c()
    }

    /// Helezonun x'te kapladığı yer.
    pub fn dx(&self) -> f32 {
        self.loops * PITCH
    }

    /// Helezonun ekseni: tepesi kayma çizgisine yatay olarak birleşir.
    fn yc(&self) -> f32 {
        GLIDE_Y + self.rr
    }
}

/// İz üzerinde bir nokta. `z` < 0: dock'un arkasında.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Helezonun tepesinde (karın aşağı bakar) ya da helezonun yanlarında
    /// (sırt görünür) olduğu bilgisi için: -1 tepe, 1 dip.
    pub ring: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Track {
    pub wrap: Option<Wrap>,
}

impl Track {
    pub fn at(&self, s: f32) -> Pt {
        let flat = |x| Pt { x, y: GLIDE_Y, z: 0.0, ring: None };
        match self.wrap {
            Some(w) if s > w.at => {
                let u = s - w.at;
                if u < w.arc() {
                    let t = u / w.c();
                    Pt { x: w.at + PITCH * t / TAU, y: w.yc() - w.rr * t.cos(), z: t.sin(), ring: Some(-t.cos()) }
                } else {
                    flat(s - w.arc() + w.dx())
                }
            }
            _ => flat(s),
        }
    }

    /// `s`'nin helezona uzaklığı (helezon yoksa sonsuz).
    fn helix_dist(&self, s: f32) -> f32 {
        match self.wrap {
            Some(w) if s < w.at => w.at - s,
            Some(w) if s > w.at + w.arc() => s - w.at - w.arc(),
            Some(_) => 0.0,
            None => f32::INFINITY,
        }
    }

    /// Helezon bitmiş (kuyruk `tail` da geçmiş) ise iz düzleştirilir; verilen
    /// iz konumları aynı noktaya denk gelecek biçimde kaydırılır.
    pub fn rebase(&mut self, tail: f32, pos: &mut [&mut f32]) {
        if let Some(w) = self.wrap {
            if tail > w.at + w.arc() {
                let shift = w.dx() - w.arc();
                for p in pos {
                    **p += shift;
                }
                self.wrap = None;
            }
        }
    }
}

/// Gövdenin iz üzerindeki noktaları (kuyruktan başa). `head` başın iz konumu.
pub fn body(track: &Track, head: f32) -> Vec<Pt> {
    (0..BEADS)
        .map(|i| {
            let s = head - (BEADS - 1 - i) as f32 * STEP;
            let mut p = track.at(s);
            if p.ring.is_none() {
                let ends = (i as f32 * STEP).min((BEADS - 1 - i) as f32 * STEP);
                let env = (ends.min(track.helix_dist(s)) / WAVE_FADE).clamp(0.0, 1.0);
                p.y -= WAVE_AMP * env * (0.5 - 0.5 * (TAU * p.x / WAVE_LEN).cos());
            }
            p
        })
        .collect()
}

/// Boynu (son `NECK` boncuk) `a` (0..1) oranında kaldırır: dik bir kavisle
/// başı yukarı taşır.
pub fn raise(pts: &mut [Pt], a: f32) {
    let n = pts.len();
    if n <= NECK + 1 || a <= 0.0 {
        return;
    }
    let b = pts[n - 1 - NECK];
    let len = NECK as f32 * STEP;
    let h = (b.x + len * (1.0 - 0.35 * a), b.y - RAISE * a);
    let c = (b.x + len * 0.65, b.y);
    for j in 1..=NECK {
        let t = j as f32 / NECK as f32;
        let u = 1.0 - t;
        let p = &mut pts[n - 1 - NECK + j];
        p.x = u * u * b.x + 2.0 * u * t * c.0 + t * t * h.0;
        p.y = u * u * b.y + 2.0 * u * t * c.1 + t * t * h.1;
        p.z = 0.0;
        p.ring = None;
    }
}

fn tangent(p: &[Pt], i: usize) -> (f32, f32) {
    let a = p[i.saturating_sub(1)];
    let b = p[(i + 1).min(p.len() - 1)];
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = (dx * dx + dy * dy).sqrt().max(1e-4);
    (dx / l, dy / l)
}

/// Başın yönü (son iki noktadan).
pub fn head_tangent(p: &[Pt]) -> (f32, f32) {
    tangent(p, p.len() - 1)
}

/// 8 yön: 0 sağ, 2 aşağı, 4 sol, 6 yukarı.
pub fn dir_index(n: (f32, f32)) -> u8 {
    let a = n.1.atan2(n.0);
    ((a / (PI / 4.0)).round() as i32).rem_euclid(8) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bead {
    pub x: i32,
    pub y: i32,
    /// 0 gövde, 1..=5 kuyruk ucuna doğru incelir.
    pub size: u8,
    /// Karın yönü (0..8); `None` sırt görünür.
    pub belly: Option<u8>,
    pub band: bool,
    /// Dock'un arkasında.
    pub back: bool,
}

/// İz noktalarından boncuklar (kuyruktan başa).
pub fn beads(p: &[Pt]) -> Vec<Bead> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let t = tangent(p, i);
            let belly = match p[i].ring {
                // Helezonun tepesinde karın aşağıda; yanlarında sırt görünür.
                Some(r) => (r < -0.55).then_some((0.0, 1.0)),
                None => Some((-t.1, t.0)),
            };
            let size = if i < 5 { (5 - i) as u8 } else { 0 };
            let (mut x, mut y) = (p[i].x, p[i].y);
            if let (true, Some(nv)) = (size > 0, belly) {
                let off = TAPER_OFF[size as usize - 1];
                x += nv.0 * off;
                y += nv.1 * off;
            }
            let from_head = n - 1 - i;
            let band = size == 0 && BAND_ON.contains(&(from_head % BAND_PERIOD));
            Bead { x: x.round() as i32, y: y.round() as i32, size, belly: belly.map(dir_index), band, back: p[i].z < -1e-3 }
        })
        .collect()
}

/// Anahtara göre ardışık şeritlere böler: (baş, son, anahtar), `son` dahil değil.
pub fn split<K: PartialEq + Copy>(n: usize, key: impl Fn(usize) -> K) -> Vec<(usize, usize, K)> {
    let mut v = Vec::new();
    let mut a = 0;
    for i in 1..=n {
        if i == n || key(i) != key(a) {
            v.push((a, i, key(a)));
            a = i;
        }
    }
    v
}

/// Baş eğimleri (derece, yukarı pozitif): sanat dosyalarıyla aynı sıra.
pub const PITCHES: [i32; 5] = [-30, 0, 30, 60, 90];

/// Yöne (ekran, y aşağı) en yakın baş eğiminin `PITCHES` sırası.
pub fn pitch_of(t: (f32, f32)) -> usize {
    let a = (-t.1).atan2(t.0).to_degrees();
    (0..PITCHES.len()).min_by(|&i, &j| (PITCHES[i] as f32 - a).abs().total_cmp(&(PITCHES[j] as f32 - a).abs())).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap() -> Wrap {
        Wrap { at: 100.0, loops: 3.0, rr: 11.0 }
    }

    #[test]
    fn duz_iz_ve_govde() {
        let t = Track::default();
        let p = body(&t, 300.0);
        assert_eq!(p.len(), BEADS);
        // Kuyruktan başa sıralı, aralık STEP.
        for w in p.windows(2) {
            assert!((w[1].x - w[0].x - STEP).abs() < 1e-3);
        }
        assert!((p[BEADS - 1].x - 300.0).abs() < 1e-3);
        // Dalga gövdeyi yalnızca yukarı kaldırır; alt çizgi dock'a girmez.
        for q in &p {
            assert!(q.y <= GLIDE_Y + 1e-3 && q.y >= GLIDE_Y - WAVE_AMP - 1e-3, "{q:?}");
        }
        // Baş ve kuyruk ucu dalgasız (zarf sıfır).
        assert!((p[0].y - GLIDE_Y).abs() < 1e-3 && (p[BEADS - 1].y - GLIDE_Y).abs() < 1e-3);
    }

    #[test]
    fn helezon_surekli_ve_kavsaklar_yatay() {
        let t = Track { wrap: Some(wrap()) };
        let w = wrap();
        // İz boyunca ardışık noktalar arası mesafe adımı aşmaz (kopukluk yok).
        let mut prev = t.at(0.0);
        let mut s = 0.0;
        while s < w.at + w.arc() + 50.0 {
            s += 0.5;
            let q = t.at(s);
            let d = ((q.x - prev.x).powi(2) + (q.y - prev.y).powi(2)).sqrt();
            assert!(d <= 0.5 + 1e-3, "s={s} d={d}");
            prev = q;
        }
        // Helezonun tepesi kayma çizgisinde, dibi dock'un içinde.
        assert!((t.at(w.at + 1e-3).y - GLIDE_Y).abs() < 1e-2);
        let bottom = t.at(w.at + w.c() * PI);
        assert!((bottom.y - (GLIDE_Y + 2.0 * w.rr)).abs() < 1e-2 && bottom.z.abs() < 1e-3);
        // Çıkışta x helezonun kapladığı kadar ilerlemiş.
        let out = t.at(w.at + w.arc() + 10.0);
        assert!((out.x - (w.at + w.dx() + 10.0)).abs() < 1e-3 && out.y == GLIDE_Y);
    }

    #[test]
    fn sarilma_on_arka_bolunur() {
        let w = wrap();
        let t = Track { wrap: Some(w) };
        // Baş helezonun içinden çıkmış, gövdenin ortası helezonda.
        let b = beads(&body(&t, w.at + w.arc() * 0.8));
        let strands = split(b.len(), |i| b[i].back);
        // 3 turda arka ve ön şeritler sırayla değişir: en az 2 arka şerit.
        let backs = strands.iter().filter(|s| s.2).count();
        let fronts = strands.iter().filter(|s| !s.2).count();
        assert!(backs >= 2 && fronts >= 3, "{strands:?}");
        // Şeritler kesintisiz ve sıralı.
        assert_eq!(strands[0].0, 0);
        assert_eq!(strands.last().unwrap().1, b.len());
        for w2 in strands.windows(2) {
            assert_eq!(w2[0].1, w2[1].0);
            assert_ne!(w2[0].2, w2[1].2);
        }
        // Arkadaki boncukların hiçbiri helezonun tepesinde değil (sırt görünür,
        // karın yok) ya da tepeye yakın; arkadakiler dock'un altına inmez.
        for bd in b.iter().filter(|b| b.back) {
            assert!(bd.y as f32 <= GLIDE_Y + 2.0 * w.rr + 1.0);
        }
    }

    #[test]
    fn boncuk_sirasi_kuyruktan_basa() {
        let b = beads(&body(&Track::default(), 200.0));
        assert_eq!(b.len(), BEADS);
        // Kuyruk ucu en ince, sonra gövde.
        assert_eq!(b.iter().take(6).map(|b| b.size).collect::<Vec<_>>(), vec![5, 4, 3, 2, 1, 0]);
        assert!(b[5..].iter().all(|b| b.size == 0));
        // Düz giderken karın aşağı (dalga eğimiyle yön 1-3), kimse arkada değil;
        // dalgasız uçlarda tam aşağı (yön 2).
        assert!(b.iter().all(|b| matches!(b.belly, Some(1..=3)) && !b.back), "{b:?}");
        assert_eq!((b[0].belly, b[BEADS - 1].belly), (Some(2), Some(2)));
        // Bant baştan sayılır: baştan 3. ve 4. boncuk koyu.
        assert!(b[BEADS - 4].band && b[BEADS - 5].band && !b[BEADS - 1].band);
        // Kuyruk ucu karına doğru (aşağı) kayar.
        assert_eq!(b[0].y, (GLIDE_Y + TAPER_OFF[4]).round() as i32);
    }

    #[test]
    fn rebase_konumu_korur() {
        let w = wrap();
        let mut t = Track { wrap: Some(w) };
        let mut head = w.at + w.arc() + LEN + 30.0;
        let before = t.at(head);
        let tail = head - LEN;
        t.rebase(tail, &mut [&mut head]);
        assert!(t.wrap.is_none());
        let after = t.at(head);
        assert!((before.x - after.x).abs() < 1e-3);
        // Kuyruk helezondayken düzleşmez.
        let mut t2 = Track { wrap: Some(w) };
        let mut h2 = w.at + w.arc();
        t2.rebase(h2 - LEN, &mut [&mut h2]);
        assert!(t2.wrap.is_some());
    }

    #[test]
    fn boyun_kalkar_ve_bas_egimi() {
        let mut p = body(&Track::default(), 200.0);
        let base = p[BEADS - 1 - NECK];
        raise(&mut p, 1.0);
        let head = p[BEADS - 1];
        assert!((head.y - (base.y - RAISE)).abs() < 1e-3);
        assert!(head.x > base.x);
        // Kavis dikleşerek biter: baş yukarı-ileri bakar.
        let t = head_tangent(&p);
        assert!(t.1 < -0.3);
        assert_eq!(PITCHES[pitch_of((1.0, 0.0))], 0);
        assert_eq!(PITCHES[pitch_of((0.0, -1.0))], 90);
        assert_eq!(PITCHES[pitch_of((1.0, 0.5))], -30);
        assert_eq!(PITCHES[pitch_of((1.0, -0.6))], 30);
        assert_eq!(PITCHES[pitch_of((0.5, -1.0))], 60);
        // Aşağı bakan baş en dik aşağı eğime düşer.
        assert_eq!(PITCHES[pitch_of((0.0, 1.0))], -30);
    }

    #[test]
    fn yon_indeksi() {
        assert_eq!(dir_index((1.0, 0.0)), 0);
        assert_eq!(dir_index((0.0, 1.0)), 2);
        assert_eq!(dir_index((-1.0, 0.0)), 4);
        assert_eq!(dir_index((0.0, -1.0)), 6);
    }
}
