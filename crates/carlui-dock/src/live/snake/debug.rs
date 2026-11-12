//! Yalnız geliştirme: kum havuzunda (`CARLUI_DOCK_SANDBOX=1` +
//! `CARLUI_LIVE_DEBUG`) seyrek durumları elle zorlar, canlı yakalama için.
//! Ortam değişkenleri yoksa `Force::read` hep `None` döner; normal sürüm
//! davranışı hiç değişmez. Gerçek imleç hiçbir zaman oynatılmaz: izleme
//! denemesi sahte bir imleç konumu besler.

use super::brain::Brain;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    /// Kayarken hemen dock'a sarılsın (helezon).
    Wrap,
    /// Sahte imleç: 4 sn başın önünde-üstünde, 4 sn yok.
    Track,
}

/// Sahte imleç döngüsünün yarı periyodu (sn).
const TRACK_HALF: f32 = 4.0;

impl Force {
    pub fn read() -> Option<Self> {
        match crate::sandbox::snake_force()?.as_str() {
            "wrap" => Some(Self::Wrap),
            "track" => Some(Self::Track),
            _ => None,
        }
    }

    /// Adımdan önce beyni dürter; izleme kipinde verilecek sahte imleci
    /// (dünya, sanat pikseli) döndürür. `t` zorlamanın kendi saati.
    pub fn apply(self, b: &mut Brain, t: f32, lo: f32, hi: f32) -> Option<(f32, f32)> {
        match self {
            Self::Wrap => {
                b.force_wrap(lo, hi);
                None
            }
            Self::Track => {
                // Kıvrıkken de, kayarken de başın 30 px önü ve 40 px üstü.
                let on = (t / TRACK_HALF) as u32 % 2 == 0;
                let (x, y) = b.look_from();
                on.then(|| (x + b.dir * 30.0, y - 40.0))
            }
        }
    }
}
