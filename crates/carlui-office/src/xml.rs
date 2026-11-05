//! Bu modül "Office XML'ini okurken her parçanın dosyadaki yerini bilelim ki
//! yalnızca değişen parçayı yeniden yazıp gerisine dokunmayalım" sorununu
//! çözüyor. Küçük, bağımlılıksız bir ayrıştırıcı: öğeleri bayt aralıklarıyla
//! birlikte bir ağaca dizer. Ad alanı öneki adın parçası olarak kalır
//! (`w:p`, `a:t`); `local()` öneksiz adı verir.

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// Öğenin tamamı: `<x ...>` başından `</x>` sonuna.
    pub start: usize,
    pub end: usize,
    /// İçerik aralığı (kendiliğinden kapanan öğede start=end).
    pub inner: (usize, usize),
    pub children: Vec<Node>,
}

impl Node {
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name || local(k) == name && !name.contains(':')).map(|(_, v)| v.as_str())
    }

    /// Öneksiz ada göre ilk çocuk.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.local() == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.local() == name)
    }

    /// Yol boyunca iner: `n.path(&["spPr", "xfrm", "off"])`.
    pub fn path(&self, names: &[&str]) -> Option<&Node> {
        let mut n = self;
        for p in names {
            n = n.child(p)?;
        }
        Some(n)
    }

    /// Alt ağaçta öneksiz adı eşleşen ilk öğe (derinlik öncelikli).
    pub fn find(&self, name: &str) -> Option<&Node> {
        for c in &self.children {
            if c.local() == name {
                return Some(c);
            }
            if let Some(f) = c.find(name) {
                return Some(f);
            }
        }
        None
    }

    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.local() == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }

    /// Öğenin ham XML'i.
    pub fn raw<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    /// Yalnızca açılış etiketi (`<w:p w:rsidR="..">`), kendiliğinden kapanan
    /// öğede `/>` yerine `>` ile.
    pub fn open_tag(&self, src: &str) -> String {
        if !self.self_closing(src) {
            src[self.start..self.inner.0].to_string()
        } else {
            let s = &src[self.start..self.end];
            format!("{}>", s.trim_end_matches('>').trim_end_matches('/').trim_end())
        }
    }

    /// `<x/>` biçiminde mi?
    pub fn self_closing(&self, src: &str) -> bool {
        self.inner.0 == self.end && src[self.start..self.end].ends_with("/>")
    }

    /// Doğrudan metin içeriği (çocuk öğe yoksa iç kısmın çözülmüşü).
    pub fn text(&self, src: &str) -> String {
        if self.children.is_empty() {
            unescape(&src[self.inner.0..self.inner.1])
        } else {
            let mut s = String::new();
            let mut p = self.inner.0;
            for c in &self.children {
                s.push_str(&unescape(strip_cdata_comments(&src[p..c.start])));
                p = c.end;
            }
            s.push_str(&unescape(strip_cdata_comments(&src[p..self.inner.1])));
            s
        }
    }

    /// Alt ağaçtaki tüm `t` öğelerinin metni (göz atma / önizleme için).
    pub fn all_text(&self, src: &str, tname: &str) -> String {
        let mut v = Vec::new();
        self.find_all(tname, &mut v);
        v.iter().map(|n| n.text(src)).collect()
    }
}

fn strip_cdata_comments(s: &str) -> &str {
    if s.contains('<') {
        ""
    } else {
        s
    }
}

pub fn local(name: &str) -> &str {
    name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name)
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';') else { break };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0'da izin verilmeyen denetim karakterleri atılır.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// Belgeyi ayrıştırır; kök öğeyi döndürür. Bozuk XML'de hata.
pub fn parse(src: &str) -> Result<Node, String> {
    let b = src.as_bytes();
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &src[i..];
        if rest.starts_with("<?") {
            i += rest.find("?>").ok_or("işlem talimatı kapanmadı")? + 2;
            continue;
        }
        if rest.starts_with("<!--") {
            i += rest.find("-->").ok_or("yorum kapanmadı")? + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i += rest.find("]]>").ok_or("CDATA kapanmadı")? + 3;
            continue;
        }
        if rest.starts_with("<!") {
            i += rest.find('>').ok_or("DOCTYPE kapanmadı")? + 1;
            continue;
        }
        if rest.starts_with("</") {
            let close = rest.find('>').ok_or("kapanış etiketi bozuk")?;
            let mut n = stack.pop().ok_or("fazladan kapanış etiketi")?;
            n.inner.1 = i;
            n.end = i + close + 1;
            match stack.last_mut() {
                Some(p) => p.children.push(n),
                None => root = Some(n),
            }
            i += close + 1;
            continue;
        }
        // Açılış etiketi: tırnak içindeki '>' karakterlerini atla.
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if j >= b.len() {
            return Err("etiket kapanmadı".into());
        }
        let self_close = b[j - 1] == b'/';
        let body = &src[i + 1..if self_close { j - 1 } else { j }];
        let (name, attrs) = parse_tag(body);
        let mut n = Node { name, attrs, start: i, end: j + 1, inner: (j + 1, j + 1), children: Vec::new() };
        if self_close {
            n.inner = (n.end, n.end);
            match stack.last_mut() {
                Some(p) => p.children.push(n),
                None => root = Some(n),
            }
        } else {
            stack.push(n);
        }
        i = j + 1;
    }
    if !stack.is_empty() {
        return Err("kapanmamış öğe".into());
    }
    root.ok_or_else(|| "kök öğe yok".into())
}

fn parse_tag(body: &str) -> (String, Vec<(String, String)>) {
    let body = body.trim();
    let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
    let name = body[..name_end].to_string();
    let mut attrs = Vec::new();
    let mut rest = &body[name_end..];
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_string();
        rest = rest[eq + 1..].trim_start();
        let Some(q) = rest.chars().next() else { break };
        if q != '"' && q != '\'' {
            break;
        }
        let Some(close) = rest[1..].find(q) else { break };
        attrs.push((key, unescape(&rest[1..1 + close])));
        rest = &rest[close + 2..];
    }
    (name, attrs)
}

/// Kaynak metinde aralıkları yenileriyle değiştirir. Aralıklar çakışmamalı.
pub fn splice(src: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| e.0);
    let mut out = String::with_capacity(src.len() + 256);
    let mut p = 0;
    for (s, e, new) in edits {
        if s < p {
            continue;
        }
        out.push_str(&src[p..s]);
        out.push_str(&new);
        p = e;
    }
    out.push_str(&src[p..]);
    out
}

/// Bir öğenin çocuklarını şema sırasına göre birleştirir: `items` (ad, ham XML)
/// listesi `order` içindeki yerine göre dizilir; listede olmayan adlar
/// göreli sıralarını koruyarak en sona gider. Word/PowerPoint öğe sırası
/// yanlış olunca dosyayı "bozuk" sayar; bu yüzden gerekli.
pub fn schema_sort(items: &mut [(String, String)], order: &[&str]) {
    let rank = |n: &str| order.iter().position(|o| *o == local(n)).unwrap_or(order.len());
    items.sort_by_key(|(n, _)| rank(n));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_text() {
        let s = r#"<?xml version="1.0"?><w:doc a="1 &amp; 2"><w:p><w:t xml:space="preserve">Merhaba &lt;dünya&gt;</w:t></w:p><w:br/></w:doc>"#;
        let r = parse(s).unwrap();
        assert_eq!(r.name, "w:doc");
        assert_eq!(r.attr("a"), Some("1 & 2"));
        let p = r.child("p").unwrap();
        assert_eq!(p.raw(s), r#"<w:p><w:t xml:space="preserve">Merhaba &lt;dünya&gt;</w:t></w:p>"#);
        assert_eq!(p.child("t").unwrap().text(s), "Merhaba <dünya>");
        let br = r.child("br").unwrap();
        assert_eq!(br.raw(s), "<w:br/>");
        assert_eq!(br.open_tag(s), "<w:br>");
        let out = splice(s, vec![(p.start, p.end, "<w:p/>".into())]);
        assert!(out.contains("<w:doc a=\"1 &amp; 2\"><w:p/><w:br/>"));
    }

    #[test]
    fn quoted_gt_and_entities() {
        let s = r#"<a x="b>c"><b>&#x130;&#305;</b></a>"#;
        let r = parse(s).unwrap();
        assert_eq!(r.attr("x"), Some("b>c"));
        assert_eq!(r.child("b").unwrap().text(s), "İı");
        assert_eq!(escape("a<b&\"c\u{1}"), "a&lt;b&amp;&quot;c");
    }

    #[test]
    fn order() {
        let mut v = vec![("w:sz".to_string(), "1".to_string()), ("w:b".into(), "2".into()), ("w:x".into(), "3".into()), ("w:rFonts".into(), "4".into())];
        schema_sort(&mut v, &["rFonts", "b", "sz"]);
        let names: Vec<&str> = v.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["w:rFonts", "w:b", "w:sz", "w:x"]);
    }
}
