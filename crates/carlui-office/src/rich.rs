//! Bu modül "Word paragrafı ile PowerPoint metin kutusu paragrafı aynı şekilde
//! düzenlensin" sorununu çözüyor: ortak paragraf modeli. Paragraf, biçimli
//! parçalardan (run) oluşur. Konumlar karakter (char) sayısıdır. Köprü,
//! alan kodu, resim gibi bizim düzenlemediğimiz parçalar "atom" olarak
//! tutulur: ham XML'leri olduğu gibi yazılır, metinleri yalnızca gösterilir
//! ve tek parça halinde silinebilir.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fmt {
    pub b: Option<bool>,
    pub i: Option<bool>,
    pub u: Option<bool>,
    pub strike: Option<bool>,
    /// Punto.
    pub size: Option<f32>,
    /// 0xRRGGBB.
    pub color: Option<u32>,
}

impl Fmt {
    /// `self` boş alanlarını `base` ile doldurur.
    pub fn over(&self, base: &Fmt) -> Fmt {
        Fmt {
            b: self.b.or(base.b),
            i: self.i.or(base.i),
            u: self.u.or(base.u),
            strike: self.strike.or(base.strike),
            size: self.size.or(base.size),
            color: self.color.or(base.color),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, Default)]
pub struct Run {
    pub text: String,
    pub fmt: Fmt,
    /// Dosyadan okunan biçim (değişip değişmediğini anlamak için).
    pub orig: Fmt,
    /// Özgün `rPr` öğesinin ham XML'i (yoksa boş).
    pub rpr: String,
    /// Düzenlenemeyen parça: ham XML.
    pub atom: Option<String>,
    /// Atom bir resimse zip içindeki yolu ve boyutu (EMU).
    pub image: Option<(String, i64, i64)>,
}

impl Run {
    pub fn plain(text: &str, fmt: Fmt) -> Self {
        Run { text: text.to_string(), fmt, orig: fmt, ..Default::default() }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Para {
    pub runs: Vec<Run>,
    pub align: Option<PAlign>,
    pub orig_align: Option<PAlign>,
    /// Paragraf biçemi (Word `pStyle` kimliği).
    pub style: Option<String>,
    pub orig_style: Option<String>,
    /// Liste/madde düzeyi (PowerPoint `lvl`).
    pub level: u8,
    /// Madde işareti metni (gösterim için; "•", "1." …).
    pub bullet: Option<String>,
    /// Özgün açılış etiketi (öznitelikleriyle).
    pub open: String,
    /// Özgün paragraf özellikleri (`pPr`) ham XML'i.
    pub ppr: String,
    /// PowerPoint `endParaRPr` ham XML'i.
    pub end: String,
    /// Dosyadaki aralık; yeni paragrafta None.
    pub span: Option<(usize, usize)>,
    pub dirty: bool,
    /// Metin olmayan blok (tablo vb.): belgenin ek blok listesindeki dizini.
    /// Böyle paragraflara imleç girmez; ham XML'i `open` içinde tutulur.
    pub block: Option<usize>,
}

impl Para {
    pub fn len(&self) -> usize {
        self.runs.iter().map(Run::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Paragrafta yalnızca resim(ler) varsa onlar.
    pub fn images_only(&self) -> Vec<(String, i64, i64)> {
        let imgs: Vec<_> = self.runs.iter().filter_map(|r| r.image.clone()).collect();
        if imgs.is_empty() || self.runs.iter().any(|r| r.image.is_none() && !r.text.trim().is_empty()) {
            return Vec::new();
        }
        imgs
    }

    /// Yeni (boş) bir paragraf; özellikleri bundan kopyalar.
    pub fn sibling(&self) -> Para {
        Para {
            runs: Vec::new(),
            align: self.align,
            orig_align: None,
            style: self.style.clone(),
            orig_style: None,
            level: self.level,
            bullet: self.bullet.clone(),
            open: String::new(),
            ppr: self.ppr.clone(),
            end: self.end.clone(),
            span: None,
            dirty: true,
            block: None,
        }
    }

    /// `pos` karakter konumunu bölen run'ı ikiye ayırır; sınırın run dizinini verir.
    fn cut(&mut self, pos: usize) -> usize {
        let mut acc = 0;
        for i in 0..self.runs.len() {
            let l = self.runs[i].len();
            if pos == acc {
                return i;
            }
            if pos < acc + l {
                if self.runs[i].atom.is_some() {
                    // Atom bölünmez: sonrasına git.
                    return i + 1;
                }
                let at = self.runs[i].text.char_indices().nth(pos - acc).map(|(b, _)| b).unwrap_or(self.runs[i].text.len());
                let mut tail = self.runs[i].clone();
                tail.text = self.runs[i].text[at..].to_string();
                self.runs[i].text.truncate(at);
                self.runs.insert(i + 1, tail);
                return i + 1;
            }
            acc += l;
        }
        self.runs.len()
    }

    /// Konumdaki yazım biçimi (bir önceki karakterinki).
    pub fn fmt_at(&self, pos: usize) -> Fmt {
        let mut acc = 0;
        let mut last = None;
        for r in &self.runs {
            if r.atom.is_none() {
                last = Some(r.fmt);
            }
            acc += r.len();
            if acc >= pos && last.is_some() {
                break;
            }
        }
        last.unwrap_or_default()
    }

    pub fn insert(&mut self, pos: usize, s: &str, fmt: Fmt) {
        if s.is_empty() {
            return;
        }
        self.dirty = true;
        // Önceki metin run'ına ekle (biçim aynıysa) — run sayısı şişmesin.
        let mut acc = 0;
        for r in self.runs.iter_mut() {
            let l = r.len();
            if r.atom.is_none() && pos > acc && pos <= acc + l && r.fmt == fmt {
                let at = r.text.char_indices().nth(pos - acc).map(|(b, _)| b).unwrap_or(r.text.len());
                r.text.insert_str(at, s);
                return;
            }
            acc += l;
        }
        let i = self.cut(pos);
        let mut run = Run::plain(s, fmt);
        // Biçim özgün rPr'den türesin (yazı tipi vb. korunsun).
        if let Some(near) = self.runs.get(i.saturating_sub(1)).or(self.runs.get(i)).filter(|r| r.atom.is_none()) {
            run.rpr = near.rpr.clone();
            run.orig = near.orig;
        }
        self.runs.insert(i, run);
    }

    pub fn delete(&mut self, a: usize, b: usize) {
        if b <= a {
            return;
        }
        self.dirty = true;
        // Atomun bir kısmı seçiliyse atomun tamamı gider.
        let (mut a, mut b) = (a, b);
        let mut acc = 0;
        for r in &self.runs {
            let l = r.len();
            if r.atom.is_some() && l > 0 {
                if a > acc && a < acc + l {
                    a = acc;
                }
                if b > acc && b < acc + l {
                    b = acc + l;
                }
            }
            acc += l;
        }
        let i = self.cut(a);
        let j = self.cut(b);
        self.runs.drain(i..j);
        self.runs.retain(|r| r.atom.is_some() || !r.text.is_empty());
    }

    /// `pos` noktasında böler; ikinci yarıyı döndürür.
    pub fn split(&mut self, pos: usize) -> Para {
        self.dirty = true;
        let i = self.cut(pos);
        let mut next = self.sibling();
        next.runs = self.runs.split_off(i);
        next
    }

    pub fn merge(&mut self, other: Para) {
        self.dirty = true;
        self.runs.extend(other.runs);
    }

    pub fn set_fmt(&mut self, a: usize, b: usize, f: impl Fn(&mut Fmt)) {
        self.dirty = true;
        let i = self.cut(a);
        let j = self.cut(b);
        for r in &mut self.runs[i..j] {
            if r.atom.is_none() {
                f(&mut r.fmt);
            }
        }
    }

    /// Aralığın tamamı bu özellikte mi (kalın düğmesi açık mı görünsün).
    pub fn all(&self, a: usize, b: usize, base: &Fmt, get: impl Fn(&Fmt) -> bool) -> bool {
        let mut acc = 0;
        let mut any = false;
        for r in &self.runs {
            let l = r.len();
            let (s, e) = (acc, acc + l);
            acc = e;
            if r.atom.is_some() || l == 0 {
                continue;
            }
            let overlap = if a == b { s < a && a <= e || a == 0 && s == 0 } else { s < b && e > a };
            if overlap {
                any = true;
                if !get(&r.fmt.over(base)) {
                    return false;
                }
            }
        }
        any || get(base)
    }
}

/// Paragraflar arası konum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub p: usize,
    pub c: usize,
}

/// Paragraf listesini düzenleyen imleç + seçim.
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    pub at: Pos,
    pub anchor: Pos,
}

impl Cursor {
    pub fn range(&self) -> (Pos, Pos) {
        if self.at <= self.anchor {
            (self.at, self.anchor)
        } else {
            (self.anchor, self.at)
        }
    }

    pub fn has_sel(&self) -> bool {
        self.at != self.anchor
    }

    pub fn set(&mut self, p: Pos, extend: bool) {
        self.at = p;
        if !extend {
            self.anchor = p;
        }
    }
}

/// Seçimi siler; imleci başa koyar.
pub fn delete_sel(paras: &mut Vec<Para>, cur: &mut Cursor) {
    let (a, b) = cur.range();
    if a == b {
        return;
    }
    if a.p == b.p {
        paras[a.p].delete(a.c, b.c);
    } else {
        let tail = {
            let last = &mut paras[b.p];
            last.delete(0, b.c);
            std::mem::take(last)
        };
        paras[a.p].delete(a.c, usize::MAX / 2);
        paras[a.p].merge(tail);
        paras.drain(a.p + 1..=b.p);
    }
    cur.set(a, false);
}

pub fn type_text(paras: &mut Vec<Para>, cur: &mut Cursor, s: &str) {
    delete_sel(paras, cur);
    let mut first = true;
    for line in s.split('\n') {
        if !first {
            enter(paras, cur);
        }
        first = false;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let at = cur.at;
        let fmt = paras[at.p].fmt_at(at.c);
        paras[at.p].insert(at.c, line, fmt);
        cur.set(Pos { p: at.p, c: at.c + line.chars().count() }, false);
    }
}

pub fn enter(paras: &mut Vec<Para>, cur: &mut Cursor) {
    delete_sel(paras, cur);
    let at = cur.at;
    let next = paras[at.p].split(at.c);
    paras.insert(at.p + 1, next);
    cur.set(Pos { p: at.p + 1, c: 0 }, false);
}

pub fn backspace(paras: &mut Vec<Para>, cur: &mut Cursor) {
    if cur.has_sel() {
        return delete_sel(paras, cur);
    }
    let at = cur.at;
    if at.c > 0 {
        paras[at.p].delete(at.c - 1, at.c);
        let c = at.c.saturating_sub(1).min(paras[at.p].len());
        cur.set(Pos { p: at.p, c }, false);
    } else if at.p > 0 {
        let p = paras.remove(at.p);
        let c = paras[at.p - 1].len();
        paras[at.p - 1].merge(p);
        cur.set(Pos { p: at.p - 1, c }, false);
    }
}

pub fn delete_fwd(paras: &mut Vec<Para>, cur: &mut Cursor) {
    if cur.has_sel() {
        return delete_sel(paras, cur);
    }
    let at = cur.at;
    if at.c < paras[at.p].len() {
        paras[at.p].delete(at.c, at.c + 1);
        let c = at.c.min(paras[at.p].len());
        cur.set(Pos { p: at.p, c }, false);
    } else if at.p + 1 < paras.len() {
        let p = paras.remove(at.p + 1);
        paras[at.p].merge(p);
    }
}

/// Seçimin metni (paragraflar satır sonuyla).
pub fn sel_text(paras: &[Para], cur: &Cursor) -> String {
    let (a, b) = cur.range();
    let mut out = String::new();
    for (p, para) in paras.iter().enumerate().take(b.p + 1).skip(a.p) {
        let t = para.text();
        let s = if p == a.p { a.c } else { 0 };
        let e = if p == b.p { b.c } else { usize::MAX };
        out.extend(t.chars().skip(s).take(e.saturating_sub(s)));
        if p != b.p {
            out.push('\n');
        }
    }
    out
}

/// Seçime (seçim yoksa imlecin bulunduğu paragrafın tamamına) biçim uygular.
pub fn apply_fmt(paras: &mut [Para], cur: &Cursor, f: impl Fn(&mut Fmt)) {
    let (a, b) = cur.range();
    if a == b {
        let l = paras[a.p].len();
        paras[a.p].set_fmt(0, l, f);
        return;
    }
    for (p, para) in paras.iter_mut().enumerate().take(b.p + 1).skip(a.p) {
        let s = if p == a.p { a.c } else { 0 };
        let e = if p == b.p { b.c } else { para.len() };
        para.set_fmt(s, e, &f);
    }
}

/// Seçim aralığındaki paragrafların dizinleri.
pub fn sel_paras(cur: &Cursor) -> std::ops::RangeInclusive<usize> {
    let (a, b) = cur.range();
    a.p..=b.p
}

pub fn word_left(t: &str, c: usize) -> usize {
    let v: Vec<char> = t.chars().collect();
    let mut i = c.min(v.len());
    while i > 0 && v[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !v[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

pub fn word_right(t: &str, c: usize) -> usize {
    let v: Vec<char> = t.chars().collect();
    let mut i = c.min(v.len());
    while i < v.len() && !v[i].is_whitespace() {
        i += 1;
    }
    while i < v.len() && v[i].is_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(parts: &[(&str, bool)]) -> Para {
        Para { runs: parts.iter().map(|(t, b)| Run::plain(t, Fmt { b: Some(*b), ..Default::default() })).collect(), ..Default::default() }
    }

    #[test]
    fn edit_ops() {
        let mut v = vec![para(&[("Merhaba ", false), ("dünya", true)])];
        let mut c = Cursor::default();
        c.set(Pos { p: 0, c: 8 }, false);
        type_text(&mut v, &mut c, "güzel ");
        assert_eq!(v[0].text(), "Merhaba güzel dünya");
        enter(&mut v, &mut c);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].text(), "dünya");
        backspace(&mut v, &mut c);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text(), "Merhaba güzel dünya");
        c.set(Pos { p: 0, c: 0 }, false);
        c.set(Pos { p: 0, c: 7 }, true);
        apply_fmt(&mut v, &c, |f| f.i = Some(true));
        assert!(v[0].all(0, 7, &Fmt::default(), |f| f.i == Some(true)));
        assert!(!v[0].all(0, 10, &Fmt::default(), |f| f.i == Some(true)));
        assert_eq!(sel_text(&v, &c), "Merhaba");
        delete_sel(&mut v, &mut c);
        assert_eq!(v[0].text(), " güzel dünya");
    }

    #[test]
    fn atom_is_whole() {
        let mut p = para(&[("a", false)]);
        p.runs.push(Run { text: "LINK".into(), atom: Some("<x/>".into()), ..Default::default() });
        p.runs.push(Run::plain("b", Fmt::default()));
        p.delete(2, 3);
        assert_eq!(p.text(), "ab");
        assert_eq!(word_left("bir iki üç", 10), 8);
        assert_eq!(word_right("bir iki üç", 0), 4);
    }
}
