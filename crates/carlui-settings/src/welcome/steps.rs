//! Karşılama adımlarının içeriği. Her adım kendisine verilen alana çizer;
//! seçili olan, ayara bakılarak hesaplanır (pencerede ayrı durum tutulmaz).

use super::{
    model::{self, DockChoice, Module, Step},
    Go, Welcome, W,
};
use crate::pages::looks_preview;
use carlui_core::{
    config::{DockPosition, IslandStyle, TaskbarSize},
    looks::{self, builtin, Which},
};
use carlui_ui::{
    paint::{mix, wrap, Align, R, TEXT},
    tween,
};

const GAP: f32 = 12.0;

pub(super) fn paint(app: &Welcome, p: &mut W, r: R) {
    match app.step {
        Step::Look => look(app, p, r),
        Step::Dock => dock(app, p, r),
        Step::Island => island(app, p, r),
        Step::Modules => modules(app, p, r),
        Step::Finish => finish(app, p, r),
    }
}

/// Seçilebilir kart zemini: üstüne gelince aydınlanır, seçiliyken vurgu kenarı.
fn card(app: &Welcome, p: &mut W, r: R, selected: bool, go: Go) {
    let rad = app.round(10.0);
    let hv = tween::to(tween::at("w-card", r.x, r.y), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    let sel = tween::to(tween::at("w-sel", r.x, r.y), if selected { 1.0 } else { 0.0 }, 14.0);
    p.round(r, rad, 0xFFFFFF, 0.045 + 0.04 * hv);
    p.outline(r, rad, 0xFFFFFF, 0.07);
    if sel > 0.0 {
        p.outline(r, rad, p.accent, 0.95 * sel);
        p.outline(r.inset(1.0), (rad - 1.0).max(0.0), p.accent, 0.55 * sel);
    }
    p.hit(r, go);
}

/// Kartın alt satırı: seçiliyse onay işareti ve ad.
fn name_row(p: &W, r: R, name: &str, selected: bool) {
    let mut x = r.x;
    if selected {
        p.glyph('\u{E73E}', R::new(x, r.y, 16.0, r.h), 12.0, p.accent, 1.0);
        x += 18.0;
    }
    p.text(name, R::new(x, r.y, r.right() - x, r.h), 13.0, selected, TEXT, 0.9, Align::Left);
}

fn look(app: &Welcome, p: &mut W, r: R) {
    let all = builtin::all();
    let which = looks::current(&app.cfg, &all, &[]);
    let cols = 3usize;
    let rows = all.len().div_ceil(cols).max(1);
    let cw = (r.w - GAP * (cols as f32 - 1.0)) / cols as f32;
    let ch = ((r.h - GAP * (rows as f32 - 1.0)) / rows as f32).min(180.0);
    let wall = app.cfg.theme.wallpaper_accent.as_str();
    for (i, l) in all.iter().enumerate() {
        let c = R::new(r.x + (i % cols) as f32 * (cw + GAP), r.y + (i / cols) as f32 * (ch + GAP), cw, ch);
        let selected = which == Which::Builtin(i);
        card(app, p, c, selected, Go::Look(i));
        looks_preview::draw(p, R::new(c.x + 8.0, c.y + 8.0, c.w - 16.0, c.h - 44.0), l, wall);
        name_row(p, R::new(c.x + 12.0, c.bottom() - 34.0, c.w - 20.0, 30.0), &l.name, selected);
    }
}

fn dock(app: &Welcome, p: &mut W, r: R) {
    let (pos, style) = model::dock_selection(&app.cfg);
    let label_w = 110.0;
    let seg_w = (r.w - label_w).min(460.0);
    let row = |p: &W, y: f32, s: &str| p.text(s, R::new(r.x, y, label_w, 34.0), 13.0, true, TEXT, 0.85, Align::Left);

    row(p, r.y, t!("Konum", "Position"));
    let at = |d| Go::Dock(DockChoice::Position(d));
    p.segments(R::new(r.x + label_w, r.y, seg_w, 34.0), &[(t!("Alt", "Bottom"), at(DockPosition::Bottom)), (t!("Sol", "Left"), at(DockPosition::Left)), (t!("Sağ", "Right"), at(DockPosition::Right))], pos);

    let y = r.y + 46.0;
    row(p, y, t!("Biçim", "Style"));
    let tb = |s| Go::Dock(DockChoice::Taskbar(s));
    p.segments(R::new(r.x + label_w, y, seg_w, 34.0), &[(t!("Yüzen dock", "Floating dock"), Go::Dock(DockChoice::Minimal)), (t!("Kısa görev çubuğu", "Compact taskbar"), tb(TaskbarSize::Compact)), (t!("Tam görev çubuğu", "Full taskbar"), tb(TaskbarSize::Full))], style);
    let note = t!("Görev çubuğu yalnızca altta durur; seçince dock alta alınır.", "The taskbar only sits at the bottom; picking it moves the dock down.");
    p.text(note, R::new(r.x + label_w, y + 38.0, seg_w, 20.0), 12.0, false, TEXT, 0.5, Align::Left);

    // Şu anki ayarın büyük önizlemesi (görünüm kartıyla aynı çizim).
    let top = y + 70.0;
    let pr = R::new(r.x, top, r.w, (r.bottom() - top).max(60.0));
    p.round(pr, app.round(10.0), 0xFFFFFF, 0.03);
    looks_preview::draw(p, pr.inset(8.0), &looks::capture(&app.cfg, ""), app.cfg.theme.wallpaper_accent.as_str());
}

fn island(app: &Welcome, p: &mut W, r: R) {
    let styles = [(IslandStyle::Pill, t!("Hap", "Pill"), t!("Ekranın biraz altında yüzer.", "Floats just below the top edge.")), (IslandStyle::Notch, t!("Çentik", "Notch"), t!("Üst kenara yapışık durur.", "Sits flush with the top edge.")), (IslandStyle::Card, t!("Kart", "Card"), t!("Yüzer, köşeleri az yuvarlak.", "Floats with softer corners."))];
    let cw = (r.w - GAP * 2.0) / 3.0;
    let ch = r.h.min(260.0);
    for (i, (s, name, desc)) in styles.iter().enumerate() {
        let c = R::new(r.x + i as f32 * (cw + GAP), r.y, cw, ch);
        let selected = app.cfg.island.style == *s;
        card(app, p, c, selected, Go::Island(*s));
        island_shape(p, R::new(c.x + 10.0, c.y + 10.0, c.w - 20.0, c.h - 74.0), *s);
        name_row(p, R::new(c.x + 14.0, c.bottom() - 62.0, c.w - 24.0, 28.0), name, selected);
        p.text(desc, R::new(c.x + 14.0, c.bottom() - 36.0, c.w - 24.0, 22.0), 12.0, false, TEXT, 0.55, Align::Left);
    }
}

/// Ekranın üst kısmı ve adanın biçimi: hap, çentik ya da kart.
fn island_shape(p: &W, r: R, s: IslandStyle) {
    let light = carlui_ui::tone::light();
    carlui_ui::tone::literal(|| {
        // Ekran: temaya göre açık ya da koyu bir masaüstü.
        let (bg, fg) = if light { (0xDCE3EE, 0x1B1E26) } else { (0x22262F, 0xFFFFFF) };
        p.round(r, 8.0, bg, 1.0);
        p.fill(R::new(r.x + 8.0, r.y + r.h * 0.55, r.w * 0.45, 6.0), fg, 0.10);
        p.fill(R::new(r.x + 8.0, r.y + r.h * 0.55 + 12.0, r.w * 0.3, 6.0), fg, 0.08);
        let iw = (r.w * 0.5).min(150.0);
        let x = r.x + (r.w - iw) / 2.0;
        let island = match s {
            IslandStyle::Pill => R::new(x, r.y + 10.0, iw, 30.0),
            IslandStyle::Notch => R::new(x, r.y, iw, 32.0),
            IslandStyle::Card => R::new(x - 6.0, r.y + 10.0, iw + 12.0, 34.0),
        };
        match s {
            IslandStyle::Pill => p.round(island, 15.0, 0x000000, 0.92),
            IslandStyle::Notch => {
                // Üst köşeler düz: yuvarlak dikdörtgenin üst yarısı düz şeritle kapatılır.
                p.round(island, 12.0, 0x000000, 0.92);
                p.fill(R::new(island.x, island.y, island.w, 12.0), 0x000000, 0.92);
            }
            IslandStyle::Card => p.round(island, 7.0, 0x000000, 0.92),
        }
        p.round(R::new(island.x + 10.0, island.y + island.h / 2.0 - 5.0, 10.0, 10.0), 5.0, 0x6CCB5F, 0.95);
        p.text("12:30", R::new(island.x, island.y, island.w - 12.0, island.h), 12.0, true, 0xFFFFFF, 0.9, Align::Right);
    });
}

fn modules(app: &Welcome, p: &mut W, r: R) {
    let foot = 26.0;
    let rows = Module::ALL.len().div_ceil(2);
    let cw = (r.w - GAP) / 2.0;
    let ch = ((r.h - foot - GAP * rows as f32) / rows as f32).min(80.0);
    for (i, m) in Module::ALL.iter().enumerate() {
        let c = R::new(r.x + (i % 2) as f32 * (cw + GAP), r.y + (i / 2) as f32 * (ch + GAP), cw, ch);
        let on = m.get(&app.cfg);
        card(app, p, c, false, Go::Module(*m));
        let tx = c.x + 14.0;
        let tw = c.w - 14.0 - 60.0;
        p.text(m.title(), R::new(tx, c.y + 10.0, tw, 22.0), 14.0, true, TEXT, if on { 0.95 } else { 0.7 }, Align::Left);
        // Açıklama tek satır; sığmazsa ikinci satıra taşar, fazlası kesilir.
        let lines = wrap(m.desc(), tw, |s| p.fonts.width(s, 12.0, false));
        let room = ((c.h - 38.0) / 17.0).floor().max(1.0) as usize;
        for (k, l) in lines.iter().take(room).enumerate() {
            p.text(l, R::new(tx, c.y + 33.0 + k as f32 * 17.0, tw, 17.0), 12.0, false, TEXT, 0.55, Align::Left);
        }
        p.switch(c.right() - 14.0 - 34.0, c.y + (c.h - 18.0) / 2.0, on, true, Go::Module(*m));
    }
    let vault = app.cfg.shortcuts.vault.trim();
    let note = if vault.is_empty() {
        t!("Carl Vault her zaman hazır; Ayarlar'dan bir kısayol atayabilirsin.", "Carl Vault is always ready; assign a shortcut in Settings.").to_string()
    } else {
        t!(format!("Carl Vault her zaman hazır: {vault}"), format!("Carl Vault is always ready: {vault}"))
    };
    p.glyph('\u{E72E}', R::new(r.x, r.bottom() - foot, 18.0, foot), 12.0, TEXT, 0.55);
    p.text(&note, R::new(r.x + 24.0, r.bottom() - foot, r.w - 24.0, foot), 12.0, false, TEXT, 0.55, Align::Left);
}

fn finish(app: &Welcome, p: &mut W, r: R) {
    let tips = model::tips(&app.cfg);
    let mut y = r.y;
    for (label, key) in &tips {
        let c = R::new(r.x, y, r.w, 46.0);
        p.round(c, app.round(10.0), 0xFFFFFF, 0.045);
        p.outline(c, app.round(10.0), 0xFFFFFF, 0.07);
        p.text(label, R::new(c.x + 16.0, c.y, c.w * 0.55, c.h), 14.0, false, TEXT, 0.9, Align::Left);
        key_chip(app, p, R::new(c.x, c.y, c.w - 12.0, c.h), key);
        y += 46.0 + 8.0;
    }
    if tips.is_empty() {
        p.text(t!("Kısayolları Ayarlar > Kısayollar'dan atayabilirsin.", "Assign shortcuts in Settings > Shortcuts."), R::new(r.x, y, r.w, 24.0), 13.0, false, TEXT, 0.6, Align::Left);
        y += 32.0;
    }

    // Kısa özet: seçilen görünüm, dock ve kaç modülün açık olduğu.
    y += 14.0;
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
    let on = Module::ALL.iter().filter(|m| m.get(&app.cfg)).count();
    let n = Module::ALL.len();
    let summary = t!(format!("Görünüm: {look} · {dock} · {on}/{n} modül açık"), format!("Look: {look} · {dock} · {on}/{n} modules on"));
    p.text(&summary, R::new(r.x, y, r.w, 22.0), 13.0, false, TEXT, 0.62, Align::Left);
    let later = t!("Her şeyi sonra Ayarlar'dan değiştirebilirsin; bu pencere bir daha kendiliğinden açılmaz.", "You can change everything later in Settings; this window won't open on its own again.");
    let lines = wrap(later, r.w, |s| p.fonts.width(s, 12.0, false));
    for (k, l) in lines.iter().take(2).enumerate() {
        p.text(l, R::new(r.x, y + 24.0 + k as f32 * 18.0, r.w, 18.0), 12.0, false, TEXT, 0.45, Align::Left);
    }
}

/// Sağa yaslı kısayol etiketi ("Ctrl+Alt+V").
fn key_chip(app: &Welcome, p: &W, row: R, key: &str) {
    let kw = p.fonts.width(key, 13.0, true) + 22.0;
    let k = R::new(row.right() - kw, row.y + (row.h - 28.0) / 2.0, kw, 28.0);
    p.round(k, app.round(6.0), mix(0xFFFFFF, p.accent, 0.3), 0.10);
    p.outline(k, app.round(6.0), p.accent, 0.45);
    p.text(key, k, 13.0, true, TEXT, 0.92, Align::Center);
}
