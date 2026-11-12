//! Ayar satırları: başlık solda, denetim (anahtar, kaydırıcı, seçici) sağda,
//! her satır bir cam kart. `Flow` satırları yukarıdan aşağı dizer; sayfalar
//! yalnızca hangi ayarın hangi satırda olduğunu söyler.
//!
//! Sayfalar kalabalık görünmesin diye uzun açıklamalar kartta yazılmaz:
//! satırlara verilen `info` metni başlığın hemen yanındaki küçük "i"
//! düğmesinin arkasına gider (bkz. `info.rs`), kart tek satıra iner. Kartta
//! görünmesi gerekenler (canlı değer, durum, hata, uyarı) `*_sub` / `row_lines`
//! ile başlığın altına yazılır.
//!
//! Metin hiçbir zaman "…" ile kesilmez: sığmayan metin alt satıra kayar, kart
//! onunla birlikte uzar. Denetim yanına sığmayacak kadar geniş olunca (küçük
//! ekran) metnin altına iner. Art arda gelen açık/kapalı ayarlar iki sütunlu
//! küçük kartlara dizilir; uzun listeler yarı yarıya kısalır.

use crate::{info, Act};
use carlui_core::config::Config;
use carlui_ui::{
    paint::{self, Align, Painter, R, TEXT},
    tween,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub type P<'a> = Painter<'a, Act>;

/// İçerik sütununun en geniş hali.
pub const W_MAX: f32 = 780.0;

/// Alt satır (durum, değer) yazısı boyutu ve satır yüksekliği.
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
pub fn lines(p: &P, text: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
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

/// "i" düğmesi varsa başlığın sığacağı genişlik ondan kadar daralır.
fn label_w(tw: f32, info: &str) -> f32 {
    if info.is_empty() {
        tw
    } else {
        tw - info::SIZE - info::GAP
    }
}

/// Başlığı satır satır çizer ve "i" düğmesini son satırın hemen sonuna koyar.
fn label_lines(p: &mut P, lv: &[String], x: f32, y: f32, w: f32, lh: f32, info: &str) {
    draw_lines(p, lv, x, y, w, LABEL, lh, false, TEXT, 0.95);
    if let Some(last) = lv.last() {
        let lw = p.fonts.width(last, LABEL, false).min(w);
        info::button(p, x + lw + info::GAP, y + (lv.len() - 1) as f32 * lh + lh / 2.0, info);
    }
}

/// Başlığın satırları; boş başlık da tek (boş) satır sayılır.
fn label_of(p: &P, label: &str, w: f32) -> Vec<String> {
    let v = lines(p, label, LABEL, false, w);
    if v.is_empty() {
        vec![label.to_string()]
    } else {
        v
    }
}

/// Sırası gelince çizilecek açık/kapalı ayar (iki sütunlu dizilim için bekler).
struct Pending {
    label: String,
    info: String,
    /// Kartta görünen alt satır (ör. durum) ve rengi.
    sub: String,
    color: Option<u32>,
    on: bool,
    act: Act,
}

/// `Flow::choice_lead_rows` için bir düğme satırı.
pub struct LeadRow<'a> {
    pub items: &'a [(&'a str, Act)],
    pub selected: usize,
    pub each: f32,
    pub lead_w: &'a [f32],
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
        let nl = label_of(p, &t.label, label_w(tw, &t.info)).len() as f32;
        let nd = if t.sub.is_empty() { 0.0 } else { lines(p, &t.sub, DESC, false, tw).len() as f32 };
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
        let lv = label_of(p, &t.label, label_w(tw, &t.info));
        if t.sub.is_empty() && lv.len() <= 1 {
            // Yanındaki kart uzunsa başlık onunkiyle aynı hizada, üstte durur.
            let (y, lh) = if h <= 64.0 { (r.y, h) } else { (r.y + 12.0, LABEL_LINE) };
            label_lines(p, &lv, r.x + 16.0, y, tw, lh, &t.info);
        } else {
            label_lines(p, &lv, r.x + 16.0, r.y + 12.0, tw, LABEL_LINE, &t.info);
            let dv = lines(p, &t.sub, DESC, false, tw);
            let dy = r.y + 12.0 + lv.len() as f32 * LABEL_LINE + 2.0;
            let (c, a) = desc_color(t.color);
            draw_lines(p, &dv, r.x + 16.0, dy, tw, DESC, LINE, false, c, a);
        }
        p.hit(r, t.act.clone());
        // Anahtar tek satırlık kartta ortada, uzun kartta başlık hizasında.
        let sy = if h <= 64.0 { r.y + (h - 18.0) / 2.0 } else { r.y + 13.0 };
        p.switch(r.right() - 16.0 - 34.0, sy, t.on, true, t.act);
        self.y += h + GAP;
    }

    pub fn section(&mut self, p: &mut P, title: &str) {
        self.section_info(p, title, "");
    }

    /// Bölüm başlığı; `info` boş değilse bölümün açıklaması başlığın yanındaki
    /// "i" düğmesinde durur.
    pub fn section_info(&mut self, p: &mut P, title: &str, info: &str) {
        self.flush(p);
        self.y += 14.0;
        p.text(title, R::new(self.x + 2.0, self.y, self.w, 22.0), 13.0, true, TEXT, 0.7, Align::Left);
        let tw = p.fonts.width(title, 13.0, true).min(self.w - 30.0);
        info::button(p, self.x + 2.0 + tw + info::GAP, self.y + 11.0, info);
        self.y += 28.0;
    }

    /// Kart + başlık (açıklaması `info` düğmesinde); `ctrl` sağdaki denetimin
    /// genişliği. Dönen dikdörtgen denetimin dikey ortalanacağı alandır:
    /// denetim yana sığmazsa metnin altındaki şerit.
    pub fn row(&mut self, p: &mut P, label: &str, info: &str, ctrl: f32) -> R {
        self.rows(p, label, info, &[], ctrl)
    }

    /// `row` gibi; başlığın altında kartta görünen bir satır (canlı değer,
    /// durum, hata) ve rengi (yoksa soluk).
    pub fn row_sub(&mut self, p: &mut P, label: &str, info: &str, sub: &str, color: Option<u32>, ctrl: f32) -> R {
        self.rows(p, label, info, &[(sub, color)], ctrl)
    }

    /// `row_sub` gibi; birden çok görünen satır, her biri kendi renginde.
    pub fn row_lines(&mut self, p: &mut P, label: &str, info: &str, subs: &[(&str, Option<u32>)], ctrl: f32) -> R {
        self.rows(p, label, info, subs, ctrl)
    }

    fn rows(&mut self, p: &mut P, label: &str, info: &str, descs: &[(&str, Option<u32>)], ctrl: f32) -> R {
        self.flush(p);
        let side = self.w - ctrl - 48.0;
        let reserve = self.w - label_w(self.w, info);
        let stacked = ctrl > 0.0 && (side < MIN_TEXT || p.fonts.width(label, LABEL, false) + reserve > side);
        let tw = if stacked { self.w - 32.0 } else { side };
        let blocks: Vec<(Vec<String>, Option<u32>)> = descs.iter().filter(|(d, _)| !d.is_empty()).map(|(d, c)| (lines(p, d, DESC, false, tw), *c)).collect();
        let lv = label_of(p, label, label_w(tw, info));
        let nd: usize = blocks.iter().map(|(v, _)| v.len()).sum();
        let one = nd == 0 && lv.len() <= 1;
        let text_h = if one { 50.0 } else { (12.0 + lv.len() as f32 * LABEL_LINE + 2.0 + nd as f32 * LINE + 12.0).max(64.0) };
        let band = 44.0;
        let h = if stacked { text_h - 6.0 + band + 6.0 } else { text_h };
        let r = R::new(self.x, self.y, self.w, h);
        p.card(r);
        if one {
            label_lines(p, &lv, r.x + 16.0, r.y, tw, if stacked { 50.0 } else { h }, info);
        } else {
            label_lines(p, &lv, r.x + 16.0, r.y + 12.0, tw, LABEL_LINE, info);
            let mut y = r.y + 12.0 + lv.len() as f32 * LABEL_LINE + 2.0;
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
    pub fn toggle(&mut self, p: &mut P, label: &str, info: &str, on: bool, f: impl Fn(&mut Config) + 'static) {
        self.toggle_sub(p, label, info, "", None, on, f);
    }

    /// `toggle` gibi; başlığın altında kartta görünen bir satırla (ör. durum).
    #[allow(clippy::too_many_arguments)]
    pub fn toggle_sub(&mut self, _p: &mut P, label: &str, info: &str, sub: &str, color: Option<u32>, on: bool, f: impl Fn(&mut Config) + 'static) {
        self.pending.push(Pending { label: label.to_string(), info: info.to_string(), sub: sub.to_string(), color, on, act: edit(f) });
    }

    /// Sayısal ayar: `min..max`, `step` adımla; `fmt` değerin yazısı.
    #[allow(clippy::too_many_arguments)]
    pub fn slider(&mut self, p: &mut P, label: &str, info: &str, value: f32, min: f32, max: f32, step: f32, fmt: impl Fn(f32) -> String, set: impl Fn(&mut Config, f32) + 'static) {
        let r = self.row(p, label, info, 300.0);
        let tr = R::new(r.right() - 16.0 - 200.0, r.y + r.h / 2.0 - 2.0, 200.0, 4.0);
        let frac = ((value - min) / (max - min)).clamp(0.0, 1.0);
        p.text(&fmt(value), R::new(tr.x - 96.0, r.y, 84.0, r.h), 13.0, false, TEXT, 0.75, Align::Right);
        let hot = p.hover(R::new(tr.x - 8.0, r.y, tr.w + 16.0, r.h));
        p.bar(tr, frac, p.accent);
        let kx = tr.x + tr.w * frac;
        let k = if hot { 9.0 } else { 8.0 };
        p.knob(kx, tr.y + 2.0, k);
        let setter = move |c: &mut Config, f: f32| {
            let v = min + f.clamp(0.0, 1.0) * (max - min);
            set(c, ((v / step).round() * step).clamp(min, max));
        };
        p.hit(R::new(tr.x - 10.0, r.y, tr.w + 20.0, r.h), Act::Slider(Rc::new(setter), tr));
    }

    /// Birkaç seçenekten biri (bölümlü seçici).
    pub fn choice(&mut self, p: &mut P, label: &str, info: &str, items: &[(&str, Act)], selected: usize, each: f32) {
        self.choice_sub(p, label, info, "", items, selected, each);
    }

    /// `choice` gibi; başlığın altında kartta görünen bir satırla (ör. kısayol).
    #[allow(clippy::too_many_arguments)]
    pub fn choice_sub(&mut self, p: &mut P, label: &str, info: &str, sub: &str, items: &[(&str, Act)], selected: usize, each: f32) {
        let w = each * items.len() as f32;
        let r = self.row_sub(p, label, info, sub, None, w);
        p.segments(R::new(r.right() - 16.0 - w, r.y + (r.h - 32.0) / 2.0, w, 32.0), items, selected);
    }

    /// Tek kartta bir ya da birkaç düğme satırı (sağa yaslı); her düğmenin
    /// yazısının soluna `lead_w[i]` genişliğinde simge alanı ayrılır, `draw`
    /// (satır, düğme, alan) oraya çizer. Başlık yanına sığarsa solda ortada,
    /// sığmazsa düğmelerin üstünde durur; satırlar arası sıkı.
    pub fn choice_lead_rows(&mut self, p: &mut P, label: &str, info: &str, rows: &[LeadRow], draw: &dyn Fn(&P, usize, usize, R)) {
        self.flush(p);
        const BTN: f32 = 32.0;
        let widest = rows.iter().map(|r| r.each * r.items.len() as f32).fold(0.0, f32::max);
        let n = rows.len() as f32;
        let grid = n * BTN + (n - 1.0) * GAP;
        let lw = p.fonts.width(label, LABEL, false) + if info.is_empty() { 0.0 } else { info::SIZE + info::GAP };
        let beside = lw + widest + 32.0 + 24.0 <= self.w;
        let h = if beside { (grid + 18.0).max(50.0) } else { 12.0 + LABEL_LINE + 6.0 + grid + 12.0 };
        let r = R::new(self.x, self.y, self.w, h);
        p.card(r);
        let top = if beside { r.y + (h - grid) / 2.0 } else { r.y + 12.0 + LABEL_LINE + 6.0 };
        if !label.is_empty() {
            let ly = if beside { r.y + (h - LABEL_LINE) / 2.0 } else { r.y + 12.0 };
            let lv = label_of(p, label, label_w(self.w - 32.0, info));
            label_lines(p, &lv, r.x + 16.0, ly, self.w - 32.0, LABEL_LINE, info);
        }
        for (i, row) in rows.iter().enumerate() {
            let w = row.each * row.items.len() as f32;
            let seg = R::new(r.right() - 16.0 - w, top + i as f32 * (BTN + GAP), w, BTN);
            p.segments_lead(seg, row.items, row.selected, row.lead_w, &|p, j, a| draw(p, i, j, a));
        }
        self.y += h + GAP;
    }

    /// Soluk açıklama satırı (kart yok); uzunsa alt satıra kayar. Yalnızca
    /// görünmesi gereken metinler (uyarı, durum) için; açıklamalar `info`'ya.
    pub fn note(&mut self, p: &mut P, text: &str) {
        self.flush(p);
        let v = lines(p, text, DESC, false, self.w - 4.0);
        draw_lines(p, &v, self.x + 2.0, self.y, self.w - 4.0, DESC, LINE + 1.0, false, TEXT, 0.5);
        self.y += v.len().max(1) as f32 * (LINE + 1.0) + 6.0;
    }
}
