//! Bu modül "PDF üstünde yapılan işler" sorununu çözüyor: döndür, sil,
//! yukarı/aşağı taşı, başka PDF ekle, sayfayı ayrı kaydet, metin ekle,
//! sayfa metnini kopyala; geri al / yinele; tuşlar, fare, araç çubuğu ve
//! menüler. Her değişiklik yeni bir bayt dizisi üretir; eskisi geri alma
//! yığınına gider.

use super::{
    edit,
    view::{Note, PdfView, RAIL, ZOOMS},
};
use crate::{
    app::{self, Act, Cmd, Keys, Mouse},
    clip, dialog, ui,
};
use carlui_core::t;
use carlui_ui::{
    glassmenu::{self, Icon, Row},
    paint::{Painter, R},
};
use std::sync::Arc;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const UNDO_MAX: usize = 30;

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

impl PdfView {
    /// Yeni baytları geçerli kılar (geri alınabilir).
    fn apply(&mut self, r: Result<Vec<u8>, String>, go: Option<usize>) {
        match r {
            Ok(b) => {
                let old = std::mem::replace(&mut self.bytes, Arc::new(b));
                if self.reload() {
                    self.undo.push(old);
                    if self.undo.len() > UNDO_MAX {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.dirty = true;
                    if let Some(i) = go {
                        self.go(i);
                    }
                } else {
                    self.bytes = old;
                    self.reload();
                }
            }
            Err(e) => app::error(&e),
        }
    }

    /// Bayt değişti: boyutları yeniden oku, çizimleri at.
    fn reload(&mut self) -> bool {
        match super::sizes(&self.bytes) {
            Ok(s) => {
                self.sizes = s;
                self.ren = None;
                self.pages.clear();
                self.thumbs.clear();
                self.failed.clear();
                self.cur = self.cur.min(self.sizes.len() - 1);
                self.goto = Some(self.cur);
                true
            }
            Err(e) => {
                app::error(&e);
                false
            }
        }
    }

    fn history(&mut self, redo: bool) {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let Some(b) = from.pop() else { return };
        to.push(std::mem::replace(&mut self.bytes, b));
        self.reload();
        self.dirty = true;
    }

    pub(super) fn commit_note(&mut self) {
        let Some(n) = self.note.take() else { return };
        if n.text.trim().is_empty() {
            return;
        }
        let r = crate::guard::deep(|| edit::add_text(&self.bytes, n.page, n.u, n.v, &n.text, self.note_size, self.note_color));
        self.apply(r, None);
    }

    fn merge(&mut self) {
        let Some(p) = dialog::open_pdf(app::hwnd()) else { return };
        let other = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return app::error(&e.to_string()),
        };
        let first = self.sizes.len();
        let r = crate::guard::deep(|| edit::merge(&self.bytes, &other));
        self.apply(r, Some(first));
        app::toast(t!("PDF sona eklendi", "PDF appended"));
    }

    fn extract(&mut self) {
        let name = format!("{} {}", t!("Sayfa", "Page"), self.cur + 1);
        let Some(p) = dialog::save(app::hwnd(), &name, "pdf") else { return };
        match crate::guard::deep(|| edit::extract(&self.bytes, self.cur)).and_then(|b| app::write_atomic(&p, &b)) {
            Ok(()) => app::toast(t!("Sayfa ayrı dosyaya kaydedildi", "Page saved as a separate file")),
            Err(e) => app::error(&e),
        }
    }

    fn copy_text(&mut self) {
        match crate::guard::deep(|| edit::text(&self.bytes, self.cur)) {
            Ok(s) if !s.trim().is_empty() => {
                clip::copy(app::hwnd(), s.trim());
                app::toast(t!("Sayfa metni kopyalandı", "Page text copied"));
            }
            Ok(_) => app::toast(t!("Bu sayfada okunabilir metin yok", "No readable text on this page")),
            Err(e) => app::error(&e),
        }
    }

    fn zoom_step(&mut self, d: i8) {
        if d == 0 {
            self.set_zoom(None);
            return;
        }
        let z = self.z;
        let next = if d > 0 { ZOOMS.iter().copied().find(|&s| s > z + 0.01) } else { ZOOMS.iter().rev().copied().find(|&s| s < z - 0.01) };
        self.set_zoom(Some(next.unwrap_or(z)));
    }

    pub(super) fn command(&mut self, c: Cmd) {
        if !matches!(c, Cmd::Size(_) | Cmd::Color(_) | Cmd::ColorMenu | Cmd::Zoom(_)) {
            self.commit_note();
        }
        let n = self.sizes.len();
        let cur = self.cur;
        match c {
            Cmd::Undo => self.history(false),
            Cmd::Redo => self.history(true),
            Cmd::Zoom(d) => self.zoom_step(d),
            Cmd::Pick(i) => self.go(i),
            Cmd::Rotate => {
                let r = crate::guard::deep(|| edit::rotate(&self.bytes, cur));
                self.apply(r, Some(cur));
            }
            Cmd::Remove => {
                let r = crate::guard::deep(|| edit::delete(&self.bytes, cur));
                self.apply(r, Some(cur.min(n.saturating_sub(2))));
            }
            Cmd::Merge => self.merge(),
            Cmd::Extract => self.extract(),
            Cmd::NoteTool | Cmd::TextBox => {
                self.note_tool = !self.note_tool;
            }
            Cmd::Size(d) => self.note_size = (self.note_size + d as f32 * 2.0).clamp(6.0, 72.0),
            Cmd::Color(c) => self.note_color = if c == u32::MAX { 0 } else { c },
            Cmd::ColorMenu => self.color_menu(),
            Cmd::More => self.more_menu(),
            _ => {}
        }
    }

    fn move_page(&mut self, up: bool) {
        let cur = self.cur;
        let to = if up { cur.checked_sub(1) } else { (cur + 1 < self.sizes.len()).then_some(cur + 1) };
        let Some(to) = to else { return };
        let r = crate::guard::deep(|| edit::move_page(&self.bytes, cur, to));
        self.apply(r, Some(to));
    }

    fn color_menu(&mut self) {
        let colors = [
            (0x000000, t!("Siyah", "Black")),
            (0xC00000, t!("Koyu kırmızı", "Dark red")),
            (0xFF0000, t!("Kırmızı", "Red")),
            (0x0070C0, t!("Mavi", "Blue")),
            (0x00B050, t!("Yeşil", "Green")),
            (0x7030A0, t!("Mor", "Purple")),
        ];
        let mut rows = vec![Row::Header(t!("Yazı rengi", "Font color").into())];
        for (i, (c, n)) in colors.iter().enumerate() {
            rows.push(Row::Item { id: 10 + i, text: n.to_string(), icon: Icon::Swatch('\u{E8D3}', *c) });
        }
        if let Some(id @ 10..=15) = glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            self.note_color = colors[id - 10].0;
        }
    }

    pub(super) fn more_menu(&mut self) {
        let rows = vec![
            Row::Header(format!("{} {}", t!("Sayfa", "Page"), self.cur + 1)),
            item(1, t!("Saat yönünde döndür  (Ctrl+R)", "Rotate clockwise  (Ctrl+R)"), '\u{E7AD}'),
            item(2, t!("Yukarı taşı", "Move up"), '\u{E74A}'),
            item(3, t!("Aşağı taşı", "Move down"), '\u{E74B}'),
            item(4, t!("Sayfayı sil", "Delete page"), '\u{E74D}'),
            Row::Sep,
            item(5, t!("Sayfa metnini kopyala", "Copy page text"), '\u{E8C8}'),
            item(6, t!("Sayfayı ayrı kaydet…", "Save page as…"), '\u{E792}'),
            item(7, t!("Sona PDF ekle…", "Append PDF…"), '\u{E710}'),
            Row::Sep,
            item(8, t!("Metin ekle", "Add text"), '\u{E8D2}'),
        ];
        match glassmenu::run(app::hwnd(), rows, None, cursor_pt()) {
            Some(1) => self.command(Cmd::Rotate),
            Some(2) => self.move_page(true),
            Some(3) => self.move_page(false),
            Some(4) => self.command(Cmd::Remove),
            Some(5) => self.copy_text(),
            Some(6) => self.extract(),
            Some(7) => self.merge(),
            Some(8) => self.note_tool = true,
            _ => {}
        }
    }

    pub(super) fn on_key(&mut self, vk: u16, k: &Keys) -> bool {
        if self.note.is_some() {
            match vk {
                0x1B => self.note = None,
                0x0D if k.shift => {
                    if let Some(n) = &mut self.note {
                        n.text.push('\n');
                    }
                }
                0x0D => self.commit_note(),
                0x08 => {
                    if let Some(n) = &mut self.note {
                        n.text.pop();
                    }
                }
                _ => return k.ctrl && self.shortcut(vk, k),
            }
            app::caret_reset();
            return true;
        }
        if k.ctrl {
            return self.shortcut(vk, k);
        }
        let h = self.canvas().h;
        match vk {
            0x1B if self.note_tool => self.note_tool = false,
            0x21 => self.scroll_by(-h * 0.9),
            0x22 | 0x20 => self.scroll_by(if k.shift { -h * 0.9 } else { h * 0.9 }),
            0x26 => self.scroll_by(-60.0),
            0x28 => self.scroll_by(60.0),
            0x25 => self.go(self.cur.saturating_sub(1)),
            0x27 => self.go(self.cur + 1),
            0x24 => self.go(0),
            0x23 => self.go(usize::MAX),
            _ => return false,
        }
        true
    }

    fn shortcut(&mut self, vk: u16, k: &Keys) -> bool {
        let c = match vk {
            0x5A if k.shift => Cmd::Redo,
            0x5A => Cmd::Undo,
            0x59 => Cmd::Redo,
            0x52 => Cmd::Rotate,
            0xBB | 0x6B => Cmd::Zoom(1),
            0xBD | 0x6D => Cmd::Zoom(-1),
            0x30 | 0x60 => Cmd::Zoom(0),
            0x43 if self.note.is_none() => {
                self.copy_text();
                return true;
            }
            _ => return false,
        };
        self.command(c);
        true
    }

    pub(super) fn on_mouse(&mut self, m: Mouse, pt: (f32, f32), _k: &Keys) {
        if pt.0 < self.area.x + RAIL {
            if matches!(m, Mouse::Down | Mouse::Right) {
                self.rail_click(pt);
                if m == Mouse::Right {
                    self.more_menu();
                }
            }
            return;
        }
        match m {
            Mouse::Down | Mouse::Double => {
                if self.note_tool {
                    self.commit_note();
                    if let Some((page, u, v)) = self.page_at(pt) {
                        self.note = Some(Note { page, u, v, text: String::new() });
                        app::caret_reset();
                    }
                } else {
                    self.drag = Some((pt, self.sy, self.sx));
                }
            }
            Mouse::Move => {
                if let Some((start, sy, sx)) = self.drag {
                    self.sy = sy - (pt.1 - start.1);
                    self.sx = sx - (pt.0 - start.0);
                }
            }
            Mouse::Up => self.drag = None,
            Mouse::Right => {
                self.commit_note();
                if let Some((page, _, _)) = self.page_at(pt) {
                    self.cur = page;
                }
                self.more_menu();
            }
        }
    }

    fn rail_click(&mut self, pt: (f32, f32)) {
        let y = pt.1 - self.area.y + self.rail_scroll;
        let mut top = 14.0;
        for i in 0..self.sizes.len() {
            let (w, h) = self.sizes[i];
            let th = (super::view::THUMB_W * h / w).min(super::view::THUMB_W * 1.6) + 20.0;
            if y < top + th {
                self.commit_note();
                self.go(i);
                return;
            }
            top += th;
        }
    }

    pub(super) fn toolbar_ui(&mut self, p: &mut Painter<Act>, r: R) {
        let y = r.y;
        let mut x = r.x;
        x += ui::tool(p, x, y, '\u{E71F}', Act::View(Cmd::Zoom(-1)), false);
        let pct = format!("%{}", (self.z * 100.0).round());
        x += ui::tool_text(p, x, y, &pct, Act::View(Cmd::Zoom(0)), self.fit, false);
        x += ui::tool(p, x, y, '\u{E8A3}', Act::View(Cmd::Zoom(1)), false);
        x += ui::sep(p, x, y);
        x += ui::tool(p, x, y, '\u{E7AD}', Act::View(Cmd::Rotate), false);
        x += ui::tool(p, x, y, '\u{E74D}', Act::View(Cmd::Remove), false);
        x += ui::sep(p, x, y);
        x += ui::tool_text(p, x, y, t!("Metin ekle", "Add text"), Act::View(Cmd::NoteTool), self.note_tool, false);
        if self.note_tool {
            x += ui::tool(p, x, y, '\u{E8E7}', Act::View(Cmd::Size(-1)), false);
            x += ui::tool_text(p, x, y, &format!("{}", self.note_size), Act::View(Cmd::Size(1)), false, false);
            x += ui::tool(p, x, y, '\u{E8E8}', Act::View(Cmd::Size(1)), false);
            let cr = R::new(x, y, 32.0, 30.0);
            x += ui::tool(p, x, y, '\u{E8D3}', Act::View(Cmd::ColorMenu), false);
            carlui_ui::tone::literal(|| p.fill(R::new(cr.x + 8.0, cr.bottom() - 6.0, 16.0, 3.0), self.note_color, 1.0));
        }
        x += ui::sep(p, x, y);
        x += ui::tool_text(p, x, y, t!("Birleştir", "Merge"), Act::View(Cmd::Merge), false, false);
        x += ui::tool_text(p, x, y, t!("Sayfayı ayır", "Extract page"), Act::View(Cmd::Extract), false, false);
        let _ = ui::tool(p, x, y, '\u{E712}', Act::View(Cmd::More), false);
    }
}
