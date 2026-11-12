//! Bu modül "ayar sayfaları uzun açıklamalarla kalabalık" sorununu çözüyor:
//! açıklama, satır başlığının hemen yanındaki küçük daire içinde "i"
//! düğmesinin arkasına taşınır. Üstüne gelip kısa bir süre bekleyince ya da
//! tıklayınca cam bir baloncukta görünür; fare düğmeden ve baloncuktan
//! ayrılınca, başka bir yere tıklayınca ya da Esc ile kapanır. Tıklanarak
//! açılan baloncuk fare ayrılsa da açık kalır.
//!
//! Baloncuk sayfanın kırpma alanının dışında, her şeyin üstünde çizilir
//! (`finish`); pencereye sığmazsa düğmenin üstüne ve/veya soluna kayar.
//! Sayfa kaydırılınca düğmeyle birlikte kayar, düğme görünmez olunca gizlenir.
//!
//! Sayfalar bu modülü doğrudan çağırmaz: `Flow`'un satırlarına verilen `info`
//! metni boş değilse düğmeyi `Flow` kendisi koyar (bkz. `ui.rs`).

use crate::{ui::P, Act};
use carlui_core::config::ThemeConfig;
use carlui_ui::{
    paint::{mix, Align, R, TEXT},
    tween,
};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// Düğmenin çapı.
pub const SIZE: f32 = 16.0;
/// Başlıkla düğme arasındaki boşluk.
pub const GAP: f32 = 6.0;
/// Üstüne gelince baloncuğun açılması için bekleme.
const DELAY: Duration = Duration::from_millis(450);
/// Baloncuğun en geniş hali, yazı boyutu, satır yüksekliği ve iç boşluğu.
const MAX_W: f32 = 340.0;
const FONT: f32 = 12.5;
const LINE: f32 = 18.0;
const PAD: f32 = 12.0;
/// Paragraflar arası ek boşluk.
const PARA: f32 = 6.0;
/// Pencere kenarına en az bu kadar uzak durur; düğmeyle baloncuk arası da bu.
const MARGIN: f32 = 8.0;

/// Çizilmiş baloncuğun düzeni (her karede yeniden bölünmesin).
#[derive(Clone)]
struct Layout {
    id: u64,
    max: u32,
    /// (satır, üstten uzaklık)
    lines: Vec<(String, f32)>,
    w: f32,
    h: f32,
}

#[derive(Default)]
struct State {
    /// Tıklanarak açılan baloncuk: fare ayrılsa da açık kalır.
    pinned: Option<u64>,
    /// Fare hangi düğmenin üstünde, ne zamandan beri.
    hover: Option<(u64, Instant)>,
    /// Tıklayıp kapatılan ya da Esc'lenen düğme: fare ondan ayrılana kadar
    /// üstüne gelmek baloncuğu yeniden açmaz.
    muted: Option<u64>,
    /// Bu karede çizilen düğmeler: (kimlik, dikdörtgen, metin).
    anchors: Vec<(u64, R, String)>,
    /// Son karede gösterilen baloncuk.
    shown: Option<u64>,
    /// Son çizilen baloncuğun düzeni ve yeri (kapanırken solarak gider).
    last: Option<(Layout, R)>,
}

thread_local! {
    static S: RefCell<State> = RefCell::default();
}

fn id_of(text: &str) -> u64 {
    tween::key(("info", text))
}

/// Karenin başında çağrılır: düğme listesi yeni kareyle yeniden dolar.
pub fn begin() {
    S.with(|s| s.borrow_mut().anchors.clear());
}

/// Başlığın yanına "i" düğmesini çizer; `x` sol kenar, `cy` başlık satırının
/// dikey ortası. `text` boşsa hiçbir şey yapmaz.
pub fn button(p: &mut P, x: f32, cy: f32, text: &str) {
    if text.is_empty() {
        return;
    }
    let id = id_of(text);
    let r = R::new(x, cy - SIZE / 2.0, SIZE, SIZE);
    let hit = r.inset(-3.0);
    let hot = p.hover(hit);
    let open = S.with(|s| s.borrow().shown == Some(id));
    let t = tween::to(tween::at("info", r.x, r.y), if hot || open { 1.0 } else { 0.0 }, 18.0);
    if t > 0.0 {
        p.round(r.inset(-2.0), SIZE / 2.0 + 2.0, 0xFFFFFF, 0.10 * t);
    }
    // Segoe Fluent Icons "Info": daire içinde i.
    p.glyph('\u{E946}', r, 14.0, mix(TEXT, p.accent, t), 0.5 + 0.45 * t);
    // Tıklama alanı `finish`'te, sayfanın bütün tıklama alanlarından sonra
    // eklenir: satırın tamamını kaplayan bir tıklama alanı onu örtmesin.
    S.with(|s| s.borrow_mut().anchors.push((id, r, text.to_string())));
}

/// Düğmeye tıklandı: kapalıysa sabitleyerek açar, açıksa kapatır.
pub fn toggle(id: u64) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if s.pinned == Some(id) {
            s.pinned = None;
            s.muted = Some(id);
            s.shown = None;
        } else {
            s.pinned = Some(id);
            s.muted = None;
        }
    });
}

/// Baloncuğun içine tıklandı: açık kalsın (fare ayrılınca kapanmasın).
pub fn keep() {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(id) = s.shown {
            s.pinned = Some(id);
        }
    });
}

/// Başka yere tıklama, Esc, sayfa değişimi: açık baloncuğu kapatır.
/// Bir baloncuk açıktıysa `true` (Esc başka işe yaramasın diye).
pub fn dismiss() -> bool {
    S.with(|s| {
        let mut s = s.borrow_mut();
        let was = s.shown.is_some() || s.pinned.is_some();
        // Fare hâlâ düğmenin üstündeyse baloncuk hemen yeniden açılmasın.
        s.muted = s.shown.or(s.hover.map(|h| h.0));
        s.pinned = None;
        s.shown = None;
        was
    })
}

/// Baloncuğun satırlarını ve boyutunu hesaplar.
fn layout(p: &P, id: u64, text: &str, max: f32) -> Layout {
    let inner = (max - 2.0 * PAD).max(40.0);
    let mut lines = Vec::new();
    let mut y = 0.0;
    let mut w: f32 = 0.0;
    for (i, para) in text.split('\n').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
        if i > 0 {
            y += PARA;
        }
        for l in crate::ui::lines(p, para, FONT, false, inner) {
            w = w.max(p.fonts.width(&l, FONT, false));
            lines.push((l, y));
            y += LINE;
        }
    }
    Layout { id, max: max as u32, lines, w: w.min(inner) + 2.0 * PAD, h: y + 2.0 * PAD - 2.0 }
}

/// Baloncuğun yeri: düğmenin altında, sola hizalı; sığmazsa üstüne / soluna.
fn place(anchor: R, w: f32, h: f32, win: R) -> R {
    let mut x = anchor.x - 10.0;
    if x + w > win.right() - MARGIN {
        // Sağa sığmıyor: sağ kenarı düğmeye hizalanır (sola açılır).
        x = anchor.right() + 10.0 - w;
    }
    x = x.min(win.right() - MARGIN - w).max(win.x + MARGIN);
    let mut y = anchor.bottom() + MARGIN;
    if y + h > win.bottom() - MARGIN {
        // Alta sığmıyor: düğmenin üstüne açılır.
        y = anchor.y - MARGIN - h;
    }
    y = y.min(win.bottom() - MARGIN - h).max(win.y + MARGIN);
    R::new(x, y, w, h)
}

/// Cam baloncuk: tema yoğunluğu ve köşe yarıçapıyla, iki temada da.
fn draw(p: &P, r: R, l: &Layout, t: f32, theme: &ThemeConfig) {
    let rad = 8.0 * theme.corner_radius.clamp(0.3, 1.5);
    let light = carlui_ui::tone::light();
    // Gölge: renkleri çevrilmesin (açık temada da koyu, ince).
    carlui_ui::tone::literal(|| p.round(R::new(r.x, r.y + 3.0, r.w, r.h), rad, 0x000000, if light { 0.10 } else { 0.28 } * t));
    // Koyu cam; açık temada boyayıcı bunu beyaz cama çevirir. Yoğunluk arttıkça
    // daha az saydam (altındaki satırlar yazıyı bulandırmasın diye taban yüksek).
    let a = 0.86 + 0.12 * theme.glass_density.clamp(0.0, 1.0);
    p.round(r, rad, 0x16161C, a * t);
    p.outline(r, rad, 0xFFFFFF, 0.14 * t);
    for (s, y) in &l.lines {
        p.text(s, R::new(r.x + PAD, r.y + PAD - 1.0 + y, r.w - 2.0 * PAD + 2.0, LINE), FONT, false, TEXT, 0.9 * t, Align::Left);
    }
}

/// Karenin sonunda, kırpma kaldırıldıktan sonra çağrılır: hangi baloncuğun
/// açık olduğuna karar verir ve onu en üste çizer. `clip` sayfanın görünen
/// alanı, `win` pencerenin tamamı. Üstüne gelme gecikmesi sürüyorsa bir
/// sonraki çizimin ne kadar sonra yapılması gerektiğini döner.
pub fn finish(p: &mut P, clip: R, win: R, theme: &ThemeConfig) -> Option<Duration> {
    S.with(|s| {
        let mut s = s.borrow_mut();
        let anchors = std::mem::take(&mut s.anchors);
        // Düğme kırpma alanında en az yarısı görünüyorsa geçerli.
        let visible = |r: &R| r.y + r.h / 2.0 >= clip.y && r.y + r.h / 2.0 <= clip.bottom();
        let mouse = p.mouse;
        let over_pop = s.shown.is_some() && s.last.as_ref().is_some_and(|(_, r)| r.inset(-MARGIN).contains(mouse));
        let hovered = anchors.iter().rev().find(|(_, r, _)| visible(r) && r.inset(-3.0).contains(mouse)).map(|a| a.0).or(if over_pop { s.shown } else { None });

        match hovered {
            Some(id) if s.hover.map(|h| h.0) != Some(id) => s.hover = Some((id, Instant::now())),
            Some(_) => {}
            None => s.hover = None,
        }
        if s.muted.is_some() && s.muted != hovered {
            s.muted = None;
        }
        let mut wake = None;
        let ready = match s.hover {
            Some((id, _)) if s.muted == Some(id) => None,
            Some((id, _)) if s.shown == Some(id) => Some(id),
            Some((id, at)) => {
                let e = at.elapsed();
                if e >= DELAY {
                    Some(id)
                } else {
                    wake = Some(DELAY - e);
                    None
                }
            }
            None => None,
        };
        let find = |id: u64| anchors.iter().rev().find(|a| a.0 == id && visible(&a.1));
        let active = s.pinned.and_then(find).or_else(|| ready.and_then(find)).cloned();

        if let Some((id, anchor, text)) = &active {
            let max = MAX_W.min(win.w - 2.0 * MARGIN);
            let l = match s.last.take() {
                Some((l, _)) if l.id == *id && l.max == max as u32 => l,
                _ => layout(p, *id, text, max),
            };
            let r = place(*anchor, l.w, l.h, win);
            s.last = Some((l, r));
        }
        s.shown = active.as_ref().map(|a| a.0);
        // Görünen düğmelerin tıklama alanları (kırpma alanıyla kesilmiş).
        for (id, r, _) in anchors.iter().filter(|a| visible(&a.1)) {
            let h = r.inset(-3.0);
            let (top, bottom) = (h.y.max(clip.y), h.bottom().min(clip.bottom()));
            if bottom > top {
                p.hit(R::new(h.x, top, h.w, bottom - top), Act::Info(Some(*id)));
            }
        }
        let t =tween::to(tween::key("info-pop"), if active.is_some() { 1.0 } else { 0.0 }, 20.0);
        if let Some((l, r)) = &s.last {
            if t > 0.01 {
                draw(p, *r, l, t, theme);
            }
            // Fare baloncuğa girip çıkınca yeniden çizilsin.
            p.hover(r.inset(-MARGIN));
            if active.is_some() {
                // Baloncuğun altındaki satırlar tıklanmasın.
                p.hit(*r, Act::Info(None));
            }
        }
        wake
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_stays_inside_the_window() {
        let win = R::new(0.0, 0.0, 800.0, 600.0);
        // Normalde düğmenin altında, sola hizalı.
        let r = place(R::new(100.0, 100.0, SIZE, SIZE), 300.0, 80.0, win);
        assert_eq!((r.x, r.y), (90.0, 100.0 + SIZE + MARGIN));
        // Sağ kenara yakın: sola açılır.
        let r = place(R::new(700.0, 100.0, SIZE, SIZE), 300.0, 80.0, win);
        assert!(r.right() <= win.right() - MARGIN && r.right() >= 700.0);
        // Alt kenara yakın: üstüne açılır.
        let a = R::new(100.0, 560.0, SIZE, SIZE);
        let r = place(a, 300.0, 80.0, win);
        assert!(r.bottom() <= a.y);
        // Pencereden büyükse bile sol üst köşe görünür kalır.
        let r = place(a, 900.0, 900.0, win);
        assert!(r.x >= MARGIN && r.y >= MARGIN);
    }

    #[test]
    fn click_pins_and_second_click_closes() {
        let id = id_of("deneme");
        toggle(id);
        S.with(|s| assert_eq!(s.borrow().pinned, Some(id)));
        toggle(id);
        S.with(|s| {
            let s = s.borrow();
            assert_eq!(s.pinned, None);
            assert_eq!(s.muted, Some(id));
        });
        toggle(id);
        assert!(dismiss());
        assert!(!dismiss());
    }
}
