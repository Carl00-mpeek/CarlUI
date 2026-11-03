//! Görünüm: renk paleti (renk örnekleri), cam yoğunluğu, köşe yuvarlaklığı,
//! animasyon hızı, imleç (bkz. `cursor`).

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::theme::{Palette, Rgba};
use carlui_ui::paint::R;

fn hex(c: Rgba) -> u32 {
    (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.theme;
    f.section(p, "Renk");
    let sw = 30.0;
    let gap = 10.0;
    let n = Palette::ALL.len() as f32;
    let r = f.row(p, "Vurgu rengi", c.palette.name(), n * (sw + gap));
    let mut x = r.right() - 16.0 - n * (sw + gap) + gap;
    for pal in Palette::ALL {
        let s = R::new(x, r.y + (r.h - sw) / 2.0, sw, sw);
        let (a, b) = pal.accent();
        if pal == c.palette {
            p.round(s.inset(-4.0), sw / 2.0 + 4.0, 0xFFFFFF, 0.9);
            p.round(s.inset(-2.0), sw / 2.0 + 2.0, 0x000000, 0.9);
        } else if p.hover(s) {
            p.round(s.inset(-3.0), sw / 2.0 + 3.0, 0xFFFFFF, 0.25);
        }
        p.round(s, sw / 2.0, hex(a), 1.0);
        p.round(s.inset(sw * 0.28), sw * 0.22, hex(b), 1.0);
        p.hit(s.inset(-3.0), edit(move |c| c.theme.palette = pal));
        x += sw + gap;
    }

    f.section(p, "Cam ve hareket");
    f.slider(p, "Cam yoğunluğu", "Düşük: daha saydam, yüksek: daha koyu ve okunaklı.", c.glass_density, 0.0, 1.0, 0.05, |v| format!("%{:.0}", v * 100.0), |c, v| c.theme.glass_density = v);
    f.slider(p, "Köşe yuvarlaklığı", "Dock, ada ve panellerin köşeleri.", c.corner_radius, 0.5, 2.0, 0.1, |v| format!("{v:.1}×").replace('.', ","), |c, v| c.theme.corner_radius = v);
    f.slider(p, "Animasyon hızı", "2× iki kat hızlı; 0,5× yavaş ve yumuşak.", c.animation_speed, 0.5, 2.5, 0.1, |v| format!("{v:.1}×").replace('.', ","), |c, v| c.theme.animation_speed = v);

    super::cursor::paint(&app.cfg, p, f);
}
