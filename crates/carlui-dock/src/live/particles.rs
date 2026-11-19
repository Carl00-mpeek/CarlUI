//! Sahne parçacıkları: sabit boyutlu havuzlar. Havuz kurulurken bir kez
//! ayrılır, sonra hiç büyümez; ölen parçacık yerinde yeniden doğar.
//! Burada yalnızca konum/boyut/saydamlık hesaplanır; çizim `scene.rs`'de
//! önbellekteki bitmap'lerin yerleştirilmesiyle yapılır.

use super::{
    mix::{self, Mix},
    rng::Rng,
};

/// Sahne türü (saat kipi bunlardan birine çözülür, bkz. `sky.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rain,
    Snow,
    Night,
    Clouds,
    /// Akvaryum: parçacık havuzu boş; balık ve kabarcıklar `aquarium.rs`'de.
    Aquarium,
}

impl Kind {
    /// Bu sahnenin kare aralığı (ms): yağmur ≈20 fps, kar ≈15, gece ≈12, bulut 8,
    /// akvaryum ≈15.
    pub fn interval_ms(self) -> u32 {
        match self {
            Kind::Rain => 50,
            Kind::Snow => 66,
            Kind::Night => 83,
            Kind::Clouds => 125,
            Kind::Aquarium => 66,
        }
    }
}

/// Önbellekteki bitmap'lerden hangisiyle çizileceği.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sprite {
    Drop,
    Streak,
    Flake,
    Star,
    Firefly,
    Cloud,
    /// Gece sahnesinde tek, duran ay (solda).
    Moon,
    /// Camda kayan damlanın ince, silinen ıslak izi (yalnız `droplets.rs` kullanır).
    Trail,
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
    pub(super) vx: f32,
    pub(super) vy: f32,
    /// Hedef boyut/saydamlık ya da salınım evresi (türe göre).
    pub(super) goal: f32,
    pub(super) phase: f32,
    sliding: bool,
    /// Derinlik 0 (uzak) .. 1 (yakın): kar tanesinin boyu, hızı ve parlaklığı buna bağlı.
    pub(super) z: f32,
}

impl P {
    fn new(sprite: Sprite) -> Self {
        Self { sprite, x: 0.0, y: 0.0, r: 1.0, len: 1.0, a: 0.0, vx: 0.0, vy: 0.0, goal: 0.0, phase: 0.0, sliding: false, z: 0.0 }
    }
}

pub struct Sim {
    pub kind: Kind,
    w: f32,
    h: f32,
    /// DPI ölçeği: hızlar ve boyutlar bununla çarpılır.
    s: f32,
    pub pool: Vec<P>,
    rng: Rng,
    t: f32,
}

/// Bir adımda en çok bu kadar saniye ilerlenir (duraklatmadan dönüşte sıçrama olmasın).
const MAX_DT: f32 = 0.25;
/// Kar tanesinin saydamlığı: uzak taneler soluk, yakınlar parlak ve hafifçe
/// parıldar (`wave` -1..1 salınım).
pub(super) fn flake_alpha(z: f32, wave: f32) -> f32 {
    let base = 0.35 + z * 0.6;
    base * (1.0 - 0.15 * z + 0.15 * z * wave)
}
/// Kar tanesinin rüzgârı (`step` ile aynı): `t` anında -1..1 civarı.
fn flake_wind(t: f32) -> f32 {
    (t * 0.23).sin() * 0.6 + (t * 0.71 + 1.3).sin() * 0.4
}

/// Rüzgârın integrali (sabit hariç): tanenin rüzgârla aldığı yol bununla orantılı.
#[cfg(test)]
fn flake_wind_path(t: f32) -> f32 {
    -(0.6 / 0.23) * (t * 0.23).cos() - (0.4 / 0.71) * (t * 0.71 + 1.3).cos()
}

/// Compositor'ın kendi başına çizdiği kar tanesi: konum ve saydamlık zamanın
/// kapalı formülüdür (`step`'teki hızların integrali), kare başına CPU işi yok.
/// Tane alttan çıkınca tepeden, yandan çıkınca öbür yandan döner
/// (`Mod`); alan boyu `c.w`/`c.h`'den okunur, dock genişleyince taneler yayılır.
#[derive(Clone, Copy, Debug)]
pub struct FlakeDrive {
    /// Alan içindeki başlangıç kesri (yatay, dikey) 0..1.
    u: f32,
    v: f32,
    r: f32,
    vy: f32,
    /// Rüzgâr yolu çarpanı, salınım genliği/frekansı, evre.
    k: f32,
    sway: f32,
    f: f32,
    ph: f32,
    z: f32,
}

/// İfadedeki sayı (eksi işaret ifadeyi bozmasın diye parantezli).
fn lit(x: f32) -> String {
    format!("({x:.5})")
}

impl FlakeDrive {
    pub fn new(p: &P, s: f32, w: f32, h: f32) -> Self {
        let f = p.vx.max(0.1);
        let top = p.r + 0.25 * h;
        let u = ((p.x + p.r) / (w + 2.0 * p.r)).clamp(0.0, 0.999);
        let v = ((p.y + top) / (1.25 * h + 2.0 * p.r)).clamp(0.0, 0.999);
        Self { u, v, r: p.r, vy: p.vy, k: (8.0 + p.z * 14.0) * s, sway: p.goal, f, ph: p.phase, z: p.z }
    }

    /// Ham (sarmasız) yatay yol: başlangıçtan bağımsız sabit hariç.
    #[cfg(test)]
    fn path_x(&self, t: f32) -> f32 {
        self.k * flake_wind_path(t) - self.sway / self.f * (self.f * t + self.ph).cos()
    }

    /// Tanenin merkezi `t` anında (`w`×`h` alanında); ifadelerle aynı formül.
    #[cfg(test)]
    pub fn at(&self, t: f32, w: f32, h: f32) -> (f32, f32) {
        let ww = w + 2.0 * self.r;
        let x = (self.u * ww + self.path_x(t)).rem_euclid(ww) - self.r;
        let pp = 1.25 * h + 2.0 * self.r;
        let y = (self.v * pp + self.vy * t).rem_euclid(pp) - self.r - 0.25 * h;
        (x, y)
    }

    /// Görselin `Offset` ifadesi (`c`: saat özellik kümesi; `t`, `w`, `h`).
    /// `half`: görselin yarı boyu (merkezden sol üst köşeye).
    pub fn offset_expr(&self, half: (f32, f32)) -> String {
        let (r, k, sf) = (self.r, self.k, self.sway / self.f);
        let ww = format!("(c.w + {})", lit(2.0 * r));
        let raw = format!(
            "{u} * {ww} - {ka} * Cos(0.23 * c.t) - {kb} * Cos(0.71 * c.t + 1.3) - {sf} * Cos({f} * c.t + {ph})",
            u = lit(self.u),
            ka = lit(k * 0.6 / 0.23),
            kb = lit(k * 0.4 / 0.71),
            sf = lit(sf),
            f = lit(self.f),
            ph = lit(self.ph),
        );
        let x = format!("Mod(Mod({raw}, {ww}) + {ww}, {ww}) - {}", lit(r + half.0));
        let pp = format!("(c.h * 1.25 + {})", lit(2.0 * r));
        let y = format!("Mod({v} * {pp} + {vy} * c.t, {pp}) - c.h * 0.25 - {}", lit(r + half.1), v = lit(self.v), vy = lit(self.vy));
        format!("Vector3({x}, {y}, 0)")
    }

    /// Saydamlık ifadesi (`flake_alpha` ile aynı parıltı).
    pub fn opacity_expr(&self) -> String {
        let base = 0.35 + self.z * 0.6;
        format!("{} + {} * Sin(3 * c.t + {})", lit(base * (1.0 - 0.15 * self.z)), lit(base * 0.15 * self.z), lit(self.ph))
    }
}

/// Yıldızın pırıltısı (`step` ile aynı) compositor ifadesi olarak.
pub fn star_opacity_expr(p: &P) -> String {
    format!("{} + {} * Sin({} * c.t + {})", lit(p.goal * 0.65), lit(p.goal * 0.35), lit(p.vx), lit(p.phase))
}

/// Ayın en büyük yarıçapı (ölçek 1'de piksel).
const MOON_R: f32 = 14.0;

impl Sim {
    /// `dense` normal yoğunluk; değilse hafif (daha az parçacık).
    #[cfg(test)]
    pub fn new(kind: Kind, w: f32, h: f32, s: f32, dense: bool, rng: Rng) -> Self {
        Self::with_mix(kind, w, h, s, dense, rng, Mix::default())
    }

    /// Canlı hava için: sayılar şiddete göre ölçeklenir, gece katmanı eklenebilir.
    pub fn with_mix(kind: Kind, w: f32, h: f32, s: f32, dense: bool, rng: Rng, mix: Mix) -> Self {
        let counts = mix::counts(kind, dense, mix);
        // Yeniden doğma sırası `u8` dizinle tutulur: havuz 255'i aşmasın.
        let n: usize = counts.iter().map(|c| c.1).sum::<usize>().min(255);
        let mut pool = Vec::with_capacity(n);
        for &(sp, count) in &counts {
            for _ in 0..count {
                if pool.len() < n {
                    pool.push(P::new(sp));
                }
            }
        }
        let mut sim = Self { kind, w: w.max(1.0), h: h.max(1.0), s: s.max(0.5), pool, rng, t: 0.0 };
        for i in 0..sim.pool.len() {
            sim.spawn(i, true);
        }
        sim
    }

    pub fn size(&self) -> (f32, f32) {
        (self.w, self.h)
    }

    /// DPI ölçeği.
    pub fn scale(&self) -> f32 {
        self.s
    }

    /// Dock genişliği değişti: parçacıklar yeni alana taşınır (havuz aynı).
    pub fn resize(&mut self, w: f32, h: f32) {
        let (w, h) = (w.max(1.0), h.max(1.0));
        if (w - self.w).abs() < 0.5 && (h - self.h).abs() < 0.5 {
            return;
        }
        let (fx, fy) = (w / self.w, h / self.h);
        self.w = w;
        self.h = h;
        let moon_r = self.moon_r();
        for p in &mut self.pool {
            p.x *= fx;
            p.y *= fy;
            if p.sprite == Sprite::Moon {
                p.r = moon_r;
            }
        }
        self.keep_in_bounds();
    }

    /// Parçacığın taşabileceği en büyük pay (sınır testi ve kırpma için).
    pub fn margin(&self) -> f32 {
        self.max_r() * 3.0 + 4.0
    }

    fn max_r(&self) -> f32 {
        match self.kind {
            Kind::Clouds => self.h * 1.2,
            Kind::Night => MOON_R * self.s,
            // Yağış/bulut altına gece katmanı eklenmişse ay da sığmalı.
            _ if self.pool.iter().any(|p| p.sprite == Sprite::Moon) => MOON_R * self.s,
            _ => 6.0 * self.s,
        }
    }

    /// Ayın yarıçapı: camın kısa kenarının ~%28'i, en çok `MOON_R`·ölçek.
    fn moon_r(&self) -> f32 {
        (self.w.min(self.h) * 0.28).clamp(1.0, MOON_R * self.s)
    }

    /// `i`. parçacığı yeniden doğurur. `first`: sahne ilk kurulurken (her yere dağıt).
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
                // Camda yoğuşan küçük damla: yerinde büyür, sonra kayar.
                p.x = r.range(0.0, w);
                p.y = r.range(0.0, h * 0.8);
                p.r = r.range(0.6, 1.2) * s;
                p.goal = r.range(2.2, 3.6) * s;
                p.a = 0.0;
                p.phase = r.range(0.0, 6.28);
            }
            Sprite::Streak => {
                p.x = r.range(0.0, w);
                // Kalın ve camda uzun kalan çizgi: hep birkaç düzine görünür.
                p.r = r.range(1.2, 1.8) * s;
                p.len = r.range(6.0, 10.0);
                p.y = if first { r.range(0.0, h) } else { -p.r * p.len - r.range(0.0, h * 0.3) };
                p.vy = r.range(170.0, 260.0) * s;
                p.vx = p.vy * 0.06;
                p.a = r.range(0.7, 1.0);
            }
            Sprite::Flake => {
                p.x = r.range(0.0, w);
                // Çoğu tane uzakta (küçük, yavaş, soluk); azı yakında (iri, hızlı, parlak).
                let z = r.f() * r.f().sqrt();
                p.z = z;
                p.r = (1.0 + z * 2.6) * s;
                p.y = if first { r.range(0.0, h) } else { -p.r - r.range(0.0, h * 0.5) };
                p.vy = (6.0 + z * 16.0) * s * r.range(0.85, 1.15);
                p.goal = (3.0 + z * 8.0) * s;
                p.vx = r.range(0.8, 1.8);
                p.phase = r.range(0.0, 6.28);
                p.a = flake_alpha(z, 0.0);
            }
            Sprite::Moon => {
                // Solda (sağ uçta saat yazısı var, okunurluğu bozmasın); hiç hareket etmez.
                p.r = moon_r;
                p.x = w * 0.14;
                p.y = h * 0.42;
                p.a = 0.95;
            }
            Sprite::Trail => {}
            Sprite::Star => {
                p.x = r.range(0.0, w);
                p.y = r.range(0.0, h);
                p.r = r.range(1.0, 2.0) * s;
                p.goal = r.range(0.6, 1.0);
                p.phase = r.range(0.0, 6.28);
                p.vx = r.range(0.6, 2.0); // pırıltı hızı
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
                        // Yer çekimiyle hızlanır, en çok bir sınıra; hafifçe yalpalar.
                        p.vy = (p.vy + 140.0 * s * dt).min(110.0 * s);
                        p.vx = (p.vx + self.rng.range(-30.0, 30.0) * s * dt).clamp(-6.0 * s, 6.0 * s);
                        p.x += p.vx * dt;
                        p.y += p.vy * dt;
                        // Kayarken biraz su bırakır: yavaşça küçülür.
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
                    // Ortak, yavaşça dönen rüzgâr; yakın taneler onu daha çok hisseder (paralaks).
                    let wind = flake_wind(t);
                    p.y += p.vy * dt;
                    p.x += (wind * (8.0 + p.z * 14.0) * s + (t * p.vx + p.phase).sin() * p.goal) * dt;
                    p.a = flake_alpha(p.z, (t * 3.0 + p.phase).sin());
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
                Sprite::Moon | Sprite::Trail => false,
                Sprite::Firefly => {
                    p.vx = (p.vx + self.rng.range(-12.0, 12.0) * s * dt).clamp(-8.0 * s, 8.0 * s);
                    p.vy = (p.vy + self.rng.range(-8.0, 8.0) * s * dt).clamp(-4.0 * s, 4.0 * s);
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    // Kenarlardan seker.
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

    /// Kayan damla yolundaki damlayı yutar: hacim korunur (r² toplanır),
    /// yutulan yeniden doğar. Havuz küçük (≤ 28), O(n²) yeter.
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
            // Çizgilerin tepesi yukarıda başlayabilir; alt ve yanlar sıkı.
            let up = m + p.r * p.len + h;
            p.x = p.x.clamp(-m, w + m);
            p.y = p.y.clamp(-up, h + m);
            p.r = p.r.clamp(0.1, max_r);
            p.a = p.a.clamp(0.0, 1.0);
            p.len = p.len.clamp(1.0, 12.0);
        }
    }

    /// Sınır testi için: (x, y) bu alanın içinde mi?
    #[cfg(test)]
    fn inside(&self, p: &P) -> bool {
        let m = self.margin();
        let up = m + p.r * p.len + self.h;
        p.x >= -m && p.x <= self.w + m && p.y >= -up && p.y <= self.h + m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(sim: &Sim, n: usize) {
        assert_eq!(sim.pool.len(), n);
        for p in &sim.pool {
            assert!(p.x.is_finite() && p.y.is_finite() && p.r.is_finite() && p.a.is_finite() && p.len.is_finite(), "{p:?}");
            assert!(sim.inside(p), "{:?} {p:?}", sim.kind);
            assert!((0.0..=1.0).contains(&p.a));
            assert!(p.r > 0.0);
        }
    }

    #[test]
    fn havuz_buyumez_nan_yok_sinir_disi_yok() {
        for kind in [Kind::Rain, Kind::Snow, Kind::Night, Kind::Clouds, Kind::Aquarium] {
            for dense in [false, true] {
                let mut sim = Sim::new(kind, 420.0, 52.0, 1.5, dense, Rng::new(7));
                let n = sim.pool.len();
                let cap = sim.pool.capacity();
                let mut r = Rng::new(99);
                for i in 0..60_000 {
                    // Karışık adımlar: sıfır, çok büyük, NaN ve normal.
                    let dt = match i % 997 {
                        0 => 0.0,
                        1 => 30.0,
                        2 => f32::NAN,
                        _ => r.range(0.01, 0.12),
                    };
                    sim.step(dt);
                    if i % 5000 == 0 {
                        // Dock genişliği oynasın (uygulama açılıp kapanıyor).
                        sim.resize(r.range(40.0, 1600.0), r.range(30.0, 80.0));
                    }
                }
                check(&sim, n);
                assert_eq!(sim.pool.capacity(), cap, "havuz yeniden ayrılmamalı");
            }
        }
    }

    #[test]
    fn compositor_tanesi_simulasyonla_ayni_yolu_izler() {
        let sim = Sim::new(Kind::Snow, 420.0, 52.0, 1.5, true, Rng::new(4));
        let (w, h) = (100_000.0, 10_000.0);
        for p0 in sim.pool.iter().take(8) {
            let p = P { x: w / 2.0, y: 0.0, ..*p0 };
            let d = FlakeDrive::new(&p, 1.5, w, h);
            let (x0, y0) = d.at(0.0, w, h);
            // `step`'teki hızları küçük adımlarla topla.
            let (mut x, mut y, mut t) = (0.0f32, 0.0f32, 0.0f32);
            let dt = 0.001;
            for _ in 0..10_000 {
                t += dt;
                y += p.vy * dt;
                x += (flake_wind(t) * (8.0 + p.z * 14.0) * 1.5 + (t * p.vx + p.phase).sin() * p.goal) * dt;
            }
            let (x1, y1) = d.at(t, w, h);
            assert!(((x1 - x0) - x).abs() < 0.5, "x {} vs {x}", x1 - x0);
            assert!(((y1 - y0) - y).abs() < 0.5, "y {} vs {y}", y1 - y0);
        }
    }

    #[test]
    fn compositor_tanesi_alanda_kalir_ve_ifade_saglam() {
        let sim = Sim::new(Kind::Snow, 420.0, 52.0, 1.5, true, Rng::new(8));
        for p in &sim.pool {
            let d = FlakeDrive::new(p, 1.5, 420.0, 52.0);
            for k in 0..2000 {
                let (x, y) = d.at(k as f32 * 1.7, 420.0, 52.0);
                assert!(x >= -p.r - 0.01 && x <= 420.0 + p.r + 0.01, "{x}");
                assert!(y >= -p.r - 13.0 - 0.01 && y <= 52.0 + p.r + 0.01, "{y}");
            }
            for e in [d.offset_expr((3.0, 3.0)), d.opacity_expr()] {
                assert!(!e.contains("NaN") && !e.contains("inf"), "{e}");
                assert_eq!(e.matches('(').count(), e.matches(')').count(), "{e}");
            }
        }
    }

    #[test]
    fn kucuk_alan_ve_dikey_dock() {
        // Sol/sağ dock: dar ve uzun alan; sıfır boyut da çökmemeli.
        for (w, h) in [(0.0, 0.0), (52.0, 700.0), (1.0, 1.0)] {
            let mut sim = Sim::new(Kind::Rain, w, h, 1.0, true, Rng::new(3));
            let n = sim.pool.len();
            for _ in 0..5_000 {
                sim.step(0.05);
            }
            check(&sim, n);
        }
    }

    #[test]
    fn damlalar_kayip_birlesir() {
        let mut sim = Sim::new(Kind::Rain, 300.0, 50.0, 1.0, true, Rng::new(11));
        let mut slid = false;
        for _ in 0..2_000 {
            sim.step(0.05);
            slid |= sim.pool.iter().any(|p| p.sprite == Sprite::Drop && p.sliding);
        }
        assert!(slid, "damlalar bir süre sonra kaymalı");
    }

    #[test]
    fn hava_karisimi_sinirda_ve_ay_altta() {
        for kind in [Kind::Rain, Kind::Snow, Kind::Clouds] {
            let mix = Mix { level: Some(100), night: true, droplets: false };
            let mut sim = Sim::with_mix(kind, 420.0, 52.0, 1.5, true, Rng::new(9), mix);
            let n = sim.pool.len();
            assert!(n > 20 && n <= 255);
            // Gece katmanı en altta: ilk parçacıklar yıldız.
            assert_eq!(sim.pool[0].sprite, Sprite::Star);
            for _ in 0..20_000 {
                sim.step(0.05);
            }
            check(&sim, n);
        }
    }

    #[test]
    fn gece_tek_ay_ve_duran() {
        let mut sim = Sim::new(Kind::Night, 1000.0, 52.0, 1.0, true, Rng::new(5));
        let moons: Vec<P> = sim.pool.iter().copied().filter(|p| p.sprite == Sprite::Moon).collect();
        assert_eq!(moons.len(), 1);
        let m0 = moons[0];
        assert!(m0.r > 10.0 && m0.r <= MOON_R, "ay okunacak boyda: {}", m0.r);
        for _ in 0..500 {
            sim.step(0.083);
        }
        let m1 = *sim.pool.iter().find(|p| p.sprite == Sprite::Moon).unwrap();
        assert_eq!((m0.x, m0.y, m0.r), (m1.x, m1.y, m1.r));
        // Havuz 64 sınırının altında (yeniden doğma dizisi).
        for kind in [Kind::Rain, Kind::Snow, Kind::Night] {
            assert!(Sim::new(kind, 500.0, 52.0, 1.0, true, Rng::new(1)).pool.len() <= 64);
        }
    }
}
