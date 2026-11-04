//! Açık kasa: solda kategoriler (kayıt sayılarıyla), sağda kayıt listesi.
//! Listede yalnızca ad, kategori ve tarih görünür; alanlar çözülmez. Bir
//! kaydı açmanın tek yolu "Düzenle" düğmesidir.

use super::{clipped, date, input, SIDEBAR};
use crate::app::{Act, App, Focus, View};
use carlui_core::t;
use carlui_ui::{
    paint::{Align, R, TEXT},
    tween,
};
use carlui_vault::{entry::Category, vault::Item};

use super::P;

fn side_row(p: &mut P, r: R, glyph: char, label: &str, count: Option<usize>, selected: bool, act: Act) {
    let hv = tween::to(tween::key(("vside", r.y as i32)), if !selected && p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    if selected {
        p.round(r, 7.0, 0xFFFFFF, 0.10);
        p.round(R::new(r.x, r.y + 10.0, 3.0, r.h - 20.0), 1.5, p.accent, 1.0);
    } else if hv > 0.0 {
        p.round(r, 7.0, 0xFFFFFF, 0.06 * hv);
    }
    p.glyph(glyph, R::new(r.x + 10.0, r.y, 24.0, r.h), 14.0, TEXT, 0.9);
    p.text(label, R::new(r.x + 42.0, r.y, r.w - 80.0, r.h), 13.0, selected, TEXT, 0.92, Align::Left);
    if let Some(n) = count {
        p.text(&n.to_string(), R::new(r.right() - 44.0, r.y, 34.0, r.h), 12.0, false, TEXT, 0.45, Align::Right);
    }
    p.hit(r, act);
}

pub fn sidebar(app: &App, p: &mut P, h: f32) {
    p.fill(R::new(0.0, 0.0, SIDEBAR, h), 0x000000, 0.12);
    p.glyph('\u{E72E}', R::new(18.0, 14.0, 22.0, 30.0), 15.0, p.accent, 1.0);
    p.text("Carl Vault", R::new(46.0, 14.0, SIDEBAR - 56.0, 30.0), 15.0, true, TEXT, 0.95, Align::Left);
    let items = app.vault.as_ref().map(|v| v.items()).unwrap_or(&[]);
    let in_list = matches!(app.view, View::List);
    let mut y = 56.0;
    side_row(p, R::new(10.0, y, SIDEBAR - 20.0, 36.0), '\u{E8A9}', t!("Tümü", "All"), Some(items.len()), in_list && app.filter.is_none(), Act::Filter(None));
    y += 40.0;
    for c in Category::ALL {
        let n = items.iter().filter(|i| i.meta.category == c).count();
        let act = Act::Filter(Some(c));
        side_row(p, R::new(10.0, y, SIDEBAR - 20.0, 36.0), c.glyph(), c.title(), Some(n), in_list && app.filter == Some(c), act);
        y += 40.0;
    }
    side_row(p, R::new(10.0, h - 96.0, SIDEBAR - 20.0, 38.0), '\u{E83D}', t!("Güvenlik", "Security"), None, matches!(app.view, View::Security), Act::Security);
    side_row(p, R::new(10.0, h - 52.0, SIDEBAR - 20.0, 38.0), '\u{E72E}', t!("Kilitle  (Ctrl+L)", "Lock  (Ctrl+L)"), None, false, Act::Lock);
}

/// Kategori + arama süzgecinden geçen kayıtlar.
fn visible(app: &App) -> Vec<&Item> {
    let q = app.search.as_str().trim().to_lowercase();
    app.vault
        .as_ref()
        .map(|v| v.items())
        .unwrap_or(&[])
        .iter()
        .filter(|i| app.filter.is_none_or(|c| i.meta.category == c))
        .filter(|i| q.is_empty() || i.meta.name.to_lowercase().contains(&q))
        .collect()
}

/// İçerik sütunu: (x, genişlik).
pub fn column(w: f32) -> (f32, f32) {
    let cw = (w - SIDEBAR - 64.0).min(820.0);
    (SIDEBAR + 32.0 + ((w - SIDEBAR - 64.0 - cw) / 2.0).max(0.0), cw)
}

pub fn paint(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, cw) = column(w);
    let title = app.filter.map(|c| c.title()).unwrap_or(t!("Tüm kayıtlar", "All items"));
    p.text(title, R::new(x, 18.0, cw - 340.0, 38.0), 22.0, true, TEXT, 0.95, Align::Left);
    let sr = R::new(x + cw - 330.0, 20.0, 200.0, 34.0);
    input(p, app, sr, Focus::Search, false, t!("Ada göre ara", "Search by name"));
    p.glyph('\u{E721}', R::new(sr.right() - 30.0, sr.y, 24.0, sr.h), 12.0, TEXT, 0.4);
    super::primary(p, R::new(x + cw - 120.0, 20.0, 120.0, 34.0), t!("+ Yeni kayıt", "+ New item"), Act::New, true);

    let list = visible(app);
    let clip = R::new(SIDEBAR, 70.0, w - SIDEBAR, h - 70.0);
    let mut y = clip.y + 6.0 - app.scroll;
    clipped(p, clip, |p| {
        if list.is_empty() {
            let msg = if app.search.text.is_empty() {
                t!("Henüz kayıt yok. \"+ Yeni kayıt\" ile ekleyebilirsin.", "No items yet. Add one with \"+ New item\".")
            } else {
                t!("Bu adla kayıt bulunamadı.", "No item with that name.")
            };
            p.text(msg, R::new(x, y + 30.0, cw, 30.0), 13.5, false, TEXT, 0.5, Align::Center);
        }
        for it in &list {
            let r = R::new(x, y, cw, 58.0);
            p.card(r);
            if p.hover(r) {
                p.round(r, 8.0, 0xFFFFFF, 0.03);
            }
            let c = it.meta.category;
            p.round(R::new(r.x + 12.0, r.y + 11.0, 36.0, 36.0), 9.0, p.accent, 0.14);
            p.glyph(c.glyph(), R::new(r.x + 12.0, r.y + 11.0, 36.0, 36.0), 15.0, p.accent, 1.0);
            p.text(&it.meta.name, R::new(r.x + 62.0, r.y + 9.0, r.w - 190.0, 22.0), 14.0, true, TEXT, 0.95, Align::Left);
            p.text(&format!("{} · {}", c.title(), date(it.meta.updated)), R::new(r.x + 62.0, r.y + 31.0, r.w - 190.0, 18.0), 12.0, false, TEXT, 0.5, Align::Left);
            p.button(R::new(r.right() - 116.0, r.y + 12.0, 104.0, 34.0), Some('\u{E70F}'), t!("Düzenle", "Edit"), Act::Edit(it.id.clone()), true, false);
            y += 64.0;
        }
    });
    (y + app.scroll - clip.y + 20.0 - clip.h).max(0.0)
}
