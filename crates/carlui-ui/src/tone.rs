//! Bu modül "aynı çizim kodu açık temada da okunur olsun" sorununu çözüyor:
//! süreç açık temaya geçince (`set_light`) gri tonlu renkler ters çevrilir —
//! beyaz yazı koyulaşır, koyu cam açılır. Doygun renkler (vurgu, kırmızı,
//! yeşil) olduğu gibi kalır. Yalnızca açık temayı destekleyen uygulamalar
//! bayrağı açar; Ofis kendi beyaz sayfasını çizdiği için açmaz.

use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

/// Bu süreçteki çizimleri açık temaya çevirir.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Ayardaki tema kipini bu sürece uygular.
pub fn follow(t: &carlui_core::config::ThemeConfig) {
    set_light(t.is_light());
}

/// Ayardaki tema kipini uygular ve pencerenin başlık/arka plan temasını
/// (Mica/akrilik) buna göre değiştirir; tema değiştiyse `true`.
pub fn follow_window(t: &carlui_core::config::ThemeConfig, hwnd: windows::Win32::Foundation::HWND) -> bool {
    let was = light();
    follow(t);
    let on = i32::from(!light());
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(hwnd, windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
    }
    was != light()
}

/// Cam yüzeyli modüller (ada, dock, tepsi) renklerini kurulurken alır; tema
/// kipi değişince süreç kapanır, gözetmen onu yeni temayla hemen açar.
pub fn restart_on_change(t: &carlui_core::config::ThemeConfig) {
    if t.is_light() != light() {
        carlui_core::info!("tema değişti; yeniden açılıyor");
        std::process::exit(carlui_core::RELOAD_EXIT);
    }
}

/// Açık temada beyazın düştüğü koyu ton.
const INK: f32 = 0x1C as f32;

/// Rengi geçerli temaya çevirir (koyu temada değişmez).
pub fn rgb8(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    if !light() {
        return (r, g, b);
    }
    invert(r, g, b)
}

fn invert(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    if hi - lo > 48 {
        return (r, g, b);
    }
    let f = |v: u8| (INK + (255.0 - v as f32) * (255.0 - INK) / 255.0).round() as u8;
    (f(r), f(g), f(b))
}

/// `0xRRGGBB` biçimi için `rgb8`.
pub fn hex(c: u32) -> u32 {
    let (r, g, b) = rgb8((c >> 16) as u8, (c >> 8) as u8, c as u8);
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// 0-1 aralığındaki kanallar için `rgb8`.
pub fn unit(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    if !light() {
        return (r, g, b);
    }
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = invert(q(r), q(g), q(b));
    (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverts_only_grays() {
        assert_eq!(invert(255, 255, 255), (0x1C, 0x1C, 0x1C));
        assert_eq!(invert(0, 0, 0), (255, 255, 255));
        assert_eq!(invert(14, 16, 28), (243, 241, 230));
        assert_eq!(invert(0x4F, 0x8B, 0xFF), (0x4F, 0x8B, 0xFF));
        assert_eq!(invert(0xFF, 0x6B, 0x6B), (0xFF, 0x6B, 0x6B));
    }
}
