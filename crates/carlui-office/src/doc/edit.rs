//! Bu modül "Word belgesinde biçim komutları" sorununu çözüyor: kalın / italik /
//! altı çizili, punto büyüt / küçült, renk, hizalama, biçem (Normal, Başlık
//! 1-3), geri al / yinele, yakınlaştırma, araç çubuğu ve sağ tık menüsü.

use super::view::{DocView, Last, Snap};
use crate::{
    app::{self, Act, Cmd, Keys},
    rich::{self, Cursor, Fmt, PAlign, Pos},
    ui,
};
use carlui_core::t;
use carlui_ui::{
    glassmenu::{self, Icon, Row},
    paint::{Painter, R},
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 60.0, 72.0];
const ZOOMS: [f32; 9] = [0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];
const UNDO_MAX: usize = 100;

fn cursor_pt() -> (i32, i32) {
    let mut p = Default::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl DocView {
    pub(super) fn push(&mut self, s: Snap) {
        self.undo.push(s);
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
    }

    fn snapshot(&self) -> Snap {
        Snap { paras: self.d.paras.clone(), cur: self.ed.cur.clone() }
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
        self.d.paras = s.paras;
        self.ed.cur = s.cur;
        self.ed.goal = None;
        self.last = Last::Other;
        self.d.renumber();
        self.invalidate_all();
        self.dirty = true;
        self.reveal = true;
        self.words.set(None);
    }

    /// Seçim yoksa imlecin altındaki sözcük (Word da böyle yapar).
    fn target(&self) -> Cursor {
        let cur = &self.ed.cur;
        if cur.has_sel() {
            return cur.clone();
        }
        let at = cur.at;
        let text = self.d.paras[at.p].text();
        let a = rich::word_left(&text, at.c);
        let b = rich::word_right(&text, a);
        let mut c = Cursor::default();
        if a < at.c && at.c <= b || a == at.c && b > a {
            c.set(Pos { p: at.p, c: a }, false);
            c.set(Pos { p: at.p, c: b.min(text.trim_end().chars().count().max(a)) }, true);
        } else {
            c.set(at, false);
        }
        c
    }

    /// Hedefin biçimi (ilk paragraf, biçem üstüne bindirilmiş).
    fn fmt_here(&self) -> Fmt {
        let at = self.ed.cur.range().0;
        let p = &self.d.paras[at.p];
        p.fmt_at(at.c).over(&self.d.style_fmt(p.style.as_deref()))
    }

    /// Biçimi hedefe uygular.
    fn format(&mut self, f: impl Fn(&mut Fmt)) {
        let tg = self.target();
        if !tg.has_sel() {
            return;
        }
        self.push(self.snapshot());
        let r = rich::sel_paras(&tg);
        let n = self.d.paras.len();
        rich::apply_fmt(&mut self.d.paras, &tg, f);
        self.last = Last::Other;
        self.changed(*r.start(), *r.end(), n);
    }

    fn toggle(&mut self, get: fn(&Fmt) -> Option<bool>, set: fn(&mut Fmt, bool)) {
        let tg = self.target();
        let (a, b) = tg.range();
        let p = &self.d.paras[a.p];
        let base = self.d.style_fmt(p.style.as_deref());
        let end = if b.p == a.p { b.c } else { p.len() };
        let on = p.all(a.c, end, &base, |f| get(f) == Some(true));
        self.format(move |f| set(f, !on));
    }

    /// Seçili paragraflara uygular (hizalama, biçem).
    fn per_para(&mut self, f: impl Fn(&mut DocView, usize)) {
        self.push(self.snapshot());
        let r = rich::sel_paras(&self.ed.cur);
        let n = self.d.paras.len();
        for i in r.clone() {
            if self.d.paras[i].block.is_none() {
                f(self, i);
                self.d.paras[i].dirty = true;
            }
        }
        self.last = Last::Other;
        self.changed(*r.start(), *r.end(), n);
    }

    pub(super) fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Bold => self.toggle(|f| f.b, |f, v| f.b = Some(v)),
            Cmd::Italic => self.toggle(|f| f.i, |f, v| f.i = Some(v)),
            Cmd::Underline => self.toggle(|f| f.u, |f, v| f.u = Some(v)),
            Cmd::Strike => self.toggle(|f| f.strike, |f, v| f.strike = Some(v)),
            Cmd::Size(d) => {
                let cur = self.fmt_here().size.unwrap_or(11.0);
                let next = if d > 0 { SIZES.iter().find(|s| **s > cur + 0.01).copied().unwrap_or(cur) } else { SIZES.iter().rev().find(|s| **s < cur - 0.01).copied().unwrap_or(cur) };
                self.format(move |f| f.size = Some(next));
            }
            Cmd::Color(c) => self.format(move |f| f.color = if c == u32::MAX { None } else { Some(c) }),
            Cmd::ColorMenu => self.color_menu(),
            Cmd::Align(a) => self.per_para(move |v, i| v.d.paras[i].align = Some(a)),
            Cmd::Heading(l) => {
                let id = if l == 0 { self.d.normal.clone() } else { self.d.heading_style(l) };
                self.per_para(move |v, i| v.d.paras[i].style = id.clone());
            }
            Cmd::Undo => self.history(false),
            Cmd::Redo => self.history(true),
            Cmd::Zoom(d) => {
                let i = ZOOMS.iter().position(|z| (*z - self.zoom).abs() < 0.01).unwrap_or(4) as i32;
                let j = (i + d as i32).clamp(0, ZOOMS.len() as i32 - 1) as usize;
                if ZOOMS[j] != self.zoom {
                    // Yakınlaştırınca görünen yer sabit kalsın.
                    let k = ZOOMS[j] / self.zoom;
                    self.scroll *= k;
                    self.zoom = ZOOMS[j];
                    self.invalidate_all();
                }
            }
            Cmd::More => self.style_menu(),
            _ => {}
        }
    }

    /// Ctrl kısayolları (B, I, U, Z, Y, E, L, R, J, +/-).
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
            0x31..=0x33 if k.shift => Cmd::Heading((vk - 0x30) as u8),
            0x30 if k.shift => Cmd::Heading(0),
            _ => return false,
        };
        self.command(c);
        true
    }

    fn style_label(&self) -> String {
        let p = &self.d.paras[self.ed.cur.at.p.min(self.d.paras.len() - 1)];
        match self.d.heading_of(p.style.as_deref()) {
            0 => match p.style.as_deref().and_then(|s| self.d.styles.get(s)) {
                Some(s) if p.style != self.d.normal && !s.name.is_empty() => s.name.clone(),
                _ => t!("Normal", "Normal").to_string(),
            },
            n => format!("{} {n}", t!("Başlık", "Heading")),
        }
    }

    fn style_menu(&mut self) {
        let mut rows = vec![Row::Header(t!("Biçem", "Style").into())];
        rows.push(Row::Item { id: 10, text: t!("Normal", "Normal").into(), icon: Icon::Glyph('\u{E8D2}', None) });
        for l in 1..=3usize {
            rows.push(Row::Item { id: 10 + l, text: format!("{} {l}  (Ctrl+Shift+{l})", t!("Başlık", "Heading")), icon: Icon::Glyph('\u{E8D2}', None) });
        }
        if let Some(id @ 10..=13) = glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            self.command(Cmd::Heading((id - 10) as u8));
        }
    }

    fn color_menu(&mut self) {
        let colors = [
            (0x000000, t!("Siyah", "Black")),
            (0xC00000, t!("Koyu kırmızı", "Dark red")),
            (0xFF0000, t!("Kırmızı", "Red")),
            (0xED7D31, t!("Turuncu", "Orange")),
            (0x00B050, t!("Yeşil", "Green")),
            (0x0070C0, t!("Mavi", "Blue")),
            (0x2F5496, t!("Lacivert", "Navy")),
            (0x7030A0, t!("Mor", "Purple")),
            (0x7F7F7F, t!("Gri", "Gray")),
        ];
        let mut rows = vec![Row::Header(t!("Yazı rengi", "Font color").into())];
        rows.push(Row::Item { id: 1, text: t!("Otomatik", "Automatic").into(), icon: Icon::Glyph('\u{E8D3}', None) });
        for (i, (c, n)) in colors.iter().enumerate() {
            rows.push(Row::Item { id: 10 + i, text: n.to_string(), icon: Icon::Glyph('\u{E8D3}', Some(*c)) });
        }
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => self.command(Cmd::Color(u32::MAX)),
            Some(id @ 10..=19) => self.command(Cmd::Color(colors[id - 10].0)),
            _ => {}
        }
    }

    pub(super) fn context_menu(&mut self) {
        let has = self.ed.cur.has_sel();
        let mut rows = Vec::new();
        if has {
            rows.push(Row::Item { id: 1, text: t!("Kes  (Ctrl+X)", "Cut  (Ctrl+X)").into(), icon: Icon::Glyph('\u{E8C6}', None) });
            rows.push(Row::Item { id: 2, text: t!("Kopyala  (Ctrl+C)", "Copy  (Ctrl+C)").into(), icon: Icon::Glyph('\u{E8C8}', None) });
        }
        rows.push(Row::Item { id: 3, text: t!("Yapıştır  (Ctrl+V)", "Paste  (Ctrl+V)").into(), icon: Icon::Glyph('\u{E77F}', None) });
        rows.push(Row::Item { id: 4, text: t!("Tümünü seç  (Ctrl+A)", "Select all  (Ctrl+A)").into(), icon: Icon::Glyph('\u{E8B3}', None) });
        rows.push(Row::Sep);
        rows.push(Row::Item { id: 5, text: t!("Kalın", "Bold").into(), icon: Icon::Glyph('\u{E8DD}', None) });
        rows.push(Row::Item { id: 6, text: t!("Biçem…", "Style…").into(), icon: Icon::Glyph('\u{E8D2}', None) });
        let ctrl = Keys { ctrl: true, shift: false };
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => {
                use crate::app::View;
                self.key(0x58, &ctrl);
            }
            Some(2) => {
                use crate::app::View;
                self.key(0x43, &ctrl);
            }
            Some(3) => {
                use crate::app::View;
                self.key(0x56, &ctrl);
            }
            Some(4) => {
                use crate::app::View;
                self.key(0x41, &ctrl);
            }
            Some(5) => self.command(Cmd::Bold),
            Some(6) => self.style_menu(),
            _ => {}
        }
    }

    pub(super) fn toolbar_ui(&mut self, p: &mut Painter<Act>, r: R) {
        let y = r.y;
        let mut x = r.x;
        let f = self.fmt_here();
        let label = self.style_label();
        x += ui::dropdown(p, x, y, 112.0, &label, Act::View(Cmd::More));
        x += ui::tool(p, x, y, '\u{E8E7}', Act::View(Cmd::Size(-1)), false);
        x += ui::tool_text(p, x, y, &format!("{}", f.size.unwrap_or(11.0)), Act::View(Cmd::Size(1)), false, false);
        x += ui::tool(p, x, y, '\u{E8E8}', Act::View(Cmd::Size(1)), false);
        x += ui::sep(p, x, y);
        x += ui::tool(p, x, y, '\u{E8DD}', Act::View(Cmd::Bold), f.b == Some(true));
        x += ui::tool(p, x, y, '\u{E8DB}', Act::View(Cmd::Italic), f.i == Some(true));
        x += ui::tool(p, x, y, '\u{E8DC}', Act::View(Cmd::Underline), f.u == Some(true));
        x += ui::tool(p, x, y, '\u{E8D3}', Act::View(Cmd::ColorMenu), false);
        x += ui::sep(p, x, y);
        let at = self.ed.cur.at.p.min(self.d.paras.len() - 1);
        let para = &self.d.paras[at];
        let al = para.align.unwrap_or_else(|| self.d.style_align(para.style.as_deref()));
        x += ui::tool(p, x, y, '\u{E8E4}', Act::View(Cmd::Align(PAlign::Left)), al == PAlign::Left);
        x += ui::tool(p, x, y, '\u{E8E3}', Act::View(Cmd::Align(PAlign::Center)), al == PAlign::Center);
        x += ui::tool(p, x, y, '\u{E8E2}', Act::View(Cmd::Align(PAlign::Right)), al == PAlign::Right);
        let _ = ui::tool_text(p, x, y, "≡", Act::View(Cmd::Align(PAlign::Justify)), al == PAlign::Justify, true);
    }
}
