//! Bu modül "özel tam ekran oyunda şarkı/ses göstergesi nerede çıksın?"
//! sorununu şu yöntemle çözüyor: oyunun monitöründe beliren her pencere ya
//! oyunu masaüstüne düşürür ya da (katmanlıysa) hiç çizilmez. Başka bir
//! monitördeki pencere oyunu etkilemez; gösterge o yüzden oyunun olmadığı
//! monitöre taşınır. Tek monitörde gösterge çıkmaz.

use carlui_core::{config::Config, win};
use windows::Win32::Foundation::RECT;

/// Oyunun olmadığı, göstergenin çıkacağı monitör. Önce adanın ayarlı
/// monitörü, sonra birincil, sonra herhangi biri. Oyunun monitörü
/// bilinmiyorsa ya da başka monitör yoksa `None`.
pub fn osd_monitor(cfg: &Config, game: &str) -> Option<RECT> {
    if game.is_empty() {
        return None;
    }
    let all: Vec<_> = win::monitors().into_iter().map(|m| (win::device_name(m.handle), m)).collect();
    let free = |m: &&(String, win::Monitor)| m.0 != game;
    let n = cfg.general.top_monitor as usize;
    let configured = if n == 0 { all.iter().find(|m| m.1.primary) } else { all.get(n) };
    configured
        .filter(free)
        .or_else(|| all.iter().filter(free).find(|m| m.1.primary))
        .or_else(|| all.iter().find(free))
        .map(|m| m.1.rect)
}
