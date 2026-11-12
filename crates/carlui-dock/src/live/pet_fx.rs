//! Hayvanın küçük parçacıkları: okşanınca kalpler, müzikte notalar.
//! Sahne parçacıklarından (`particles.rs`) ayrıdır: sahne havuzları sabit ve
//! sürekli dolar; bunlar ise ara sıra birkaç tane doğar ve 1-2 sn'de söner.
//! Burada yalnızca konum/saydamlık hesaplanır ve piksel sanatı üretilir
//! (dış dosya yok); çizim `pet_fx_view.rs`'de.

use super::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Puff {
    Heart,
    Note,
}

/// Aynı anda en çok bu kadar parçacık (görsel havuzu da bu boyutta).
pub const MAX: usize = 6;
/// Ekran kare aralığı: parçacık varken ~15 fps yeter (piksel sanatı).
pub const STEP_MS: f32 = 66.0;

/// Tek parçacık. Konumlar sanat pikseli cinsinden, hayvanın başına göre
/// (x sağa, y yukarı pozitif).
#[derive(Clone, Copy, Debug)]
pub struct P {
    pub kind: Puff,
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    pub age: f32,
    life: f32,
    phase: f32,
}

impl P {
    /// Saydamlık: hızlı belirir, son üçte birde söner.
    pub fn alpha(&self) -> f32 {
        let t = (self.age / self.life).clamp(0.0, 1.0);
        if t < 0.1 {
            t / 0.1
        } else if t > 0.66 {
            (1.0 - t) / 0.34
        } else {
            1.0
        }
    }
}

pub struct Fx {
    pub list: Vec<P>,
    rng: Rng,
}

impl Fx {
    pub fn new(rng: Rng) -> Self {
        Self { list: Vec::with_capacity(MAX), rng }
    }

    pub fn alive(&self) -> bool {
        !self.list.is_empty()
    }

    /// `n` parçacık doğurur; havuz doluysa en yaşlısı yer açar.
    pub fn spawn(&mut self, kind: Puff, n: u32) {
        for i in 0..n {
            if self.list.len() >= MAX {
                let oldest = self.list.iter().enumerate().max_by(|a, b| a.1.age.total_cmp(&b.1.age)).map(|(i, _)| i).unwrap_or(0);
                self.list.swap_remove(oldest);
            }
            let (x, vx, vy, life, delay) = match kind {
                // Kalpler yelpaze gibi açılır: yan yana doğar, dışa doğru kayar ve
                // sırayla gecikir (canlı denemede üst üste binip okunmuyordu).
                Puff::Heart => {
                    let slot = i as f32 - (n as f32 - 1.0) / 2.0;
                    let x = slot * 8.0 + self.rng.range(-1.0, 1.0);
                    let vx = slot * self.rng.range(2.0, 4.0) + self.rng.range(-1.0, 1.0);
                    (x, vx, self.rng.range(10.0, 13.0), self.rng.range(1.2, 1.5), i as f32 * 0.22)
                }
                Puff::Note => {
                    let side = if self.rng.f() < 0.5 { -1.0 } else { 1.0 };
                    (self.rng.range(-4.0, 4.0), self.rng.range(2.0, 6.0) * side, self.rng.range(7.0, 10.0), self.rng.range(1.4, 1.9), 0.0)
                }
            };
            self.list.push(P { kind, x, y: 0.0, vx, vy, age: -delay, life, phase: self.rng.range(0.0, 6.28) });
        }
    }

    /// `dt` saniye ilerletir; ölenler atılır.
    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.25) } else { 0.0 };
        for p in &mut self.list {
            p.age += dt;
            if p.age <= 0.0 {
                continue;
            }
            p.phase += dt * 6.0;
            // Hafif yalpalayarak yükselir; giderek yavaşlar.
            let wobble = if p.kind == Puff::Heart { 1.5 } else { 3.0 };
            p.x += (p.vx + p.phase.sin() * wobble) * dt;
            p.y += p.vy * dt;
            p.vy *= 1.0 - 0.4 * dt;
        }
        self.list.retain(|p| p.age < p.life);
    }
}

/// Parçacığın piksel sanatı: (genişlik, yükseklik, PBGRA). `.` boş,
/// `X` koyu kontur, öbürleri dolgu.
pub fn art(kind: Puff) -> (u32, u32, Vec<u8>) {
    const HEART: [&str; 6] = [".XX.XX.", "XhrXrrX", "XrrrrrX", ".XrrrX.", "..XrX..", "...X..."];
    const NOTE: [&str; 8] = ["..XXX.", "..XwwX", "..XwXX", "..XwX.", ".XXwX.", "XwwwX.", "XwwwX.", ".XXX.."];
    let rows: &[&str] = match kind {
        Puff::Heart => &HEART,
        Puff::Note => &NOTE,
    };
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for r in rows {
        for c in r.bytes() {
            // (B, G, R, A); alfa tam olduğundan önceden çarpılmış hali aynı.
            let p: [u8; 4] = match (kind, c) {
                (_, b'.') => [0, 0, 0, 0],
                (Puff::Heart, b'X') => [0x30, 0x14, 0x48, 255],
                (Puff::Heart, b'h') => [0xD0, 0xC0, 0xFF, 255],
                (Puff::Heart, _) => [0x78, 0x58, 0xF0, 255],
                (Puff::Note, b'X') => [0x24, 0x28, 0x30, 255],
                (Puff::Note, _) => [0x80, 0xE0, 0xFF, 255],
            };
            px.extend_from_slice(&p);
        }
    }
    (w, h, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanat_boyutlari_tutarli() {
        for k in [Puff::Heart, Puff::Note] {
            let (w, h, px) = art(k);
            assert_eq!(px.len(), (w * h * 4) as usize);
            assert!(w <= 8 && h <= 8, "küçük kalmalı");
            // Önceden çarpılmış: renk kanalları alfayı aşmaz; saydam pikseller sıfır.
            for c in px.chunks(4) {
                assert!(c[0] <= c[3] && c[1] <= c[3] && c[2] <= c[3]);
            }
            assert!(px.chunks(4).any(|c| c[3] == 0) && px.chunks(4).any(|c| c[3] == 255));
        }
    }

    #[test]
    fn yukselir_soner_ve_temizlenir() {
        let mut fx = Fx::new(Rng::new(7));
        fx.spawn(Puff::Heart, 3);
        assert_eq!(fx.list.len(), 3);
        let mut steps = 0;
        let mut top = 0.0f32;
        while fx.alive() {
            fx.step(STEP_MS / 1000.0);
            for p in &fx.list {
                assert!(p.x.is_finite() && p.y >= 0.0);
                assert!((0.0..=1.0).contains(&p.alpha()));
                top = top.max(p.y);
            }
            steps += 1;
            assert!(steps < 100, "parçacıklar sönmeli");
        }
        assert!(top > 6.0, "yukarı çıkar: {top}");
        // Başın üstünde kısa bir yol: tünek penceresinin pay bıraktığı yükseklikte kalsın.
        assert!(top < 20.0, "{top}");
    }

    #[test]
    fn kalpler_ust_uste_binmez() {
        let (w, _, _) = art(Puff::Heart);
        for seed in 0..20 {
            let mut fx = Fx::new(Rng::new(seed));
            fx.spawn(Puff::Heart, 3);
            for _ in 0..10 {
                fx.step(STEP_MS / 1000.0);
                let xs: Vec<f32> = fx.list.iter().map(|p| p.x).collect();
                for a in 0..xs.len() {
                    for b in a + 1..xs.len() {
                        assert!((xs[a] - xs[b]).abs() >= w as f32 - 1.0, "tohum {seed}: {xs:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn havuz_tasmaz() {
        let mut fx = Fx::new(Rng::new(8));
        for _ in 0..10 {
            fx.spawn(Puff::Note, 2);
            fx.step(0.05);
        }
        assert!(fx.list.len() <= MAX);
        // Uzun duraklamadan dönüş tek adımda sıçratmaz.
        let mut one = Fx::new(Rng::new(9));
        one.spawn(Puff::Heart, 1);
        one.step(30.0);
        assert!(one.list.iter().all(|p| p.y < 5.0));
    }
}
