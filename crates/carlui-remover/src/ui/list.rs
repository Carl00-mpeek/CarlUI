//! Üst çubuk, uygulama listesi ve seçili uygulamanın ayrıntıları.

use super::{human, Flow, Ui, A, PAD};
use crate::apps::App;
use carlui_ui::paint::{Align, Painter, ORANGE, R, TEXT};
use windows::{core::Interface, Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR};

const ROW: f32 = 52.0;
const PANEL_W: f32 = 370.0;
const TOP: f32 = 60.0;

/// Sığmayan yolu ortadan kısaltır; dosya adı görünür kalır.
pub fn shorten(p: &Painter<A>, s: &str, size: f32, max: f32) -> String {
    let w = p.fonts.width(s, size, false);
    if w <= max {
        return s.to_string();
    }
    let n = s.chars().count();
    let keep = ((n as f32 * max / w) as usize).saturating_sub(2);
    cut_middle(s, keep)
}

/// Karakter sayısı `keep`i aşan metni ortadan kısaltır: baştan %40, sondan kalanı.
/// Bayt değil karakter sayar; Türkçe yollarda ve kısa metinde panik olmaz.
fn cut_middle(s: &str, keep: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= keep {
        return s.to_string();
    }
    let head = keep * 2 / 5;
    let tail = keep - head;
    let mut out: String = c[..head].iter().collect();
    out.push('…');
    out.extend(&c[c.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::cut_middle;

    #[test]
    fn short_and_exact_unchanged() {
        assert_eq!(cut_middle("abc", 10), "abc");
        assert_eq!(cut_middle("abcdefghij", 10), "abcdefghij");
        assert_eq!(cut_middle("", 0), "");
    }

    #[test]
    fn long_ascii() {
        assert_eq!(cut_middle("0123456789ABCDEFGHIJ", 10), "0123…EFGHIJ");
    }

    #[test]
    fn turkish_chars_no_panic() {
        let s = "ğüşıöç".repeat(10);
        for keep in 0..70 {
            let r = cut_middle(&s, keep);
            if keep < 60 {
                assert_eq!(r.chars().count(), keep + 1);
            }
        }
        assert_eq!(cut_middle("ğüşıöçğüşıöç", 5), "ğü…ıöç");
    }
}

fn date(d: &str) -> String {
    match (d.get(..4), d.get(4..6), d.get(6..8)) {
        (Some(y), Some(m), Some(g)) => format!("{g}.{m}.{y}"),
        _ => String::new(),
    }
}

fn sub(a: &App) -> String {
    let parts: Vec<&str> = [a.publisher.as_str(), a.version.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    parts.join(" · ")
}

impl Ui {
    pub(super) fn draw_icon(&mut self, p: &mut Painter<A>, path: &str, r: R) {
        // Çizim hedefi yenilenince eski bitmap'ler geçersizdir.
        let id = p.rt.as_raw() as usize;
        if id != self.rt {
            self.rt = id;
            self.icons.clear();
        }
        if !path.is_empty() {
            if let Some(b) = self.icons.get(p.rt, path) {
                unsafe { p.rt.DrawBitmap(&b, Some(&r.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
                return;
            }
        }
        p.round(r, 6.0, p.accent, 0.2);
        p.glyph('\u{E71D}', r, r.h * 0.5, TEXT, 0.85);
    }

    pub(super) fn paint_browse(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        self.paint_top(p, w);
        let body = R::new(PAD, TOP, w - PAD * 2.0, h - TOP - 36.0);
        let list = R::new(body.x, body.y, body.w - PANEL_W - 12.0, body.h);
        self.paint_list(p, list);
        self.paint_detail(p, R::new(list.right() + 12.0, body.y, PANEL_W, body.h));
        let total: u64 = self.view.iter().map(|&i| self.apps[i].size).sum();
        let line = if self.loading { t!("Uygulamalar okunuyor…", "Reading apps…").to_string() } else { t!(format!("{} uygulama · {}", self.view.len(), human(total)), format!("{} apps · {}", self.view.len(), human(total))) };
        p.text(&line, R::new(PAD, h - 32.0, w - PAD * 2.0, 26.0), 12.0, false, TEXT, 0.6, Align::Left);
    }

    fn paint_top(&mut self, p: &mut Painter<A>, w: f32) {
        let (y, bh) = (12.0, 34.0);
        let right = w - PAD;
        p.button(R::new(right - 104.0, y, 104.0, bh), Some('\u{E72C}'), t!("Yenile", "Refresh"), A::Refresh, !self.loading, false);
        let sort = R::new(right - 104.0 - 12.0 - 204.0, y, 204.0, bh);
        p.segments(sort, &[(t!("Ad", "Name"), A::Sort(0)), (t!("Boyut", "Size"), A::Sort(1)), (t!("Tarih", "Date"), A::Sort(2))], self.sort);
        let tabs = R::new(sort.x - 12.0 - 250.0, y, 250.0, bh);
        p.segments(tabs, &[(t!("Hepsi", "All"), A::Tab(0)), (t!("Programlar", "Programs"), A::Tab(1)), (t!("Mağaza", "Store"), A::Tab(2))], self.tab);
        self.search.paint(p, R::new(PAD, y, tabs.x - 12.0 - PAD, bh), true, t!("Uygulama ara…", "Search apps…"), A::Search);
    }

    fn paint_list(&mut self, p: &mut Painter<A>, area: R) {
        p.card(area);
        self.rows = ((area.h - 8.0) / ROW).floor().max(1.0) as usize;
        self.scroll = self.scroll.min(self.view.len().saturating_sub(self.rows));
        if self.view.is_empty() {
            let msg = if self.loading { t!("Yükleniyor…", "Loading…") } else { t!("Sonuç yok", "No results") };
            p.text(msg, area, 13.0, false, TEXT, 0.6, Align::Center);
            return;
        }
        let end = (self.scroll + self.rows).min(self.view.len());
        for k in self.scroll..end {
            let i = self.view[k];
            let r = R::new(area.x + 4.0, area.y + 4.0 + (k - self.scroll) as f32 * ROW, area.w - 16.0, ROW - 2.0);
            if self.sel == Some(i) {
                p.round(r, 6.0, p.accent, 0.2);
            } else if p.hover(r) {
                p.round(r, 6.0, 0xFFFFFF, 0.07);
            }
            let icon = self.apps[i].icon.clone();
            self.draw_icon(p, &icon, R::new(r.x + 8.0, r.y + 9.0, 32.0, 32.0));
            let a = &self.apps[i];
            let tw = r.w - 56.0 - 84.0;
            p.text(&a.name, R::new(r.x + 50.0, r.y + 6.0, tw, 20.0), 13.0, true, TEXT, 0.95, Align::Left);
            p.text(&sub(a), R::new(r.x + 50.0, r.y + 26.0, tw, 18.0), 11.5, false, TEXT, 0.6, Align::Left);
            if a.size > 0 {
                p.text(&human(a.size), R::new(r.right() - 86.0, r.y, 78.0, r.h), 12.0, false, TEXT, 0.7, Align::Right);
            }
            p.hit(r, A::Row(i));
        }
        if self.view.len() > self.rows {
            let track = area.h - 12.0;
            let th = (track * self.rows as f32 / self.view.len() as f32).max(24.0);
            let max = self.view.len() - self.rows;
            let ty = area.y + 6.0 + (track - th) * self.scroll as f32 / max as f32;
            p.round(R::new(area.right() - 8.0, ty, 4.0, th), 2.0, TEXT, 0.25);
        }
    }

    fn paint_detail(&mut self, p: &mut Painter<A>, area: R) {
        p.card(area);
        let Some(a) = self.selected().cloned() else { return };
        let x = area.x + 18.0;
        let cw = area.w - 36.0;
        self.draw_icon(p, &a.icon, R::new(x, area.y + 18.0, 48.0, 48.0));
        let lines = carlui_ui::paint::wrap(&a.name, cw - 60.0, |s| p.fonts.width(s, 16.0, true));
        for (i, l) in lines.iter().take(2).enumerate() {
            p.text(l, R::new(x + 60.0, area.y + 16.0 + i as f32 * 22.0, cw - 60.0, 22.0), 16.0, true, TEXT, 0.95, Align::Left);
        }
        let mut y = area.y + 84.0;
        let kind = if a.is_store() { t!("Mağaza uygulaması", "Store app") } else { t!("Program", "Program") };
        let mut info = vec![(t!("Tür", "Type"), kind.to_string())];
        for (label, value) in [(t!("Yayıncı", "Publisher"), a.publisher.clone()), (t!("Sürüm", "Version"), a.version.clone()), (t!("Boyut", "Size"), if a.size > 0 { human(a.size) } else { String::new() }), (t!("Kurulum", "Installed"), date(&a.date))] {
            if !value.is_empty() {
                info.push((label, value));
            }
        }
        for (label, value) in &info {
            p.text(label, R::new(x, y, 84.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
            p.text(value, R::new(x + 88.0, y, cw - 88.0, 22.0), 12.5, false, TEXT, 0.9, Align::Left);
            y += 24.0;
        }
        if !a.location.is_empty() {
            p.text(t!("Konum", "Location"), R::new(x, y, 84.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
            let s = shorten(p, &a.location, 12.0, cw - 88.0);
            p.text(&s, R::new(x + 88.0, y, cw - 88.0, 22.0), 12.0, false, TEXT, 0.9, Align::Left);
        }

        let bottom = area.bottom() - 18.0;
        if matches!(self.flow, Flow::Confirm) {
            let msg = if a.is_store() { t!(format!("{} kaldırılacak. Uygulamanın verileri de silinir.", a.name), format!("{} will be removed, including its data.", a.name)) } else { t!(format!("{} kaldırılacak. Önce kendi kaldırıcısı çalışır, sonra geride kalanlar taranır.", a.name), format!("{} will be removed. Its own uninstaller runs first, then leftovers are scanned.", a.name)) };
            let lines = carlui_ui::paint::wrap(&msg, cw, |s| p.fonts.width(s, 13.0, false));
            let top = bottom - 40.0 - 12.0 - lines.len() as f32 * 20.0;
            for (i, l) in lines.iter().enumerate() {
                p.text(l, R::new(x, top + i as f32 * 20.0, cw, 20.0), 13.0, false, TEXT, 0.9, Align::Left);
            }
            let half = (cw - 10.0) / 2.0;
            p.button(R::new(x, bottom - 38.0, half, 38.0), Some('\u{E74D}'), t!("Evet, kaldır", "Yes, remove"), A::Yes, true, true);
            p.button(R::new(x + half + 10.0, bottom - 38.0, half, 38.0), None, t!("Vazgeç", "Cancel"), A::Back, true, false);
            return;
        }
        let mut by = bottom - 38.0;
        if !a.location.is_empty() && !a.is_store() {
            p.button(R::new(x, by, cw, 38.0), Some('\u{E8DA}'), t!("Konumu aç", "Open location"), A::OpenPlace, true, false);
            by -= 46.0;
        }
        if !a.is_store() {
            let label = if a.stale { t!("Artıkları tara", "Scan for leftovers") } else { t!("Yalnızca artıkları tara", "Scan leftovers only") };
            p.button(R::new(x, by, cw, 38.0), Some('\u{E721}'), label, A::ScanOnly, true, false);
            by -= 46.0;
        }
        if a.stale {
            let lines = carlui_ui::paint::wrap(t!("Kaldırıcı dosyası bulunamadı. Uygulama zaten silinmiş olabilir; kalanları tarayabilirsin.", "The uninstaller file is missing. The app may already be gone; you can scan what remains."), cw, |s| p.fonts.width(s, 12.5, false));
            let top = by + 30.0 - lines.len() as f32 * 19.0;
            for (i, l) in lines.iter().enumerate() {
                p.text(l, R::new(x, top + i as f32 * 19.0, cw, 19.0), 12.5, false, ORANGE, 0.95, Align::Left);
            }
        } else {
            p.button(R::new(x, by, cw, 38.0), Some('\u{E74D}'), t!("Kaldır", "Uninstall"), A::Uninstall, true, true);
        }
    }
}
