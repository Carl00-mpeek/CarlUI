//! Metin kutusu. Pencere her şeyi kendi çizdiği için hazır denetim yok; yazı
//! WM_CHAR ile gelir. İçerik `Zeroizing<String>` içinde durur (kutu düşünce ya
//! da temizlenince bellekten silinir); yeniden ayırma kopya bırakmasın diye
//! baştan geniş yer ayrılır. İmleç her zaman sondadır: Geri siler,
//! Ctrl+Geri kelimeyi siler, Ctrl+V yapıştırır, Enter gönderir (çok satırlıda
//! yeni satır), Tab sonraki kutuya geçer, Esc vazgeçer.

use super::{clip, P};
use carlui_ui::paint::{Align, R, TEXT};
use windows::Win32::Foundation::HWND;
use zeroize::{Zeroize, Zeroizing};

pub struct TextBox {
    pub text: Zeroizing<String>,
    pub multiline: bool,
}

pub enum Key {
    Changed,
    Submit,
    Cancel,
    Next,
    None,
}

impl TextBox {
    pub fn new(initial: &str, multiline: bool) -> Self {
        let mut s = String::with_capacity(initial.len().max(256) * 2);
        s.push_str(initial);
        Self { text: Zeroizing::new(s), multiline }
    }

    pub fn clear(&mut self) {
        self.text.zeroize();
    }

    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    fn push(&mut self, s: &str) {
        if self.text.len() + s.len() > 16 * 1024 {
            return;
        }
        if self.text.capacity() < self.text.len() + s.len() {
            // Büyütürken eski tamponu kendimiz silelim (String::push bırakırdı).
            let mut bigger = String::with_capacity((self.text.len() + s.len()) * 2);
            bigger.push_str(&self.text);
            self.text.zeroize();
            *self.text = bigger;
        }
        self.text.push_str(s);
    }

    pub fn char(&mut self, hwnd: HWND, c: u32) -> Key {
        match c {
            0x0D if self.multiline && !ctrl_down() => self.push("\n"),
            0x0D | 0x0A => return Key::Submit,
            0x1B => return Key::Cancel,
            0x09 => return Key::Next,
            0x08 => {
                self.text.pop();
            }
            // Ctrl+Geri: son kelimeyi sil.
            0x7F => {
                while self.text.ends_with(' ') {
                    self.text.pop();
                }
                while self.text.chars().last().is_some_and(|c| !c.is_whitespace()) {
                    self.text.pop();
                }
            }
            0x16 => {
                if let Some(t) = clip::paste(hwnd) {
                    let t: Zeroizing<String> = Zeroizing::new(t.chars().filter(|c| !c.is_control() || (self.multiline && *c == '\n')).collect());
                    self.push(if self.multiline { &t } else { t.trim() });
                }
            }
            c if c >= 0x20 => {
                if let Some(ch) = char::from_u32(c) {
                    let mut b = [0u8; 4];
                    self.push(ch.encode_utf8(&mut b));
                }
            }
            _ => return Key::None,
        }
        Key::Changed
    }

    /// Kutunun yüksekliği (çok satırlıda satır sayısına göre büyür).
    pub fn height(&self) -> f32 {
        if self.multiline {
            (self.text.split('\n').count().max(3) as f32) * 20.0 + 16.0
        } else {
            36.0
        }
    }
}

fn ctrl_down() -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0 }
}

/// Çizer. `masked`: her karakter yerine nokta; `caret`: odaktaysa ve yanıp sönme açıksa.
pub fn draw(p: &mut P, r: R, tb: &TextBox, focused: bool, masked: bool, placeholder: &str, caret: bool) {
    let hot = p.hover(r);
    p.round(r, 6.0, 0x000000, if focused { 0.28 } else { 0.18 });
    p.outline(r, 6.0, if focused { p.accent } else { 0xFFFFFF }, if focused { 0.9 } else if hot { 0.22 } else { 0.12 });
    let inner = R::new(r.x + 12.0, r.y, r.w - 24.0, r.h);
    if tb.text.is_empty() && !focused {
        p.text(placeholder, inner, 13.0, false, TEXT, 0.38, Align::Left);
        return;
    }
    if tb.multiline && !masked {
        let mut y = r.y + 8.0;
        let lines: Vec<&str> = tb.text.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            let lr = R::new(inner.x, y, inner.w, 20.0);
            p.text(line, lr, 13.0, false, TEXT, 0.92, Align::Left);
            if caret && focused && i == lines.len() - 1 {
                let x = inner.x + p.fonts.width(line, 13.0, false).min(inner.w);
                p.fill(R::new(x + 1.0, y + 3.0, 1.5, 14.0), TEXT, 0.9);
            }
            y += 20.0;
        }
        return;
    }
    let shown: Zeroizing<String> = if masked { Zeroizing::new("•".repeat(tb.text.chars().count().min(40))) } else { Zeroizing::new(tb.text.replace('\n', " ⏎ ")) };
    // Sığmıyorsa sonunu göster (imleç sondadır).
    let mut start = 0;
    let chars: Vec<(usize, char)> = shown.char_indices().collect();
    while start < chars.len() && p.fonts.width(&shown[chars[start].0..], 13.0, false) > inner.w - 4.0 {
        start += 1;
    }
    let visible = chars.get(start).map(|(i, _)| &shown[*i..]).unwrap_or("");
    p.text(visible, inner, 13.0, false, TEXT, 0.92, Align::Left);
    if caret && focused {
        let x = inner.x + p.fonts.width(visible, 13.0, false);
        p.fill(R::new(x + 1.0, r.y + (r.h - 16.0) / 2.0, 1.5, 16.0), TEXT, 0.9);
    }
}
