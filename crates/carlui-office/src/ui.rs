//! Bu modül "pencerenin ortak çerçevesi: üst araç çubuğu, alt durum çubuğu,
//! ana ekran ve bilgi baloncuğu" sorununu çözüyor. Belgeye özel düğmeleri
//! görünümün kendisi çizer (`View::toolbar`); burada yalnızca ortak parçalar
//! ve görünümlerin kullandığı küçük araç düğmesi çizimi var.

use crate::app::{self, Act, App, Cmd};
use carlui_core::t;
use carlui_ui::{
    paint::{mix, Align, Painter, R, TEXT},
    tween,
};

pub const TOP: f32 = 46.0;
pub const BOTTOM: f32 = 26.0;

/// Simgeli küçük araç düğmesi; `on` basılı (etkin) görünüm. Genişliği döner.
pub fn tool(p: &mut Painter<Act>, x: f32, y: f32, glyph: char, act: Act, on: bool) -> f32 {
    let r = R::new(x, y, 32.0, 30.0);
    let h = tween::to(tween::key(("tool", x as i32, y as i32)), if p.hover(r) { 1.0 } else { 0.0 }, 20.0);
    if on {
        p.round(r, 6.0, p.accent, 0.28);
    }
    if h > 0.0 {
        p.round(r, 6.0, 0xFFFFFF, 0.09 * h);
    }
    p.glyph(glyph, r, 15.0, if on { mix(TEXT, p.accent, 0.3) } else { TEXT }, 0.9);
    p.hit(r, act);
    r.w + 2.0
}

/// Yazılı araç düğmesi (ör. "B1", "%"). Genişliği döner.
pub fn tool_text(p: &mut Painter<Act>, x: f32, y: f32, label: &str, act: Act, on: bool, bold: bool) -> f32 {
    let w = p.fonts.width(label, 13.0, bold).max(14.0) + 16.0;
    let r = R::new(x, y, w, 30.0);
    let h = tween::to(tween::key(("toolt", x as i32, y as i32)), if p.hover(r) { 1.0 } else { 0.0 }, 20.0);
    if on {
        p.round(r, 6.0, p.accent, 0.28);
    }
    if h > 0.0 {
        p.round(r, 6.0, 0xFFFFFF, 0.09 * h);
    }
    p.text(label, r, 13.0, bold, TEXT, 0.9, Align::Center);
    p.hit(r, act);
    w + 2.0
}

/// Araç grupları arasındaki ince çizgi. Genişliği döner.
pub fn sep(p: &mut Painter<Act>, x: f32, y: f32) -> f32 {
    p.fill(R::new(x + 5.0, y + 6.0, 1.0, 18.0), 0xFFFFFF, 0.14);
    11.0
}

/// Seçim (açılır) kutusu görünümlü düğme: etiket + aşağı ok.
pub fn dropdown(p: &mut Painter<Act>, x: f32, y: f32, w: f32, label: &str, act: Act) -> f32 {
    let r = R::new(x, y + 2.0, w, 26.0);
    p.round(r, 5.0, 0xFFFFFF, if p.hover(r) { 0.12 } else { 0.07 });
    p.text(label, R::new(r.x + 8.0, r.y, r.w - 26.0, r.h), 12.5, false, TEXT, 0.9, Align::Left);
    p.glyph('\u{E70D}', R::new(r.right() - 22.0, r.y, 18.0, r.h), 9.0, TEXT, 0.7);
    p.hit(r, act);
    w + 4.0
}

pub fn paint(a: &mut App, p: &mut Painter<Act>, w: f32, h: f32, _scale: f32) {
    // Sunu kipi: yalnızca görünüm, tam ekran.
    if let Some(v) = a.view.as_mut().filter(|v| v.fullscreen()) {
        let area = R::new(0.0, 0.0, w, h);
        p.hit(area, Act::Content);
        v.paint(p, area);
        return;
    }
    // Üst çubuk: dosya menüsü, kaydet, geri al / yinele, sonra görünüme ait düğmeler.
    let y = 8.0;
    let file = R::new(8.0, y, 74.0, 30.0);
    p.round(file, 6.0, p.accent, if p.hover(file) { 0.95 } else { 0.8 });
    p.glyph('\u{E700}', R::new(file.x + 6.0, y, 20.0, 30.0), 12.0, 0x000000, 0.85);
    p.text(t!("Dosya", "File"), R::new(file.x + 28.0, y, 44.0, 30.0), 13.0, true, 0x000000, 0.85, Align::Left);
    p.hit(file, Act::FileMenu);
    let mut x = file.right() + 8.0;
    let name = a.name();
    if let Some(v) = a.view.as_mut() {
        x += tool(p, x, y, '\u{E74E}', Act::Save, false);
        x += tool(p, x, y, '\u{E7A7}', Act::View(Cmd::Undo), false);
        x += tool(p, x, y, '\u{E7A6}', Act::View(Cmd::Redo), false);
        x += sep(p, x, y);
        v.toolbar(p, R::new(x, y, w - x - 8.0, 30.0));
        let area = R::new(0.0, TOP, w, h - TOP - BOTTOM);
        p.hit(area, Act::Content);
        v.paint(p, area);
        // Durum çubuğu.
        let sb = R::new(0.0, h - BOTTOM, w, BOTTOM);
        p.fill(sb, 0x000000, 0.18);
        p.text(&name, R::new(12.0, sb.y, w * 0.4, sb.h), 12.0, false, TEXT, 0.6, Align::Left);
        let st = v.status();
        p.text(&st, R::new(w * 0.35, sb.y, w * 0.65 - 12.0, sb.h), 12.0, false, TEXT, 0.75, Align::Right);
    } else {
        home(a, p, w, h);
    }
    toast(p, w, h);
}

fn home(a: &App, p: &mut Painter<Act>, w: f32, h: f32) {
    let cw = 640.0f32.min(w - 48.0);
    let x0 = (w - cw) / 2.0;
    let mut y = TOP + 24.0;
    p.text("Carl Office", R::new(x0, y, cw, 40.0), 28.0, true, TEXT, 0.95, Align::Left);
    y += 40.0;
    p.text(t!("Excel, Word, PowerPoint ve PDF dosyalarını aç, düzenle, kaydet.", "Open, edit and save Excel, Word, PowerPoint and PDF files."), R::new(x0, y, cw, 22.0), 13.5, false, TEXT, 0.6, Align::Left);
    y += 40.0;
    let items = [
        ('\u{E80A}', t!("Yeni tablo", "New spreadsheet"), Act::NewSheet),
        ('\u{E8A5}', t!("Yeni belge", "New document"), Act::NewDoc),
        ('\u{E8E5}', t!("Dosya aç", "Open file"), Act::Open),
    ];
    let bw = (cw - 24.0) / 3.0;
    for (i, (g, label, act)) in items.into_iter().enumerate() {
        let r = R::new(x0 + i as f32 * (bw + 12.0), y, bw, 96.0);
        let hv = tween::to(tween::key(("home", i)), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
        p.round(r, 10.0, 0xFFFFFF, 0.05 + 0.05 * hv);
        p.outline(r, 10.0, mix(0xFFFFFF, p.accent, hv), 0.08 + 0.3 * hv);
        p.glyph(g, R::new(r.x, r.y + 14.0, r.w, 36.0), 26.0, p.accent, 0.95);
        p.text(label, R::new(r.x, r.y + 56.0, r.w, 24.0), 13.5, true, TEXT, 0.9, Align::Center);
        p.hit(r, act);
    }
    y += 124.0;
    p.text(t!("Son kullanılanlar", "Recent"), R::new(x0, y, cw, 22.0), 13.0, true, p.accent, 0.95, Align::Left);
    y += 30.0;
    if a.recent.is_empty() {
        p.text(t!("Henüz dosya açılmadı. Dosyayı pencereye sürükleyip bırakabilirsin.", "No files yet. You can drag a file onto this window."), R::new(x0, y, cw, 22.0), 13.0, false, TEXT, 0.5, Align::Left);
        return;
    }
    for (i, path) in a.recent.iter().enumerate() {
        let r = R::new(x0, y, cw, 44.0);
        if r.bottom() > h - 12.0 {
            break;
        }
        if p.hover(r) {
            p.round(r, 7.0, 0xFFFFFF, 0.07);
        }
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (g, c) = kind_glyph(&ext);
        p.glyph(g, R::new(r.x + 6.0, r.y, 30.0, r.h), 18.0, c, 0.95);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.text(&name, R::new(r.x + 44.0, r.y + 4.0, r.w - 52.0, 20.0), 13.5, false, TEXT, 0.92, Align::Left);
        let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
        p.text(&dir, R::new(r.x + 44.0, r.y + 22.0, r.w - 52.0, 18.0), 11.5, false, TEXT, 0.5, Align::Left);
        p.hit(r, Act::Recent(i));
        y += 46.0;
    }
}

/// Dosya türünün simgesi ve rengi (Office'in alışılmış renkleri).
pub fn kind_glyph(ext: &str) -> (char, u32) {
    match ext {
        "xlsx" | "xlsm" | "xls" | "csv" => ('\u{E80A}', 0x3FBF6F),
        "docx" => ('\u{E8A5}', 0x5B9BF0),
        "pptx" => ('\u{E8A1}', 0xF0855B),
        "pdf" => ('\u{EA90}', 0xF05B5B),
        _ => ('\u{E8A5}', TEXT),
    }
}

fn toast(p: &mut Painter<Act>, w: f32, h: f32) {
    let n = app::notice();
    let shown = n.as_ref().is_some_and(|(_, at)| at.elapsed().as_millis() < 2500);
    let t = tween::to(tween::key("toast"), if shown { 1.0 } else { 0.0 }, 14.0);
    if t > 0.01 {
        if let Some((m, _)) = &n {
            let tw = p.fonts.width(m, 13.0, false) + 40.0;
            let r = R::new((w - tw) / 2.0, h - BOTTOM - 52.0 + 24.0 * (1.0 - t), tw, 34.0);
            p.round(r, 17.0, 0x202024, 0.96 * t);
            p.outline(r, 17.0, 0xFFFFFF, 0.12 * t);
            p.glyph('\u{E73E}', R::new(r.x + 12.0, r.y, 16.0, r.h), 12.0, p.accent, t);
            p.text(m, R::new(r.x + 30.0, r.y, r.w - 38.0, r.h), 13.0, false, TEXT, 0.95 * t, Align::Left);
        }
    }
}
