//! Bu modül "aynı çizim kodu açık temada beyaz cam gibi görünsün" sorununu
//! çözüyor. Çizim kodu koyu cama göre yazılmıştır (beyazın alfası hiyerarşi
//! kurar); süreç açık temaya geçince (`set_light`) renkler rolüne göre çevrilir:
//! koyu perde beyaz cama, beyaz yazı mürekkebe, soluk beyaz dolgu serin bir
//! tül tonuna döner. Yazının alfası ve doygun renkler WCAG AA'yı tutturacak
//! kadar koyulaşır. Koyu temada hiçbir değer değişmez. Belge içeriği gibi
//! renkleri gerçek olan çizimler `literal` içinde yapılır.

use carlui_core::theme::deepen;
use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);
/// Açıkken renkler çevrilmez (ör. Ofis'in beyaz sayfası).
static LITERAL: AtomicBool = AtomicBool::new(false);

/// Bu süreçteki çizimleri açık temaya çevirir.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

pub fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Bu an çizilen renkler çevrilecek mi?
fn mapping() -> bool {
    light() && !LITERAL.load(Ordering::Relaxed)
}

/// `f` içindeki çizimlerin renkleri olduğu gibi kalır (belge içeriği, önizleme).
pub fn literal<T>(f: impl FnOnce() -> T) -> T {
    let was = LITERAL.swap(true, Ordering::Relaxed);
    let out = f();
    LITERAL.store(was, Ordering::Relaxed);
    out
}

/// `yes` ise `literal`: vurgu zemini üstündeki yazı beyaz kalır.
pub fn literal_if<T>(yes: bool, f: impl FnOnce() -> T) -> T {
    if yes { literal(f) } else { f() }
}

/// Ayardaki tema kipini bu sürece uygular.
/// Hız ayarı ve "hareketi azalt" da buradan sürecin geneline geçer
/// (bkz. `carlui_core::theme::motion_ms`).
pub fn follow(t: &carlui_core::config::ThemeConfig) {
    set_light(t.is_light());
    carlui_core::theme::set_motion(t.motion_scale());
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
    caption(hwnd);
    was != light()
}

/// Açık temada başlık çubuğu pencereyle aynı beyaza oturur (yoksa DWM griye boyar).
pub fn caption(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR};
    // COLORREF; 0xFFFFFFFF sistem rengine döner.
    let c: u32 = if light() { 0x00FCFAFA } else { 0xFFFFFFFF };
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &c as *const _ as _, 4);
    }
}

/// Akrilik pencerenin her karedeki zemini (`Clear` düz renk ister, alfayı kendi çarpar). Pencere
/// odağı kaybedince DWM akriliği düz griye çevirir; açık temada bu beyaz perde
/// camı süt beyazında tutar. Koyu temada zemin saydamdır.
pub fn backdrop() -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    let (v, a) = if light() { (1.0, 0.88) } else { (0.0, 0.0) };
    windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: v, g: v, b: v, a }
}

/// Cam yüzeyli modüller (ada, dock, tepsi) renklerini kurulurken alır; tema
/// kipi değişince süreç kapanır, gözetmen onu yeni temayla hemen açar.
/// Hareket ayarı yeniden başlatmadan güncellenir.
pub fn restart_on_change(t: &carlui_core::config::ThemeConfig) {
    carlui_core::theme::set_motion(t.motion_scale());
    if t.is_light() != light() {
        carlui_core::info!("tema değişti; yeniden açılıyor");
        std::process::exit(carlui_core::RELOAD_EXIT);
    }
}

/// Yazı ve simge mürekkebi: saf siyah yerine hafif serin bir lacivert.
const INK: (f32, f32, f32) = (0x14 as f32, 0x18 as f32, 0x26 as f32);
/// Soluk dolgu ve kenarların tonu: gri değil, camın serin gölgesi.
const TINT: u32 = 0x2A365C;
/// Bu kadar koyu griler "perde" sayılır ve beyaz cama döner.
const DARK: u8 = 0x40;
/// Bu kadar açık griler "beyazın alfası" rolündedir.
const PALE: u8 = 0xC0;
/// Doygunluğu bundan azsa renk gri sayılır.
const CHROMA: u8 = 48;
/// Beyaz camın en koyu hâli (koyu duvar kâğıdı üstünde); kontrast buna göre.
const GLASS: u32 = 0xE6E6E6;
/// Beyaz perdenin üst alfası: arkası hep biraz görünsün, cam kalsın.
const VEIL_MAX: f32 = 0.78;

fn split(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn join(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn chroma(r: u8, g: u8, b: u8) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

/// Griyi mürekkep-beyaz ekseninde ters çevirir: koyu tonlar tam beyaza oturur.
fn invert(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    if chroma(r, g, b) > CHROMA {
        return (r, g, b);
    }
    let v = (r as f32 + g as f32 + b as f32) / 3.0;
    let t = ((255.0 - v) / (255.0 - DARK as f32)).clamp(0.0, 1.0);
    let f = |ink: f32| (ink + (255.0 - ink) * t).round() as u8;
    (f(INK.0), f(INK.1), f(INK.2))
}

/// Dolgu rengi ve alfası: perde beyaz cama, soluk beyaz tül tonuna döner.
fn surface(c: u32, a: f32) -> (u32, f32) {
    let (r, g, b) = split(c);
    if chroma(r, g, b) > CHROMA {
        // Dolu düğme beyaz yazı taşır: yarı saydamlık beyaz camda kontrastı AA altına iter.
        return (c, if a >= 0.6 { 1.0 } else { a });
    }
    let hi = r.max(g).max(b);
    if hi <= DARK {
        return (0xFFFFFF, a.max((a * 2.4).min(VEIL_MAX)));
    }
    if hi >= PALE && a < 0.3 {
        // En hafif basamak (0.045-0.05) kart zeminidir: griye değil, beyaza çıkar.
        return if a <= 0.055 { (0xFFFFFF, a * 11.0) } else { (TINT, a * 1.3) };
    }
    let (r, g, b) = invert(r, g, b);
    (join(r, g, b), a)
}

/// Yazı rengi ve alfası: soluk basamaklar AA'ya çıkar, doygun renk koyulaşır.
fn writing(c: u32, a: f32) -> (u32, f32) {
    let (r, g, b) = split(c);
    let a = a.clamp(0.0, 1.0).sqrt();
    if chroma(r, g, b) > CHROMA {
        // Alfa kontrastı biraz düşürür; hedef bu payı içerir.
        return (deepen(c, GLASS, 5.5), a);
    }
    let (r, g, b) = invert(r, g, b);
    (join(r, g, b), a)
}

/// Rengi geçerli temaya çevirir (koyu temada değişmez).
pub fn rgb8(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    if !mapping() {
        return (r, g, b);
    }
    invert(r, g, b)
}

/// `0xRRGGBB` biçimi için `rgb8`.
pub fn hex(c: u32) -> u32 {
    let (r, g, b) = split(c);
    let (r, g, b) = rgb8(r, g, b);
    join(r, g, b)
}

/// Dolgu ve kenarlar için renk + alfa (koyu temada değişmez).
pub fn fill(c: u32, a: f32) -> (u32, f32) {
    if mapping() { surface(c, a) } else { (c, a) }
}

/// Yazı ve simgeler için renk + alfa (koyu temada değişmez).
pub fn ink(c: u32, a: f32) -> (u32, f32) {
    if mapping() { writing(c, a) } else { (c, a) }
}

/// Yalnızca alfası verilen yazılar (beyaz cam yazıları) için `ink`.
pub fn ink_alpha(a: f32) -> f32 {
    ink(0xFFFFFF, a).1
}

/// Beyaz zeminde seçilmesi gereken ince renkli işaretler (grafik, çubuk): 3:1.
pub fn mark(c: u32) -> u32 {
    if mapping() { deepen(c, GLASS, 3.0) } else { c }
}

/// Vurgu rengi: açık temada üstündeki beyaz yazı okunacak kadar koyu ve doygun.
pub fn accent(c: u32) -> u32 {
    if !light() {
        return c;
    }
    carlui_core::theme::light_accent(c)
}

/// 0-1 aralığındaki kanallar için `rgb8`.
pub fn unit(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    if !mapping() {
        return (r, g, b);
    }
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = invert(q(r), q(g), q(b));
    (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use carlui_core::theme::contrast;

    /// `c` rengini `a` alfasıyla `on` üstüne bindirir.
    fn over(c: u32, a: f32, on: u32) -> u32 {
        let (x, y) = (split(c), split(on));
        let m = |p: u8, q: u8| (p as f32 * a + q as f32 * (1.0 - a)).round() as u8;
        join(m(x.0, y.0), m(x.1, y.1), m(x.2, y.2))
    }

    #[test]
    fn inverts_only_grays() {
        assert_eq!(invert(255, 255, 255), (0x14, 0x18, 0x26));
        assert_eq!(invert(0, 0, 0), (255, 255, 255));
        assert_eq!(invert(14, 16, 28), (255, 255, 255));
        assert_eq!(invert(0x4F, 0x8B, 0xFF), (0x4F, 0x8B, 0xFF));
        assert_eq!(invert(0xFF, 0x6B, 0x6B), (0xFF, 0x6B, 0x6B));
    }

    #[test]
    fn dark_veils_become_white_glass() {
        assert_eq!(surface(0x101014, 0.30).0, 0xFFFFFF);
        assert!((surface(0x101014, 0.30).1 - 0.72).abs() < 1e-4);
        assert_eq!(surface(0x202024, 0.96), (0xFFFFFF, 0.96));
        assert_eq!(surface(0x000000, 0.0), (0xFFFFFF, 0.0));
    }

    #[test]
    fn pale_fills_never_turn_gray() {
        assert_eq!(surface(0xFFFFFF, 0.045).0, 0xFFFFFF);
        assert_eq!(surface(0xFFFFFF, 0.10).0, TINT);
    }

    #[test]
    fn text_steps_pass_aa_on_glass() {
        for a in [0.95, 0.9, 0.7, 0.55, 0.45] {
            let (c, a) = writing(0xFFFFFF, a);
            assert!(contrast(over(c, a, GLASS), GLASS) >= 4.5, "alfa {a}");
        }
        for c in [0xFF6B6B, 0xFFB454, 0xFFD43B, 0x6CCB5F, 0x60CDFF, 0xC084FC] {
            let (c, a) = writing(c, 0.95);
            assert!(contrast(over(c, a, GLASS), GLASS) >= 4.5, "renk {c:06X}");
        }
    }

    #[test]
    fn accent_carries_white_text() {
        LIGHT.store(true, Ordering::Relaxed);
        for c in [0x60CDFF, 0xE8A0FF, 0x9CA3AF, 0xFFD43B] {
            let out = accent(c);
            let (r, g, b) = split(out);
            assert!(contrast(out, 0xFFFFFF) >= 4.5, "vurgu {c:06X}");
            assert!(chroma(r, g, b) > CHROMA);
        }
        LIGHT.store(false, Ordering::Relaxed);
    }
}
