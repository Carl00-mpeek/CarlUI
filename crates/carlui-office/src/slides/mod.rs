//! Bu modül "PowerPoint sunusunu bellekte nasıl tutalım" sorununu çözüyor:
//! sunu (Deck) → slaytlar → şekiller. PowerPoint'te bir slayttaki yer
//! tutucunun konumu ve yazı boyutu çoğu zaman slaytta yazmaz; slayt
//! düzeninden (layout), o da asıldan (master) devralır. Bu yüzden asıl ve
//! düzenler de "katman" olarak okunur, slayt şekilleri onlardan tamamlanır.
//! Tema renkleri (tx1, accent1…) ve parlaklık değiştiricileri de burada.
//! Birimler: konum ve boyut punto (1 pt = 12700 EMU).

pub mod pptx;
pub mod render;
pub mod save;
pub mod table;
pub mod view;
mod mouse;
mod present;
mod edit;
#[cfg(test)]
mod roundtrip;

use crate::{
    rich::{Fmt, PAlign, Para},
    xml::Node,
    zip::Zip,
};
use std::collections::HashMap;

pub const EMU_PT: f32 = 12700.0;

/// Dokuz liste düzeyinin varsayılanları (lstStyle / txStyles).
#[derive(Clone, Default, Debug)]
pub struct Levels {
    pub fmt: [Fmt; 9],
    /// Madde işareti: None = bu katman söylemedi; Some(None) = işaretsiz;
    /// Some(Some('#')) = numaralı.
    pub bullet: [Option<Option<char>>; 9],
    pub algn: [Option<PAlign>; 9],
    /// Sol boşluk (pt).
    pub mar: [Option<f32>; 9],
}

impl Levels {
    /// `o`'da yazanlar bunun üstüne biner.
    pub fn merge(&mut self, o: &Levels) {
        for i in 0..9 {
            self.fmt[i] = o.fmt[i].over(&self.fmt[i]);
            if o.bullet[i].is_some() {
                self.bullet[i] = o.bullet[i];
            }
            if o.algn[i].is_some() {
                self.algn[i] = o.algn[i];
            }
            if o.mar[i].is_some() {
                self.mar[i] = o.mar[i];
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Metin kutusu / yer tutucu / dolgulu şekil.
    Text,
    /// Resim: zip içindeki yol.
    Pic(String),
    /// Tablo (hücreler, biçimler, sütun genişlikleri).
    Table(Box<table::Table>),
    /// Grafik, SmartArt vb.: yalnızca adı gösterilir.
    Frame(String),
}

#[derive(Clone, Debug, Default)]
pub struct Ph {
    /// Normalleştirilmiş tür: title, body, dt, ftr, sldNum, pic…
    pub ty: String,
    pub idx: Option<u32>,
    /// Özgün `p:ph` öğesi (yeni slayt üretirken kopyalanır).
    pub raw: String,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: Kind,
    pub fill: Option<u32>,
    pub line: Option<(u32, f32)>,
    /// prstGeom: ellipse, roundRect, rect…
    pub geom: String,
    pub paras: Vec<Para>,
    /// Paragrafların (ilk a:p başı, son a:p sonu) aralığı.
    pub body: Option<(usize, usize)>,
    /// Bütün öğenin aralığı (silmek için).
    pub span: (usize, usize),
    /// xfrm öğesinin aralığı; yoksa spPr'nin (araya eklemek için).
    pub xf_span: Option<(usize, usize)>,
    pub sppr_span: Option<(usize, usize)>,
    pub lv: Levels,
    /// 0 üst, 1 orta, 2 alt.
    pub anchor: u8,
    /// İç boşluklar (sol, üst, sağ, alt) pt.
    pub ins: [f32; 4],
    /// normAutofit fontScale (1 = %100).
    pub scale: f32,
    /// Metin kutusu metne göre uzar (spAutoFit).
    pub grow: bool,
    pub ph: Option<Ph>,
    pub title: bool,
    /// Grup içinde (taşınamaz; koordinatlar dönüştürülmüş).
    pub grouped: bool,
    /// Kullanıcı taşıdı / boyutlandırdı.
    pub moved: bool,
    /// Kaydederken gövdesi yeniden yazılacak.
    pub dirty: bool,
    /// Metin rengi `p:style` fontRef'ten (dolgulu şekillerde genelde beyaz).
    pub font_color: Option<u32>,
    pub wrap: bool,
}

impl Default for Shape {
    fn default() -> Self {
        Shape {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            kind: Kind::Text,
            fill: None,
            line: None,
            geom: String::new(),
            paras: Vec::new(),
            body: None,
            span: (0, 0),
            xf_span: None,
            sppr_span: None,
            lv: Levels::default(),
            anchor: 0,
            ins: [7.2, 3.6, 7.2, 3.6],
            scale: 1.0,
            grow: false,
            ph: None,
            title: false,
            grouped: false,
            moved: false,
            dirty: false,
            font_color: None,
            wrap: true,
        }
    }
}

impl Shape {
    pub fn has_text(&self) -> bool {
        self.kind == Kind::Text && self.body.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.paras.iter().all(|p| p.text().trim().is_empty())
    }

    /// Madde işaretlerini (numaralar dahil) yeniden hesaplar.
    pub fn renumber(&mut self) {
        let mut count = [0u32; 9];
        for p in &mut self.paras {
            let l = (p.level as usize).min(8);
            let own = own_bullet(&p.ppr);
            let b = own.or(self.lv.bullet[l]).flatten();
            for c in count.iter_mut().skip(l + 1) {
                *c = 0;
            }
            p.bullet = match b {
                _ if p.is_empty() => None,
                Some('#') => {
                    count[l] += 1;
                    Some(format!("{}.", count[l]))
                }
                Some(c) => {
                    count[l] = 0;
                    Some(c.to_string())
                }
                None => {
                    count[l] = 0;
                    None
                }
            };
        }
    }
}

/// Paragrafın kendi `a:pPr`'sindeki madde ayarı.
pub fn own_bullet(ppr: &str) -> Option<Option<char>> {
    if !ppr.contains("bu") {
        return None;
    }
    let n = crate::xml::parse(ppr).ok()?;
    bullet_of(&n)
}

pub fn bullet_of(ppr: &Node) -> Option<Option<char>> {
    for c in &ppr.children {
        match c.local() {
            "buNone" => return Some(None),
            "buAutoNum" => return Some(Some('#')),
            "buChar" => return Some(Some(c.attr("char").and_then(|s| s.chars().next()).map(fix_bullet).unwrap_or('•'))),
            "buBlip" => return Some(Some('•')),
            _ => {}
        }
    }
    None
}

/// Wingdings/Symbol yazı tipindeki işaretler Unicode'da başka yerde.
fn fix_bullet(c: char) -> char {
    match c {
        'Ø' => '➢',
        'ü' => '✓',
        '§' => '▪',
        'q' | 'n' => '■',
        'l' => '●',
        c if ('\u{F000}'..='\u{F0FF}').contains(&c) => '•',
        c => c,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Bg {
    #[default]
    None,
    Solid(u32),
    Image(String),
}

/// Asıl ya da düzen: üstüne slayt çizilen sabit şekiller ve yer tutucular.
#[derive(Clone, Default)]
pub struct Layer {
    pub shapes: Vec<Shape>,
    pub bg: Bg,
    /// Asıldaki sabit şekiller gösterilsin mi (showMasterSp).
    pub show_master: bool,
    /// Düzenin asılı (asıl için boş).
    pub master: String,
    /// Düzen türü: title, obj, blank…
    pub ty: String,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct Theme {
    pub colors: HashMap<String, u32>,
    pub major: String,
    pub minor: String,
}

/// Asılın tema ve metin biçemleri.
#[derive(Clone, Default)]
pub struct Master {
    pub theme: Theme,
    pub title: Levels,
    pub body: Levels,
    pub other: Levels,
}

#[derive(Clone)]
pub struct Slide {
    pub part: String,
    pub src: String,
    pub shapes: Vec<Shape>,
    pub bg: Bg,
    pub layout: String,
    pub show_master: bool,
    /// İlişkiler: kimlik → (tür, yol).
    pub rels: HashMap<String, (String, String)>,
    /// Yeni slaytın ilişki dosyası (kaydederken yazılır).
    pub new_rels: Option<String>,
    /// presentation.xml'deki kimlik ve ilişki (yeni slaytta 0 / boş).
    pub id: u32,
    pub rid: String,
    /// spTree'nin kapanış etiketinin yeri (yeni şekil buraya eklenir).
    pub tree_end: usize,
    pub hidden: bool,
}

impl Slide {
    pub fn dirty(&self) -> bool {
        self.shapes.iter().any(|s| s.dirty || s.moved)
    }
}

pub struct Deck {
    pub zip: Zip,
    /// Slayt boyutu (pt).
    pub w: f32,
    pub h: f32,
    pub slides: Vec<Slide>,
    pub layers: HashMap<String, Layer>,
    pub masters: HashMap<String, Master>,
    /// Yer tutucu olmayan metin kutularının biçemi (presentation.xml).
    pub default_text: Levels,
    /// Kaydedilmiş haldeki slaytlar: (yol, kimlik, ilişki).
    pub orig: Vec<(String, u32, String)>,
}

impl Deck {
    pub fn master_of(&self, layout: &str) -> Option<&Master> {
        let m = &self.layers.get(layout)?.master;
        self.masters.get(m)
    }

    pub fn theme_for(&self, layout: &str) -> Theme {
        self.master_of(layout).map(|m| m.theme.clone()).unwrap_or_else(|| self.masters.values().next().map(|m| m.theme.clone()).unwrap_or_default())
    }

    /// Slaytın altındaki sabit şekiller: önce asıl, sonra düzen.
    pub fn statics(&self, s: &Slide) -> Vec<&Shape> {
        let mut out = Vec::new();
        let Some(lay) = self.layers.get(&s.layout) else { return out };
        if s.show_master && lay.show_master {
            if let Some(m) = self.layers.get(&lay.master) {
                out.extend(m.shapes.iter().filter(|x| x.ph.is_none()));
            }
        }
        out.extend(lay.shapes.iter().filter(|x| x.ph.is_none()));
        out
    }

    /// Arka plan: slayt → düzen → asıl.
    pub fn bg(&self, s: &Slide) -> Bg {
        if s.bg != Bg::None {
            return s.bg.clone();
        }
        if let Some(l) = self.layers.get(&s.layout) {
            if l.bg != Bg::None {
                return l.bg.clone();
            }
            if let Some(m) = self.layers.get(&l.master) {
                return m.bg.clone();
            }
        }
        Bg::None
    }

    pub fn structure_changed(&self) -> bool {
        self.orig.len() != self.slides.len() || self.orig.iter().zip(&self.slides).any(|(o, s)| o.0 != s.part)
    }
}

// ---- Renkler ----

fn hsl(c: u32) -> (f32, f32, f32) {
    let (r, g, b) = (((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if (mx - mn).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> u32 {
    let f = |p: f32, q: f32, mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (f(p, q, h + 1.0 / 3.0), f(p, q, h), f(p, q, h - 1.0 / 3.0))
    };
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

/// Renk öğesi (srgbClr, schemeClr…) → 0xRRGGBB; değiştiriciler (lumMod,
/// lumOff, tint, shade) uygulanır.
pub fn color_el(n: &Node, th: &Theme) -> Option<u32> {
    let base = match n.local() {
        "srgbClr" => crate::doc::docx::hex(n.attr("val")?)?,
        "sysClr" => n.attr("lastClr").and_then(crate::doc::docx::hex).unwrap_or(if n.attr("val") == Some("window") { 0xFFFFFF } else { 0 }),
        "schemeClr" => {
            let v = n.attr("val")?;
            let key = match v {
                "tx1" => "dk1",
                "bg1" => "lt1",
                "tx2" => "dk2",
                "bg2" => "lt2",
                "phClr" => "accent1",
                v => v,
            };
            *th.colors.get(v).or_else(|| th.colors.get(key))?
        }
        "prstClr" => match n.attr("val")? {
            "black" => 0,
            "white" => 0xFFFFFF,
            "red" => 0xFF0000,
            "green" => 0x008000,
            "blue" => 0x0000FF,
            "yellow" => 0xFFFF00,
            "gray" | "grey" => 0x808080,
            _ => 0,
        },
        "scrgbClr" => {
            let p = |k: &str| n.attr(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / 100_000.0;
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
            (c(p("r")) << 16) | (c(p("g")) << 8) | c(p("b"))
        }
        _ => return None,
    };
    let mut c = base;
    for m in &n.children {
        let v = m.attr("val").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100_000.0) / 100_000.0;
        match m.local() {
            "lumMod" => {
                let (h, s, l) = hsl(c);
                c = from_hsl(h, s, l * v);
            }
            "lumOff" => {
                let (h, s, l) = hsl(c);
                c = from_hsl(h, s, l + v);
            }
            "tint" => {
                let ch = |x: u32| (x as f32 + (255.0 - x as f32) * (1.0 - v)) as u32;
                c = (ch((c >> 16) & 255) << 16) | (ch((c >> 8) & 255) << 8) | ch(c & 255);
            }
            "shade" => {
                let ch = |x: u32| (x as f32 * v) as u32;
                c = (ch((c >> 16) & 255) << 16) | (ch((c >> 8) & 255) << 8) | ch(c & 255);
            }
            _ => {}
        }
    }
    Some(c)
}

/// `solidFill` (ya da rengi doğrudan içeren öğe) → renk.
pub fn fill_color(n: &Node, th: &Theme) -> Option<u32> {
    n.children.iter().find_map(|c| color_el(c, th))
}

pub fn emu(v: Option<&str>) -> Option<f32> {
    v.and_then(|s| s.parse::<f64>().ok()).map(|e| (e / EMU_PT as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lum() {
        let th = Theme { colors: [("dk1".to_string(), 0x000000u32), ("accent1".to_string(), 0x4472C4)].into_iter().collect(), ..Default::default() };
        let x = crate::xml::parse(r#"<a:schemeClr val="tx1"><a:lumMod val="75000"/><a:lumOff val="25000"/></a:schemeClr>"#).unwrap();
        assert_eq!(color_el(&x, &th), Some(0x404040));
        let a = crate::xml::parse(r#"<a:schemeClr val="accent1"/>"#).unwrap();
        assert_eq!(color_el(&a, &th), Some(0x4472C4));
    }
}
