//! Dock: boyut, konum, gizlenme davranışı, gösterilecekler ve sağ tık menüsü.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::config::DockBehavior;

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.dock;
    f.section(p, "Boyut ve konum");
    f.slider(p, "Simge boyutu", "", c.icon_size, 20.0, 64.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.dock.icon_size = v);
    f.slider(p, "Alt kenardan uzaklık", "", c.bottom_margin, 0.0, 60.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.dock.bottom_margin = v);
    let sel = match c.behavior {
        DockBehavior::AlwaysVisible => 0,
        DockBehavior::Intellihide => 1,
        DockBehavior::AutoHide => 2,
    };
    f.choice(
        p,
        "Davranış",
        "Akıllı gizle: bir pencere dock'un üstüne gelince çekilir.",
        &[
            ("Her zaman", edit(|c| c.dock.behavior = DockBehavior::AlwaysVisible)),
            ("Akıllı gizle", edit(|c| c.dock.behavior = DockBehavior::Intellihide)),
            ("Otomatik", edit(|c| c.dock.behavior = DockBehavior::AutoHide)),
        ],
        sel,
        110.0,
    );

    f.section(p, "Gösterilecekler");
    f.toggle(p, "Arama düğmesi", "Kapalıyken arama kısayolu ve Win tuşu yine çalışır.", c.show_search, |c| c.dock.show_search ^= true);
    f.toggle(p, "Açık uygulamalar", "Sabitlenmemiş açık uygulamalar da dock'ta görünür.", c.show_running_apps, |c| c.dock.show_running_apps ^= true);
    f.toggle(p, "Uygulama adları", "Simgenin üstüne gelince adı görünür.", c.show_labels, |c| c.dock.show_labels ^= true);
    f.toggle(p, "Pencere önizlemeleri", "Üstüne gelince açık pencerelerin küçük resmi.", c.show_previews, |c| c.dock.show_previews ^= true);
    f.toggle(p, "İndirilenler yığını", "Tıklayınca son indirilen dosyalar açılır.", c.downloads_stack, |c| c.dock.downloads_stack ^= true);
    f.toggle(p, "Geri Dönüşüm Kutusu", "Sağ tıkla boşaltılabilir; Windows onay sorar.", c.recycle_bin, |c| c.dock.recycle_bin ^= true);

    f.section(p, "Sağ tık menüsü");
    let accent = c.menu_accent.trim().to_lowercase();
    let custom = accent.trim_start_matches('#').len() == 6;
    let sel = match accent.as_str() {
        "off" | "kapalı" | "kapali" | "none" => 1,
        _ if custom => 2,
        _ => 0,
    };
    let mut items = vec![("Sistem rengi", edit(|c| c.dock.menu_accent = "system".into())), ("Renksiz", edit(|c| c.dock.menu_accent = "off".into()))];
    if custom {
        let v = c.menu_accent.clone();
        items.push(("Özel", edit(move |c| c.dock.menu_accent = v.clone())));
    }
    f.choice(p, "Başlık rengi", "Menüdeki kategori başlıkları.", &items, sel, 110.0);
    f.toggle(p, "Uygulama listeleri", "\"Sık ziyaret edilenler\", \"Son oynanan\" gibi listeler de görünür.", c.menu_lists, |c| c.dock.menu_lists ^= true);
}
