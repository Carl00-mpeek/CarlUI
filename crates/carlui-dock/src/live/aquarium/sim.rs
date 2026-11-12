//! Akvaryum simülasyonu (saf; sanat pikseli birimi, camın sol üstüne göre).
//! Balıklar ≈20 px/sn yüzer, hafif sinüs salınımıyla yukarı aşağı oynar ve
//! cam kenarında döner. Kabarcıklar ≈12 px/sn yükselir, yüzeyde patlar ve
//! dipte yeniden doğar. Havuz bir kez ayrılır, hiç büyümez.

use crate::live::rng::Rng;

pub const FISH: (f32, f32) = (12.0, 8.0);
pub const BUBBLE: f32 = 8.0;
const FISH_FRAMES: u8 = 4;
const FISH_MS: f32 = 0.15;
const RISE: f32 = 12.0;
const POP: f32 = 0.15;
/// Kabarcık yüzeye bu kadar kala patlar.
const TOP: f32 = 4.0;

#[derive(Clone, Copy, Debug)]
pub struct Fish {
    pub x: f32,
    pub y: f32,
    pub dir: f32,
    /// 0..3: hangi balık sanatı.
    pub kind: u8,
    pub frame: u8,
    base: f32,
    speed: f32,
    phase: f32,
    wobble: f32,
    frame_left: f32,
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
    /// Balık büyütmesi (tam sayı; sanat pikseli keskin kalsın). Balık dock
    /// yüksekliğinin ~%60'ı kadar olsun diye `zoom_for` ile seçilir.
    pub z: f32,
    pub fish: Vec<Fish>,
    pub bubbles: Vec<Bubble>,
    rng: Rng,
}

/// Cam yüksekliğine (sanat pikseli) göre balık büyütmesi: balık boyu ≈ %62.
pub fn zoom_for(h: f32) -> f32 {
    (h * 0.62 / FISH.1).floor().clamp(1.0, 6.0)
}

impl Sim {
    /// Balığın (genişlik, yükseklik) sanat pikseli.
    pub fn fish_size(&self) -> (f32, f32) {
        (FISH.0 * self.z, FISH.1 * self.z)
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

    pub fn new(w: f32, h: f32, dense: bool, z: f32, mut rng: Rng) -> Self {
        let z = z.max(1.0);
        let (fw, fh) = (FISH.0 * z, FISH.1 * z);
        let (w, h) = (w.max(fw * 3.0), h.max(FISH.1 * 2.0));
        let (nf, nb) = if dense { (5, 8) } else { (3, 5) };
        let (lo, hi) = Self::base_range(h, fh);
        // Büyük balık biraz daha hızlı yüzer (boyuna göre ağır görünmesin).
        let sp = 1.0 + (z - 1.0) * 0.35;
        let fish = (0..nf)
            .map(|i| {
                let base = rng.range(lo, hi);
                let mut f = Fish {
                    x: rng.range(fw, (w - fw).max(fw + 0.1)),
                    y: base,
                    dir: if rng.f() < 0.5 { 1.0 } else { -1.0 },
                    kind: (i % 3) as u8,
                    frame: (rng.f() * FISH_FRAMES as f32) as u8 % FISH_FRAMES,
                    base,
                    speed: rng.range(17.0, 23.0) * sp,
                    phase: rng.range(0.0, std::f32::consts::TAU),
                    wobble: rng.range(1.5, 2.5),
                    frame_left: FISH_MS,
                };
                f.y = f.base + f.phase.sin();
                f
            })
            .collect();
        let bubbles = (0..nb).map(|_| Bubble { x: rng.range(4.0, w - 4.0), y: rng.range(TOP, h + BUBBLE), pop: None, phase: rng.range(0.0, 6.0) }).collect();
        Self { w, h, z, fish, bubbles, rng }
    }

    /// Cam boyutu değişti: herkes yeni alana orantılı taşınır.
    pub fn resize(&mut self, w: f32, h: f32) {
        let (fw, fh) = self.fish_size();
        let (w, h) = (w.max(fw * 3.0), h.max(FISH.1 * 2.0));
        if (w - self.w).abs() < 0.5 && (h - self.h).abs() < 0.5 {
            return;
        }
        let (fx, fy) = (w / self.w, h / self.h);
        let (lo, hi) = Self::base_range(h, fh);
        for f in &mut self.fish {
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
        let half = self.fish_size().0 / 2.0;
        for f in &mut self.fish {
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
        for z in [1.0, 3.0] {
            let mut s = Sim::new(300.0, 40.0, true, z, Rng::new(2));
            assert_eq!((s.fish.len(), s.bubbles.len()), (5, 8));
            assert_eq!(Sim::new(300.0, 40.0, false, z, Rng::new(2)).fish.len(), 3);
            let (fw, fh) = s.fish_size();
            let vmax = 23.0 * (1.0 + (z - 1.0) * 0.35);
            let mut turned = false;
            for _ in 0..3000 {
                let before: Vec<f32> = s.fish.iter().map(|f| f.x).collect();
                let dirs: Vec<f32> = s.fish.iter().map(|f| f.dir).collect();
                s.step(0.066);
                for (i, f) in s.fish.iter().enumerate() {
                    assert!(f.x >= fw / 2.0 - 1e-3 && f.x <= 300.0 - fw / 2.0 + 1e-3);
                    // Balığın tamamı camın içinde (±1 salınım payı).
                    assert!(f.y - fh / 2.0 >= -1e-3 && f.y + fh / 2.0 <= 40.0 + 1e-3, "z={z} y={}", f.y);
                    assert!((f.x - before[i]).abs() <= vmax * 0.066 + 1e-3);
                    turned |= f.dir != dirs[i];
                }
            }
            assert!(turned);
        }
    }

    #[test]
    fn balik_boyu_camin_yuzde_altmisi() {
        // 52 px cam, ölçek 1: balık 8·z ≈ %60.
        let z = zoom_for(52.0);
        assert_eq!(z, 4.0);
        assert!((FISH.1 * z / 52.0) > 0.55 && (FISH.1 * z / 52.0) < 0.7);
        // Küçük camda en az 1, dev camda sınırlı.
        assert_eq!(zoom_for(5.0), 1.0);
        assert_eq!(zoom_for(1000.0), 6.0);
        // Cam balıktan alçaksa balık ortalanır.
        let s = Sim::new(300.0, 20.0, false, 4.0, Rng::new(1));
        assert!(s.fish.iter().all(|f| (f.y - 10.0).abs() <= 1.2));
    }

    #[test]
    fn kabarciklar_yukselir_ve_patlar() {
        let mut s = Sim::new(200.0, 40.0, false, 1.0, Rng::new(8));
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
