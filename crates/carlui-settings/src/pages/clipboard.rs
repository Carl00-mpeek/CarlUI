//! Pano geçmişi: açık/kapalı, kayıt sayısı, Enter davranışı, gizli kayıtlar.

use crate::{
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.clipboard;
    f.section(p, t!("Pano geçmişi", "Clipboard history"));
    f.toggle(p, t!("Pano geçmişi", "Clipboard history"), t!("Kopyaladıkların adada listelenir (kısayol: Kısayollar sayfası).", "What you copy is listed on the island (shortcut: Shortcuts page)."), c.enabled, |c| c.clipboard.enabled ^= true);
    f.slider(p, t!("En fazla kayıt", "Maximum items"), t!("Sabitlenenler bu sayıya dahil değildir.", "Pinned items don't count."), c.max_items as f32, 20.0, 1000.0, 10.0, |v| format!("{v:.0}"), |c, v| c.clipboard.max_items = v as usize);
    f.toggle(p, t!("Enter ile yapıştır", "Paste with Enter"), t!("Seçileni önceki pencereye yapıştırır; kapalıysa yalnızca panoya kopyalar.", "Pastes the selection into the previous window; when off it only copies to the clipboard."), c.paste_on_enter, |c| c.clipboard.paste_on_enter ^= true);
    f.section(p, t!("Gizli kayıtlar", "Secret items"));
    f.slider(
        p,
        t!("Şifreleri sil", "Delete passwords"),
        t!("Şifre/anahtar gibi görünen kopyalar bu süre sonra silinir.", "Copies that look like passwords or keys are deleted after this time."),
        c.secret_seconds as f32,
        0.0,
        600.0,
        10.0,
        |v| if v < 1.0 { t!("Silinmez", "Never").into() } else if v < 120.0 { t!(format!("{v:.0} sn"), format!("{v:.0} s")) } else { t!(format!("{:.1} dk", v / 60.0).replace('.', ","), format!("{:.1} min", v / 60.0)) },
        |c, v| c.clipboard.secret_seconds = v as u64,
    );
    f.note(p, t!("Silinmez seçilirse gizli kayıtlar yalnızca maskelenir.", "With Never, secret items are only masked."));
}
