//! Bu modül "küçük araçları nereden açıp kapatırız?" sorununu çözüyor:
//! Araçlar sayfası. Renk seçicinin biçimi, toplu yeniden adlandırma ve disk
//! analizörü modülleri, metin genişletici ve kısaltma listesi burada. Araçların
//! kısayolları Kısayollar sayfasında.

use crate::{
    input::Form,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::config::ColorFormat;
use carlui_ui::paint::{Align, R, TEXT};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.tools;
    let sc = &app.cfg.shortcuts;
    let key_hint = |v: &str| if v.trim().is_empty() { t!("Kısayolu Kısayollar sayfasından atayın.", "Assign a shortcut on the Shortcuts page.").to_string() } else { t!(format!("Kısayol: {v}"), format!("Shortcut: {v}")) };

    f.section(p, t!("Renk seçici", "Color picker"));
    let sel = match c.color_format {
        ColorFormat::Hex => 0,
        ColorFormat::Rgb => 1,
        ColorFormat::Hsl => 2,
    };
    f.choice(
        p,
        t!("Panoya kopyalanan biçim", "Format copied to the clipboard"),
        &key_hint(&sc.color_picker),
        &[("HEX", edit(|c| c.tools.color_format = ColorFormat::Hex)), ("RGB", edit(|c| c.tools.color_format = ColorFormat::Rgb)), ("HSL", edit(|c| c.tools.color_format = ColorFormat::Hsl))],
        sel,
        72.0,
    );

    f.section(p, t!("Dosyalar", "Files"));
    f.toggle(
        p,
        t!("Toplu yeniden adlandırma", "Batch rename"),
        &t!(format!("Gezgin'de dosyaları seçip kısayola basın ({}).", sc.rename), format!("Select files in Explorer and press the shortcut ({}).", sc.rename)),
        c.rename_enabled,
        |c| c.tools.rename_enabled ^= true,
    );
    f.toggle(p, t!("Disk analizörü", "Disk analyzer"), &key_hint(&sc.disk), c.disk_enabled, |c| c.tools.disk_enabled ^= true);
    if c.disk_enabled {
        let r = f.row(p, t!("Disk analizörünü aç", "Open the disk analyzer"), t!("Hangi klasörün ne kadar yer kapladığını gösterir.", "Shows how much space each folder takes."), 120.0);
        p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{EDA2}'), t!("Aç", "Open"), Act::Tool("--disk"), true, false);
    }

    f.section(p, t!("Metin genişletici", "Text expander"));
    f.toggle(
        p,
        t!("Kısaltmaları aç", "Expand abbreviations"),
        t!("Bir kısaltma yazınca yerine metni yazılır. {tarih}, {saat}, {pano} kullanılabilir.", "Typing an abbreviation replaces it with its text. {date}, {time}, {clipboard} can be used."),
        c.expander,
        |c| c.tools.expander ^= true,
    );
    if !c.expander {
        return;
    }
    for (i, s) in c.snippets.iter().enumerate() {
        let r = f.row(p, &s.key, &s.text.replace('\n', " ⏎ "), 60.0);
        let del = R::new(r.right() - 12.0 - 32.0, r.y + (r.h - 32.0) / 2.0, 32.0, 32.0);
        if p.hover(del) {
            p.round(del, 6.0, 0xFFFFFF, 0.10);
        }
        p.glyph('\u{E74D}', del, 13.0, TEXT, 0.75);
        p.hit(
            del,
            edit(move |c| {
                if i < c.tools.snippets.len() {
                    c.tools.snippets.remove(i);
                }
            }),
        );
    }
    if c.snippets.is_empty() {
        p.text(t!("Henüz kısaltma yok.", "No abbreviations yet."), R::new(f.x + 4.0, f.y, f.w, 24.0), 12.5, false, TEXT, 0.5, Align::Left);
        f.y += 28.0;
    }
    let r = f.row(p, t!("Kısaltma ekle", "Add abbreviation"), t!("Aynı kısaltma yeniden eklenirse metni güncellenir.", "Adding the same abbreviation again updates its text."), 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E710}'), t!("Ekle", "Add"), Act::Input(5), app.input.is_none(), false);
    super::form::draw(app, p, f, Form::Snippet, [(t!("Kısaltma", "Abbreviation"), t!("ör. ;adres", "e.g. ;addr")), (t!("Metin", "Text"), t!("ör. Atatürk Cad. No 1, İstanbul — Ctrl+V çok satırlı metni de yapıştırır", "e.g. 1 Main St, Springfield — Ctrl+V also pastes multi-line text"))]);
}
