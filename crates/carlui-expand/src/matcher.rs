//! Bu modül "yazılan son harflerden bir kısaltma oluştu mu?" sorununu
//! çözüyor: klavye kancasının topladığı karakterler kısa bir tamponda tutulur,
//! her yeni karakterde tamponun sonu kısaltmalarla karşılaştırılır. Ayrıca
//! açılımdaki `{tarih}`, `{saat}`, `{pano}` yer tutucularını doldurur. Saf
//! mantık; Windows'a dokunmaz, test edilir.

use carlui_core::config::Snippet;

/// Tamponda tutulan en fazla karakter (en uzun kısaltmadan uzun olmalı).
const CAP: usize = 64;

#[derive(Default)]
pub struct Buffer {
    chars: Vec<char>,
}

impl Buffer {
    pub fn push(&mut self, c: char) {
        if self.chars.len() >= CAP {
            self.chars.drain(..CAP / 2);
        }
        self.chars.push(c);
    }

    pub fn backspace(&mut self) {
        self.chars.pop();
    }

    pub fn clear(&mut self) {
        self.chars.clear();
    }

    /// Tamponun sonunda biten kısaltma (en uzunu). Kısaltma harf ya da rakamla
    /// başlıyorsa öncesinde başka bir harf/rakam olmamalı ki "kalem" içindeki
    /// "lem" tetiklenmesin. Harflerde büyük/küçük ayrımı yapılmaz.
    pub fn find(&self, keys: &[Vec<char>]) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (i, k) in keys.iter().enumerate() {
            let n = k.len();
            if n == 0 || n > self.chars.len() {
                continue;
            }
            let tail = &self.chars[self.chars.len() - n..];
            if !tail.iter().zip(k).all(|(a, b)| same(*a, *b)) {
                continue;
            }
            if k[0].is_alphanumeric() && self.chars.len() > n && self.chars[self.chars.len() - n - 1].is_alphanumeric() {
                continue;
            }
            if best.map_or(true, |(_, l)| n > l) {
                best = Some((i, n));
            }
        }
        best.map(|(i, _)| i)
    }
}

fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Kısaltmaları karşılaştırmaya hazır biçime getirir.
pub fn keys(snippets: &[Snippet]) -> Vec<Vec<char>> {
    snippets.iter().map(|s| s.key.chars().collect()).collect()
}

/// Açılım metnindeki yer tutucuları doldurur. `clip` yalnızca `{pano}`
/// geçiyorsa çağrılır. Satır sonları `\n`'e indirgenir.
pub fn expand(text: &str, date: &str, time: &str, clip: impl FnOnce() -> Option<String>) -> String {
    let mut out = text.replace("\r\n", "\n");
    for k in ["{tarih}", "{date}"] {
        out = out.replace(k, date);
    }
    for k in ["{saat}", "{time}"] {
        out = out.replace(k, time);
    }
    if out.contains("{pano}") || out.contains("{clipboard}") {
        let c = clip().unwrap_or_default().replace("\r\n", "\n");
        out = out.replace("{pano}", &c).replace("{clipboard}", &c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        let mut b = Buffer::default();
        s.chars().for_each(|c| b.push(c));
        b
    }

    fn ks(v: &[&str]) -> Vec<Vec<char>> {
        v.iter().map(|s| s.chars().collect()).collect()
    }

    #[test]
    fn matching() {
        let k = ks(&[";t", ";tarih", "adr"]);
        assert_eq!(buf("bugün ;tarih").find(&k), Some(1));
        assert_eq!(buf(";t").find(&k), Some(0));
        assert_eq!(buf(";Tarih").find(&k), Some(1));
        assert_eq!(buf("x adr").find(&k), Some(2));
        assert_eq!(buf("madr").find(&k), None);
        assert_eq!(buf("adr").find(&k), Some(2));
        let mut b = buf(";tarihx");
        assert_eq!(b.find(&k), None);
        b.backspace();
        assert_eq!(b.find(&k), Some(1));
        b.clear();
        assert_eq!(b.find(&k), None);
    }

    #[test]
    fn cap() {
        let mut b = Buffer::default();
        for _ in 0..200 {
            b.push('a');
        }
        b.push(';');
        b.push('t');
        assert_eq!(b.find(&ks(&[";t"])), Some(0));
        assert!(b.chars.len() <= CAP);
    }

    #[test]
    fn placeholders() {
        assert_eq!(expand("{tarih} {saat}\r\nx", "05.11.2026", "14:30", || panic!()), "05.11.2026 14:30\nx");
        assert_eq!(expand("[{pano}]", "", "", || Some("a\r\nb".into())), "[a\nb]");
        assert_eq!(expand("{clipboard}", "", "", || None), "");
    }
}
