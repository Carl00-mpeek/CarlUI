//! Oyun modu: tam ekranda gizlenme, modülleri uyutma, bildirimleri saklama,
//! oyun sayılan programlar.

use crate::{
    input::Form,
    ui::{Flow, P},
    App,
};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.game;
    f.section(p, t!("Tam ekran ve oyunlar", "Fullscreen and games"));
    f.toggle(p, t!("Tam ekranda gizlen", "Hide in fullscreen"), t!("Oyun ya da tam ekran video açılınca dock ve ada çekilir.", "The dock and island step aside for games and fullscreen video."), c.hide_on_fullscreen, |c| c.game.hide_on_fullscreen ^= true);
    f.toggle(p, t!("Bildirimleri sonraya sakla", "Hold notifications"), t!("Oyunda gelenler çıkınca gösterilir; kapalıysa hiç gösterilmez.", "Notifications that arrive in a game show when you leave it; when off they are not shown."), c.hold_notes, |c| c.game.hold_notes ^= true);
    f.toggle(p, t!("Tam ekranda şarkı ve sesi göster", "Show song and volume in fullscreen"), t!("Şarkı değişince ya da ses açılıp kısılınca ada kısa süre görünür. Özel tam ekran oyunlarda görünmeyebilir.", "The island appears briefly when the song or volume changes. It may not appear in exclusive fullscreen games."), c.show_osd, |c| c.game.show_osd ^= true);
    super::applist::draw(
        app,
        p,
        f,
        &c.apps,
        |c| &mut c.game.apps,
        Form::GameApp,
        11,
        t!("Oyun sayılan programlar", "Programs treated as games"),
        t!("Büyütülmüş ya da kenarlıksız penceresi oyun sayılır: Ctrl+Space gibi kısayollar oyuna bırakılır. CarlUI'nin kendiliğinden tanımadığı oyunları ekleyin.", "A maximized or borderless window counts as a game, so shortcuts like Ctrl+Space go to the game. Add games CarlUI doesn't recognize on its own."),
        t!("exe adı, ör. RobloxPlayerBeta.exe", "exe name, e.g. RobloxPlayerBeta.exe"),
    );
}
