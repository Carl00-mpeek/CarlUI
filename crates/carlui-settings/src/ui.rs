//! Ayar satırları: başlık + açıklama solda, denetim (anahtar, kaydırıcı,
//! seçici) sağda, her satır bir cam kart. `Flow` satırları yukarıdan aşağı
//! dizer; sayfalar yalnızca hangi ayarın hangi satırda olduğunu söyler.
//!
//! Açıklamalar hiçbir zaman "…" ile kesilmez: sığmayan metin alt satıra kayar,
//! kart onunla birlikte uzar. Denetim yanına sığmayacak kadar geniş olunca
//! (küçük ekran) metnin altına iner. Art arda gelen açık/kapalı ayarlar iki
//! sütunlu küçük kartlara dizilir; uzun listeler yarı yarıya kısalır.

use crate::Act;
use carlui_core::config::Config;
use carlui_ui::{
    paint::{self, Align, Painter, R, TEXT},
    tween,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub type P<'a> = Painter<'a, Act>;

/// İçerik sütununun en geniş hali.
pub const W_MAX: f32 = 780.0;

/// Açıklama yazısı boyutu ve satır yüksekliği.
const DESC: f32 = 12.0;
const LINE: f32 = 17.0;
/// Başlık yazısı boyutu ve satır yüksekliği.
const LABEL: f32 = 14.0;
const LABEL_LINE: f32 = 20.0;
/// Denetimin yanında metne en az bu kadar yer kalmalı; yoksa denetim alta iner.
const MIN_TEXT: f32 = 260.0;
/// İki sütunlu anahtar kartları bu genişliğin altında tek sütuna döner.
const PAIR_MIN: f32 = 620.0;
const GAP: f32 = 6.0;

pub fn edit(f: impl Fn(&mut Config) + 'static) -> Act {
    Act::Edit(Rc::new(f))
}

thread_local! {
    static WRAPS: RefCell<HashMap<(String, u32, u32, bool), Vec<String>>> = RefCell::default();
}

/// Metni `max` genişliğe göre satırlara böler; sonuç önbellekte tutulur
/// (her karede yeniden ölçülmesin).
fn lines(p: &P, text: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let key = (text.to_string(), (size * 10.0) as u32, max.max(0.0) as u32, bold);
    if let Some(v) = WRAPS.with(|w| w.borrow().get(&key).cloned()) {
        return v;
    }
    let v = paint::wrap(text, max, |s| p.fonts.width(s, size, bold));
    WRAPS.with(|w| {
        let mut w = w.borrow_mut();
        if w.len() > 4000 {
            w.clear();
        }
        w.insert(key, v.clone());
    });
    v
}

fn draw_lines(p: &P, v: &[String], x: f32, y: f32, w: f32, size: f32, lh: f32, bold: bool, c: u32, a: f32) {
    for (i, s) in v.iter().enumerate() {
        p.text(s, R::new(x, y + i as f32 * lh, w, lh), size, bold, c, a, Align::Left);
    }
}

fn desc_color(color: Option<u32>) -> (u32, f32) {
    color.map(|c| (c, 0.95)).unwrap_or((TEXT, 0.55))
}

/// Sırası gelince çizilecek açık/kapalı ayar (iki sütunlu dizilim için bekler).
struct Pending {
    label: String,
    desc: String,
    on: bool,
    act: Act,
}

pub struct Flow {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pending: Vec<Pending>,
}

impl Flow {
    pub fn new(x: f32, y: f32, w: f32) -> Self {
        Self { x, y, w, pending: Vec::new() }
    }

    /// Bekleyen anahtarları çizer. Her `Flow` çağrısı bunu kendisi yapar;
    /// `f.y`'yi doğrudan kullanan sayfalar önce bunu çağırmalı.
    pub fn flush(&mut self, p: &mut P) {
        let items = std::mem::take(&mut self.pending);
        if self.w < PAIR_MIN || items.len() == 1 {
            for it in items {
                self.tile(p, it, self.x, self.w, None);
            }
            return;
        }
        let half = (self.w - GAP) / 2.0;
        let mut it = items.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => {
                    let h = self.tile_h(p, &a, half).max(self.tile_h(p, &b, half));
                    let x = self.x;
                    self.tile(p, a, x, half, Some(h));
                    self.y -= h + GAP;
                    self.tile(p, b, x + half + GAP, half, Some(h));
                }
                // Tek kalan anahtar tam genişlikte, ötekilerle aynı biçimde.
                None => self.tile(p, a, self.x, self.w, None),
            }
        }
    }

    fn tile_h(&self, p: &P, t: &Pending, w: f32) -> f32 {
        let tw = w - 32.0 - 50.0;
        let nl = lines(p, &t.label, LABEL, false, tw).len().max(1) as f32;
        let nd = if t.desc.is_empty() { 0.0 } else { lines(p, &t.desc, DESC, false, tw).len() as f32 };
        if nd == 0.0 && nl == 1.0 {
            return 50.0;
        }
        (12.0 + nl * LABEL_LINE + 2.0 + nd * LINE + 12.0).max(64.0)
    }

    fn tile(&mut self, p: &mut P, t: Pending, x: f32, w: f32, h: Option<f32>) {
        let h = h.unwrap_or_else(|| self.tile_h(p, &t, w));
        let r = R::new(x, self.y, w, h);
        p.card(r);
        let hv = tween::to(tween::at("row", r.x, r.y), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
        if hv > 0.0 {
            p.round(r, 8.0, 0xFFFFFF, 0.035 * hv);
        }
        let tw = w - 32.0 - 50.0;
        let lv = lines(p, &t.label, LABEL, false, tw);
        if t.desc.is_empty() && lv.len() <= 1 {
            // Yanındaki kart uzunsa başlık onunkiyle aynı hizada, üstte durur.
            let lr = if h <= 64.0 { R::new(r.x + 16.0, r.y, tw, h) } else { R::new(r.x + 16.0, r.y + 12.0, tw, LABEL_LINE) };
            p.text(&t.label, lr, LABEL, false, TEXT, 0.95, Align::Left);
        } else {
            draw_lines(p, &lv, r.x + 16.0, r.y + 12.0, tw, LABEL, LABEL_LINE, false, TEXT, 0.95);
            let dv = lines(p, &t.desc, DESC, false, tw);
            let dy = r.y + 12.0 + lv.len().max(1) as f32 * LABEL_LINE + 2.0;
            draw_lines(p, &dv, r.x + 16.0, dy, tw, DESC, LINE, false, TEXT, 0.55);
        }
        p.hit(r, t.act.clone());
        // Anahtar tek satırlık kartta ortada, uzun kartta başlık hizasında.
        let sy = if h <= 64.0 { r.y + (h - 18.0) / 2.0 } else { r.y + 13.0 };
        p.switch(r.right() - 16.0 - 34.0, sy, t.on, true, t.act);
        self.y += h + GAP;
    }

    pub fn section(&mut self, p: &mut P, title: &str) {
        self.flush(p);
        self.y += 14.0;
        p.text(title, R::new(self.x + 2.0, self.y, self.w, 22.0), 13.0, true, TEXT, 0.7, Align::Left);
        self.y += 28.0;
    }

    /// Kart + başlık/açıklama; `ctrl` sağdaki denetimin genişliği. Dönen
    /// dikdörtgen denetimin dikey ortalanacağı alandır: denetim yana sığmazsa
    /// metnin altındaki şerit.
    pub fn row(&mut self, p: &mut P, label: &str, desc: &str, ctrl: f32) -> R {
        self.row_c(p, label, desc, ctrl, None)
    }

    /// `row` gibi; açıklama verilen renkte (ör. hata için turuncu).
    pub fn row_c(&mut self, p: &mut P, label: &str, desc: &str, ctrl: f32, color: Option<u32>) -> R {
        self.rows(p, label, &[(desc, color)], ctrl)
    }

    /// `row_c` gibi; altında üçüncü bir satır (ör. önerilen sistem) ve kendi rengi.
    #[allow(clippy::too_many_arguments)]
    pub fn row3(&mut self, p: &mut P, label: &str, desc: &str, color: Option<u32>, extra: &str, extra_color: Option<u32>, ctrl: f32) -> R {
        self.rows(p, label, &[(desc, color), (extra, extra_color)], ctrl)
    }

    fn rows(&mut self, p: &mut P, label: &str, descs: &[(&str, Option<u32>)], ctrl: f32) -> R {
        self.flush(p);
        let side = self.w - ctrl - 48.0;
        let stacked = ctrl > 0.0 && (side < MIN_TEXT || p.fonts.width(label, LABEL, false) > side);
        let tw = if stacked { self.w - 32.0 } else { side };
        let blocks: Vec<(Vec<String>, Option<u32>)> = descs.iter().filter(|(d, _)| !d.is_empty()).map(|(d, c)| (lines(p, d, DESC, false, tw), *c)).collect();
        let lv = lines(p, label, LABEL, false, tw);
        let nd: usize = blocks.iter().map(|(v, _)| v.len()).sum();
        let one = nd == 0 && lv.len() <= 1;
        let text_h = if one { 50.0 } else { (12.0 + lv.len().max(1) as f32 * LABEL_LINE + 2.0 + nd as f32 * LINE + 12.0).max(64.0) };
        let band = 44.0;
        let h = if stacked { text_h - 6.0 + band + 6.0 } else { text_h };
        let r = R::new(self.x, self.y, self.w, h);
        p.card(r);
        if one && !stacked {
            p.text(label, R::new(r.x + 16.0, r.y, tw, h), LABEL, false, TEXT, 0.95, Align::Left);
        } else if one {
            p.text(label, R::new(r.x + 16.0, r.y, tw, 50.0), LABEL, false, TEXT, 0.95, Align::Left);
        } else {
            draw_lines(p, &lv, r.x + 16.0, r.y + 12.0, tw, LABEL, LABEL_LINE, false, TEXT, 0.95);
            let mut y = r.y + 12.0 + lv.len().max(1) as f32 * LABEL_LINE + 2.0;
            for (v, c) in &blocks {
                let (c, a) = desc_color(*c);
                draw_lines(p, v, r.x + 16.0, y, tw, DESC, LINE, false, c, a);
                y += v.len() as f32 * LINE;
            }
        }
        self.y += h + GAP;
        if stacked {
            R::new(r.x, r.y + text_h - 6.0, r.w, band)
        } else {
            r
        }
    }

    /// Açık/kapalı ayar; kartın tamamı tıklanabilir. Art arda gelenler iki
    /// sütunlu küçük kartlara dizilir (bir sonraki çağrıda çizilir).
    pub fn toggle(&mut self, _p: &mut P, label: &str, desc: &str, on: bool, f: impl Fn(&mut Config) + 'static) {
        self.pending.push(Pending { label: label.to_string(), desc: desc.to_string(), on, act: edit(f) });
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

    /// Soluk açıklama satırı (kart yok); uzunsa alt satıra kayar.
    pub fn note(&mut self, p: &mut P, text: &str) {
        self.flush(p);
        let v = lines(p, text, DESC, false, self.w - 4.0);
        draw_lines(p, &v, self.x + 2.0, self.y, self.w - 4.0, DESC, LINE + 1.0, false, TEXT, 0.5);
        self.y += v.len().max(1) as f32 * (LINE + 1.0) + 6.0;
    }
}
