//! Pano geçmişi: açık/kapalı, kayıt sayısı, Enter davranışı, resimler,
//! hariç tutulan uygulamalar, süreler, gizli kayıtlar.

use crate::{
    input::Form,
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.clipboard;
    f.section(p, t!("Pano geçmişi", "Clipboard history"));
    f.toggle(p, t!("Pano geçmişi", "Clipboard history"), t!("Kopyaladıkların adada listelenir (kısayol: Kısayollar sayfası).", "What you copy is listed on the island (shortcut: Shortcuts page)."), c.enabled, |c| c.clipboard.enabled ^= true);
    f.slider(p, t!("En fazla kayıt", "Maximum items"), t!("Sabitlenenler bu sayıya dahil değildir.", "Pinned items don't count."), c.max_items as f32, 20.0, 1000.0, 10.0, |v| format!("{v:.0}"), |c, v| c.clipboard.max_items = v as usize);
    f.toggle(p, t!("Enter ile yapıştır", "Paste with Enter"), t!("Seçileni önceki pencereye yapıştırır; kapalıysa yalnızca panoya kopyalar.", "Pastes the selection into the previous window; when off it only copies to the clipboard."), c.paste_on_enter, |c| c.clipboard.paste_on_enter ^= true);
    f.toggle(p, t!("Resimleri kaydet", "Save images"), t!("Kapalıyken yalnızca metin ve dosyalar geçmişe alınır.", "When off, only text and files are kept."), c.images, |c| c.clipboard.images ^= true);
    if c.images {
        f.slider(p, t!("En büyük resim", "Largest image"), t!("Bundan büyük resimler geçmişe alınmaz.", "Larger images are not kept."), c.max_image_mb as f32, 1.0, 100.0, 1.0, |v| format!("{v:.0} MB"), |c, v| c.clipboard.max_image_mb = v as u32);
    }
    f.slider(p, t!("Önizleme gecikmesi", "Preview delay"), t!("Resmin üstünde bu kadar bekleyince büyür.", "An image grows after you hover it this long."), c.hover_ms as f32, 100.0, 2000.0, 50.0, |v| format!("{v:.0} ms"), |c, v| c.clipboard.hover_ms = v as u32);
    f.slider(p, t!("Geri alma süresi", "Undo time"), t!("Silinen kayıt bu süre içinde geri alınabilir.", "A deleted item can be restored within this time."), c.undo_ms as f32, 2000.0, 20000.0, 500.0, |v| t!(format!("{:.1} sn", v / 1000.0).replace('.', ","), format!("{:.1} s", v / 1000.0)), |c, v| c.clipboard.undo_ms = v as u32);
    super::applist::draw(
        app,
        p,
        f,
        &c.excluded_apps,
        |c| &mut c.clipboard.excluded_apps,
        Form::ClipApp,
        7,
        t!("Hariç tutulan uygulamalar", "Excluded apps"),
        t!("Bu uygulamalardan kopyalananlar geçmişe alınmaz (exe adı).", "Copies from these apps are not kept (exe name)."),
        t!("ör. KeePass.exe", "e.g. KeePass.exe"),
    );
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
