//! Kasa penceresinin çizimi. Görev yöneticisi ve ayarlar gibi anlık modlu:
//! her karede baştan çizilir, tıklanabilir her şey (dikdörtgen, `Act`) olarak
//! kaydedilir. Kilit ekranları ortada tek bir kart; açık kasa solda
//! kategoriler, sağda liste / düzenleyici / güvenlik sayfası.

pub mod clip;
mod editor;
mod import;
mod list;
mod lock;
mod security;
pub mod textbox;

use crate::app::{Act, App, Focus, View};
use carlui_ui::paint::{Align, Painter, R, TEXT};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED;

pub type P<'a> = Painter<'a, Act>;

pub const SIDEBAR: f32 = 220.0;

pub fn ctrl_down() -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0 }
}

/// Son klavye/fare kullanımından bu yana geçen süre (tüm sistem).
pub fn idle_ms() -> u64 {
    use windows::Win32::{System::SystemInformation::GetTickCount, UI::Input::KeyboardAndMouse::*};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return 0;
        }
        GetTickCount().wrapping_sub(li.dwTime) as u64
    }
}

/// İmleç şu an görünür mü (yanıp sönme).
pub fn caret(app: &App) -> bool {
    (app.caret_since.elapsed().as_millis() / 530).is_multiple_of(2)
}

#[allow(clippy::too_many_arguments)]
/// Sözcüklere bölüp satır satır yazar; kullanılan yüksekliği döner.
pub fn para(p: &P, text: &str, x: f32, y: f32, w: f32, size: f32, color: u32, alpha: f32) -> f32 {
    let lh = size * 1.45;
    let mut line = String::new();
    let mut yy = y;
    for word in text.split(' ') {
        let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && p.fonts.width(&cand, size, false) > w {
            p.text(&line, R::new(x, yy, w, lh), size, false, color, alpha, Align::Left);
            yy += lh;
            line = word.to_string();
        } else {
            line = cand;
        }
    }
    if !line.is_empty() {
        p.text(&line, R::new(x, yy, w, lh), size, false, color, alpha, Align::Left);
        yy += lh;
    }
    yy - y
}

/// Metin kutusu + tıklayınca odak.
pub fn input(p: &mut P, app: &App, r: R, f: Focus, masked: bool, placeholder: &str) {
    let focused = app.focus == Some(f);
    let tb = match (f, &app.view) {
        (Focus::Form(i), _) => &app.form[i],
        (Focus::Search, _) => &app.search,
        (Focus::Name, View::Edit(e)) => &e.name,
        (Focus::Label(i), View::Edit(e)) => &e.fields[i].label,
        (Focus::Value(i), View::Edit(e)) => &e.fields[i].value,
        _ => return,
    };
    textbox::draw(p, r, tb, focused, masked, placeholder, caret(app));
    p.hit(r, Act::Focus(f));
}

/// Küçük simge düğmesi.
pub fn icon_button(p: &mut P, r: R, glyph: char, act: Act, tip_alpha: f32) {
    if p.hover(r) {
        p.round(r, 6.0, 0xFFFFFF, 0.10);
    }
    p.glyph(glyph, r, 14.0, TEXT, tip_alpha);
    p.hit(r, act);
}

/// Bağlantı gibi görünen yazı düğmesi.
pub fn link(p: &mut P, r: R, label: &str, act: Act) {
    let a = if p.hover(r) { 1.0 } else { 0.85 };
    p.text(label, r, 13.0, false, p.accent, a, Align::Center);
    p.hit(r, act);
}

/// Vurgu renkli ana düğme.
pub fn primary(p: &mut P, r: R, label: &str, act: Act, enabled: bool) {
    let hot = enabled && p.hover(r);
    p.round(r, 6.0, p.accent, if !enabled { 0.35 } else if hot { 1.0 } else { 0.88 });
    p.text(label, r, 13.5, true, 0x000000, if enabled { 0.88 } else { 0.5 }, Align::Center);
    if enabled {
        p.hit(r, act);
    }
}

/// Kırpılmış, kaydırılabilir bölge: dışında kalan tıklama alanları atılır.
pub fn clipped(p: &mut P, clip: R, body: impl FnOnce(&mut P)) {
    p.hit(clip, Act::Scroll);
    let start = p.hits.len();
    unsafe { p.rt.PushAxisAlignedClip(&clip.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
    body(p);
    unsafe { p.rt.PopAxisAlignedClip() };
    let tail = p.hits.split_off(start);
    for (r, a) in tail {
        let top = r.y.max(clip.y);
        let bottom = r.bottom().min(clip.bottom());
        if bottom > top {
            p.hits.push((R::new(r.x, top, r.w, bottom - top), a));
        }
    }
}

/// Unix saniyesini gg.aa.yyyy yazar (yerel saat dilimi önemsiz: yalnızca gün).
pub fn date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Howard Hinnant'ın civil_from_days algoritması.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{d:02}.{m:02}.{y}")
}

/// Tüm pencereyi çizer; kaydırılabilir içeriğin taşan yüksekliğini döner.
pub fn paint(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    p.fill(R::new(0.0, 0.0, w, h), 0x101014, 0.30);
    match &app.view {
        View::Setup => lock::setup(app, p, w, h),
        View::ShowRecovery { key, .. } => lock::show_recovery(app, p, w, h, key),
        View::Locked => lock::locked(app, p, w, h),
        View::Recover => lock::recover(app, p, w, h),
        View::NewPassword => lock::new_password(app, p, w, h),
        View::List => {
            list::sidebar(app, p, h);
            list::paint(app, p, w, h)
        }
        View::Edit(e) => {
            list::sidebar(app, p, h);
            editor::paint(app, e, p, w, h)
        }
        View::Security => {
            list::sidebar(app, p, h);
            security::paint(app, p, w, h)
        }
        View::Import(pv) => {
            list::sidebar(app, p, h);
            import::paint(app, pv, p, w, h)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::date(0), "01.01.1970");
        assert_eq!(super::date(1_793_750_400), "04.11.2026");
    }
}
