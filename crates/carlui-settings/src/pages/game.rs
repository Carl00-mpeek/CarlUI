//! Oyun modu: tam ekranda gizlenme, modülleri uyutma, bildirimleri saklama.

use crate::{
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.game;
    f.section(p, "Tam ekran ve oyunlar");
    f.toggle(p, "Tam ekranda gizlen", "Oyun ya da tam ekran video açılınca dock ve ada çekilir.", c.hide_on_fullscreen, |c| c.game.hide_on_fullscreen ^= true);
    f.toggle(p, "Bildirimleri sonraya sakla", "Oyunda gelenler çıkınca gösterilir; kapalıysa hiç gösterilmez.", c.hold_notes, |c| c.game.hold_notes ^= true);
}
