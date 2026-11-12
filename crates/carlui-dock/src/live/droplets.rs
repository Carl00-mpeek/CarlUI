//! Gündüz yağmuru: "yağmurlu pencere camı". Camda duran küçük damlalar
//! (bazıları yavaşça büyür); yeterince büyüyen damla biraz yalpalayarak
//! aşağı kayar, ara sıra durur, yolundaki damlaları yutar ve arkasında
//! ince, ~1.5 sn'de silinen ıslak bir iz bırakır. Yeni küçük damlalar
//! rastgele yerlerde belirir. Sayı ve hız şiddete göre ölçeklenir.
//!
//! Yalnızca simülasyon (saf, belirlenimci): çizim `droplets_view.rs`'de.
//! Havuz kurulurken bir kez ayrılır, sonra büyümez.

use super::rng::Rng;

/// Damla sayısı tavanı (şiddet 1.0'da): CPU bugünkü yağmurun düzeyinde kalsın.
pub const MAX_BEADS: usize = 140;
/// İz parçası havuzu.
pub const MAX_TRAILS: usize = 40;
/// Bir adımda en çok bu kadar saniye ilerlenir.
const MAX_DT: f32 = 0.25;
/// Silinen izin ömrü (sn) ve en yüksek saydamlığı.
const TRAIL_LIFE: f32 = 1.5;
const TRAIL_A: f32 = 0.6;
/// Kayan damlanın izi bu sürede bir parçaya bölünür.
const SEG_SECS: f32 = 0.3;
const NO_TRAIL: u8 = u8::MAX;

/// Şiddete (0..1) göre damla havuzunun büyüklüğü.
pub fn bead_cap(level: f32) -> usize {
    (24.0 + 116.0 * level.clamp(0.0, 1.0)).round() as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    /// Henüz ya da artık yok; `timer` sonra yeni yerde belirir.
    Dead,
    Sit,
    Run,
    /// Kayarken kısa duraklama.
    Pause,
    /// Duran damla buharlaşıp siliniyor.
    Fade,
}

#[derive(Clone, Copy, Debug)]
pub struct Bead {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    /// Dikey uzama (kayarken hafif uzar).
    pub len: f32,
    pub a: f32,
    st: St,
    vx: f32,
    vy: f32,
    vmax: f32,
    /// Duran damlanın büyüme hızı (px/sn), hedef saydamlık.
    grow: f32,
    ta: f32,
    /// Dead: belirmeye kalan; Sit: ömrüne kalan; Pause: duraklamaya kalan.
    timer: f32,
    trail: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Trail {
    pub x: f32,
    pub y0: f32,
    pub y1: f32,
    pub a: f32,
    alive: bool,
    open: bool,
    /// Açıkken parçanın yaşı, kapalıyken silinme süresi.
    age: f32,
}

pub struct Droplets {
    w: f32,
    h: f32,
    s: f32,
    level: f32,
    beads: Vec<Bead>,
    trails: Vec<Trail>,
    rng: Rng,
}

impl Droplets {
    pub fn new(w: f32, h: f32, s: f32, level: f32, rng: Rng) -> Self {
        let level = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.5 };
        let blank = Bead { x: 0.0, y: 0.0, r: 1.0, len: 1.0, a: 0.0, st: St::Dead, vx: 0.0, vy: 0.0, vmax: 0.0, grow: 0.0, ta: 0.0, timer: 0.0, trail: NO_TRAIL };
        let n = bead_cap(level).min(MAX_BEADS);
        let mut d = Self { w: w.max(1.0), h: h.max(1.0), s: s.max(0.5), level, beads: vec![blank; n], trails: vec![Trail::default(); MAX_TRAILS], rng };
        for i in 0..n {
            if d.rng.f() < 0.55 {
                d.appear(i, true);
            } else {
                d.beads[i].timer = d.rng.range(0.0, 3.0) / d.rate();
            }
        }
        d
    }

    #[cfg(test)]
    pub fn size(&self) -> (f32, f32) {
        (self.w, self.h)
    }

    pub fn beads(&self) -> &[Bead] {
        &self.beads
    }

    pub fn trails(&self) -> &[Trail] {
        &self.trails
    }

    /// Camda görünen (yok olmayan) damla sayısı.
    #[cfg(test)]
    pub fn visible(&self) -> usize {
        self.beads.iter().filter(|b| b.st != St::Dead).count()
    }

    /// Yeni damla belirme hızı çarpanı.
    fn rate(&self) -> f32 {
        0.35 + 1.3 * self.level
    }

    pub fn resize(&mut self, w: f32, h: f32) {
        let (w, h) = (w.max(1.0), h.max(1.0));
        if (w - self.w).abs() < 0.5 && (h - self.h).abs() < 0.5 {
            return;
        }
        let (fx, fy) = (w / self.w, h / self.h);
        self.w = w;
        self.h = h;
        for b in &mut self.beads {
            b.x *= fx;
            b.y *= fy;
        }
        for t in &mut self.trails {
            t.x *= fx;
            t.y0 *= fy;
            t.y1 *= fy;
        }
    }

    /// `i`. damla yeni bir yerde küçük bir damla olarak belirir.
    fn appear(&mut self, i: usize, first: bool) {
        let (w, h, s) = (self.w, self.h, self.s);
        let r = &mut self.rng;
        let big = r.f() < 0.18;
        let b = &mut self.beads[i];
        b.st = St::Sit;
        b.x = r.range(0.0, w);
        b.y = r.range(0.0, h);
        b.r = if big { r.range(2.0, 3.4) } else { r.range(0.9, 2.0) } * s;
        b.len = 1.0;
        b.vx = 0.0;
        b.vy = 0.0;
        b.grow = if r.f() < 0.3 { r.range(0.05, 0.18) * s } else { 0.0 };
        b.ta = r.range(0.8, 1.0);
        b.a = if first { b.ta } else { 0.0 };
        // Büyüyenler uzun yaşar (kaymaya vakit bulsunlar); diğerleri buharlaşır.
        b.timer = if b.grow > 0.0 { r.range(60.0, 120.0) } else { r.range(20.0, 70.0) };
        if first {
            b.timer *= r.range(0.2, 1.0);
        }
        b.trail = NO_TRAIL;
    }

    fn die(&mut self, i: usize) {
        self.close_trail(i);
        let wait = self.rng.range(0.4, 3.0) / self.rate();
        let b = &mut self.beads[i];
        b.st = St::Dead;
        b.a = 0.0;
        b.timer = wait;
    }

    fn close_trail(&mut self, i: usize) {
        let t = self.beads[i].trail;
        if t != NO_TRAIL {
            let tr = &mut self.trails[t as usize];
            tr.open = false;
            tr.age = 0.0;
        }
        self.beads[i].trail = NO_TRAIL;
    }

    /// Damlanın bulunduğu yerden yeni bir iz parçası açar (havuz doluysa izsiz).
    fn open_trail(&mut self, i: usize) {
        let (x, y) = (self.beads[i].x, self.beads[i].y);
        self.beads[i].trail = match self.trails.iter().position(|t| !t.alive) {
            Some(k) => {
                self.trails[k] = Trail { x, y0: y, y1: y, a: TRAIL_A, alive: true, open: true, age: 0.0 };
                k as u8
            }
            None => NO_TRAIL,
        };
    }

    fn start_run(&mut self, i: usize) {
        let s = self.s;
        let vmax = self.rng.range(28.0, 75.0) * s;
        let vy = self.rng.range(4.0, 10.0) * s;
        let b = &mut self.beads[i];
        b.st = St::Run;
        b.vmax = vmax;
        b.vy = vy;
        b.vx = 0.0;
        self.open_trail(i);
    }

    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_DT) } else { 0.0 };
        if dt == 0.0 {
            return;
        }
        let (w, h, s) = (self.w, self.h, self.s);
        let hazard = 0.01 + 0.05 * self.level;
        for i in 0..self.beads.len() {
            match self.beads[i].st {
                St::Dead => {
                    self.beads[i].timer -= dt;
                    if self.beads[i].timer <= 0.0 {
                        self.appear(i, false);
                    }
                }
                St::Sit => {
                    let roll = self.rng.f();
                    let b = &mut self.beads[i];
                    b.a = (b.a + dt * 1.5).min(b.ta);
                    b.r += b.grow * dt;
                    b.timer -= dt;
                    let ripe = b.r >= 3.6 * s;
                    if ripe || (b.r >= 2.4 * s && roll < hazard * dt) {
                        self.start_run(i);
                    } else if b.timer <= 0.0 {
                        b.st = St::Fade;
                    }
                }
                St::Fade => {
                    let b = &mut self.beads[i];
                    b.a -= dt * 1.2;
                    if b.a <= 0.0 {
                        self.die(i);
                    }
                }
                St::Pause => {
                    let b = &mut self.beads[i];
                    b.timer -= dt;
                    if b.timer <= 0.0 {
                        b.st = St::Run;
                        b.vy = 3.0 * s;
                    }
                    self.follow_trail(i, dt);
                }
                St::Run => {
                    let jitter = self.rng.range(-40.0, 40.0);
                    let pause = self.rng.f() < 0.9 * dt;
                    let pause_for = self.rng.range(0.15, 0.5);
                    let b = &mut self.beads[i];
                    b.vy = (b.vy + 90.0 * s * dt).min(b.vmax);
                    b.vx = (b.vx + jitter * s * dt).clamp(-5.0 * s, 5.0 * s);
                    b.x = (b.x + b.vx * dt).clamp(0.0, w);
                    b.y += b.vy * dt;
                    b.r = (b.r - 0.08 * s * dt).max(1.4 * s);
                    b.len = 1.0 + 0.45 * (b.vy / b.vmax.max(1.0));
                    if pause && b.y > 0.1 * h {
                        b.st = St::Pause;
                        b.timer = pause_for;
                        b.vy = 0.0;
                        b.len = 1.0;
                    }
                    let gone = b.y - b.r > h;
                    self.follow_trail(i, dt);
                    if gone {
                        self.die(i);
                    } else {
                        self.absorb(i);
                    }
                }
            }
        }
        for t in &mut self.trails {
            if t.alive && !t.open {
                t.age += dt;
                t.a = TRAIL_A * (1.0 - t.age / TRAIL_LIFE);
                if t.age >= TRAIL_LIFE || t.a <= 0.0 {
                    t.alive = false;
                    t.a = 0.0;
                }
            }
        }
    }

    /// Kayan damlanın açık iz parçasını damlaya uydurur; süre dolunca böler.
    fn follow_trail(&mut self, i: usize, dt: f32) {
        let t = self.beads[i].trail;
        if t == NO_TRAIL {
            return;
        }
        let (x, y) = (self.beads[i].x, self.beads[i].y);
        let tr = &mut self.trails[t as usize];
        tr.x = x;
        tr.y1 = y;
        tr.age += dt;
        if tr.age >= SEG_SECS {
            self.close_trail(i);
            self.open_trail(i);
        }
    }

    /// Kayan damla yolundaki duran damlaları yutar (hacim: r² toplanır).
    fn absorb(&mut self, i: usize) {
        let cap = 4.2 * self.s;
        for j in 0..self.beads.len() {
            if i == j || !matches!(self.beads[j].st, St::Sit | St::Fade) {
                continue;
            }
            let (a, b) = (self.beads[i], self.beads[j]);
            let (dx, dy) = (a.x - b.x, a.y - b.y);
            let reach = a.r + b.r;
            if dx * dx + dy * dy < reach * reach * 0.8 {
                self.beads[i].r = (a.r * a.r + b.r * b.r).sqrt().min(cap);
                self.beads[i].vy += 5.0 * self.s;
                self.die(j);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 420.0;
    const H: f32 = 52.0;

    fn run(level: f32, seed: u32, ticks: usize) -> Droplets {
        let mut d = Droplets::new(W, H, 1.5, level, Rng::new(seed));
        for _ in 0..ticks {
            d.step(0.05);
        }
        d
    }

    #[test]
    fn sayi_tavani_ve_sinirlar() {
        for level in [0.0, 0.2, 0.5, 1.0] {
            let mut d = Droplets::new(W, H, 1.5, level, Rng::new(7));
            let cap = d.beads().len();
            assert!(cap <= MAX_BEADS, "{cap}");
            let mut r = Rng::new(3);
            for i in 0..20_000 {
                let dt = match i % 503 {
                    0 => 0.0,
                    1 => 30.0,
                    2 => f32::NAN,
                    _ => r.range(0.01, 0.12),
                };
                d.step(dt);
                if i % 4000 == 0 {
                    d.resize(r.range(40.0, 1200.0), r.range(30.0, 80.0));
                }
                assert!(d.visible() <= cap);
            }
            assert_eq!(d.beads().len(), cap, "havuz büyümemeli");
            let (w, h) = d.size();
            let m = 40.0;
            for b in d.beads() {
                assert!(b.x.is_finite() && b.y.is_finite() && b.r.is_finite() && b.a.is_finite() && b.len.is_finite());
                assert!(b.x >= -m && b.x <= w + m && b.y >= -m && b.y <= h + m, "{b:?}");
                assert!((0.0..=1.0).contains(&b.a) && b.r > 0.0);
            }
            for t in d.trails() {
                assert!(t.a.is_finite() && (0.0..=1.0).contains(&t.a));
            }
        }
    }

    #[test]
    fn siddet_damla_sayisini_arttirir() {
        let (mut light, mut heavy) = (0, 0);
        for seed in 1..=5 {
            light += run(0.2, seed, 400).visible();
            heavy += run(1.0, seed, 400).visible();
        }
        assert!(heavy > light * 2, "hafif {light}, şiddetli {heavy}");
    }

    #[test]
    fn belirlenimci() {
        let (a, b) = (run(0.7, 42, 300), run(0.7, 42, 300));
        for (x, y) in a.beads().iter().zip(b.beads()) {
            assert_eq!((x.x, x.y, x.r, x.a), (y.x, y.y, y.r, y.a));
        }
        let c = run(0.7, 43, 300);
        assert!(a.beads().iter().zip(c.beads()).any(|(x, y)| x.x != y.x));
    }

    #[test]
    fn kayar_iz_birakir_ve_iz_silinir() {
        let mut d = Droplets::new(W, H, 1.0, 1.0, Rng::new(11));
        let (mut ran, mut trailed) = (false, false);
        for _ in 0..1_000 {
            d.step(0.05);
            ran |= d.beads().iter().any(|b| b.st == St::Run);
            trailed |= d.trails().iter().any(|t| t.alive && t.y1 > t.y0 + 1.0);
        }
        assert!(ran && trailed, "damlalar kaymalı ve iz bırakmalı");
        // Hiçbir iz kapandıktan 1.5 sn sonra yaşamaz.
        for t in d.trails() {
            assert!(!t.alive || t.open || t.age < TRAIL_LIFE);
        }
    }

    #[test]
    fn kucuk_alan_cokmez() {
        for (w, h) in [(0.0, 0.0), (52.0, 700.0), (1.0, 1.0)] {
            let mut d = Droplets::new(w, h, 1.0, 1.0, Rng::new(3));
            for _ in 0..2_000 {
                d.step(0.05);
            }
            assert!(d.beads().iter().all(|b| b.x.is_finite() && b.y.is_finite()));
        }
    }
}
