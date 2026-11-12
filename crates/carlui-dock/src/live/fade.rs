//! Sahne değişirken yumuşak geçiş: yeni sahne ~2 sn'de belirir, eskisi aynı
//! sürede silinir. Saf zaman işlevleri; saydamlığı `scene.rs` uygular.

use std::time::{Duration, Instant};

/// Geçişin süresi.
pub const FADE: Duration = Duration::from_millis(2000);
/// Geçiş sırasında kare aralığı (ms): sahnenin kendi aralığı daha seyrekse bu kullanılır.
pub const FADE_FRAME_MS: u64 = 50;

#[derive(Clone, Copy, Debug)]
pub struct Fade {
    start: Instant,
    /// Başladığı görünürlük (geçiş yarıda kesilip ters dönerse sıçrama olmasın).
    base: f32,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Fade {
    pub fn new(start: Instant, base: f32) -> Self {
        Self { start, base: base.clamp(0.0, 1.0) }
    }

    fn t(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32() / FADE.as_secs_f32()
    }

    /// Beliren sahnenin görünürlüğü (0 → 1).
    pub fn up(&self, now: Instant) -> f32 {
        self.base + (1.0 - self.base) * smooth(self.t(now))
    }

    /// Silinen sahnenin görünürlüğü (başlangıç → 0).
    pub fn down(&self, now: Instant) -> f32 {
        self.base * (1.0 - smooth(self.t(now)))
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= FADE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gecis_uclari_ve_tekduze() {
        let t0 = Instant::now();
        let up = Fade::new(t0, 0.0);
        assert_eq!(up.up(t0), 0.0);
        assert_eq!(up.up(t0 + FADE), 1.0);
        let down = Fade::new(t0, 1.0);
        assert_eq!(down.down(t0), 1.0);
        assert_eq!(down.down(t0 + FADE), 0.0);
        let mut last = 0.0;
        for ms in (0..=2000).step_by(100) {
            let v = up.up(t0 + Duration::from_millis(ms));
            assert!(v >= last && (0.0..=1.0).contains(&v));
            last = v;
        }
        assert!(!up.done(t0 + Duration::from_millis(1999)));
        assert!(up.done(t0 + FADE));
        // Yarıdan ters dönen geçiş sıçramaz.
        let half = Fade::new(t0, 0.5);
        assert_eq!(half.down(t0), 0.5);
    }
}
