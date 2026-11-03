//! Ayar satırları: başlık + açıklama solda, denetim (anahtar, kaydırıcı,
//! seçici) sağda, her satır bir cam kart. `Flow` satırları yukarıdan aşağı
//! dizer; sayfalar yalnızca hangi ayarın hangi satırda olduğunu söyler.

use crate::Act;
use carlui_core::config::Config;
use carlui_ui::{
    paint::{Align, Painter, R, TEXT},
    tween,
};
use std::rc::Rc;

pub type P<'a> = Painter<'a, Act>;

/// İçerik sütununun en geniş hali.
pub const W_MAX: f32 = 780.0;

pub fn edit(f: impl Fn(&mut Config) + 'static) -> Act {
    Act::Edit(Rc::new(f))
}

pub struct Flow {
    pub x: f32,
    pub y: f32,
    pub w: f32,
}

impl Flow {
    pub fn section(&mut self, p: &P, title: &str) {
        self.y += 14.0;
        p.text(title, R::new(self.x + 2.0, self.y, self.w, 22.0), 13.0, true, TEXT, 0.7, Align::Left);
        self.y += 28.0;
    }

    /// Kart + başlık/açıklama; `ctrl` sağdaki denetimin genişliği.
    pub fn row(&mut self, p: &P, label: &str, desc: &str, ctrl: f32) -> R {
        self.row_c(p, label, desc, ctrl, None)
    }

    /// `row` gibi; açıklama verilen renkte (ör. hata için turuncu).
    pub fn row_c(&mut self, p: &P, label: &str, desc: &str, ctrl: f32, color: Option<u32>) -> R {
        let h = if desc.is_empty() { 50.0 } else { 64.0 };
        let r = R::new(self.x, self.y, self.w, h);
        p.card(r);
        let tw = r.w - ctrl - 48.0;
        if desc.is_empty() {
            p.text(label, R::new(r.x + 16.0, r.y, tw, h), 14.0, false, TEXT, 0.95, Align::Left);
        } else {
            p.text(label, R::new(r.x + 16.0, r.y + 10.0, tw, 22.0), 14.0, false, TEXT, 0.95, Align::Left);
            let (c, a) = color.map(|c| (c, 0.95)).unwrap_or((TEXT, 0.55));
            p.text(desc, R::new(r.x + 16.0, r.y + 33.0, tw, 20.0), 12.0, false, c, a, Align::Left);
        }
        self.y += h + 6.0;
        r
    }

    /// Açık/kapalı ayar; satırın tamamı tıklanabilir.
    pub fn toggle(&mut self, p: &mut P, label: &str, desc: &str, on: bool, f: impl Fn(&mut Config) + 'static) {
        let r = self.row(p, label, desc, 60.0);
        let hv = tween::to(tween::key(("row", r.x as i32, r.y as i32)), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
        if hv > 0.0 {
            p.round(r, 8.0, 0xFFFFFF, 0.035 * hv);
        }
        let act = edit(f);
        p.hit(r, act.clone());
        p.switch(r.right() - 16.0 - 34.0, r.y + (r.h - 18.0) / 2.0, on, true, act);
    }

    /// Sayısal ayar: `min..max`, `step` adımla; `fmt` değerin yazısı.
    #[allow(clippy::too_many_arguments)]
    pub fn slider(&mut self, p: &mut P, label: &str, desc: &str, value: f32, min: f32, max: f32, step: f32, fmt: impl Fn(f32) -> String, set: impl Fn(&mut Config, f32) + 'static) {
        let r = self.row(p, label, desc, 300.0);
        let tr = R::new(r.right() - 16.0 - 200.0, r.y + r.h / 2.0 - 2.0, 200.0, 4.0);
        let frac = ((value - min) / (max - min)).clamp(0.0, 1.0);
        p.text(&fmt(value), R::new(tr.x - 96.0, r.y, 84.0, r.h), 13.0, false, TEXT, 0.75, Align::Right);
        let hot = p.hover(R::new(tr.x - 8.0, r.y, tr.w + 16.0, r.h));
        p.bar(tr, frac, p.accent);
        let kx = tr.x + tr.w * frac;
        let k = if hot { 9.0 } else { 8.0 };
        p.round(R::new(kx - k, tr.y + 2.0 - k, k * 2.0, k * 2.0), k, 0xFFFFFF, 0.95);
        let setter = move |c: &mut Config, f: f32| {
            let v = min + f.clamp(0.0, 1.0) * (max - min);
            set(c, ((v / step).round() * step).clamp(min, max));
        };
        p.hit(R::new(tr.x - 10.0, r.y, tr.w + 20.0, r.h), Act::Slider(Rc::new(setter), tr));
    }

    /// Birkaç seçenekten biri (bölümlü seçici).
    pub fn choice(&mut self, p: &mut P, label: &str, desc: &str, items: &[(&str, Act)], selected: usize, each: f32) {
        let w = each * items.len() as f32;
        let r = self.row(p, label, desc, w);
        p.segments(R::new(r.right() - 16.0 - w, r.y + (r.h - 32.0) / 2.0, w, 32.0), items, selected);
    }

    /// Soluk açıklama satırı (kart yok).
    pub fn note(&mut self, p: &P, text: &str) {
        p.text(text, R::new(self.x + 2.0, self.y, self.w, 20.0), 12.0, false, TEXT, 0.5, Align::Left);
        self.y += 24.0;
    }
}
