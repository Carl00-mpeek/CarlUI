//! Bu modül "sunuda biçim ve slayt komutları" sorununu çözüyor: kalın / italik /
//! altı çizili, punto, renk, hizalama, madde düzeyi; slayt ekle / çoğalt / sil /
//! taşı, metin kutusu ekle, şekil sil; geri al / yinele; araç çubuğu ve menüler.
//! Metin düzenlenmiyorken biçim komutu seçili şeklin bütün metnine uygulanır
//! (PowerPoint'te de böyledir).

use super::{
    save,
    view::{Last, SlideView, Snap},
};
use crate::{
    app::{self, Act, Cmd, Keys},
    rich::{self, Cursor, Fmt, PAlign, Para, Pos},
    ui,
};
use carlui_core::t;
use carlui_ui::{
    glassmenu::{self, Icon, Row},
    paint::{Painter, R},
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const SIZES: [f32; 20] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 54.0, 60.0, 72.0, 96.0];
const ZOOMS: [f32; 6] = [0.4, 0.5, 0.67, 0.75, 0.9, 1.0];
const UNDO_MAX: usize = 60;

fn cursor_pt() -> (i32, i32) {
    let mut p = Default::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

fn item(id: usize, text: &str, g: char) -> Row {
    Row::Item { id, text: text.into(), icon: Icon::Glyph(g, None) }
}

impl SlideView {
    pub(super) fn push(&mut self, s: Snap) {
        self.undo.push(s);
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
    }

    fn history(&mut self, redo: bool) {
        let from = if redo { self.redo.pop() } else { self.undo.pop() };
        let Some(s) = from else { return };
        let now = self.snapshot();
        if redo {
            self.undo.push(now);
        } else {
            self.redo.push(now);
        }
        let keep = std::mem::take(&mut self.redo);
        self.d.slides = s.slides;
        self.cur = s.cur;
        self.restructured();
        self.redo = keep;
        let n = self.d.slides.get(self.cur).map(|x| x.shapes.len()).unwrap_or(0);
        self.sel = s.sel.filter(|&i| i < n);
        self.edit = s.edit && self.sel.is_some();
        self.ed.cur = s.ed;
        self.ed.goal = None;
        self.last = Last::Other;
        if let Some(si) = self.sel {
            // İmleç geri yüklenen metnin dışında kalmasın.
            let paras = &self.d.slides[self.cur].shapes[si].paras;
            let fix = |p: Pos| {
                let pi = p.p.min(paras.len().saturating_sub(1));
                Pos { p: pi, c: p.c.min(paras.get(pi).map(Para::len).unwrap_or(0)) }
            };
            let (a, b) = (fix(self.ed.cur.range().0), fix(self.ed.cur.at));
            self.ed.cur = Cursor::default();
            self.ed.cur.set(a, false);
            self.ed.cur.set(b, true);
        }
    }

    fn paras(&self) -> Option<&Vec<Para>> {
        let s = self.d.slides.get(self.cur)?.shapes.get(self.sel?)?;
        s.has_text().then_some(&s.paras)
    }

    /// Biçimin uygulanacağı aralık: seçim, imleçteki sözcük ya da (düzenleme yokken) bütün metin.
    fn target(&self) -> Option<Cursor> {
        let paras = self.paras()?;
        let mut c = Cursor::default();
        if !self.edit {
            let last = paras.len().checked_sub(1)?;
            c.set(Pos { p: 0, c: 0 }, false);
            c.set(Pos { p: last, c: paras[last].len() }, true);
            return Some(c);
        }
        let cur = &self.ed.cur;
        if cur.has_sel() {
            return Some(cur.clone());
        }
        let at = cur.at;
        let text = paras.get(at.p)?.text();
        let a = rich::word_left(&text, at.c);
        let b = rich::word_right(&text, a);
        if a < at.c && at.c <= b || a == at.c && b > a {
            c.set(Pos { p: at.p, c: a }, false);
            c.set(Pos { p: at.p, c: b.min(text.trim_end().chars().count().max(a)) }, true);
        } else {
            c.set(at, false);
        }
        Some(c)
    }

    fn base(&self, p: &Para) -> Fmt {
        let s = &self.d.slides[self.cur].shapes[self.sel.unwrap_or(0)];
        s.lv.fmt[(p.level as usize).min(8)]
    }

    /// Hedefin başındaki biçim (düzey biçimi üstüne bindirilmiş).
    fn fmt_here(&self) -> Fmt {
        let Some(paras) = self.paras() else { return Fmt::default() };
        let at = if self.edit { self.ed.cur.range().0 } else { Pos::default() };
        let Some(p) = paras.get(at.p) else { return Fmt::default() };
        p.fmt_at(at.c).over(&self.base(p))
    }

    fn format(&mut self, f: impl Fn(&mut Fmt)) {
        let (Some(tg), Some(si)) = (self.target(), self.sel) else { return };
        if !tg.has_sel() {
            return;
        }
        self.push(self.snapshot());
        rich::apply_fmt(&mut self.d.slides[self.cur].shapes[si].paras, &tg, f);
        self.last = Last::Other;
        self.text_changed(si);
    }

    fn toggle(&mut self, get: fn(&Fmt) -> Option<bool>, set: fn(&mut Fmt, bool)) {
        let (Some(tg), Some(paras)) = (self.target(), self.paras()) else { return };
        let (a, b) = tg.range();
        let on = (a.p..=b.p).all(|i| {
            let p = &paras[i];
            let s = if i == a.p { a.c } else { 0 };
            let e = if i == b.p { b.c } else { p.len() };
            p.all(s, e, &self.base(p), |f| get(f) == Some(true))
        });
        self.format(move |f| set(f, !on));
    }

    /// Seçili (düzenleme yokken bütün) paragraflara uygular.
    fn per_para(&mut self, f: impl Fn(&mut Para)) {
        let Some(si) = self.sel else { return };
        let Some(n) = self.paras().map(Vec::len) else { return };
        let r = if self.edit { rich::sel_paras(&self.ed.cur) } else { 0..=n.saturating_sub(1) };
        self.push(self.snapshot());
        for p in &mut self.d.slides[self.cur].shapes[si].paras[*r.start()..=(*r.end()).min(n - 1)] {
            f(p);
            p.dirty = true;
        }
        self.last = Last::Other;
        self.text_changed(si);
    }

    /// Paragraf başında Tab / Shift+Tab: madde düzeyini değiştirir.
    pub(super) fn indent_at_start(&mut self, out: bool) -> bool {
        let cur = &self.ed.cur;
        if !cur.has_sel() && cur.at.c != 0 || self.sel.is_none() {
            return false;
        }
        self.per_para(move |p| p.level = if out { p.level.saturating_sub(1) } else { (p.level + 1).min(8) });
        true
    }

    // ---- Slayt işlemleri ----

    fn new_slide(&mut self) {
        let s = self.snapshot();
        match save::new_slide(&mut self.d, self.cur) {
            Ok(i) => {
                self.push(s);
                self.restructured();
                self.go(i);
            }
            Err(e) => app::error(&e),
        }
    }

    fn duplicate(&mut self) {
        let s = self.snapshot();
        match save::duplicate(&mut self.d, self.cur) {
            Ok(i) => {
                self.push(s);
                self.restructured();
                self.go(i);
            }
            Err(e) => app::error(&e),
        }
    }

    fn delete_slide(&mut self) {
        if self.d.slides.len() <= 1 {
            app::toast(t!("Son slayt silinemez", "Can't delete the last slide"));
            return;
        }
        self.push(self.snapshot());
        self.d.slides.remove(self.cur);
        self.sel = None;
        self.edit = false;
        self.restructured();
    }

    fn move_slide(&mut self, up: bool) {
        let i = self.cur;
        let j = if up { i.checked_sub(1) } else { (i + 1 < self.d.slides.len()).then_some(i + 1) };
        let Some(j) = j else { return };
        self.push(self.snapshot());
        self.d.slides.swap(i, j);
        self.restructured();
        self.cur = j;
    }

    fn toggle_hidden(&mut self) {
        self.push(self.snapshot());
        let s = &mut self.d.slides[self.cur];
        s.hidden = !s.hidden;
        // `show="0"` kök öğede tutulur; kaydederken XML'e işlenir.
        self.touched();
    }

    fn text_box(&mut self) {
        let s = self.snapshot();
        let (w, h) = (self.d.w * 0.5, 40.0);
        let (x, y) = ((self.d.w - w) / 2.0, self.d.h * 0.42);
        match save::insert_textbox(&mut self.d, self.cur, x, y, w, h) {
            Ok(si) => {
                self.push(s);
                self.touched();
                self.start_edit(si, false);
            }
            Err(e) => app::error(&e),
        }
    }

    fn delete_shape(&mut self) {
        let Some(si) = self.sel else { return };
        let s = self.snapshot();
        match save::delete_shape(&mut self.d, self.cur, si) {
            Ok(()) => {
                self.push(s);
                self.sel = None;
                self.edit = false;
                self.touched();
            }
            Err(e) => app::error(&e),
        }
    }

    pub(super) fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Bold => self.toggle(|f| f.b, |f, v| f.b = Some(v)),
            Cmd::Italic => self.toggle(|f| f.i, |f, v| f.i = Some(v)),
            Cmd::Underline => self.toggle(|f| f.u, |f, v| f.u = Some(v)),
            Cmd::Strike => self.toggle(|f| f.strike, |f, v| f.strike = Some(v)),
            Cmd::Size(d) => {
                let cur = self.fmt_here().size.unwrap_or(18.0);
                let next = if d > 0 { SIZES.iter().find(|s| **s > cur + 0.01).copied().unwrap_or(cur) } else { SIZES.iter().rev().find(|s| **s < cur - 0.01).copied().unwrap_or(cur) };
                self.format(move |f| f.size = Some(next));
            }
            Cmd::Color(c) => self.format(move |f| f.color = if c == u32::MAX { None } else { Some(c) }),
            Cmd::ColorMenu => self.color_menu(),
            Cmd::Align(a) => self.per_para(move |p| p.align = Some(a)),
            Cmd::Undo => self.history(false),
            Cmd::Redo => self.history(true),
            Cmd::Zoom(d) => {
                let i = ZOOMS.iter().position(|z| (*z - self.zoom).abs() < 0.01).unwrap_or(ZOOMS.len() - 1) as i32;
                self.zoom = ZOOMS[(i + d as i32).clamp(0, ZOOMS.len() as i32 - 1) as usize];
            }
            Cmd::Pick(i) => self.go(i),
            Cmd::Add => self.new_slide(),
            Cmd::Duplicate => self.duplicate(),
            Cmd::Remove if self.sel.is_some() => self.delete_shape(),
            Cmd::Remove => self.delete_slide(),
            Cmd::TextBox => self.text_box(),
            Cmd::Present => self.present_from(self.cur),
            Cmd::More => self.slide_menu(),
            _ => {}
        }
    }

    /// Ctrl kısayolları.
    pub(super) fn shortcut(&mut self, vk: u16, k: &Keys) -> bool {
        if !k.ctrl {
            return false;
        }
        let c = match vk {
            0x42 => Cmd::Bold,
            0x49 => Cmd::Italic,
            0x55 => Cmd::Underline,
            0x5A if k.shift => Cmd::Redo,
            0x5A => Cmd::Undo,
            0x59 => Cmd::Redo,
            0x45 => Cmd::Align(PAlign::Center),
            0x4C => Cmd::Align(PAlign::Left),
            0x52 => Cmd::Align(PAlign::Right),
            0x4A => Cmd::Align(PAlign::Justify),
            0xBB | 0x6B => Cmd::Zoom(1),
            0xBD | 0x6D => Cmd::Zoom(-1),
            0xDD => Cmd::Size(1),
            0xDB => Cmd::Size(-1),
            0x4D => Cmd::Add,
            0x44 => Cmd::Duplicate,
            _ => return false,
        };
        self.command(c);
        true
    }

    fn color_menu(&mut self) {
        let colors = [
            (0x000000, t!("Siyah", "Black")),
            (0xFFFFFF, t!("Beyaz", "White")),
            (0xC00000, t!("Koyu kırmızı", "Dark red")),
            (0xFF0000, t!("Kırmızı", "Red")),
            (0xED7D31, t!("Turuncu", "Orange")),
            (0xFFC000, t!("Altın", "Gold")),
            (0x00B050, t!("Yeşil", "Green")),
            (0x0070C0, t!("Mavi", "Blue")),
            (0x7030A0, t!("Mor", "Purple")),
            (0x7F7F7F, t!("Gri", "Gray")),
        ];
        let mut rows = vec![Row::Header(t!("Yazı rengi", "Font color").into())];
        rows.push(item(1, t!("Otomatik", "Automatic"), '\u{E8D3}'));
        for (i, (c, n)) in colors.iter().enumerate() {
            rows.push(Row::Item { id: 10 + i, text: n.to_string(), icon: Icon::Glyph('\u{E8D3}', Some(*c)) });
        }
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => self.command(Cmd::Color(u32::MAX)),
            Some(id @ 10..=19) => self.command(Cmd::Color(colors[id - 10].0)),
            _ => {}
        }
    }

    pub(super) fn slide_menu(&mut self) {
        let hidden = self.d.slides.get(self.cur).is_some_and(|s| s.hidden);
        let rows = vec![
            Row::Header(format!("{} {}", t!("Slayt", "Slide"), self.cur + 1)),
            item(1, t!("Yeni slayt  (Ctrl+M)", "New slide  (Ctrl+M)"), '\u{E710}'),
            item(2, t!("Slaytı çoğalt  (Ctrl+D)", "Duplicate slide  (Ctrl+D)"), '\u{E8C8}'),
            item(3, t!("Slaytı sil", "Delete slide"), '\u{E74D}'),
            Row::Sep,
            item(4, t!("Yukarı taşı", "Move up"), '\u{E74A}'),
            item(5, t!("Aşağı taşı", "Move down"), '\u{E74B}'),
            item(6, if hidden { t!("Slaytı göster", "Unhide slide") } else { t!("Slaytı gizle", "Hide slide") }, '\u{ED1A}'),
            Row::Sep,
            item(7, t!("Bu slayttan sun  (Shift+F5)", "Present from here  (Shift+F5)"), '\u{E768}'),
        ];
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => self.new_slide(),
            Some(2) => self.duplicate(),
            Some(3) => self.delete_slide(),
            Some(4) => self.move_slide(true),
            Some(5) => self.move_slide(false),
            Some(6) => self.toggle_hidden(),
            Some(7) => self.present_from(self.cur),
            _ => {}
        }
    }

    pub(super) fn context_menu(&mut self) {
        use crate::app::View;
        let mut rows = Vec::new();
        if self.edit {
            if self.ed.cur.has_sel() {
                rows.push(item(1, t!("Kes  (Ctrl+X)", "Cut  (Ctrl+X)"), '\u{E8C6}'));
                rows.push(item(2, t!("Kopyala  (Ctrl+C)", "Copy  (Ctrl+C)"), '\u{E8C8}'));
            }
            rows.push(item(3, t!("Yapıştır  (Ctrl+V)", "Paste  (Ctrl+V)"), '\u{E77F}'));
            rows.push(item(4, t!("Tümünü seç  (Ctrl+A)", "Select all  (Ctrl+A)"), '\u{E8B3}'));
            rows.push(Row::Sep);
        }
        if self.sel.is_some() {
            rows.push(item(5, t!("Şekli sil", "Delete shape"), '\u{E74D}'));
        }
        rows.push(item(6, t!("Metin kutusu ekle", "Insert text box"), '\u{E8D2}'));
        rows.push(Row::Sep);
        rows.push(item(7, t!("Slayt…", "Slide…"), '\u{E8A1}'));
        let ctrl = Keys { ctrl: true, shift: false };
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => {
                self.key(0x58, &ctrl);
            }
            Some(2) => {
                self.key(0x43, &ctrl);
            }
            Some(3) => {
                self.key(0x56, &ctrl);
            }
            Some(4) => {
                self.key(0x41, &ctrl);
            }
            Some(5) => self.delete_shape(),
            Some(6) => self.text_box(),
            Some(7) => self.slide_menu(),
            _ => {}
        }
    }

    pub(super) fn toolbar_ui(&mut self, p: &mut Painter<Act>, r: R) {
        let y = r.y;
        let mut x = r.x;
        x += ui::tool(p, x, y, '\u{E710}', Act::View(Cmd::Add), false);
        x += ui::tool(p, x, y, '\u{E8C8}', Act::View(Cmd::Duplicate), false);
        x += ui::tool(p, x, y, '\u{E74D}', Act::View(Cmd::Remove), false);
        x += ui::tool(p, x, y, '\u{E8D2}', Act::View(Cmd::TextBox), false);
        x += ui::sep(p, x, y);
        let has = self.paras().is_some();
        let f = if has { self.fmt_here() } else { Fmt::default() };
        x += ui::tool(p, x, y, '\u{E8E7}', Act::View(Cmd::Size(-1)), false);
        let size = f.size.map(|s| format!("{s}")).unwrap_or_else(|| "–".into());
        x += ui::tool_text(p, x, y, &size, Act::View(Cmd::Size(1)), false, false);
        x += ui::tool(p, x, y, '\u{E8E8}', Act::View(Cmd::Size(1)), false);
        x += ui::sep(p, x, y);
        x += ui::tool(p, x, y, '\u{E8DD}', Act::View(Cmd::Bold), f.b == Some(true));
        x += ui::tool(p, x, y, '\u{E8DB}', Act::View(Cmd::Italic), f.i == Some(true));
        x += ui::tool(p, x, y, '\u{E8DC}', Act::View(Cmd::Underline), f.u == Some(true));
        x += ui::tool(p, x, y, '\u{E8D3}', Act::View(Cmd::ColorMenu), false);
        x += ui::sep(p, x, y);
        let al = self.paras().and_then(|ps| {
            let at = if self.edit { self.ed.cur.at.p } else { 0 };
            let para = ps.get(at)?;
            let s = &self.d.slides[self.cur].shapes[self.sel?];
            Some(para.align.or(s.lv.algn[(para.level as usize).min(8)]).unwrap_or(PAlign::Left))
        });
        x += ui::tool(p, x, y, '\u{E8E4}', Act::View(Cmd::Align(PAlign::Left)), al == Some(PAlign::Left));
        x += ui::tool(p, x, y, '\u{E8E3}', Act::View(Cmd::Align(PAlign::Center)), al == Some(PAlign::Center));
        x += ui::tool(p, x, y, '\u{E8E2}', Act::View(Cmd::Align(PAlign::Right)), al == Some(PAlign::Right));
        x += ui::sep(p, x, y);
        let _ = ui::tool_text(p, x, y, t!("▶ Sun", "▶ Present"), Act::View(Cmd::Present), false, true);
    }
}
