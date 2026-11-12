//! Akvaryum simülasyonu (saf; sanat pikseli birimi, camın sol üstüne göre).
//! Balıklar büyütülmeden (yunusla aynı ölçekte) çizilir: 12-18 px boyla
//! 48 px'lik yunusun ≈%25-38'i. Tek balıklar ≈15-21 px/sn yüzer, hafif
//! sinüs salınımıyla yukarı aşağı oynar ve cam kenarında döner. Küçük
//! tetralar aynı hızla, sıkı bir sürü halinde gezer; her üye kendi kenarında
//! döner, hızlar eşit olduğundan aralıklar korunur. Kabarcıklar ≈12 px/sn yükselir, yüzeyde
//! patlar ve dipte yeniden doğar. Havuz bir kez ayrılır, hiç büyümez.

use crate::live::rng::Rng;

/// Balık türlerinin (sanat sırası: fish_a..fish_e) kare boyu.
pub const KINDS: [(f32, f32); 5] = [(12.0, 8.0), (12.0, 8.0), (12.0, 8.0), (18.0, 10.0), (12.0, 6.0)];
/// Sürü balığı (neon tetra).
pub const SCHOOL_KIND: u8 = 4;
pub const BUBBLE: f32 = 8.0;
const FISH_FRAMES: u8 = 4;
const FISH_MS: f32 = 0.15;
const RISE: f32 = 12.0;
const POP: f32 = 0.15;
/// Kabarcık yüzeye bu kadar kala patlar.
const TOP: f32 = 4.0;
/// Yüzme hızının üst sınırı (px/sn; sürü toparlanırken de aşılmaz).
pub const VMAX: f32 = 21.0;

#[derive(Clone, Copy, Debug)]
pub struct Fish {
    pub x: f32,
    pub y: f32,
    pub dir: f32,
    /// 0..5: hangi balık sanatı (`KINDS`).
    pub kind: u8,
    pub frame: u8,
    base: f32,
    speed: f32,
    phase: f32,
    wobble: f32,
    frame_left: f32,
}

impl Fish {
    pub fn size(&self) -> (f32, f32) {
        KINDS[self.kind as usize % KINDS.len()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bubble {
    pub x: f32,
    pub y: f32,
    /// Patlıyorsa kalan süre.
    pub pop: Option<f32>,
    phase: f32,
}

impl Bubble {
    /// 0 küçük, 1 büyük (yüzeye yaklaşınca), 2 patlama.
    pub fn frame(&self, h: f32) -> u8 {
        match self.pop {
            Some(_) => 2,
            None if self.y < h * 0.5 => 1,
            None => 0,
        }
    }
}

pub struct Sim {
    pub w: f32,
    pub h: f32,
    pub fish: Vec<Fish>,
    pub bubbles: Vec<Bubble>,
    rng: Rng,
}

/// (tek balık, sürü üyesi, kabarcık) sayıları.
pub fn counts(dense: bool) -> (usize, usize, usize) {
    if dense {
        (5, 4, 8)
    } else {
        (3, 3, 5)
    }
}

/// Balığın dikey ekseninin (taban) alabileceği aralık.
fn base_range(h: f32, fh: f32) -> (f32, f32) {
    let lo = fh / 2.0 + 1.0;
    let hi = h - fh / 2.0 - 1.0;
    if hi > lo {
        (lo, hi)
    } else {
        (h / 2.0, h / 2.0 + 0.1)
    }
}

impl Sim {
    pub fn new(w: f32, h: f32, dense: bool, mut rng: Rng) -> Self {
        let (w, h) = (w.max(60.0), h.max(20.0));
        let (singles, school, nb) = counts(dense);
        let mut fish = Vec::with_capacity(singles + school);
        // Tek balıklar: üç küçük ve bir büyük tür sırayla.
        for i in 0..singles {
            let kind = [0u8, 3, 1, 2][i % 4];
            let (fw, fh) = KINDS[kind as usize];
            let (lo, hi) = base_range(h, fh);
            let base = rng.range(lo, hi);
            fish.push(Fish {
                x: rng.range(fw, (w - fw).max(fw + 0.1)),
                y: base,
                dir: if rng.f() < 0.5 { 1.0 } else { -1.0 },
                kind,
                frame: (rng.f() * FISH_FRAMES as f32) as u8 % FISH_FRAMES,
                base,
                speed: rng.range(15.0, VMAX),
                phase: rng.range(0.0, std::f32::consts::TAU),
                wobble: rng.range(1.5, 2.5),
                frame_left: FISH_MS,
            });
        }
        // Sürü: ortak hız ve yön, yakın konum ve derinlik.
        let (fw, fh) = KINDS[SCHOOL_KIND as usize];
        let (lo, hi) = base_range(h, fh + 6.0);
        let (cx, cy) = (rng.range(fw * 2.0, (w - fw * 2.0).max(fw * 2.0 + 0.1)), rng.range(lo, hi));
        let dir = if rng.f() < 0.5 { 1.0 } else { -1.0 };
        let speed = rng.range(16.0, 19.0);
        for i in 0..school {
            let (bl, bh) = base_range(h, fh);
            let base = (cy + [0.0, -3.0, 3.0, -1.0][i % 4]).clamp(bl, bh);
            fish.push(Fish {
                x: (cx - i as f32 * 7.0 * dir).clamp(fw / 2.0, w - fw / 2.0),
                y: base,
                dir,
                kind: SCHOOL_KIND,
                frame: (i % FISH_FRAMES as usize) as u8,
                base,
                speed,
                phase: i as f32 * 1.3,
                wobble: 2.5,
                frame_left: FISH_MS,
            });
        }
        for f in &mut fish {
            f.y = f.base + f.phase.sin();
        }
        let bubbles = (0..nb).map(|_| Bubble { x: rng.range(4.0, w - 4.0), y: rng.range(TOP, h + BUBBLE), pop: None, phase: rng.range(0.0, 6.0) }).collect();
        Self { w, h, fish, bubbles, rng }
    }

    /// Cam boyutu değişti: herkes yeni alana orantılı taşınır.
    pub fn resize(&mut self, w: f32, h: f32) {
        let (w, h) = (w.max(60.0), h.max(20.0));
        if (w - self.w).abs() < 0.5 && (h - self.h).abs() < 0.5 {
            return;
        }
        let (fx, fy) = (w / self.w, h / self.h);
        for f in &mut self.fish {
            let (lo, hi) = base_range(h, f.size().1);
            f.x *= fx;
            f.base = (f.base * fy).clamp(lo, hi);
        }
        for b in &mut self.bubbles {
            b.x *= fx;
            b.y *= fy;
        }
        self.w = w;
        self.h = h;
    }

    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        let (w, h) = (self.w, self.h);
        for f in &mut self.fish {
            let half = f.size().0 / 2.0;
            f.x += f.dir * f.speed * dt;
            if (f.dir > 0.0 && f.x > w - half) || (f.dir < 0.0 && f.x < half) {
                f.x = f.x.clamp(half, w - half);
                f.dir = -f.dir;
            }
            f.phase += f.wobble * dt;
            f.y = f.base + f.phase.sin();
            f.frame_left -= dt;
            while f.frame_left <= 0.0 {
                f.frame_left += FISH_MS;
                f.frame = (f.frame + 1) % FISH_FRAMES;
            }
        }
        for b in &mut self.bubbles {
            match b.pop.as_mut() {
                Some(t) => {
                    *t -= dt;
                    if *t <= 0.0 {
                        // Dipte, yeni bir yerde yeniden doğar.
                        b.pop = None;
                        b.y = h + BUBBLE / 2.0 + self.rng.range(0.0, h * 0.5);
                        b.x = self.rng.range(4.0, (w - 4.0).max(4.1));
                    }
                }
                None => {
                    b.y -= RISE * dt;
                    b.phase += 3.0 * dt;
                    b.x += b.phase.sin() * 2.0 * dt;
                    if b.y <= TOP {
                        b.pop = Some(POP);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baliklar_camda_ve_hizli_degil() {
        for dense in [true, false] {
            let mut s = Sim::new(300.0, 40.0, dense, Rng::new(2));
            let (a, b, n) = counts(dense);
            assert_eq!((s.fish.len(), s.bubbles.len()), (a + b, n));
            let mut turned = false;
            for _ in 0..3000 {
                let before: Vec<f32> = s.fish.iter().map(|f| f.x).collect();
                let dirs: Vec<f32> = s.fish.iter().map(|f| f.dir).collect();
                s.step(0.066);
                for (i, f) in s.fish.iter().enumerate() {
                    let (fw, fh) = f.size();
                    assert!(f.x >= fw / 2.0 - 1e-3 && f.x <= 300.0 - fw / 2.0 + 1e-3);
                    // Balığın tamamı camın içinde (±1 salınım payı).
                    assert!(f.y - fh / 2.0 >= -1e-3 && f.y + fh / 2.0 <= 40.0 + 1e-3, "y={}", f.y);
                    assert!((f.x - before[i]).abs() <= VMAX * 0.066 + 1e-3);
                    turned |= f.dir != dirs[i];
                }
            }
            assert!(turned);
        }
    }

    #[test]
    fn balik_boyu_yunusa_oranli() {
        // Yunus 48 px; balıklar onun %25-40'ı, en az iki farklı boy.
        for (w, _) in KINDS {
            let r = w / 48.0;
            assert!((0.24..=0.4).contains(&r), "{w}");
        }
        let s = Sim::new(300.0, 52.0, true, Rng::new(3));
        let mut boys: Vec<i32> = s.fish.iter().map(|f| f.size().0 as i32).collect();
        boys.dedup();
        boys.sort();
        boys.dedup();
        assert!(boys.len() >= 2);
        // Sürü birlikte yüzer: aynı yön ve hız.
        let school: Vec<&Fish> = s.fish.iter().filter(|f| f.kind == SCHOOL_KIND).collect();
        assert_eq!(school.len(), counts(true).1);
        assert!(school.iter().all(|f| f.dir == school[0].dir && f.speed == school[0].speed));
        // Cam balıktan alçaksa (en az 20 px sayılır) balık ortalanır.
        let s = Sim::new(300.0, 8.0, false, Rng::new(1));
        assert_eq!(s.h, 20.0);
        assert!(s.fish.iter().all(|f| f.y - f.size().1 / 2.0 >= -1e-3 && f.y + f.size().1 / 2.0 <= 20.0 + 1e-3));
    }

    #[test]
    fn kabarciklar_yukselir_ve_patlar() {
        let mut s = Sim::new(200.0, 40.0, false, Rng::new(8));
        let mut pops = 0;
        let mut was: Vec<bool> = s.bubbles.iter().map(|b| b.pop.is_some()).collect();
        for _ in 0..2000 {
            let ys: Vec<f32> = s.bubbles.iter().map(|b| b.y).collect();
            s.step(0.066);
            for (i, b) in s.bubbles.iter().enumerate() {
                if b.pop.is_none() && !was[i] {
                    // Yükselir: ≈12 px/sn.
                    assert!(ys[i] - b.y > 0.0 && ys[i] - b.y <= RISE * 0.066 + 1e-3);
                }
                if b.pop.is_some() && !was[i] {
                    pops += 1;
                    assert!(b.y <= TOP && b.frame(40.0) == 2);
                }
                was[i] = b.pop.is_some();
            }
        }
        assert!(pops > 10);
        // Büyüklük yüzeye yakınken değişir.
        let b = Bubble { x: 0.0, y: 5.0, pop: None, phase: 0.0 };
        assert_eq!(b.frame(40.0), 1);
        assert_eq!(Bubble { y: 35.0, ..b }.frame(40.0), 0);
    }
}
