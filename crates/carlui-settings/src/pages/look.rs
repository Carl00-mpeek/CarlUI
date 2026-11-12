//! Görünüm: hazır/kayıtlı görünümler (bkz. `looks`), tema, vurgu rengi (bkz. `look_accent`), cam yoğunluğu, köşe
//! yuvarlaklığı, animasyon hızı, hareketi azalt, imleç (bkz. `cursor`).

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::theme::ThemeMode;

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.theme;
    super::looks::paint(app, p, f);
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
    super::look_accent::paint(app, p, f);

    f.section(p, t!("Cam ve hareket", "Glass and motion"));
    f.slider(p, t!("Cam yoğunluğu", "Glass density"), t!("Düşük: daha saydam, yüksek: daha koyu ve okunaklı.", "Low: more see-through. High: darker and easier to read."), c.glass_density, 0.0, 1.0, 0.05, |v| t!(format!("%{:.0}", v * 100.0), format!("{:.0}%", v * 100.0)), |c, v| c.theme.glass_density = v);
    f.slider(p, t!("Köşe yuvarlaklığı", "Corner roundness"), t!("Dock, ada ve panellerin köşeleri.", "Corners of the dock, island and panels."), c.corner_radius, 0.5, 2.0, 0.1, |v| t!(format!("{v:.1}×").replace('.', ","), format!("{v:.1}×")), |c, v| c.theme.corner_radius = v);
    if !c.reduce_motion {
        f.slider(p, t!("Animasyon hızı", "Animation speed"), t!("2× iki kat hızlı; 0,5× yavaş ve yumuşak.", "2× is twice as fast; 0.5× is slow and smooth."), c.animation_speed, 0.5, 2.5, 0.1, |v| t!(format!("{v:.1}×").replace('.', ","), format!("{v:.1}×")), |c, v| c.theme.animation_speed = v);
    }
    f.toggle(
        p,
        t!("Hareketi azalt", "Reduce motion"),
        t!(
            "Açılış, kapanış, kayma ve geçiş animasyonları anında biter; dock'ta küçültme uçuşu yapılmaz. Müzik dalgası gibi sürekli göstergeler sürer.",
            "Opening, closing, sliding and transition animations finish instantly; the dock skips the minimize flight. Continuous visuals such as the music wave keep running."
        ),
        c.reduce_motion,
        |c| c.theme.reduce_motion = !c.theme.reduce_motion,
    );

    super::cursor::paint(&app.cfg, p, f);
}
