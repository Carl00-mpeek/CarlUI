//! Bu modül "araç pencerelerinde yazı kutusu" sorununu çözüyor: tek satırlı,
//! imleci sonda duran basit bir alan. Yazma, Backspace (Ctrl ile kelime),
//! Ctrl+V yapıştırma ve Ctrl+A ile temizleme desteklenir. Çizimi `Painter`
//! üstündedir; odaklı alan vurgu renginde çerçevelenir.

use carlui_ui::paint::{Align, Painter, R, TEXT};

#[derive(Default, Clone)]
pub struct Field {
    pub text: String,
}

impl Field {
    pub fn new(s: &str) -> Self {
        Self { text: s.to_string() }
    }

    pub fn char(&mut self, c: char) {
        if self.text.chars().count() < 260 {
            self.text.push(c);
        }
    }

    /// Klavye tuşu; `true` = işlendi.
    pub fn key(&mut self, vk: u16, ctrl: bool) -> bool {
        match (vk, ctrl) {
            (0x08, false) => {
                self.text.pop();
            }
            (0x08, true) => {
                let t = self.text.trim_end_matches(|c: char| !c.is_alphanumeric());
                let cut = t.rfind(|c: char| !c.is_alphanumeric()).map(|i| i + t[i..].chars().next().map_or(1, char::len_utf8)).unwrap_or(0);
                self.text.truncate(cut);
            }
            (0x41, true) => self.text.clear(),
            (0x56, true) => {
                let paste = carlui_clip::capture::plain_text().unwrap_or_default();
                for c in paste.chars().filter(|c| !c.is_control()) {
                    self.char(c);
                }
            }
            _ => return false,
        }
        true
    }

    pub fn paint<A: Clone>(&self, p: &mut Painter<A>, r: R, focused: bool, placeholder: &str, act: A) {
        let hot = p.hover(r);
        p.round(r, 6.0, 0x000000, 0.22);
        if focused {
            p.outline(r, 6.0, p.accent, 0.9);
        } else {
            p.outline(r, 6.0, 0xFFFFFF, if hot { 0.22 } else { 0.12 });
        }
        let inner = R::new(r.x + 10.0, r.y, r.w - 20.0, r.h);
        if self.text.is_empty() {
            p.text(placeholder, inner, 13.0, false, TEXT, 0.35, Align::Left);
        }
        let tw = p.fonts.width(&self.text, 13.0, false);
        // Taşan yazı sağa yaslanır: imleç hep görünür.
        let align = if tw > inner.w - 4.0 { Align::Right } else { Align::Left };
        p.text(&self.text, inner, 13.0, false, TEXT, 0.92, align);
        if focused {
            let cx = if align == Align::Right { inner.right() } else { inner.x + tw + 1.0 };
            p.fill(R::new(cx, r.y + r.h / 2.0 - 8.0, 1.2, 16.0), TEXT, 0.85);
        }
        p.hit(r, act);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_delete() {
        let mut f = Field::new("tatil fotoğraf ");
        f.key(0x08, true);
        assert_eq!(f.text, "tatil ");
        f.key(0x08, true);
        assert_eq!(f.text, "");
        let mut f = Field::new("ab");
        f.key(0x08, false);
        assert_eq!(f.text, "a");
    }
}
