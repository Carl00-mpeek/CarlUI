//! Turun ilk ve son sayfası: kısa tanıtım (şu anki görünümün önizlemesi ve
//! turun haritası) ve "neler yapabilirsin" özeti (dört kart, gerçek
//! kısayollar, seçimlerin kısa özeti, "her şey Ayarlar'da" notu).

use super::{
    choices::{self, Highlight},
    model::{Module, Nav, Step},
    steps::GAP,
    Go, Welcome, W,
};
use crate::pages::looks_preview;
use carlui_core::{
    config::DockPosition,
    looks::{self, builtin, Which},
};
use carlui_ui::{
    paint::{mix, wrap, Align, R, TEXT},
    tween,
};

/// Tanıtım: büyük önizleme, altında turun adımları (tıklanınca o adıma gider).
pub(super) fn hello(app: &Welcome, p: &mut W, r: R) {
    let pr = R::new(r.x, r.y, r.w, 196.0);
    p.round(pr, app.round(10.0), 0xFFFFFF, 0.03);
    p.outline(pr, app.round(10.0), 0xFFFFFF, 0.06);
    // Önizleme ekran oranında (16:9) ortada; geniş alana yayılınca duvar
    // kâğıdı lekesi dev bir hap gibi görünüyordu.
    let ph = pr.h - 16.0;
    let pw = (ph * 16.0 / 9.0).min(pr.w - 16.0);
    let screen = R::new(pr.x + (pr.w - pw) / 2.0, pr.y + 8.0, pw, ph);
    looks_preview::draw(p, screen, &looks::capture(&app.cfg, ""), app.cfg.theme.wallpaper_accent.as_str());

    let map: [(char, &str, Step); 5] = [
        ('\u{E790}', t!("Görünüm", "Look"), Step::Look),
        ('\u{E76E}', t!("Hayvan", "Pet"), Step::Pet),
        ('\u{E8A9}', "Dock", Step::Dock),
        ('\u{E7F4}', t!("Ada", "Island"), Step::Island),
        ('\u{E713}', t!("Modüller", "Modules"), Step::Modules),
    ];
    let n = map.len() as f32;
    let tw = (r.w - GAP * (n - 1.0)) / n;
    let y = pr.bottom() + 16.0;
    for (i, (glyph, label, step)) in map.iter().enumerate() {
        let t = R::new(r.x + i as f32 * (tw + GAP), y, tw, 58.0);
        let hv = tween::to(tween::at("w-map", t.x, t.y), if p.hover(t) { 1.0 } else { 0.0 }, 18.0);
        p.round(t, app.round(10.0), 0xFFFFFF, 0.04 + 0.04 * hv);
        p.outline(t, app.round(10.0), 0xFFFFFF, 0.07);
        let dot = R::new(t.x + 12.0, t.y + (t.h - 30.0) / 2.0, 30.0, 30.0);
        p.round(dot, 15.0, p.accent, 0.18);
        p.glyph(*glyph, dot, 13.0, p.accent, 1.0);
        p.text(&format!("{}", i + 1), R::new(t.right() - 26.0, t.y + 6.0, 18.0, 16.0), 11.0, false, TEXT, 0.4, Align::Right);
        p.text(label, R::new(dot.right() + 10.0, t.y, t.right() - dot.right() - 18.0, t.h), 13.0, true, TEXT, 0.9, Align::Left);
        p.hit(t, Go::Nav(Nav::Go(*step)));
    }

    let note = t!(
        "Yaklaşık bir dakika sürer. Her adımı geçebilir ya da turun tamamını atlayabilirsin; seçimlerin hemen uygulanır.",
        "It takes about a minute. You can pass any step or skip the whole tour; your choices apply right away."
    );
    let lines = wrap(note, r.w, |s| p.fonts.width(s, 12.5, false));
    for (k, l) in lines.iter().take(2).enumerate() {
        p.text(l, R::new(r.x, y + 74.0 + k as f32 * 18.0, r.w, 18.0), 12.5, false, TEXT, 0.55, Align::Left);
    }
}

/// Özet: 2×2 kart, seçimlerin kısa özeti ve "sonra değiştirebilirsin" notu.
pub(super) fn finish(app: &Welcome, p: &mut W, r: R) {
    let cards = choices::highlights(&app.cfg);
    let ch = 132.0;
    let cw = (r.w - GAP) / 2.0;
    for (i, h) in cards.iter().enumerate() {
        let c = R::new(r.x + (i % 2) as f32 * (cw + GAP), r.y + (i / 2) as f32 * (ch + GAP), cw, ch);
        highlight(app, p, c, h);
    }

    let y = r.y + 2.0 * ch + GAP + 14.0;
    p.text(&summary(app), R::new(r.x, y, r.w, 22.0), 13.0, false, TEXT, 0.62, Align::Left);
    let later = t!(
        "Her şeyi sonra Ayarlar'dan değiştirebilirsin; turu Ayarlar > Genel'den yeniden açabilirsin.",
        "You can change everything later in Settings, and reopen this tour from Settings > General."
    );
    let lines = wrap(later, r.w, |s| p.fonts.width(s, 12.0, false));
    for (k, l) in lines.iter().take(2).enumerate() {
        p.text(l, R::new(r.x, y + 24.0 + k as f32 * 18.0, r.w, 18.0), 12.0, false, TEXT, 0.45, Align::Left);
    }
}

/// Seçilen görünüm, hayvan, dock ve açık modül sayısı tek satırda.
fn summary(app: &Welcome) -> String {
    let all = builtin::all();
    let look = match looks::current(&app.cfg, &all, &[]) {
        Which::Builtin(i) => all.get(i).map(|l| l.name.clone()).unwrap_or_default(),
        _ => t!("Özel", "Custom").to_string(),
    };
    let dock = match (app.cfg.dock.taskbar, app.cfg.dock.position) {
        (true, _) => t!("görev çubuğu", "taskbar"),
        (false, DockPosition::Bottom) => t!("altta dock", "dock at the bottom"),
        (false, DockPosition::Left) => t!("solda dock", "dock on the left"),
        (false, DockPosition::Right) => t!("sağda dock", "dock on the right"),
    };
    let pet = choices::pet_label(choices::pet_main(&app.cfg.dock.live.pet));
    let on = Module::ALL.iter().filter(|m| m.get(&app.cfg)).count();
    let n = Module::ALL.len();
    t!(format!("Görünüm: {look} · hayvan: {pet} · {dock} · {on}/{n} modül açık"), format!("Look: {look} · pet: {pet} · {dock} · {on}/{n} modules on"))
}

/// Tek "neler yapabilirsin" kartı: simge, başlık, açıklama, altta kısayol satırları.
fn highlight(app: &Welcome, p: &mut W, c: R, h: &Highlight) {
    const KEY_H: f32 = 26.0;
    let rad = app.round(10.0);
    p.round(c, rad, 0xFFFFFF, 0.045);
    p.outline(c, rad, 0xFFFFFF, 0.07);
    let dot = R::new(c.x + 14.0, c.y + 14.0, 30.0, 30.0);
    p.round(dot, 15.0, p.accent, 0.18);
    p.glyph(h.glyph, dot, 13.0, p.accent, 1.0);
    p.text(h.title, R::new(dot.right() + 10.0, dot.y, c.right() - dot.right() - 24.0, dot.h), 14.0, true, TEXT, 0.95, Align::Left);

    let tw = c.w - 28.0;
    let keys_top = c.bottom() - 8.0 - KEY_H * h.keys.len() as f32;
    let room = (((keys_top - (c.y + 52.0)) / 17.0).floor().max(1.0)) as usize;
    let lines = wrap(h.desc, tw, |s| p.fonts.width(s, 12.0, false));
    for (k, l) in lines.iter().take(room).enumerate() {
        p.text(l, R::new(c.x + 14.0, c.y + 52.0 + k as f32 * 17.0, tw, 17.0), 12.0, false, TEXT, 0.6, Align::Left);
    }
    for (k, (label, key)) in h.keys.iter().enumerate() {
        let row = R::new(c.x + 14.0, keys_top + k as f32 * KEY_H, tw, KEY_H);
        p.text(label, R::new(row.x, row.y, row.w * 0.5, row.h), 12.0, false, TEXT, 0.75, Align::Left);
        key_chip(app, p, row, key);
    }
}

/// Sağa yaslı kısayol etiketi ("Ctrl+Alt+V").
fn key_chip(app: &Welcome, p: &W, row: R, key: &str) {
    let kw = p.fonts.width(key, 12.0, true) + 18.0;
    let k = R::new(row.right() - kw, row.y + (row.h - 22.0) / 2.0, kw, 22.0);
    p.round(k, app.round(5.0), mix(0xFFFFFF, p.accent, 0.3), 0.10);
    p.outline(k, app.round(5.0), p.accent, 0.45);
    p.text(key, k, 12.0, true, TEXT, 0.92, Align::Center);
}
