//! Bu modül "Word belgesinde ve slayt metin kutusunda aynı tuşlar aynı işi
//! yapsın" sorununu çözüyor: ok tuşları (kelime/satır/sayfa atlama dahil),
//! Home/End, silme, Enter, Sekme, tümünü seç ve pano. Satır bilgisi (yukarı /
//! aşağı inince hangi karaktere düşüleceği) görünümün `Nav` uygulamasından gelir.

use crate::{
    app::{self, Keys},
    clip,
    rich::{self, Cursor, Para, Pos},
};

/// Görünümün metin düzeni hakkında verdiği bilgiler.
pub trait Nav {
    /// İmlecin belge koordinatları: (x, y, satır yüksekliği).
    fn caret(&self, at: Pos) -> Option<(f32, f32, f32)>;
    /// Belge koordinatındaki konum.
    fn hit(&self, x: f32, y: f32) -> Option<Pos>;
    /// Konumun bulunduğu görsel satırın (başı, sonu).
    fn line_bounds(&self, at: Pos) -> (usize, usize);
    /// Bu paragrafa imleç girmez (tablo gibi bloklar).
    fn locked(&self, _p: usize) -> bool {
        false
    }
    /// Sayfa yukarı / aşağı mesafesi.
    fn page(&self) -> f32 {
        400.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Tuş bizim değil.
    No,
    /// İmleç / seçim değişti.
    Moved,
    /// Metin değişti.
    Changed,
}

#[derive(Default, Clone)]
pub struct Editor {
    pub cur: Cursor,
    /// Yukarı / aşağı giderken korunan yatay konum.
    pub goal: Option<f32>,
}

fn first_open(paras: &[Para], nav: &dyn Nav, from: usize, down: bool) -> Option<usize> {
    if down {
        (from..paras.len()).find(|&p| !nav.locked(p))
    } else {
        (0..=from.min(paras.len().saturating_sub(1))).rev().find(|&p| !nav.locked(p))
    }
}

pub fn doc_start(paras: &[Para], nav: &dyn Nav) -> Pos {
    Pos { p: first_open(paras, nav, 0, true).unwrap_or(0), c: 0 }
}

pub fn doc_end(paras: &[Para], nav: &dyn Nav) -> Pos {
    let p = first_open(paras, nav, paras.len().saturating_sub(1), false).unwrap_or(0);
    Pos { p, c: paras.get(p).map(Para::len).unwrap_or(0) }
}

fn left(paras: &[Para], at: Pos, word: bool, nav: &dyn Nav) -> Pos {
    if at.c > 0 {
        let c = if word { rich::word_left(&paras[at.p].text(), at.c) } else { at.c - 1 };
        return Pos { p: at.p, c };
    }
    match at.p.checked_sub(1).and_then(|p| first_open(paras, nav, p, false)) {
        Some(p) => Pos { p, c: paras[p].len() },
        None => at,
    }
}

fn right(paras: &[Para], at: Pos, word: bool, nav: &dyn Nav) -> Pos {
    let n = paras[at.p].len();
    if at.c < n {
        let c = if word { rich::word_right(&paras[at.p].text(), at.c) } else { at.c + 1 };
        return Pos { p: at.p, c };
    }
    match first_open(paras, nav, at.p + 1, true) {
        Some(p) => Pos { p, c: 0 },
        None => at,
    }
}

/// Yukarı (`up`) / aşağı bir satır; `extra` > 0 ise o kadar daha (sayfa).
fn vertical(paras: &[Para], ed: &mut Editor, nav: &dyn Nav, up: bool, extra: f32) -> Pos {
    let at = ed.cur.at;
    let Some((x, y, h)) = nav.caret(at) else { return at };
    let gx = *ed.goal.get_or_insert(x);
    let ty = if up { y - 2.0 - extra } else { y + h + 2.0 + extra };
    match nav.hit(gx, ty) {
        Some(p) if p.p < paras.len() && !nav.locked(p.p) => p,
        // Tablonun üstünden atla.
        Some(p) if p.p < paras.len() => {
            let q = if up { p.p.checked_sub(1).and_then(|i| first_open(paras, nav, i, false)) } else { first_open(paras, nav, p.p + 1, true) };
            match q {
                Some(q) if up => Pos { p: q, c: paras[q].len() },
                Some(q) => Pos { p: q, c: 0 },
                None => at,
            }
        }
        _ if up => if extra > 0.0 { doc_start(paras, nav) } else { at },
        _ => if extra > 0.0 { doc_end(paras, nav) } else { at },
    }
}

/// Tuşu uygular. Ctrl+B/I/U gibi biçim tuşları görünümde kalır.
pub fn key(paras: &mut Vec<Para>, ed: &mut Editor, vk: u16, k: &Keys, nav: &dyn Nav) -> Out {
    if paras.is_empty() {
        return Out::No;
    }
    let at = ed.cur.at;
    let vertical_key = matches!(vk, 0x26 | 0x28 | 0x21 | 0x22);
    if !vertical_key {
        ed.goal = None;
    }
    let mv = |ed: &mut Editor, p: Pos| {
        ed.cur.set(p, k.shift);
        Out::Moved
    };
    match vk {
        0x25 => {
            if ed.cur.has_sel() && !k.shift {
                let a = ed.cur.range().0;
                return mv(ed, a);
            }
            mv(ed, left(paras, at, k.ctrl, nav))
        }
        0x27 => {
            if ed.cur.has_sel() && !k.shift {
                let b = ed.cur.range().1;
                return mv(ed, b);
            }
            mv(ed, right(paras, at, k.ctrl, nav))
        }
        0x26 if k.ctrl => {
            let p = if at.c > 0 { at.p } else { at.p.checked_sub(1).and_then(|p| first_open(paras, nav, p, false)).unwrap_or(at.p) };
            mv(ed, Pos { p, c: 0 })
        }
        0x28 if k.ctrl => {
            let p = first_open(paras, nav, at.p + 1, true).unwrap_or(at.p);
            let c = if p == at.p { paras[p].len() } else { 0 };
            mv(ed, Pos { p, c })
        }
        0x26 => {
            let p = vertical(paras, ed, nav, true, 0.0);
            mv(ed, p)
        }
        0x28 => {
            let p = vertical(paras, ed, nav, false, 0.0);
            mv(ed, p)
        }
        0x21 => {
            let p = vertical(paras, ed, nav, true, nav.page());
            mv(ed, p)
        }
        0x22 => {
            let p = vertical(paras, ed, nav, false, nav.page());
            mv(ed, p)
        }
        0x24 => {
            let p = if k.ctrl { doc_start(paras, nav) } else { Pos { p: at.p, c: nav.line_bounds(at).0 } };
            mv(ed, p)
        }
        0x23 => {
            let p = if k.ctrl { doc_end(paras, nav) } else { Pos { p: at.p, c: nav.line_bounds(at).1 } };
            mv(ed, p)
        }
        0x08 => {
            if !ed.cur.has_sel() {
                if at.c == 0 && at.p > 0 && nav.locked(at.p - 1) {
                    // Tablonun ardındaki boş paragraf silinebilir; dolu olan tabloya karışmaz.
                    if paras[at.p].is_empty() && first_open(paras, nav, at.p + 1, true).is_some() {
                        paras.remove(at.p);
                        let p = first_open(paras, nav, at.p, true).unwrap_or(0);
                        ed.cur.set(Pos { p, c: 0 }, false);
                        return Out::Changed;
                    }
                    return Out::Moved;
                }
                if k.ctrl && at.c > 0 {
                    ed.cur.anchor = Pos { p: at.p, c: rich::word_left(&paras[at.p].text(), at.c) };
                }
            }
            rich::backspace(paras, &mut ed.cur);
            Out::Changed
        }
        0x2E => {
            if !ed.cur.has_sel() {
                if at.c == paras[at.p].len() && nav.locked(at.p + 1) {
                    return Out::Moved;
                }
                if k.ctrl {
                    ed.cur.anchor = Pos { p: at.p, c: rich::word_right(&paras[at.p].text(), at.c) };
                }
            }
            rich::delete_fwd(paras, &mut ed.cur);
            Out::Changed
        }
        0x0D => {
            rich::enter(paras, &mut ed.cur);
            Out::Changed
        }
        0x09 if !k.ctrl => {
            rich::type_text(paras, &mut ed.cur, "\t");
            Out::Changed
        }
        0x41 if k.ctrl => {
            ed.cur.anchor = doc_start(paras, nav);
            ed.cur.at = doc_end(paras, nav);
            Out::Moved
        }
        0x43 if k.ctrl => {
            if ed.cur.has_sel() {
                clip::copy(app::hwnd(), &rich::sel_text(paras, &ed.cur).replace('\n', "\r\n"));
            }
            Out::Moved
        }
        0x58 if k.ctrl => {
            if !ed.cur.has_sel() {
                return Out::Moved;
            }
            clip::copy(app::hwnd(), &rich::sel_text(paras, &ed.cur).replace('\n', "\r\n"));
            rich::delete_sel(paras, &mut ed.cur);
            Out::Changed
        }
        0x56 if k.ctrl => match clip::paste(app::hwnd()) {
            Some(s) if !s.is_empty() => {
                rich::type_text(paras, &mut ed.cur, &s.replace("\r\n", "\n"));
                Out::Changed
            }
            _ => Out::Moved,
        },
        _ => Out::No,
    }
}

/// Yazılan karakter.
pub fn typed(paras: &mut Vec<Para>, ed: &mut Editor, ch: char) -> Out {
    if paras.is_empty() || ch.is_control() {
        return Out::No;
    }
    ed.goal = None;
    let mut b = [0u8; 4];
    rich::type_text(paras, &mut ed.cur, ch.encode_utf8(&mut b));
    Out::Changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::{Fmt, Run};

    struct Flat;
    impl Nav for Flat {
        fn caret(&self, at: Pos) -> Option<(f32, f32, f32)> {
            Some((at.c as f32 * 10.0, at.p as f32 * 20.0, 20.0))
        }
        fn hit(&self, x: f32, y: f32) -> Option<Pos> {
            Some(Pos { p: (y / 20.0).max(0.0) as usize, c: (x / 10.0) as usize })
        }
        fn line_bounds(&self, _at: Pos) -> (usize, usize) {
            (0, 3)
        }
        fn locked(&self, p: usize) -> bool {
            p == 1
        }
    }

    fn p(t: &str) -> Para {
        Para { runs: vec![Run::plain(t, Fmt::default())], ..Default::default() }
    }

    #[test]
    fn skips_locked_blocks() {
        let mut v = vec![p("abc"), p(""), p("xyz")];
        let mut ed = Editor::default();
        ed.cur.set(Pos { p: 0, c: 3 }, false);
        let k = Keys { ctrl: false, shift: false };
        assert_eq!(key(&mut v, &mut ed, 0x27, &k, &Flat), Out::Moved);
        assert_eq!(ed.cur.at, Pos { p: 2, c: 0 });
        // Tablonun ardında Backspace tabloya karışmaz.
        key(&mut v, &mut ed, 0x08, &k, &Flat);
        assert_eq!(v.len(), 3);
        assert_eq!(typed(&mut v, &mut ed, 'Q'), Out::Changed);
        assert_eq!(v[2].text(), "Qxyz");
        key(&mut v, &mut ed, 0x24, &Keys { ctrl: true, shift: true }, &Flat);
        assert_eq!(ed.cur.at, Pos { p: 0, c: 0 });
    }
}
