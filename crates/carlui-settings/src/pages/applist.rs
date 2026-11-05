//! Bu bölüm "bir ayarın uygulama listesini (engel / hariç tutma) nasıl
//! düzenleriz?" sorununu çözüyor: her ad bir satır ve silme düğmesi; altta
//! "Ekle" düğmesi tek alanlı formu açar. Bildirim engeli, pano ve metin
//! genişletici hariç tutma listeleri aynı bileşeni kullanır.

use crate::{
    input::Form,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::config::Config;
use carlui_ui::paint::{Align, R, TEXT};

/// `list`: gösterilecek adlar; `pick`: ayardaki listeye erişim; `open`: formu açan `Act::Input` kodu.
pub fn draw(app: &App, p: &mut P, f: &mut Flow, list: &[String], pick: fn(&mut Config) -> &mut Vec<String>, kind: Form, open: u8, label: &str, desc: &str, hint: &str) {
    let r = f.row(p, label, desc, 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E710}'), t!("Ekle", "Add"), Act::Input(open), app.input.is_none(), false);
    super::form::draw(app, p, f, kind, &[(t!("Uygulama", "App"), hint)]);
    for (i, name) in list.iter().enumerate() {
        let r = f.row(p, name, "", 60.0);
        let del = R::new(r.right() - 12.0 - 32.0, r.y + (r.h - 32.0) / 2.0, 32.0, 32.0);
        if p.hover(del) {
            p.round(del, 6.0, 0xFFFFFF, 0.10);
        }
        p.glyph('\u{E74D}', del, 13.0, TEXT, 0.75);
        p.hit(
            del,
            edit(move |c| {
                let l = pick(c);
                if i < l.len() {
                    l.remove(i);
                }
            }),
        );
    }
    f.flush(p);
    if list.is_empty() {
        p.text(t!("Liste boş.", "The list is empty."), R::new(f.x + 4.0, f.y, f.w, 24.0), 12.5, false, TEXT, 0.5, Align::Left);
        f.y += 28.0;
    }
}
