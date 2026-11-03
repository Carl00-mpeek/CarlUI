//! Oyun modu: tam ekranda gizlenme, modülleri uyutma, bildirimleri saklama.

use crate::{
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.game;
    f.section(p, t!("Tam ekran ve oyunlar", "Fullscreen and games"));
    f.toggle(p, t!("Tam ekranda gizlen", "Hide in fullscreen"), t!("Oyun ya da tam ekran video açılınca dock ve ada çekilir.", "The dock and island step aside for games and fullscreen video."), c.hide_on_fullscreen, |c| c.game.hide_on_fullscreen ^= true);
    f.toggle(p, t!("Bildirimleri sonraya sakla", "Hold notifications"), t!("Oyunda gelenler çıkınca gösterilir; kapalıysa hiç gösterilmez.", "Notifications that arrive in a game show when you leave it; when off they are not shown."), c.hold_notes, |c| c.game.hold_notes ^= true);
}
