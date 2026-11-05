//! Görünüm: renk paleti (renk örnekleri), cam yoğunluğu, köşe yuvarlaklığı,
//! animasyon hızı, imleç (bkz. `cursor`).

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::theme::{Palette, Rgba, ThemeMode};
use carlui_ui::paint::R;

fn hex(c: Rgba) -> u32 {
    (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.theme;
    f.section(p, t!("Renk", "Color"));
    let sel = match c.mode {
        ThemeMode::Dark => 0,
        ThemeMode::Light => 1,
        ThemeMode::System => 2,
    };
    f.choice(
        p,
        t!("Tema", "Theme"),
        t!("Sistem, Windows'un açık/koyu ayarını izler. Ada, dock ve tepsi değişince kendini yeniden başlatır.", "System follows Windows' light/dark setting. The island, dock and tray restart themselves when it changes."),
        &[
            (t!("Koyu", "Dark"), edit(|c| c.theme.mode = ThemeMode::Dark)),
            (t!("Açık", "Light"), edit(|c| c.theme.mode = ThemeMode::Light)),
            (t!("Sistem", "System"), edit(|c| c.theme.mode = ThemeMode::System)),
        ],
        sel,
        80.0,
    );
    let sw = 30.0;
    let gap = 10.0;
    let n = Palette::ALL.len() as f32;
    let r = f.row(p, t!("Vurgu rengi", "Accent color"), c.palette.name(), n * (sw + gap));
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

    f.section(p, t!("Cam ve hareket", "Glass and motion"));
    f.slider(p, t!("Cam yoğunluğu", "Glass density"), t!("Düşük: daha saydam, yüksek: daha koyu ve okunaklı.", "Low: more see-through. High: darker and easier to read."), c.glass_density, 0.0, 1.0, 0.05, |v| t!(format!("%{:.0}", v * 100.0), format!("{:.0}%", v * 100.0)), |c, v| c.theme.glass_density = v);
    f.slider(p, t!("Köşe yuvarlaklığı", "Corner roundness"), t!("Dock, ada ve panellerin köşeleri.", "Corners of the dock, island and panels."), c.corner_radius, 0.5, 2.0, 0.1, |v| t!(format!("{v:.1}×").replace('.', ","), format!("{v:.1}×")), |c, v| c.theme.corner_radius = v);
    f.slider(p, t!("Animasyon hızı", "Animation speed"), t!("2× iki kat hızlı; 0,5× yavaş ve yumuşak.", "2× is twice as fast; 0.5× is slow and smooth."), c.animation_speed, 0.5, 2.5, 0.1, |v| t!(format!("{v:.1}×").replace('.', ","), format!("{v:.1}×")), |c, v| c.theme.animation_speed = v);

    super::cursor::paint(&app.cfg, p, f);
}
