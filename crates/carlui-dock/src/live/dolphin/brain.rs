//! Yunusun davranışı ve sıçrama geometrisi (saf; sanat pikseli birimi).
//! Koordinatlar camın içeriğine göre: x pill'in solundan, y = 0 dock'un üst
//! kenarı (su yüzeyi), aşağı pozitif.
//!
//! Yunus camın içinde, orta derinlikte yüzer; uçlarda döner. Ara sıra yüzeye
//! yükselir ve ≈48 px ileriye, ≈33 px yükseğe, 0,9 sn süren bir parabolle
//! sıçrar. Sıçrama karesi hız vektörünün açısına (atan2(-vy, vx)) en yakın
//! eğim karesidir; sola giderken aynalanır. Kalkışta `splash_out`, açı
//! -45°'nin altına inince giriş noktasında `splash_in` oynar.

use crate::live::rng::Rng;

/// Sıçrama boyutları.
pub const LEAP_W: f32 = 48.0;
pub const LEAP_H: f32 = 33.0;
pub const LEAP_T: f32 = 0.9;
/// Sıçrama karelerinin eğimleri (derece, yukarı pozitif): sanat sırası.
pub const LEAP_PITCH: [f32; 5] = [60.0, 30.0, 0.0, -30.0, -60.0];
/// Sıçramadan önce yükseldiği derinlik (merkez).
pub const SURFACE_Y: f32 = 7.0;
const SWIM_SPEED: f32 = 14.0;
const SWIM_FRAMES: u8 = 8;
const SWIM_MS: f32 = 0.1;
pub const SPLASH_FRAMES: u8 = 6;
const SPLASH_MS: f32 = 0.07;
/// Yunusun yarı genişliği: uçlarda bu kadar pay bırakır.
const HALF: f32 = 24.0;

/// Sıçramanın `t` (0..1) anındaki konumu (kalkışa göre, +x ileri).
pub fn leap_pos(t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    (LEAP_W * t, -4.0 * LEAP_H * t * (1.0 - t))
}

/// Hız vektörünün açısı (derece, yukarı pozitif, hep ileri bakar).
pub fn leap_angle(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (vx, vy) = (LEAP_W, -4.0 * LEAP_H * (1.0 - 2.0 * t));
    (-vy).atan2(vx).to_degrees()
}

/// Açıya en yakın sıçrama karesi.
pub fn leap_frame(angle: f32) -> u8 {
    (0..LEAP_PITCH.len()).min_by(|&a, &b| (LEAP_PITCH[a] - angle).abs().total_cmp(&(LEAP_PITCH[b] - angle).abs())).unwrap_or(2) as u8
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Swim,
    /// Yüzeye çıkıyor.
    Rise,
    Leap { t: f32, x0: f32 },
    /// Orta derinliğe iniyor.
    Sink,
}

/// Su sıçraması: giriş mi, çıkış mı, merkez x ve kare.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splash {
    pub entry: bool,
    pub x: f32,
    pub frame: u8,
    left: f32,
}

/// Çizilecek görünüm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Body {
    Swim(u8),
    Leap(u8),
}

pub struct Dolphin {
    pub x: f32,
    pub y: f32,
    pub dir: f32,
    pub mode: Mode,
    swim_frame: u8,
    swim_left: f32,
    pub splash: Option<Splash>,
    entered: bool,
    leap_in: f32,
    rng: Rng,
}

impl Dolphin {
    pub fn new(x: f32, y: f32, mut rng: Rng) -> Self {
        let dir = if rng.f() < 0.5 { 1.0 } else { -1.0 };
        let leap_in = rng.range(4.0, 9.0);
        Self { x, y, dir, mode: Mode::Swim, swim_frame: 0, swim_left: SWIM_MS, splash: None, entered: false, leap_in, rng }
    }

    pub fn body(&self) -> Body {
        match self.mode {
            Mode::Leap { t, .. } => Body::Leap(leap_frame(leap_angle(t))),
            _ => Body::Swim(self.swim_frame),
        }
    }

    /// Dürtme: ilk fırsatta sıçrar.
    pub fn poke(&mut self) {
        self.leap_in = self.leap_in.min(0.2);
    }

    fn splash(&mut self, entry: bool, x: f32) {
        self.splash = Some(Splash { entry, x, frame: 0, left: SPLASH_MS });
    }

    /// `w`×`h` cam (sanat pikseli) içinde `dt` saniye ilerletir; dönüş: sonraki tike kadar ms.
    pub fn step(&mut self, dt: f32, w: f32, h: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.25);
        let (lo, hi) = (HALF, (w - HALF).max(HALF));
        let mid = (h / 2.0).max(SURFACE_Y);
        if let Some(s) = self.splash.as_mut() {
            s.left -= dt;
            while s.left <= 0.0 {
                s.left += SPLASH_MS;
                s.frame += 1;
            }
            if s.frame >= SPLASH_FRAMES {
                self.splash = None;
            }
        }
        // Yüzerken kuyruk çırpar (sıçrarken kare açıdan gelir).
        self.swim_left -= dt;
        while self.swim_left <= 0.0 {
            self.swim_left += SWIM_MS;
            self.swim_frame = (self.swim_frame + 1) % SWIM_FRAMES;
        }
        match self.mode {
            Mode::Swim | Mode::Sink | Mode::Rise => {
                self.x += self.dir * SWIM_SPEED * dt;
                if (self.dir > 0.0 && self.x >= hi) || (self.dir < 0.0 && self.x <= lo) {
                    self.x = self.x.clamp(lo, hi);
                    self.dir = -self.dir;
                }
                let goal = if self.mode == Mode::Rise { SURFACE_Y } else { mid };
                let dy = goal - self.y;
                let v = 10.0 * dt;
                self.y += dy.clamp(-v, v);
                match self.mode {
                    Mode::Sink if (self.y - mid).abs() < 0.5 => self.mode = Mode::Swim,
                    Mode::Swim => {
                        self.leap_in -= dt;
                        // Önünde sıçrayacak yer varsa yükselir.
                        let room = if self.dir > 0.0 { hi - self.x } else { self.x - lo };
                        if self.leap_in <= 0.0 && room > LEAP_W + 30.0 {
                            self.mode = Mode::Rise;
                        }
                    }
                    Mode::Rise if (self.y - SURFACE_Y).abs() < 0.5 => {
                        let room = if self.dir > 0.0 { hi - self.x } else { self.x - lo };
                        if room > LEAP_W + 4.0 {
                            self.mode = Mode::Leap { t: 0.0, x0: self.x };
                            self.entered = false;
                            self.splash(false, self.x);
                        } else {
                            self.mode = Mode::Sink;
                        }
                        self.leap_in = self.rng.range(6.0, 15.0);
                    }
                    _ => {}
                }
            }
            Mode::Leap { t, x0 } => {
                let t = t + dt / LEAP_T;
                let (dx, dy) = leap_pos(t);
                self.x = x0 + self.dir * dx;
                self.y = SURFACE_Y + dy;
                if !self.entered && leap_angle(t) < -45.0 {
                    self.entered = true;
                    self.splash(true, x0 + self.dir * LEAP_W);
                }
                self.mode = if t >= 1.0 { Mode::Sink } else { Mode::Leap { t, x0 } };
            }
        }
        // Sıçrarken ≤20 fps; yüzerken kuyruk karesi kadar.
        if matches!(self.mode, Mode::Leap { .. }) || self.splash.is_some() {
            50.0
        } else {
            SWIM_MS * 1000.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sicrama_parabolu() {
        assert_eq!(leap_pos(0.0), (0.0, 0.0));
        let (x, y) = leap_pos(0.5);
        assert!((x - LEAP_W / 2.0).abs() < 1e-4 && (y + LEAP_H).abs() < 1e-4);
        let (x, y) = leap_pos(1.0);
        assert!((x - LEAP_W).abs() < 1e-4 && y.abs() < 1e-4);
    }

    #[test]
    fn aci_kareye() {
        // Kalkışta dik yukarı, tepede yatay, inişte dik aşağı.
        assert_eq!(leap_frame(leap_angle(0.0)), 0);
        assert_eq!(leap_frame(leap_angle(0.5)), 2);
        assert_eq!(leap_frame(leap_angle(1.0)), 4);
        assert_eq!(leap_frame(44.0), 1);
        assert_eq!(leap_frame(-46.0), 4);
        assert_eq!(leap_frame(10.0), 2);
        // Kareler zamanla sırayla ilerler (geri dönmez).
        let mut last = 0;
        for i in 0..=20 {
            let f = leap_frame(leap_angle(i as f32 / 20.0));
            assert!(f >= last);
            last = f;
        }
    }

    #[test]
    fn sicrama_su_sicratir() {
        let mut d = Dolphin::new(60.0, 20.0, Rng::new(1));
        d.dir = 1.0;
        d.poke();
        let (mut outs, mut ins) = (Vec::new(), Vec::new());
        let mut leap_x0 = None;
        for _ in 0..400 {
            d.step(0.05, 400.0, 40.0);
            if let Mode::Leap { x0, t } = d.mode {
                leap_x0.get_or_insert(x0);
                // Giriş sıçraması yalnızca açı -45°'nin altına indikten sonra.
                if d.splash.is_some_and(|s| s.entry) {
                    assert!(leap_angle(t) < -45.0);
                }
            }
            if let Some(s) = d.splash {
                if s.frame == 0 {
                    if s.entry { ins.push(s.x) } else { outs.push(s.x) }
                }
            }
            if leap_x0.is_some() && d.mode == Mode::Swim {
                break;
            }
        }
        let x0 = leap_x0.expect("sıçramadı");
        assert!(outs.iter().all(|&x| (x - x0).abs() < 1e-3) && !outs.is_empty());
        assert!(ins.iter().all(|&x| (x - (x0 + LEAP_W)).abs() < 1e-3) && !ins.is_empty());
    }

    #[test]
    fn camda_kalir() {
        let mut d = Dolphin::new(100.0, 20.0, Rng::new(4));
        for _ in 0..20_000 {
            let ms = d.step(0.05, 300.0, 40.0);
            assert!(ms >= 50.0);
            assert!(d.x >= HALF - 1e-3 && d.x <= 300.0 - HALF + 1e-3, "{}", d.x);
            assert!(d.y >= SURFACE_Y - LEAP_H - 1e-3 && d.y <= 20.0 + 1e-3);
        }
    }
}
