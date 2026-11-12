//! Hayvan adımı: "Yok" ve sekiz yerleşik hayvan kart olarak (Ayarlar'daki
//! dock sayfasının gömülü kareleri, daha büyük), altında dock camının
//! sahnesi ve kedi/köpek seçiliyse renk satırı.

use super::{
    choices,
    steps::{card, name_row, GAP},
    Go, Welcome, W,
};
use crate::pages::dock_live_thumbs;
use carlui_ui::paint::{wrap, Align, R, TEXT};

const COLS: usize = 5;
const CARD_H: f32 = 92.0;
const LABEL_W: f32 = 110.0;
const ROW_H: f32 = 32.0;

pub(super) fn pet(app: &Welcome, p: &mut W, r: R) {
    let pet = app.cfg.dock.live.pet.trim();
    let main = choices::pet_main(pet);
    let ids: Vec<&'static str> = std::iter::once("").chain(choices::PETS).collect();
    let cw = (r.w - GAP * (COLS as f32 - 1.0)) / COLS as f32;
    let cell = |i: usize| R::new(r.x + (i % COLS) as f32 * (cw + GAP), r.y + (i / COLS) as f32 * (CARD_H + GAP), cw, CARD_H);
    for (i, id) in ids.iter().enumerate() {
        let c = cell(i);
        let selected = *id == main;
        card(app, p, c, selected, Go::Pet(*id));
        let art = R::new(c.x + 6.0, c.y + 8.0, c.w - 12.0, 48.0);
        if id.is_empty() {
            p.glyph('\u{E711}', art, 18.0, TEXT, 0.4);
        } else {
            // Seçili renk varyantı kendi karesiyle görünür.
            let shown = if selected && choices::variants(pet).is_some() { pet } else { id };
            dock_live_thumbs::draw_in(p, shown, art, 96.0, 44.0);
        }
        name_row(p, R::new(c.x + 12.0, c.bottom() - 32.0, c.w - 20.0, 28.0), choices::pet_label(id), selected);
    }
    // Son boş hücre: kısa bilgi.
    if ids.len() < COLS * 2 {
        let c = cell(ids.len());
        p.round(c, app.round(10.0), 0xFFFFFF, 0.02);
        p.glyph('\u{E946}', R::new(c.x + 12.0, c.y + 10.0, 16.0, 18.0), 12.0, TEXT, 0.5);
        let info = t!("Tıklamaları engellemez; yalnızca dock alttayken görünür.", "Never blocks clicks; only shows when the dock is at the bottom.");
        let lines = wrap(info, c.w - 24.0, |s| p.fonts.width(s, 11.5, false));
        for (k, l) in lines.iter().take(4).enumerate() {
            p.text(l, R::new(c.x + 12.0, c.y + 30.0 + k as f32 * 15.0, c.w - 20.0, 15.0), 11.5, false, TEXT, 0.55, Align::Left);
        }
    }

    let label = |p: &W, y: f32, s: &str| p.text(s, R::new(r.x, y, LABEL_W, ROW_H), 13.0, true, TEXT, 0.85, Align::Left);
    let mut y = r.y + 2.0 * CARD_H + GAP + 16.0;
    // Sahne: dock camında hafif hareket.
    label(p, y, t!("Sahne", "Scene"));
    let scenes = choices::scenes();
    let items: Vec<(&str, Go)> = scenes.iter().map(|(s, name)| (*name, Go::Scene(*s))).collect();
    let sel = scenes.iter().position(|(s, _)| *s == app.cfg.dock.live.scene).unwrap_or(usize::MAX);
    p.segments(R::new(r.x + LABEL_W, y, r.w - LABEL_W, ROW_H), &items, sel);
    y += ROW_H + 12.0;

    if let Some(vars) = choices::variants(pet) {
        label(p, y, t!("Renk", "Color"));
        let items: Vec<(&str, Go)> = vars.iter().map(|(id, name)| (*name, Go::PetColor(*id))).collect();
        let sel = vars.iter().position(|(id, _)| *id == pet).unwrap_or(usize::MAX);
        p.segments(R::new(r.x + LABEL_W, y, (r.w - LABEL_W).min(440.0), ROW_H), &items, sel);
        y += ROW_H + 12.0;
    }
    let note = t!("Hayvan ve sahne Ayarlar > Dock > Canlı dock'ta da değişir; orada kendi hayvan paketini de ekleyebilirsin.", "The pet and scene also change in Settings > Dock > Living dock, where you can add your own pet packs too.");
    let lines = wrap(note, r.w - LABEL_W, |s| p.fonts.width(s, 12.0, false));
    for (k, l) in lines.iter().take(2).enumerate() {
        p.text(l, R::new(r.x + LABEL_W, y + k as f32 * 17.0, r.w - LABEL_W, 17.0), 12.0, false, TEXT, 0.5, Align::Left);
    }
}
