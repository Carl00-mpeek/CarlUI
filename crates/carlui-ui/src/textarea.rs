//! Bu modül "araç pencerelerinde uzun, çok satırlı metin nasıl yazılır ve
//! düzenlenir?" sorununu çözüyor. `TextArea` sözcük kaydırmalı bir düzenleyici:
//! imleç, Shift/fare ile seçim, Ctrl+ok ile sözcük atlama, kaydırma, pano
//! (Ctrl+A/C/X/V) ve geri alma (Ctrl+Z/Y). Görünüm (`View`) olaylarını olduğu
//! gibi buraya iletir; çizim `Painter` ile yapılır.

use crate::paint::{Align, Fonts, Painter, R, TEXT};
use std::collections::HashMap;
use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL},
    Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED,
    System::{
        DataExchange::*,
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
    },
    UI::Input::KeyboardAndMouse::*,
};

const SIZE: f32 = 14.0;
const LINE: f32 = 21.0;
const PAD: f32 = 12.0;

pub struct TextArea {
    text: Vec<char>,
    caret: usize,
    anchor: usize,
    /// Görsel satırlar: (başlangıç, bitiş) karakter sırası; bitiş satır sonu hariç.
    lines: Vec<(usize, usize)>,
    wrap_w: f32,
    dirty: bool,
    widths: HashMap<String, f32>,
    scroll: f32,
    follow: bool,
    /// Yukarı/aşağı giderken korunan yatay konum.
    want_x: Option<f32>,
    undo: Vec<(Vec<char>, usize)>,
    redo: Vec<(Vec<char>, usize)>,
    typing: bool,
    rect: R,
    fonts: Option<Fonts>,
    pub placeholder: String,
    pub max: usize,
    pub read_only: bool,
}

impl TextArea {
    pub fn new(placeholder: &str) -> Self {
        Self {
            text: Vec::new(),
            caret: 0,
            anchor: 0,
            lines: vec![(0, 0)],
            wrap_w: 0.0,
            dirty: true,
            widths: HashMap::new(),
            scroll: 0.0,
            follow: false,
            want_x: None,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
            rect: R::new(0.0, 0.0, 0.0, 0.0),
            fonts: Fonts::new().ok(),
            placeholder: placeholder.into(),
            max: 100_000,
            read_only: false,
        }
    }

    fn measure(&self, s: &str, size: f32, bold: bool) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * size * 0.5, |f| f.width(s, size, bold))
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.iter().all(|c| c.is_whitespace())
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Tüm metni değiştirir (geri alınabilir).
    pub fn set_text(&mut self, s: &str) {
        self.save(false);
        self.text = s.chars().take(self.max).collect();
        self.caret = self.text.len();
        self.anchor = self.caret;
        self.scroll = 0.0;
        self.dirty = true;
    }

    /// İmlecin olduğu yere metin ekler (seçim varsa yerine).
    pub fn insert(&mut self, s: &str) {
        self.save(false);
        self.replace(s);
    }

    /// Seçili metin; seçim yoksa boş.
    pub fn selected(&self) -> String {
        let (a, b) = self.sel();
        self.text[a..b].iter().collect()
    }

    fn sel(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    fn save(&mut self, typing: bool) {
        if typing && self.typing {
            return;
        }
        self.typing = typing;
        self.undo.push((self.text.clone(), self.caret));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn replace(&mut self, s: &str) {
        let (a, b) = self.sel();
        let room = self.max.saturating_sub(self.text.len() - (b - a));
        let ins: Vec<char> = s.chars().filter(|&c| c != '\r').take(room).collect();
        let n = ins.len();
        self.text.splice(a..b, ins);
        self.caret = a + n;
        self.anchor = self.caret;
        self.dirty = true;
        self.follow = true;
        self.want_x = None;
    }

    fn move_to(&mut self, i: usize, extend: bool) {
        self.caret = i.min(self.text.len());
        if !extend {
            self.anchor = self.caret;
        }
        self.follow = true;
        self.typing = false;
    }

    // ---- Sözcük ve satır yardımcıları ----

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && !self.text[i - 1].is_alphanumeric() {
            i -= 1;
        }
        while i > 0 && self.text[i - 1].is_alphanumeric() {
            i -= 1;
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        let n = self.text.len();
        while i < n && self.text[i].is_alphanumeric() {
            i += 1;
        }
        while i < n && !self.text[i].is_alphanumeric() {
            i += 1;
        }
        i
    }

    /// İmlecin bulunduğu görsel satır.
    fn line_of(&self, i: usize) -> usize {
        match self.lines.binary_search_by(|l| l.0.cmp(&i)) {
            Ok(k) => k,
            Err(k) => k.saturating_sub(1),
        }
    }

    fn line_str(&self, k: usize, upto: usize) -> String {
        let (s, e) = self.lines[k];
        self.text[s..upto.clamp(s, e)].iter().collect()
    }

    // ---- Olaylar ----

    pub fn char(&mut self, c: char) {
        if self.read_only {
            return;
        }
        self.save(true);
        let mut b = [0u8; 4];
        self.replace(c.encode_utf8(&mut b));
        // Her sözcük ayrı geri alınır.
        if c == ' ' {
            self.typing = false;
        }
    }

    /// Tuş; `true` = işlendi.
    pub fn key(&mut self, vk: u16) -> bool {
        let ctrl = crate::view::ctrl();
        let shift = crate::view::shift();
        let n = self.text.len();
        let k = VIRTUAL_KEY(vk);
        if ctrl {
            match k {
                VK_A => {
                    self.anchor = 0;
                    self.caret = n;
                    return true;
                }
                VK_C => {
                    if self.caret != self.anchor {
                        set_clipboard(&self.selected());
                    }
                    return true;
                }
                VK_X if !self.read_only => {
                    if self.caret != self.anchor {
                        set_clipboard(&self.selected());
                        self.save(false);
                        self.replace("");
                    }
                    return true;
                }
                VK_V if !self.read_only => {
                    if let Some(s) = get_clipboard() {
                        self.insert(&s);
                    }
                    return true;
                }
                VK_Z if !self.read_only => {
                    if let Some((t, c)) = self.undo.pop() {
                        self.redo.push((std::mem::replace(&mut self.text, t), self.caret));
                        self.move_to(c, false);
                        self.dirty = true;
                    }
                    return true;
                }
                VK_Y if !self.read_only => {
                    if let Some((t, c)) = self.redo.pop() {
                        self.undo.push((std::mem::replace(&mut self.text, t), self.caret));
                        self.move_to(c, false);
                        self.dirty = true;
                    }
                    return true;
                }
                _ => {}
            }
        }
        let vertical = matches!(k, VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT);
        if !vertical {
            self.want_x = None;
        }
        match k {
            VK_LEFT => {
                let (a, _) = self.sel();
                let to = if ctrl { self.word_left(self.caret) } else if !shift && self.caret != self.anchor { a } else { self.caret.saturating_sub(1) };
                self.move_to(to, shift);
            }
            VK_RIGHT => {
                let (_, b) = self.sel();
                let to = if ctrl { self.word_right(self.caret) } else if !shift && self.caret != self.anchor { b } else { (self.caret + 1).min(n) };
                self.move_to(to, shift);
            }
            VK_HOME => {
                let to = if ctrl { 0 } else { self.lines[self.line_of(self.caret)].0 };
                self.move_to(to, shift);
            }
            VK_END => {
                let to = if ctrl { n } else { self.lines[self.line_of(self.caret)].1 };
                self.move_to(to, shift);
            }
            VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT => {
                let cur = self.line_of(self.caret);
                let page = ((self.rect.h - 2.0 * PAD) / LINE).floor().max(1.0) as isize;
                let step: isize = match k {
                    VK_UP => -1,
                    VK_DOWN => 1,
                    VK_PRIOR => -page,
                    _ => page,
                };
                let x = match self.want_x {
                    Some(x) => x,
                    None => self.measure(&self.line_str(cur, self.caret), SIZE, false),
                };
                let target = cur as isize + step;
                let to = if target < 0 {
                    0
                } else if target as usize >= self.lines.len() {
                    n
                } else {
                    self.index_at_x(target as usize, x)
                };
                self.move_to(to, shift);
                self.want_x = Some(x);
            }
            VK_BACK if !self.read_only => {
                if self.caret == self.anchor {
                    let to = if ctrl { self.word_left(self.caret) } else { self.caret.saturating_sub(1) };
                    self.anchor = to;
                }
                if self.caret != self.anchor {
                    self.save(false);
                    self.replace("");
                }
            }
            VK_DELETE if !self.read_only => {
                if self.caret == self.anchor {
                    let to = if ctrl { self.word_right(self.caret) } else { (self.caret + 1).min(n) };
                    self.anchor = to;
                }
                if self.caret != self.anchor {
                    self.save(false);
                    self.replace("");
                }
            }
            VK_RETURN if !self.read_only => {
                self.save(false);
                self.replace("\n");
            }
            // Pencereyi kapatmasın; yalnızca seçimi bırakır.
            VK_ESCAPE => self.anchor = self.caret,
            _ => return false,
        }
        true
    }

    /// Sol tuş metin alanında basıldı.
    pub fn press(&mut self, at: (f32, f32)) {
        let i = self.index_at(at);
        self.move_to(i, crate::view::shift());
        self.want_x = None;
    }

    pub fn drag(&mut self, at: (f32, f32)) {
        let i = self.index_at(at);
        self.move_to(i, true);
    }

    /// Fare tekerleği; alanın üstündeyse `true`.
    pub fn wheel(&mut self, delta: f32, at: (f32, f32)) -> bool {
        if !self.rect.contains(at) {
            return false;
        }
        self.scroll -= delta * LINE * 3.0;
        self.follow = false;
        true
    }

    fn index_at(&self, at: (f32, f32)) -> usize {
        let y = at.1 - self.rect.y - PAD + self.scroll;
        if y < 0.0 {
            return 0;
        }
        let k = (y / LINE) as usize;
        if k >= self.lines.len() {
            return self.text.len();
        }
        self.index_at_x(k, at.0 - self.rect.x - PAD)
    }

    /// Satırda `x` konumuna en yakın karakter sınırı (ikili arama).
    fn index_at_x(&self, k: usize, x: f32) -> usize {
        let (s, e) = self.lines[k];
        let (mut lo, mut hi) = (s, e);
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let w = self.measure(&self.line_str(k, mid), SIZE, false);
            let cw = self.measure(&self.text[mid - 1].to_string(), SIZE, false);
            if w - cw / 2.0 <= x {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    // ---- Kaydırma ----

    fn width_of(&mut self, s: &str) -> f32 {
        if let Some(w) = self.widths.get(s) {
            return *w;
        }
        let w = self.measure(s, SIZE, false);
        if self.widths.len() > 20_000 {
            self.widths.clear();
        }
        self.widths.insert(s.to_string(), w);
        w
    }

    fn rewrap(&mut self, max_w: f32) {
        let text = std::mem::take(&mut self.text);
        let mut lines = Vec::new();
        let mut start = 0;
        let n = text.len();
        while start <= n {
            let end = text[start..].iter().position(|&c| c == '\n').map_or(n, |p| start + p);
            // Paragrafı sözcük öbeklerine (sözcük + ardındaki boşluklar) böl.
            let mut ls = start;
            let mut x = 0.0;
            let mut i = start;
            while i < end {
                let mut j = i;
                while j < end && !text[j].is_whitespace() {
                    j += 1;
                }
                let word_end = j;
                while j < end && text[j].is_whitespace() {
                    j += 1;
                }
                let word: String = text[i..word_end].iter().collect();
                let ww = self.width_of(&word);
                let tail: String = text[word_end..j].iter().collect();
                let sw = if tail.is_empty() { 0.0 } else { self.width_of(&tail) };
                if x + ww > max_w && i > ls {
                    lines.push((ls, i));
                    ls = i;
                    x = 0.0;
                }
                if ww > max_w {
                    // Tek başına sığmayan uzun sözcük: karakterlerden böl.
                    let mut cx = x;
                    for c in i..word_end {
                        let cw = self.width_of(&text[c].to_string());
                        if cx + cw > max_w && c > ls {
                            lines.push((ls, c));
                            ls = c;
                            cx = 0.0;
                        }
                        cx += cw;
                    }
                    x = cx + sw;
                } else {
                    x += ww + sw;
                }
                i = j;
            }
            lines.push((ls, end));
            start = end + 1;
        }
        self.text = text;
        self.lines = lines;
    }

    // ---- Çizim ----

    /// Alanı `r` içine çizer; `act` alanın tıklama/sürükleme eylemi.
    pub fn paint<A: Clone>(&mut self, p: &mut Painter<A>, r: R, act: A) {
        self.rect = r;
        let inner_w = (r.w - 2.0 * PAD - 8.0).max(40.0);
        if self.dirty || (inner_w - self.wrap_w).abs() > 0.5 {
            self.rewrap(inner_w);
            self.wrap_w = inner_w;
            self.dirty = false;
        }
        p.round(r, 8.0, 0x000000, 0.18);
        p.outline(r, 8.0, if p.hover(r) { p.accent } else { 0xFFFFFF }, if p.hover(r) { 0.35 } else { 0.1 });
        p.hit(r, act);

        let view_h = r.h - 2.0 * PAD;
        let content_h = self.lines.len() as f32 * LINE;
        let max_scroll = (content_h - view_h).max(0.0);
        if self.follow {
            let cy = self.line_of(self.caret) as f32 * LINE;
            if cy < self.scroll {
                self.scroll = cy;
            } else if cy + LINE > self.scroll + view_h {
                self.scroll = cy + LINE - view_h;
            }
            self.follow = false;
        }
        self.scroll = self.scroll.clamp(0.0, max_scroll);

        let clip = R::new(r.x + 1.0, r.y + PAD / 2.0, r.w - 2.0, r.h - PAD);
        unsafe { p.rt.PushAxisAlignedClip(&clip.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        let x0 = r.x + PAD;
        let top = r.y + PAD - self.scroll;
        if self.text.is_empty() {
            p.text(&self.placeholder, R::new(x0, r.y + PAD, inner_w, LINE), SIZE, false, TEXT, 0.4, Align::Left);
        }
        let first = (self.scroll / LINE) as usize;
        let last = (((self.scroll + view_h) / LINE).ceil() as usize + 1).min(self.lines.len());
        let (sa, sb) = self.sel();
        for k in first..last {
            let (s, e) = self.lines[k];
            let y = top + k as f32 * LINE;
            if sa != sb && sa <= e && sb >= s {
                let a = sa.max(s);
                let b = sb.min(e);
                let xa = self.measure(&self.line_str(k, a), SIZE, false);
                let mut xb = self.measure(&self.line_str(k, b), SIZE, false);
                // Seçim satır sonunu da kapsıyorsa küçük bir pay.
                if sb > e && self.text.get(e) == Some(&'\n') {
                    xb += 6.0;
                }
                if xb > xa {
                    p.fill(R::new(x0 + xa, y + 1.0, xb - xa, LINE - 2.0), p.accent, 0.35);
                }
            }
            let line: String = self.text[s..e].iter().collect();
            if !line.is_empty() {
                p.text(&line, R::new(x0, y, inner_w + 40.0, LINE), SIZE, false, TEXT, 0.92, Align::Left);
            }
        }
        if !self.read_only {
            let k = self.line_of(self.caret);
            let cx = x0 + self.measure(&self.line_str(k, self.caret), SIZE, false);
            let cy = top + k as f32 * LINE;
            p.fill(R::new(cx, cy + 3.0, 1.5, LINE - 6.0), TEXT, 0.9);
        }
        unsafe { p.rt.PopAxisAlignedClip() };

        // Kaydırma çubuğu
        if max_scroll > 0.0 {
            let track = r.h - 8.0;
            let th = (track * view_h / content_h).max(24.0);
            let ty = r.y + 4.0 + (track - th) * (self.scroll / max_scroll);
            p.round(R::new(r.right() - 7.0, ty, 3.0, th), 1.5, 0xFFFFFF, 0.3);
        }
    }
}

// ---- Pano ----

pub fn get_clipboard() -> Option<String> {
    unsafe {
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let out = (|| {
            let h: HANDLE = GetClipboardData(13 /* CF_UNICODETEXT */).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 && n < 8 * 1024 * 1024 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s.replace("\r\n", "\n"))
        })();
        let _ = CloseClipboard();
        out
    }
}

pub fn set_clipboard(s: &str) -> bool {
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let w: Vec<u16> = s.replace('\n', "\r\n").encode_utf16().chain(std::iter::once(0)).collect();
        let ok = (|| {
            let g = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2).ok()?;
            let p = GlobalLock(g) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            let _ = GlobalUnlock(g);
            SetClipboardData(13, HANDLE(g.0)).ok()
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        let mut t = TextArea::new("");
        t.text = "merhaba  dünya, nasılsın".chars().collect();
        assert_eq!(t.word_right(0), 9);
        assert_eq!(t.word_left(t.text.len()), 16);
        assert_eq!(t.word_left(9), 0);
    }

    #[test]
    fn edit_and_undo() {
        let mut t = TextArea::new("");
        for c in "abc".chars() {
            t.char(c);
        }
        t.char(' ');
        t.char('d');
        assert_eq!(t.text(), "abc d");
        t.anchor = 1;
        t.caret = 3;
        t.insert("X\r\nY");
        assert_eq!(t.text(), "aX\nY d");
        if let Some((s, c)) = t.undo.pop() {
            t.text = s;
            t.caret = c;
        }
        assert_eq!(t.text(), "abc d");
    }
}
