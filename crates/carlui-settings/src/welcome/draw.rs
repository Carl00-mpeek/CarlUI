//! Karşılama penceresinin çerçevesi: ilerleme göstergesi, başlık, kayarak
//! gelen adım içeriği ve alttaki düğmeler (Atla, Geri, İleri/Geç, Bitti).

use super::{model::Nav, model::Step, steps, Go, Welcome, W};
use carlui_ui::{
    paint::{mix, wrap, Align, R, TEXT},
    tween,
};
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_ALIASED, D2D1_LAYER_PARAMETERS};

/// Kenar boşluğu, alt çubuğun yüksekliği ve düğme yüksekliği.
const PAD: f32 = 32.0;
const FOOT: f32 = 64.0;
const BTN_H: f32 = 34.0;

pub(super) fn frame(app: &Welcome, p: &mut W, w: f32, h: f32) {
    // Cam yoğunluğu arttıkça zemin koyulaşır (ayarlar penceresiyle aynı his).
    let glass = app.cfg.theme.glass_density.clamp(0.0, 1.0);
    p.fill(R::new(0.0, 0.0, w, h), 0x101014, 0.18 + 0.24 * glass);

    progress(app, p, w);

    let step = app.step;
    let cw = w - PAD * 2.0;
    p.text(step.title(), R::new(PAD, 46.0, cw, 34.0), 22.0, true, TEXT, 0.95, Align::Left);
    let lines = wrap(step.subtitle(), cw, |s| p.fonts.width(s, 13.0, false));
    for (i, l) in lines.iter().take(2).enumerate() {
        p.text(l, R::new(PAD, 82.0 + i as f32 * 19.0, cw, 19.0), 13.0, false, TEXT, 0.62, Align::Left);
    }

    // İçerik: yeni adım geldiği yönden kayarak ve belirerek girer.
    let top = 132.0;
    let area = R::new(PAD, top, cw, h - top - FOOT - 8.0);
    let slide = tween::to(tween::key("w-step"), 0.0, 13.0);
    let clip = R::new(0.0, top - 4.0, w, area.h + 8.0);
    unsafe {
        p.rt.PushAxisAlignedClip(&clip.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
        let lp = D2D1_LAYER_PARAMETERS { contentBounds: clip.d2d(), opacity: (1.0 - slide.abs() / 30.0).clamp(0.0, 1.0), ..Default::default() };
        p.rt.PushLayer(&lp, None);
    }
    // Tween anahtarları adıma göre ayrılır: adım değişince önceki adımın
    // öğelerinin "seçilip bırakılma" animasyonu yeni adımda oynamaz.
    tween::place(step.index() as u64 + 1, 0.0, 0.0);
    steps::paint(app, p, R::new(area.x + slide, area.y, area.w, area.h));
    tween::place(0, 0.0, 0.0);
    unsafe {
        p.rt.PopLayer();
        p.rt.PopAxisAlignedClip();
    }

    footer(app, p, w, h);
}

/// Üstte dört adım çizgisi (tıklanınca o adıma gider) ve "Adım n/4".
fn progress(app: &Welcome, p: &mut W, w: f32) {
    let cur = app.step.index();
    let n = Step::COUNT;
    let (gap, sw) = (6.0, 44.0);
    for (i, s) in Step::ALL.iter().take(n).enumerate() {
        let r = R::new(PAD + i as f32 * (sw + gap), 22.0, sw, 4.0);
        let t = tween::to(tween::at("w-dot", r.x, 0.0), if i <= cur { 1.0 } else { 0.0 }, 14.0);
        p.round(r, 2.0, 0xFFFFFF, 0.14);
        if t > 0.0 {
            let a = if i == cur { 1.0 } else { 0.55 };
            p.round(R::new(r.x, r.y, r.w * t, r.h), 2.0, p.accent, a);
        }
        let hit = R::new(r.x, r.y - 8.0, r.w, r.h + 16.0);
        if p.hover(hit) && i != cur {
            p.round(r, 2.0, 0xFFFFFF, 0.18);
        }
        p.hit(hit, Go::Nav(Nav::Go(*s)));
    }
    let label = if app.step == Step::Finish {
        t!("Tamam", "Done").to_string()
    } else {
        t!(format!("Adım {}/{}", cur + 1, n), format!("Step {} of {}", cur + 1, n))
    };
    p.text(&label, R::new(w - PAD - 160.0, 12.0, 160.0, 24.0), 12.0, false, TEXT, 0.55, Align::Right);
}

fn footer(app: &Welcome, p: &mut W, w: f32, h: f32) {
    let y = h - FOOT + (FOOT - BTN_H) / 2.0 - 4.0;
    p.fill(R::new(0.0, h - FOOT - 4.0, w, 1.0), 0xFFFFFF, 0.06);
    let width = |p: &W, s: &str| (p.fonts.width(s, 13.0, false) + 36.0).max(96.0);

    if app.step == Step::Finish {
        let done = t!("Bitti", "Done");
        let dw = width(p, done);
        let r = R::new(w - PAD - dw, y, dw, BTN_H);
        primary(app, p, r, done, Go::Nav(Nav::Finish));
        let open = t!("Tüm ayarları aç", "Open all settings");
        let ow = width(p, open) + 22.0;
        p.button(R::new(r.x - 10.0 - ow, y, ow, BTN_H), Some('\u{E713}'), open, Go::OpenSettings, true, false);
        return;
    }

    // Solda "Atla": akışın tamamını atlar, ayara dokunmaz.
    let skip = t!("Atla", "Skip");
    let sr = R::new(PAD - 10.0, y, p.fonts.width(skip, 13.0, false) + 20.0, BTN_H);
    let hv = tween::to(tween::at("w-skip", sr.x, sr.y), if p.hover(sr) { 1.0 } else { 0.0 }, 18.0);
    if hv > 0.0 {
        p.round(sr, app.round(6.0), 0xFFFFFF, 0.07 * hv);
    }
    p.text(skip, sr, 13.0, false, TEXT, 0.6 + 0.3 * hv, Align::Center);
    p.hit(sr, Go::Nav(Nav::Skip));

    // Sağda birincil düğme: bir şey seçildiyse "İleri", seçilmediyse "Geç".
    let next = if app.touched[app.step.index()] { t!("İleri", "Next") } else { t!("Geç", "Skip step") };
    let nw = width(p, next);
    let r = R::new(w - PAD - nw, y, nw, BTN_H);
    primary(app, p, r, next, Go::Nav(Nav::Next));
    if app.step != Step::Look {
        let back = t!("Geri", "Back");
        let bw = width(p, back);
        p.button(R::new(r.x - 10.0 - bw, y, bw, BTN_H), None, back, Go::Nav(Nav::Back), true, false);
    }
}

/// Vurgu renginde dolu düğme (Enter ile de çalışır).
fn primary(app: &Welcome, p: &mut W, r: R, label: &str, go: Go) {
    let hv = tween::to(tween::at("w-primary", r.x, r.y), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    let rad = app.round(6.0);
    p.round(r, rad, mix(p.accent, 0xFFFFFF, 0.12 * hv), 0.95);
    p.text(label, r, 13.0, true, 0x000000, 0.88, Align::Center);
    p.hit(r, go);
}
