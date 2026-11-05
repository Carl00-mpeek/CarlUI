//! Bu modül "Word belgesini bellekte nasıl tutalım" sorununu çözüyor: belge
//! paragraf listesidir (`rich::Para`); tablolar ve tanımadığımız gövde
//! öğeleri imleç girmeyen "blok" paragraflar olarak aynı listede durur, böylece
//! sıraları ve ham XML'leri korunur. Biçemler (Normal, Başlık 1…) ve sayfa
//! ölçüleri de burada; okuma `docx.rs`, yazma `save.rs`, çizim `layout.rs`.

pub mod docx;
mod edit;
pub mod layout;
pub mod save;
pub mod view;

use crate::{
    rich::{Fmt, PAlign, Para, Run},
    zip::Zip,
};
use std::collections::HashMap;

/// Paragraf biçemi (styles.xml içindeki bir `w:style`).
#[derive(Clone, Debug, Default)]
pub struct Style {
    pub name: String,
    pub based: Option<String>,
    pub fmt: Fmt,
    pub align: Option<PAlign>,
    /// Paragraf öncesi / sonrası boşluk (punto) ve satır aralığı çarpanı.
    pub before: Option<f32>,
    pub after: Option<f32>,
    pub line: Option<f32>,
    /// Başlık düzeyi (1..9), değilse 0.
    pub heading: u8,
}

/// Tablo hücresi: yan yana kaç sütun kapladığı, dolgusu ve paragrafları.
#[derive(Clone, Debug, Default)]
pub struct CellBlock {
    pub span: usize,
    pub fill: Option<u32>,
    pub paras: Vec<Para>,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    /// Sütun genişlikleri (punto).
    pub cols: Vec<f32>,
    pub rows: Vec<Vec<CellBlock>>,
}

/// İmleç girmeyen blok.
#[derive(Clone, Debug)]
pub enum Block {
    Table(Table),
    /// İçindekiler tablosu gibi tanımadığımız öğe: yalnızca metni gösterilir.
    Other(String),
}

/// Sayfa ölçüleri (punto).
#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub w: f32,
    pub h: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Default for Page {
    fn default() -> Self {
        // A4, Word'ün varsayılan 2,5 cm kenar boşlukları.
        Page { w: 595.3, h: 841.9, top: 70.85, bottom: 70.85, left: 70.85, right: 70.85 }
    }
}

pub struct Document {
    /// Özgün paket (yeni belgede iskelet); kaydederken yalnızca değişen parçalar yazılır.
    pub zip: Zip,
    /// Ana belge parçasının yolu (genelde word/document.xml) ve özgün XML'i.
    pub part: String,
    pub src: String,
    /// `w:body` içinin aralığı ve gövde sonundaki `sectPr` ham XML'i.
    pub body: (usize, usize),
    pub sect: String,
    pub paras: Vec<Para>,
    pub blocks: Vec<Block>,
    pub styles: HashMap<String, Style>,
    /// Varsayılan paragraf biçemi kimliği (genelde "Normal").
    pub normal: Option<String>,
    /// docDefaults: yazı boyutu vb., paragraf sonrası boşluk, satır aralığı.
    pub defaults: Style,
    pub font: String,
    pub heading_font: String,
    pub page: Page,
    /// Parça ilişkileri: kimlik → (tür, hedef yol).
    pub rels: HashMap<String, (String, String)>,
    /// Numaralandırma: (numId, düzey) → (biçim, metin şablonu).
    pub numbering: HashMap<(String, u8), (String, String)>,
    /// Kaydederken styles.xml'e eklenecek biçemler (başlık biçemi yoksa).
    pub add_styles: Vec<String>,
}

impl Document {
    /// Biçem zincirinin (basedOn) toplam yazı biçimi; docDefaults en altta.
    pub fn style_fmt(&self, id: Option<&str>) -> Fmt {
        let mut f = Fmt::default();
        let mut cur = id.or(self.normal.as_deref()).map(str::to_string);
        let mut guard = 0;
        while let Some(s) = cur.as_ref().and_then(|c| self.styles.get(c)) {
            f = f.over(&s.fmt);
            cur = s.based.clone();
            guard += 1;
            if guard > 20 {
                break;
            }
        }
        f.over(&self.defaults.fmt)
    }

    /// Zincirdeki ilk değeri bulur (hizalama, boşluklar).
    fn style_get<T>(&self, id: Option<&str>, get: impl Fn(&Style) -> Option<T>) -> Option<T> {
        let mut cur = id.or(self.normal.as_deref()).map(str::to_string);
        let mut guard = 0;
        while let Some(s) = cur.as_ref().and_then(|c| self.styles.get(c)) {
            if let Some(v) = get(s) {
                return Some(v);
            }
            cur = s.based.clone();
            guard += 1;
            if guard > 20 {
                break;
            }
        }
        get(&self.defaults)
    }

    pub fn style_align(&self, id: Option<&str>) -> PAlign {
        self.style_get(id, |s| s.align).unwrap_or_default()
    }

    /// (öncesi, sonrası, satır aralığı) punto / çarpan.
    pub fn style_spacing(&self, id: Option<&str>) -> (f32, f32, f32) {
        (self.style_get(id, |s| s.before).unwrap_or(0.0), self.style_get(id, |s| s.after).unwrap_or(0.0), self.style_get(id, |s| s.line).unwrap_or(1.0))
    }

    pub fn heading_of(&self, id: Option<&str>) -> u8 {
        id.and_then(|i| self.styles.get(i)).map(|s| s.heading).unwrap_or(0)
    }

    /// Başlık düzeyinin biçem kimliği; belgede yoksa eklenir.
    pub fn heading_style(&mut self, level: u8) -> Option<String> {
        if level == 0 {
            return None;
        }
        if let Some((id, _)) = self.styles.iter().find(|(_, s)| s.heading == level) {
            return Some(id.clone());
        }
        let id = format!("Heading{level}");
        let (size, color) = match level {
            1 => (16.0, 0x2F5496),
            2 => (13.0, 0x2F5496),
            _ => (12.0, 0x1F3763),
        };
        let lvl = level - 1;
        let half = (size * 2.0) as u32;
        self.add_styles.push(format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"{normal}\"/><w:next w:val=\"{normal}\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"{lvl}\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"{color:06X}\"/><w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/></w:rPr></w:style>",
            normal = self.normal.clone().unwrap_or_else(|| "Normal".into())
        ));
        self.styles.insert(
            id.clone(),
            Style { name: format!("heading {level}"), based: self.normal.clone(), fmt: Fmt { b: Some(true), size: Some(size), color: Some(color), ..Default::default() }, before: Some(12.0), after: Some(4.0), heading: level, ..Default::default() },
        );
        Some(id)
    }

    /// Paragrafların madde / numara işaretlerini yeniden hesaplar.
    pub fn renumber(&mut self) {
        let mut counters: HashMap<(String, u8), u32> = HashMap::new();
        for p in &mut self.paras {
            if p.block.is_some() {
                continue;
            }
            let Some(num) = attr_in(&p.ppr, "w:numId") else {
                p.bullet = None;
                continue;
            };
            if num == "0" {
                p.bullet = None;
                continue;
            }
            let lvl = attr_in(&p.ppr, "w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0u8);
            p.level = lvl;
            // Üst düzeye dönünce alt düzeylerin sayacı sıfırlanır.
            counters.retain(|(n, l), _| *n != num || *l <= lvl);
            let n = counters.entry((num.clone(), lvl)).or_insert(0);
            *n += 1;
            let (fmt, text) = self.numbering.get(&(num.clone(), lvl)).cloned().unwrap_or_else(|| ("bullet".into(), "•".into()));
            p.bullet = Some(match fmt.as_str() {
                "bullet" => bullet_char(&text),
                "none" => String::new(),
                _ => text.replace(&format!("%{}", lvl + 1), &number(*n, &fmt)).split('%').next().unwrap_or("").to_string(),
            });
        }
    }

    pub fn blank() -> Self {
        save::blank()
    }
}

/// Wingdings/Symbol madde imleri (özel kullanım alanı) yerine okunur bir işaret.
fn bullet_char(t: &str) -> String {
    match t.chars().next() {
        Some('\u{F0B7}') | Some('\u{F0A7}') | None => "•".into(),
        Some('o') => "◦".into(),
        Some(c) if ('\u{F000}'..='\u{F0FF}').contains(&c) => "▪".into(),
        Some(c) => c.to_string(),
    }
}

fn number(n: u32, fmt: &str) -> String {
    match fmt {
        "lowerLetter" => char::from_u32('a' as u32 + (n - 1) % 26).map(String::from).unwrap_or_default(),
        "upperLetter" => char::from_u32('A' as u32 + (n - 1) % 26).map(String::from).unwrap_or_default(),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        _ => n.to_string(),
    }
}

fn roman(mut n: u32) -> String {
    let t = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Ham XML içinde `<tag w:val="…"` değerini bulur (pPr içindeki numId gibi).
pub fn attr_in(xml: &str, tag: &str) -> Option<String> {
    let i = xml.find(&format!("<{tag} "))?;
    let rest = &xml[i..];
    let end = rest.find('>')?;
    let open = &rest[..end];
    let v = open.find("w:val=\"")? + 7;
    let e = open[v..].find('"')?;
    Some(open[v..v + e].to_string())
}

/// Düz metinden paragraf (yeni belge, testler).
pub fn plain_para(text: &str) -> Para {
    Para { runs: if text.is_empty() { Vec::new() } else { vec![Run::plain(text, Fmt::default())] }, dirty: true, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_and_attrs() {
        assert_eq!(attr_in("<w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"3\"/></w:numPr></w:pPr>", "w:numId").as_deref(), Some("3"));
        assert_eq!(roman(14), "XIV");
        assert_eq!(number(2, "lowerLetter"), "b");
        let mut d = Document::blank();
        d.paras = vec![plain_para("a"), plain_para("b")];
        for p in &mut d.paras {
            p.ppr = "<w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"7\"/></w:numPr></w:pPr>".into();
        }
        d.numbering.insert(("7".into(), 0), ("decimal".into(), "%1.".into()));
        d.renumber();
        assert_eq!(d.paras[1].bullet.as_deref(), Some("2."));
        let h = d.heading_style(2).unwrap();
        assert_eq!(d.heading_of(Some(&h)), 2);
        assert_eq!(d.style_fmt(Some(&h)).b, Some(true));
    }
}
