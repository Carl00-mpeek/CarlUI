//! Bu modül "slaytta fareyle ne olacak" sorununu çözüyor: şekle tıklamak
//! seçer, ikinci tıklama metne imleç koyar, kenardan sürüklemek taşır,
//! tutamaçtan sürüklemek boyutlar (Shift oranı korur), çift tıklama sözcük
//! seçer. Sunu kipinde tıklama ilerletir, sağ tık geri alır.

use super::view::{Drag, Last, SlideView, RAIL};
use crate::{
    app::{Keys, Mouse},
    rich::{self, Pos},
    richedit::Nav,
};

impl SlideView {
    pub(super) fn on_mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        if self.present.is_some() {
            match m {
                Mouse::Down => self.present_step(true),
                Mouse::Right => self.present_step(false),
                _ => {}
            }
            return;
        }
        if self.d.slides.is_empty() {
            return;
        }
        if matches!(m, Mouse::Down | Mouse::Double | Mouse::Right) && pt.0 < self.area.x + RAIL {
            if m != Mouse::Double {
                self.mouse_rail(m, pt);
            }
            return;
        }
        match m {
            Mouse::Down => {
                self.last = Last::Other;
                if let Some(h) = self.handle_at(pt) {
                    let si = self.sel.unwrap_or(0);
                    let s = &self.d.slides[self.cur].shapes[si];
                    self.drag = Drag::Shape { h: Some(h), start: pt, orig: (s.x, s.y, s.w, s.h), snap: Some(Box::new(self.snapshot())) };
                    return;
                }
                let hit = self.shape_at(pt);
                // Düzenlenen metnin içine tıklama: imleci taşı.
                if let (true, Some(si)) = (self.edit, self.sel) {
                    let r = self.rect_of(si);
                    if hit == Some(si) && !r.is_some_and(|r| Self::on_border(r, pt)) {
                        if let Some(at) = self.nav(si).and_then(|n| n.hit(pt.0, pt.1)) {
                            self.ed.cur.set(at, k.shift);
                            self.ed.goal = None;
                            self.drag = Drag::Text;
                        }
                        return;
                    }
                }
                self.edit = false;
                let Some(si) = hit else {
                    self.sel = None;
                    return;
                };
                self.sel = Some(si);
                let s = &self.d.slides[self.cur].shapes[si];
                let orig = (s.x, s.y, s.w, s.h);
                let border = self.rect_of(si).is_some_and(|r| Self::on_border(r, pt));
                if s.has_text() && !border {
                    self.start_edit(si, false);
                    if let Some(at) = self.nav(si).and_then(|n| n.hit(pt.0, pt.1)) {
                        self.ed.cur.set(at, false);
                    }
                    self.drag = Drag::Text;
                } else if !s.grouped {
                    self.drag = Drag::Shape { h: None, start: pt, orig, snap: Some(Box::new(self.snapshot())) };
                }
            }
            Mouse::Double => {
                if let (true, Some(si)) = (self.edit, self.sel) {
                    if let Some(at) = self.nav(si).and_then(|n| n.hit(pt.0, pt.1)) {
                        let text = self.d.slides[self.cur].shapes[si].paras[at.p].text();
                        let a = rich::word_left(&text, (at.c + 1).min(text.chars().count()));
                        let b = rich::word_right(&text, at.c);
                        self.ed.cur.set(Pos { p: at.p, c: a.min(at.c) }, false);
                        self.ed.cur.set(Pos { p: at.p, c: b }, true);
                    }
                }
            }
            Mouse::Move => match &mut self.drag {
                Drag::Text => {
                    if let Some(si) = self.sel {
                        if let Some(at) = self.nav(si).and_then(|n| n.hit(pt.0, pt.1)) {
                            self.ed.cur.set(at, true);
                        }
                    }
                }
                Drag::Shape { h, start, orig, snap } => {
                    let (dx, dy) = ((pt.0 - start.0) / self.k, (pt.1 - start.1) / self.k);
                    if snap.is_some() && dx.abs() + dy.abs() < 2.0 / self.k {
                        return;
                    }
                    let (h, orig) = (*h, *orig);
                    if let Some(s) = snap.take() {
                        self.push(*s);
                    }
                    let Some(si) = self.sel else { return };
                    let s = &mut self.d.slides[self.cur].shapes[si];
                    let (mut x, mut y, mut w, mut hh) = orig;
                    match h {
                        None => {
                            x += dx;
                            y += dy;
                        }
                        Some(h) => {
                            if matches!(h, 0 | 6 | 7) {
                                x += dx;
                                w -= dx;
                            }
                            if matches!(h, 2..=4) {
                                w += dx;
                            }
                            if matches!(h, 0..=2) {
                                y += dy;
                                hh -= dy;
                            }
                            if matches!(h, 4..=6) {
                                hh += dy;
                            }
                            if w < 4.0 {
                                if matches!(h, 0 | 6 | 7) {
                                    x -= 4.0 - w;
                                }
                                w = 4.0;
                            }
                            if hh < 4.0 {
                                if matches!(h, 0..=2) {
                                    y -= 4.0 - hh;
                                }
                                hh = 4.0;
                            }
                        }
                    }
                    (s.x, s.y, s.w, s.h) = (x, y, w, hh);
                    s.moved = true;
                    self.touched();
                }
                Drag::None => {}
            },
            Mouse::Up => self.drag = Drag::None,
            Mouse::Right => {
                if !self.edit || self.shape_at(pt) != self.sel {
                    self.edit = false;
                    self.sel = self.shape_at(pt);
                }
                self.context_menu();
            }
        }
    }
}
