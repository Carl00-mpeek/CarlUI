//! Bu modül ayarlar penceresinde "listede olmayan bir arama motorunu nasıl
//! ekleriz?" sorununu çözüyor: pencere tamamen kendi çizdiği için hazır metin
//! kutusu yok; "+" düğmesi sayfanın içinde iki alanlı küçük bir form açar
//! (Ad, Adres). Klavye WM_CHAR ile buraya gelir: yazma, Geri, Tab (alan
//! değiştir), Ctrl+V (yapıştır), Enter (ekle), Esc (vazgeç). Adreste `{q}`
//! yoksa ve "=" ile bitiyorsa sona eklenir; değilse hata gösterilir.

use carlui_core::config::Config;
use std::time::Instant;
use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL, HWND},
    System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalUnlock},
    },
};

const CF_UNICODETEXT: u32 = 13;

pub struct Input {
    pub name: String,
    pub url: String,
    /// 0 ad, 1 adres.
    pub field: u8,
    pub error: Option<String>,
    /// İmleç yanıp sönmesi için.
    pub since: Instant,
}

impl Input {
    pub fn new() -> Self {
        Self { name: String::new(), url: String::new(), field: 0, error: None, since: Instant::now() }
    }

    fn cur(&mut self) -> &mut String {
        if self.field == 0 { &mut self.name } else { &mut self.url }
    }

    /// Bir karakteri işler. Dönüş: Some(true) ekle, Some(false) vazgeç, None sürüyor.
    pub fn char(&mut self, hwnd: HWND, c: u32) -> Option<bool> {
        self.since = Instant::now();
        match c {
            0x0D => return Some(true),
            0x1B => return Some(false),
            0x08 => {
                self.cur().pop();
            }
            0x09 => self.field ^= 1,
            0x16 => {
                if let Some(t) = clipboard(hwnd) {
                    let t: String = t.chars().filter(|c| !c.is_control()).collect();
                    self.cur().push_str(t.trim());
                }
            }
            c if c >= 0x20 => {
                if let Some(ch) = char::from_u32(c) {
                    if self.cur().chars().count() < 400 {
                        self.cur().push(ch);
                    }
                }
            }
            _ => {}
        }
        self.error = None;
        None
    }

    /// Ayara ekler; olmazsa hatayı yazar ve false döner.
    pub fn apply(&mut self, cfg: &mut Config) -> bool {
        let name = self.name.trim().to_string();
        let mut url = self.url.trim().to_string();
        if name.is_empty() {
            self.error = Some("Bir ad yazın (ör. YouTube, Wikipedia).".into());
            self.field = 0;
            return false;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            if url.contains('.') && !url.contains(' ') {
                url = format!("https://{url}");
            } else {
                self.error = Some("Adres http:// ya da https:// ile başlamalı.".into());
                self.field = 1;
                return false;
            }
        }
        if !url.contains("{q}") {
            if url.ends_with('=') {
                url.push_str("{q}");
            } else {
                self.error = Some("Aranan metnin yerine {q} yazın (ör. https://site.com/ara?q={q}).".into());
                self.field = 1;
                return false;
            }
        }
        cfg.launcher.engines.insert(name, url.clone());
        cfg.launcher.web_search = url;
        true
    }
}

fn clipboard(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(hwnd).ok()?;
        let out = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 && n < 4096 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines() {
        let mut c = Config::default();
        let mut i = Input::new();
        i.name = "Wiki".into();
        i.url = "tr.wikipedia.org/w/index.php?search=".into();
        assert!(i.apply(&mut c));
        assert_eq!(c.launcher.web_search, "https://tr.wikipedia.org/w/index.php?search={q}");
        let mut j = Input::new();
        j.name = "X".into();
        j.url = "https://x.com/ara".into();
        assert!(!j.apply(&mut c));
    }
}
