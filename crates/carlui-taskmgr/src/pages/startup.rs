//! Başlangıç: kayıt defteri Run anahtarları, başlangıç klasörleri, Winlogon,
//! oturum açılışında çalışan zamanlanmış görevler ve otomatik hizmetler.
//! Windows'a ait olanlar varsayılan gizli; imzasızlar en üstte.

use super::{cell, col, colored, dim, table_ex, title, Cell};
use crate::{
    draw::{Align, Painter, ORANGE, R, TEXT},
    sign::Sign,
    Act, App, Tab,
};

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let Some(all) = &app.startup else {
        title(p, area, t!("Başlangıç", "Startup"), t!("taranıyor…", "scanning…"));
        p.text(t!("Kayıt defteri, başlangıç klasörleri, zamanlanmış görevler ve hizmetler okunuyor…", "Reading the registry, startup folders, scheduled tasks and services…"), R::new(area.x, area.y + 60.0, area.w, 30.0), 13.0, false, TEXT, 0.6, Align::Left);
        return 0.0;
    };
    let rows = app.visible_startup();
    let enabled = all.iter().filter(|i| i.enabled).count();
    let unsigned = rows.iter().filter(|(_, i)| app.signs.get(&i.path) == Sign::Unsigned).count();
    title(p, area, t!("Başlangıç", "Startup"), &t!(format!("{} öğe · {} etkin · {} gösteriliyor · {} imzasız", all.len(), enabled, rows.len(), unsigned), format!("{} items · {} enabled · {} shown · {} unsigned", all.len(), enabled, rows.len(), unsigned)));

    let y = area.y + 46.0;
    let mut x = area.x;
    x += p.toggle(x, y, t!("Windows öğelerini gizle", "Hide Windows items"), app.start_hide_windows, Act::StartHideWindows) + 24.0;
    if !app.filter.is_empty() {
        p.text(&format!("{}: {}", t!("Filtre", "Filter"), app.filter), R::new(x, y, 300.0, 26.0), 12.5, false, TEXT, 0.7, Align::Left);
        p.hit(R::new(x, y, 300.0, 26.0), Act::ClearFilter);
    }

    let sel = app.start_selected.and_then(|i| all.get(i));
    let by = y + 40.0;
    let can_toggle = sel.map(|i| i.toggle != crate::startup::Toggle::None).unwrap_or(false);
    let on = sel.map(|i| i.enabled).unwrap_or(true);
    let label = if on { t!("Devre dışı bırak", "Disable") } else { t!("Etkinleştir", "Enable") };
    p.button(R::new(area.x, by, 160.0, 30.0), Some(if on { '\u{E733}' } else { '\u{E768}' }), label, Act::StartToggle, can_toggle, false);
    p.button(R::new(area.x + 170.0, by, 150.0, 30.0), Some('\u{E838}'), t!("Dosya konumu", "File location"), Act::StartReveal, sel.map(|i| !i.path.is_empty()).unwrap_or(false), false);
    p.button(R::new(area.x + 330.0, by, 110.0, 30.0), Some('\u{E72C}'), t!("Yenile", "Refresh"), Act::StartRefresh, true, false);
    if let Some(i) = sel {
        p.text(&i.command, R::new(area.x + 454.0, by, area.right() - area.x - 454.0, 30.0), 12.0, false, TEXT, 0.55, Align::Left);
    }

    let cols = [col("", 52.0, Align::Left, None), col(t!("Ad", "Name"), 220.0, Align::Left, None), col(t!("Kaynak", "Source"), 160.0, Align::Left, None), col(t!("İmza", "Signature"), 90.0, Align::Left, None), col(t!("Komut", "Command"), 0.0, Align::Left, None)];
    let selected = app.start_selected.and_then(|s| rows.iter().position(|(i, _)| *i == s));
    let ty = by + 44.0;
    let scroll = app.scroll.get(&Tab::Startup).copied().unwrap_or(0.0);
    table_ex(
        p,
        R::new(area.x, ty, area.w, area.bottom() - ty),
        &cols,
        None,
        rows.len(),
        scroll,
        selected,
        |r, c| {
            let it = rows[r].1;
            // Kapalı öğeler sönük.
            let k = if it.enabled { 1.0 } else { 0.5 };
            let mut v = match c {
                0 => cell(""),
                1 => cell(it.name.clone()),
                2 => dim(it.source),
                3 => {
                    let s = app.signs.get(&it.path);
                    match s {
                        Sign::Unsigned => colored(s.label(), ORANGE),
                        _ => Cell { text: s.label().into(), color: TEXT, alpha: 0.5 },
                    }
                }
                _ => dim(it.command.clone()),
            };
            v.alpha *= k;
            v
        },
        |r| Act::StartRow(rows[r].0),
        Act::Scroll,
        |_| 1.0,
        |p, r, rect| {
            let (i, it) = rows[r];
            let can = it.toggle != crate::startup::Toggle::None;
            p.switch(rect.x + 6.0, rect.y + (rect.h - 18.0) / 2.0, it.enabled, can, Act::StartSwitch(i));
        },
    )
}