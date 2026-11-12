//! Bildirim kartının sahne parçacıkları. Dock'taki `carlui-dock/src/live/particles.rs`
//! ve `rng.rs` dosyalarının aynasıdır (dock ayrı bir ikili, ortak kütüphane değil;
//! orada bir şey değişirse burası da gözden geçirilmeli). Akvaryum yok: küçük
//! karta sığmıyor.
//!
//! Havuz kurulurken bir kez ayrılır, sonra büyümez; ölen parçacık yerinde yeniden
//! doğar. Burada yalnızca konum/boyut/saydamlık hesaplanır; çizim `fx.rs`'de.

/// Küçük, belirlenimci rastgele sayı üreteci (xorshift32).
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    pub fn from_time() -> Self {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(1);
        Self::new(n)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// [0, 1) aralığında.
    fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// [a, b) aralığında.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rain,
    Snow,
    Night,
    Clouds,
}

impl Kind {
    /// Kare aralığı (ms): yağmur ≈20 fps, kar ≈15, gece ≈12, bulut 8.
    pub fn interval_ms(self) -> u32 {
        match self {
            Kind::Rain => 50,
            Kind::Snow => 66,
            Kind::Night => 83,
            Kind::Clouds => 125,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sprite {
    Drop,
    Streak,
    Flake,
    Star,
    Firefly,
    Cloud,
    Moon,
}

/// Tek parçacık. `x`, `y` merkez; `r` yarıçap (piksel); `len` dikey uzama
/// çarpanı (yağmur çizgisi); `a` saydamlık 0..1.
#[derive(Clone, Copy, Debug)]
pub struct P {
    pub sprite: Sprite,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub len: f32,
    pub a: f32,
    vx: f32,
    vy: f32,
    goal: f32,
    phase: f32,
    sliding: bool,
}

impl P {
    fn new(sprite: Sprite) -> Self {
        Self { sprite, x: 0.0, y: 0.0, r: 1.0, len: 1.0, a: 0.0, vx: 0.0, vy: 0.0, goal: 0.0, phase: 0.0, sliding: false }
    }
}

pub struct Sim {
    pub kind: Kind,
    w: f32,
    h: f32,
    s: f32,
    pub pool: Vec<P>,
    rng: Rng,
    t: f32,
}

/// Bir adımda en çok bu kadar saniye ilerlenir.
const MAX_DT: f32 = 0.25;
/// Ayın en büyük yarıçapı (ölçek 1'de piksel).
const MOON_R: f32 = 14.0;

impl Sim {
    pub fn new(kind: Kind, w: f32, h: f32, s: f32, dense: bool, rng: Rng) -> Self {
        let counts: &[(Sprite, usize, usize)] = match kind {
            Kind::Rain => &[(Sprite::Drop, 6, 10), (Sprite::Streak, 24, 40)],
            Kind::Snow => &[(Sprite::Flake, 26, 44)],
            // Sıra çizim sırasıdır: ay yıldızların önünde, ateşböceklerinin arkasında.
            Kind::Night => &[(Sprite::Star, 14, 24), (Sprite::Moon, 1, 1), (Sprite::Firefly, 4, 7)],
            Kind::Clouds => &[(Sprite::Cloud, 4, 6)],
        };
        let mut pool = Vec::new();
        for &(sp, light, heavy) in counts {
            for _ in 0..if dense { heavy } else { light } {
                pool.push(P::new(sp));
            }
        }
        let mut sim = Self { kind, w: w.max(1.0), h: h.max(1.0), s: s.max(0.5), pool, rng, t: 0.0 };
        for i in 0..sim.pool.len() {
            sim.spawn(i, true);
        }
        sim
    }

    pub fn margin(&self) -> f32 {
        self.max_r() * 3.0 + 4.0
    }

    fn max_r(&self) -> f32 {
        match self.kind {
            Kind::Clouds => self.h * 1.2,
            Kind::Night => MOON_R * self.s,
            _ => 6.0 * self.s,
        }
    }

    fn moon_r(&self) -> f32 {
        (self.w.min(self.h) * 0.28).clamp(1.0, MOON_R * self.s)
    }

    fn spawn(&mut self, i: usize, first: bool) {
        let (w, h, s) = (self.w, self.h, self.s);
        let moon_r = self.moon_r();
        let r = &mut self.rng;
        let p = &mut self.pool[i];
        p.sliding = false;
        p.vx = 0.0;
        p.vy = 0.0;
        p.len = 1.0;
        match p.sprite {
            Sprite::Drop => {
                p.x = r.range(0.0, w);
                p.y = r.range(0.0, h * 0.8);
                p.r = r.range(0.6, 1.2) * s;
                p.goal = r.range(2.2, 3.6) * s;
                p.a = 0.0;
                p.phase = r.range(0.0, 6.28);
            }
            Sprite::Streak => {
                p.x = r.range(0.0, w);
                p.r = r.range(1.2, 1.8) * s;
                p.len = r.range(6.0, 10.0);
                p.y = if first { r.range(0.0, h) } else { -p.r * p.len - r.range(0.0, h * 0.3) };
                p.vy = r.range(170.0, 260.0) * s;
                p.vx = p.vy * 0.06;
                p.a = r.range(0.7, 1.0);
            }
            Sprite::Flake => {
                p.x = r.range(0.0, w);
                p.r = r.range(1.6, 3.2) * s;
                p.y = if first { r.range(0.0, h) } else { -p.r - r.range(0.0, h * 0.5) };
                p.vy = (8.0 + p.r / s * 6.0) * s;
                p.goal = r.range(4.0, 10.0) * s;
                p.phase = r.range(0.0, 6.28);
                p.a = r.range(0.7, 1.0);
            }
            Sprite::Moon => {
                // Sağda: solda uygulama simgesi var. Hiç hareket etmez.
                p.r = moon_r;
                p.x = w * 0.86;
                p.y = h * 0.5;
                p.a = 0.95;
            }
            Sprite::Star => {
                p.x = r.range(0.0, w);
                p.y = r.range(0.0, h);
                p.r = r.range(1.0, 2.0) * s;
                p.goal = r.range(0.6, 1.0);
                p.phase = r.range(0.0, 6.28);
                p.vx = r.range(0.6, 2.0);
                p.a = p.goal;
            }
            Sprite::Firefly => {
                p.x = r.range(0.0, w);
                p.y = r.range(h * 0.2, h * 0.9);
                p.r = r.range(1.2, 2.0) * s;
                p.vx = r.range(-6.0, 6.0) * s;
                p.vy = r.range(-3.0, 3.0) * s;
                p.phase = r.range(0.0, 6.28);
                p.a = 0.0;
            }
            Sprite::Cloud => {
                p.r = r.range(0.6, 1.1) * h;
                p.x = if first { r.range(0.0, w) } else { -p.r };
                p.y = r.range(0.15, 0.6) * h;
                p.vx = r.range(3.0, 8.0) * s;
                p.goal = r.range(0.25, 0.45);
                p.a = p.goal;
            }
        }
    }

    /// Simülasyonu `dt` saniye ilerletir.
    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_DT) } else { 0.0 };
        if dt == 0.0 {
            return;
        }
        self.t = (self.t + dt) % 3600.0;
        let (w, h, s, t) = (self.w, self.h, self.s, self.t);
        let mut respawn: [u8; 64] = [0; 64];
        let mut nr = 0;
        for i in 0..self.pool.len() {
            let p = &mut self.pool[i];
            let dead = match p.sprite {
                Sprite::Drop => {
                    if p.sliding {
                        p.vy = (p.vy + 140.0 * s * dt).min(110.0 * s);
                        p.vx = (p.vx + self.rng.range(-30.0, 30.0) * s * dt).clamp(-6.0 * s, 6.0 * s);
                        p.x += p.vx * dt;
                        p.y += p.vy * dt;
                        p.r = (p.r - 0.15 * s * dt).max(0.8 * s);
                        p.len = (1.0 + p.vy / (110.0 * s) * 0.6).min(1.6);
                    } else {
                        p.r += 0.35 * s * dt;
                        p.a = (p.a + dt * 0.8).min(0.85);
                        if p.r >= p.goal {
                            p.sliding = true;
                            p.vy = 6.0 * s;
                        }
                    }
                    p.y - p.r * p.len > h
                }
                Sprite::Streak => {
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    p.y - p.r * p.len > h || p.x - p.r > w
                }
                Sprite::Flake => {
                    p.y += p.vy * dt;
                    p.x += (t * 1.3 + p.phase).sin() * p.goal * dt;
                    if p.x < -p.r {
                        p.x += w + 2.0 * p.r;
                    } else if p.x > w + p.r {
                        p.x -= w + 2.0 * p.r;
                    }
                    p.y - p.r > h
                }
                Sprite::Star => {
                    p.a = p.goal * (0.65 + 0.35 * (t * p.vx + p.phase).sin());
                    false
                }
                Sprite::Moon => false,
                Sprite::Firefly => {
                    p.vx = (p.vx + self.rng.range(-12.0, 12.0) * s * dt).clamp(-8.0 * s, 8.0 * s);
                    p.vy = (p.vy + self.rng.range(-8.0, 8.0) * s * dt).clamp(-4.0 * s, 4.0 * s);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    if p.x < 0.0 || p.x > w {
                        p.vx = -p.vx;
                        p.x = p.x.clamp(0.0, w);
                    }
                    if p.y < 0.0 || p.y > h {
                        p.vy = -p.vy;
                        p.y = p.y.clamp(0.0, h);
                    }
                    p.a = (0.5 + 0.5 * (t * 1.7 + p.phase).sin()).powi(2) * 0.9;
                    false
                }
                Sprite::Cloud => {
                    p.x += p.vx * dt;
                    p.x - p.r > w
                }
            };
            if dead && nr < respawn.len() {
                respawn[nr] = i as u8;
                nr += 1;
            }
        }
        if self.kind == Kind::Rain {
            self.merge_drops(&mut respawn, &mut nr);
        }
        for k in 0..nr {
            self.spawn(respawn[k] as usize, false);
        }
        self.keep_in_bounds();
    }

    /// Kayan damla yolundaki damlayı yutar; yutulan yeniden doğar.
    fn merge_drops(&mut self, respawn: &mut [u8; 64], nr: &mut usize) {
        let n = self.pool.len();
        let cap = 4.5 * self.s;
        for i in 0..n {
            if self.pool[i].sprite != Sprite::Drop || !self.pool[i].sliding {
                continue;
            }
            for j in 0..n {
                if i == j || self.pool[j].sprite != Sprite::Drop || respawn[..*nr].contains(&(j as u8)) {
                    continue;
                }
                let (a, b) = (self.pool[i], self.pool[j]);
                let (dx, dy) = (a.x - b.x, a.y - b.y);
                if dx * dx + dy * dy < (a.r + b.r) * (a.r + b.r) {
                    self.pool[i].r = (a.r * a.r + b.r * b.r).sqrt().min(cap);
                    self.pool[i].vy += 10.0 * self.s;
                    if *nr < respawn.len() {
                        respawn[*nr] = j as u8;
                        *nr += 1;
                    }
                }
            }
        }
    }

    /// Sayısal güvenlik: NaN olmasın, hiçbir parçacık alanın çok dışına kaçmasın.
    fn keep_in_bounds(&mut self) {
        let m = self.margin();
        let (w, h) = (self.w, self.h);
        let max_r = self.max_r();
        for p in &mut self.pool {
            if !(p.x.is_finite() && p.y.is_finite() && p.r.is_finite() && p.a.is_finite() && p.len.is_finite()) {
                *p = P { x: w / 2.0, y: h / 2.0, r: 1.0, a: 0.0, ..*p };
                p.len = 1.0;
            }
            let up = m + p.r * p.len + h;
            p.x = p.x.clamp(-m, w + m);
            p.y = p.y.clamp(-up, h + m);
            p.r = p.r.clamp(0.1, max_r);
            p.a = p.a.clamp(0.0, 1.0);
            p.len = p.len.clamp(1.0, 12.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn havuz_buyumez_nan_yok() {
        for kind in [Kind::Rain, Kind::Snow, Kind::Night, Kind::Clouds] {
            for dense in [false, true] {
                // Bildirim kartı boyutu ve uç boyutlar.
                for (w, h) in [(380.0, 62.0), (380.0, 108.0), (1.0, 1.0), (0.0, 0.0)] {
                    let mut sim = Sim::new(kind, w, h, 1.5, dense, Rng::new(7));
                    let n = sim.pool.len();
                    assert!(n <= 64, "yeniden doğma dizisi 64 ile sınırlı");
                    let cap = sim.pool.capacity();
                    for i in 0..4000 {
                        let dt = match i % 997 {
                            0 => 0.0,
                            1 => 30.0,
                            2 => f32::NAN,
                            _ => 0.05,
                        };
                        sim.step(dt);
                    }
                    assert_eq!(sim.pool.len(), n);
                    assert_eq!(sim.pool.capacity(), cap);
                    for p in &sim.pool {
                        assert!(p.x.is_finite() && p.y.is_finite() && p.r.is_finite() && (0.0..=1.0).contains(&p.a) && p.r > 0.0, "{p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn gece_tek_ay_ve_duran() {
        let mut sim = Sim::new(Kind::Night, 380.0, 62.0, 1.0, true, Rng::new(5));
        let m0 = *sim.pool.iter().find(|p| p.sprite == Sprite::Moon).unwrap();
        assert_eq!(sim.pool.iter().filter(|p| p.sprite == Sprite::Moon).count(), 1);
        for _ in 0..200 {
            sim.step(0.083);
        }
        let m1 = *sim.pool.iter().find(|p| p.sprite == Sprite::Moon).unwrap();
        assert_eq!((m0.x, m0.y, m0.r), (m1.x, m1.y, m1.r));
    }
}
