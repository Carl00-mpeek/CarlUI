//! Dock: boyut, konum, gizlenme davranışı, gösterilecekler ve sağ tık menüsü.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::config::DockBehavior;

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.dock;
    f.section(p, t!("Boyut ve konum", "Size and position"));
    f.slider(p, t!("Simge boyutu", "Icon size"), "", c.icon_size, 20.0, 64.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.dock.icon_size = v);
    f.slider(p, t!("Alt kenardan uzaklık", "Distance from bottom edge"), "", c.bottom_margin, 0.0, 60.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.dock.bottom_margin = v);
    let sel = match c.behavior {
        DockBehavior::AlwaysVisible => 0,
        DockBehavior::Intellihide => 1,
        DockBehavior::AutoHide => 2,
    };
    f.choice(
        p,
        t!("Davranış", "Behavior"),
        t!("Akıllı gizle: bir pencere dock'un üstüne gelince çekilir.", "Smart hide: steps aside when a window covers the dock."),
        &[
            (t!("Her zaman", "Always"), edit(|c| c.dock.behavior = DockBehavior::AlwaysVisible)),
            (t!("Akıllı gizle", "Smart hide"), edit(|c| c.dock.behavior = DockBehavior::Intellihide)),
            (t!("Otomatik", "Auto hide"), edit(|c| c.dock.behavior = DockBehavior::AutoHide)),
        ],
        sel,
        110.0,
    );

    f.section(p, t!("Gösterilecekler", "Show on dock"));
    f.toggle(p, t!("Arama düğmesi", "Search button"), t!("Kapalıyken arama kısayolu ve Win tuşu yine çalışır.", "When off, the search shortcut and the Win key still work."), c.show_search, |c| c.dock.show_search ^= true);
    f.toggle(p, t!("Açık uygulamalar", "Running apps"), t!("Sabitlenmemiş açık uygulamalar da dock'ta görünür.", "Running apps that aren't pinned also appear on the dock."), c.show_running_apps, |c| c.dock.show_running_apps ^= true);
    f.toggle(p, t!("Uygulama adları", "App names"), t!("Simgenin üstüne gelince adı görünür.", "Shows the name when you hover an icon."), c.show_labels, |c| c.dock.show_labels ^= true);
    f.toggle(p, t!("Pencere önizlemeleri", "Window previews"), t!("Üstüne gelince açık pencerelerin küçük resmi.", "Thumbnails of open windows on hover."), c.show_previews, |c| c.dock.show_previews ^= true);
    f.toggle(p, t!("İndirilenler yığını", "Downloads stack"), t!("Tıklayınca son indirilen dosyalar açılır.", "Click to see your latest downloads."), c.downloads_stack, |c| c.dock.downloads_stack ^= true);
    f.toggle(p, t!("Geri Dönüşüm Kutusu", "Recycle Bin"), t!("Sağ tıkla boşaltılabilir; Windows onay sorar.", "Right-click to empty it; Windows asks to confirm."), c.recycle_bin, |c| c.dock.recycle_bin ^= true);

    f.section(p, t!("Sağ tık menüsü", "Right-click menu"));
    let accent = c.menu_accent.trim().to_lowercase();
    let custom = accent.trim_start_matches('#').len() == 6;
    let sel = match accent.as_str() {
        "off" | "kapalı" | "kapali" | "none" => 1,
        _ if custom => 2,
        _ => 0,
    };
    let mut items = vec![(t!("Sistem rengi", "System color"), edit(|c| c.dock.menu_accent = "system".into())), (t!("Renksiz", "No color"), edit(|c| c.dock.menu_accent = "off".into()))];
    if custom {
        let v = c.menu_accent.clone();
        items.push((t!("Özel", "Custom"), edit(move |c| c.dock.menu_accent = v.clone())));
    }
    f.choice(p, t!("Başlık rengi", "Header color"), t!("Menüdeki kategori başlıkları.", "Category headers in the menu."), &items, sel, 110.0);
    f.toggle(p, t!("Uygulama listeleri", "App lists"), t!("\"Sık ziyaret edilenler\", \"Son oynanan\" gibi listeler de görünür.", "Also shows lists like \"Frequent\" or \"Recently played\"."), c.menu_lists, |c| c.dock.menu_lists ^= true);
}
