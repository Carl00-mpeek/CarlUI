//! Bu modül adlı pencere düzenlerinin küçük penceresi: o anki yerleşimi "iş",
//! "oyun" gibi bir adla kaydeder, kayıtlı düzeni tek tıkla uygular ya da siler.
//! Yalnızca açık pencereler yerleştirilir; kapalı uygulamalar başlatılmaz.

use crate::{
    field::Field,
    view::{self, Ctx, Opts, View},
};
use carlui_core::layouts::{self, Layout, Store};
use carlui_ui::paint::{Align, Painter, GREEN, ORANGE, R, TEXT};
use windows::core::Result;

const PAD: f32 = 18.0;
const ROW: f32 = 48.0;
const ROWS: usize = 5;

#[derive(Clone, Debug)]
enum A {
    Nothing,
    Name,
    Save,
    Apply(usize),
    Delete(usize),
    Close,
}

struct Layouts {
    store: Store,
    name: Field,
    scroll: usize,
    message: Option<(String, u32)>,
}

impl Layouts {
    fn save(&mut self) {
        let name = layouts::clean_name(&self.name.text);
        if name.is_empty() {
            return;
        }
        let windows = layouts::capture();
        let n = windows.len();
        self.store.put(Layout { name: name.clone(), windows });
        self.message = Some(match self.store.save() {
            Ok(()) => (t!(format!("{name} kaydedildi · {n} pencere"), format!("{name} saved · {n} windows")), GREEN),
            Err(e) => (t!(format!("Kaydedilemedi: {e}"), format!("Couldn't save: {e}")), ORANGE),
        });
        self.name = Field::new("");
    }
}

impl View for Layouts {
    type A = A;

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        let x = PAD;
        let cw = w - PAD * 2.0;
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        p.glyph('\u{E737}', R::new(x, 14.0, 24.0, 28.0), 16.0, p.accent, 1.0);
        p.text(t!("Pencere düzenleri", "Window layouts"), R::new(x + 30.0, 14.0, cw - 70.0, 28.0), 16.0, true, TEXT, 0.95, Align::Left);
        p.button(R::new(w - PAD - 28.0, 14.0, 28.0, 28.0), Some('\u{E711}'), "", A::Close, true, false);

        self.name.paint(p, R::new(x, 54.0, cw - 112.0, 36.0), true, t!("Düzen adı: iş, oyun…", "Layout name: work, games…"), A::Name);
        let can = !layouts::clean_name(&self.name.text).is_empty();
        p.button(R::new(x + cw - 104.0, 54.0, 104.0, 36.0), Some('\u{E74E}'), t!("Kaydet", "Save"), A::Save, can, false);

        let top = 102.0;
        if self.store.layouts.is_empty() {
            let s = t!("Pencereleri yerleştirin, ad verip kaydedin.", "Arrange your windows, then name and save them.");
            p.text(s, R::new(x, top + 70.0, cw, 24.0), 13.0, false, TEXT, 0.7, Align::Center);
        }
        self.scroll = self.scroll.min(self.store.layouts.len().saturating_sub(ROWS));
        let rows: Vec<(usize, String, usize)> = self.store.layouts.iter().enumerate().skip(self.scroll).take(ROWS).map(|(i, l)| (i, l.name.clone(), l.windows.len())).collect();
        for (k, (i, name, n)) in rows.into_iter().enumerate() {
            let r = R::new(x, top + k as f32 * (ROW + 6.0), cw, ROW);
            p.card(r);
            p.text(&name, R::new(r.x + 14.0, r.y + 6.0, cw - 190.0, 20.0), 13.5, true, TEXT, 0.95, Align::Left);
            p.text(&t!(format!("{n} pencere"), format!("{n} windows")), R::new(r.x + 14.0, r.y + 25.0, cw - 190.0, 18.0), 11.5, false, TEXT, 0.6, Align::Left);
            p.button(R::new(r.right() - 150.0, r.y + 8.0, 96.0, 32.0), None, t!("Uygula", "Apply"), A::Apply(i), true, false);
            p.button(R::new(r.right() - 46.0, r.y + 8.0, 36.0, 32.0), Some('\u{E74D}'), "", A::Delete(i), true, false);
        }

        let foot = match &self.message {
            Some((m, c)) => (m.as_str(), *c, 0.95),
            None => (t!("Kapalı uygulamalar açılmaz, yalnızca açık pencereler yerine gider.", "Closed apps aren't started; only open windows are moved."), TEXT, 0.55),
        };
        p.text(foot.0, R::new(x, h - 34.0, cw, 20.0), 12.0, false, foot.1, foot.2, Align::Left);
    }

    fn click(&mut self, a: A, ctx: &mut Ctx) {
        match a {
            A::Nothing | A::Name => {}
            A::Save => self.save(),
            A::Apply(i) => {
                let Some(l) = self.store.layouts.get(i) else { return };
                let n = layouts::apply(l);
                carlui_core::info!("düzen uygulandı: {} ({n}/{})", l.name, l.windows.len());
                if n == 0 {
                    self.message = Some((t!("Bu düzendeki uygulamaların hiçbiri açık değil.", "None of this layout's apps are open.").to_string(), ORANGE));
                } else {
                    ctx.close = true;
                }
            }
            A::Delete(i) => {
                if i < self.store.layouts.len() {
                    self.store.layouts.remove(i);
                    if let Err(e) = self.store.save() {
                        self.message = Some((t!(format!("Silinemedi: {e}"), format!("Couldn't delete: {e}")), ORANGE));
                    }
                }
            }
            A::Close => ctx.close = true,
        }
    }

    fn key(&mut self, vk: u16, ctx: &mut Ctx) -> bool {
        match vk {
            0x1B => ctx.close = true,
            0x0D => self.save(),
            _ => return self.name.key(vk, view::ctrl()),
        }
        true
    }

    fn char(&mut self, c: char, _ctx: &mut Ctx) {
        self.name.char(c);
    }

    fn wheel(&mut self, delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {
        let max = self.store.layouts.len().saturating_sub(ROWS);
        self.scroll = if delta > 0.0 { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
    }
}

pub fn run() -> Result<()> {
    let v = Layouts { store: Store::load(), name: Field::new(""), scroll: 0, message: None };
    view::run(v, Opts { title: t!("Pencere düzenleri", "Window layouts").into(), w: 480.0, h: 420.0, ..Default::default() })?;
    Ok(())
}
