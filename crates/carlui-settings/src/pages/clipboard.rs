//! Pano geçmişi: açık/kapalı, kayıt sayısı, Enter davranışı, gizli kayıtlar.

use crate::{
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.clipboard;
    f.section(p, "Pano geçmişi");
    f.toggle(p, "Pano geçmişi", "Kopyaladıkların adada listelenir (kısayol: Kısayollar sayfası).", c.enabled, |c| c.clipboard.enabled ^= true);
    f.slider(p, "En fazla kayıt", "Sabitlenenler bu sayıya dahil değildir.", c.max_items as f32, 20.0, 1000.0, 10.0, |v| format!("{v:.0}"), |c, v| c.clipboard.max_items = v as usize);
    f.toggle(p, "Enter ile yapıştır", "Seçileni önceki pencereye yapıştırır; kapalıysa yalnızca panoya kopyalar.", c.paste_on_enter, |c| c.clipboard.paste_on_enter ^= true);
    f.section(p, "Gizli kayıtlar");
    f.slider(
        p,
        "Şifreleri sil",
        "Şifre/anahtar gibi görünen kopyalar bu süre sonra silinir.",
        c.secret_seconds as f32,
        0.0,
        600.0,
        10.0,
        |v| if v < 1.0 { "Silinmez".into() } else if v < 120.0 { format!("{v:.0} sn") } else { format!("{:.1} dk", v / 60.0).replace('.', ",") },
        |c, v| c.clipboard.secret_seconds = v as u64,
    );
    f.note(p, "Silinmez seçilirse gizli kayıtlar yalnızca maskelenir.");
}
