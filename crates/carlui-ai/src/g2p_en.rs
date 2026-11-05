//! Bu modül "İngilizce metni ses modelinin anladığı seslere nasıl çeviririz?"
//! sorusunu çözüyor. İngilizce yazıldığı gibi okunmaz; bu yüzden misaki
//! sözlüğü (us_gold + us_silver, Apache-2.0, ~180 bin sözcük) kullanılır.
//! Sözlükte olmayan sözcükte önce ek ayıklanır (-s, -ed, -ing …), yine yoksa
//! basit yazım kurallarıyla tahmin edilir. Çıktı misaki IPA'sıdır (Kokoro
//! bunu doğrudan anlar); Piper için `to_espeak` ile espeak işaretlerine çevrilir.

use std::{collections::HashMap, path::Path};

pub struct Lexicon {
    words: HashMap<String, String>,
}

impl Lexicon {
    /// Model klasöründeki `us_gold.json` ve `us_silver.json`'u yükler.
    pub fn load(dir: &Path) -> Result<Lexicon, String> {
        let mut words = HashMap::new();
        // Önce silver, sonra gold: gold'daki okunuş üstüne yazar.
        for name in ["us_silver.json", "us_gold.json"] {
            let p = crate::store::find(dir, name).ok_or_else(|| format!("{name} yok"))?;
            let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let v: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&s).map_err(|e| e.to_string())?;
            for (k, v) in v {
                let ipa = match v {
                    serde_json::Value::String(s) => Some(s),
                    serde_json::Value::Object(o) => o.get("DEFAULT").or_else(|| o.values().find(|v| v.is_string())).and_then(|v| v.as_str()).map(String::from),
                    _ => None,
                };
                if let Some(ipa) = ipa {
                    words.insert(k, ipa);
                }
            }
        }
        Ok(Lexicon { words })
    }

    #[cfg(test)]
    pub fn from(pairs: &[(&str, &str)]) -> Lexicon {
        Lexicon { words: pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }
    }

    fn get(&self, w: &str) -> Option<String> {
        if let Some(s) = self.words.get(w) {
            return Some(s.clone());
        }
        let l = w.to_lowercase();
        if let Some(s) = self.words.get(&l) {
            return Some(s.clone());
        }
        let mut c = l.chars();
        let cap: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
        self.words.get(&cap).cloned()
    }

    /// Tek sözcüğün okunuşu (misaki IPA).
    pub fn word(&self, w: &str) -> String {
        let w = w.replace('’', "'");
        if let Some(s) = self.get(&w) {
            return s;
        }
        // NASA, FBI: harf harf (sözlükte yoksa)
        let n = w.chars().count();
        if n >= 2 && n <= 5 && w.chars().all(|c| c.is_ascii_uppercase()) {
            return w.chars().map(|c| self.get(&c.to_string()).unwrap_or_default()).collect::<Vec<_>>().join(" ");
        }
        let l = w.to_lowercase();
        if let Some(base) = l.strip_suffix("'s") {
            let b = self.word(base);
            return format!("{b}{}", plural_tail(&b));
        }
        if let Some(s) = self.suffixed(&l) {
            return s;
        }
        if l.contains('-') {
            return l.split('-').filter(|p| !p.is_empty()).map(|p| self.word(p)).collect::<Vec<_>>().join(" ");
        }
        rules(&l)
    }

    fn suffixed(&self, l: &str) -> Option<String> {
        if let Some(b) = l.strip_suffix("ies") {
            return self.get(&format!("{b}y")).map(|s| format!("{s}z"));
        }
        if let Some(b) = l.strip_suffix("es") {
            if let Some(s) = self.get(b) {
                return Some(format!("{s}ɪz"));
            }
        }
        if let Some(b) = l.strip_suffix('s') {
            if let Some(s) = self.get(b) {
                let t = plural_tail(&s);
                return Some(format!("{s}{t}"));
            }
        }
        if let Some(b) = l.strip_suffix("ing") {
            for b in [b.to_string(), format!("{b}e"), b.get(..b.len().saturating_sub(1)).unwrap_or("").to_string()] {
                if let Some(s) = self.get(&b) {
                    return Some(format!("{s}ɪŋ"));
                }
            }
        }
        if let Some(b) = l.strip_suffix("ed") {
            for b in [b.to_string(), format!("{b}e"), b.get(..b.len().saturating_sub(1)).unwrap_or("").to_string()] {
                if let Some(s) = self.get(&b) {
                    let last = s.chars().last().unwrap_or(' ');
                    let tail = if "td".contains(last) { "ɪd" } else if "pkfθʃʧs".contains(last) { "t" } else { "d" };
                    return Some(format!("{s}{tail}"));
                }
            }
        }
        if let Some(b) = l.strip_suffix("ly") {
            if let Some(s) = self.get(b) {
                return Some(format!("{s}li"));
            }
        }
        if let Some(b) = l.strip_suffix("er") {
            if let Some(s) = self.get(b).or_else(|| self.get(&format!("{b}e"))) {
                return Some(format!("{s}əɹ"));
            }
        }
        None
    }
}

fn plural_tail(ipa: &str) -> &'static str {
    match ipa.chars().last().unwrap_or(' ') {
        's' | 'z' | 'ʃ' | 'ʒ' | 'ʧ' | 'ʤ' => "ɪz",
        'p' | 't' | 'k' | 'f' | 'θ' => "s",
        _ => "z",
    }
}

/// Sözlükte olmayan sözcük için kaba yazım kuralları.
fn rules(w: &str) -> String {
    let cs: Vec<char> = w.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let n = cs.len();
    let is_v = |c: char| "aeiou".contains(c);
    let at = |i: usize, s: &str| -> bool { s.chars().enumerate().all(|(k, c)| cs.get(i + k) == Some(&c)) };
    // Sessiz son e: "make" → A, "time" → I
    let magic = n >= 3 && cs[n - 1] == 'e' && !is_v(cs[n - 2]) && is_v(cs[n - 3]) && !(n >= 4 && is_v(cs[n - 4]));
    let mut out = String::new();
    let mut stressed = false;
    let mut i = 0;
    let mut vowel = |out: &mut String, s: &str| {
        if !stressed {
            out.push('ˈ');
            stressed = true;
        }
        out.push_str(s);
    };
    while i < n {
        let c = cs[i];
        if magic && i == n - 1 {
            break;
        }
        let pairs: &[(&str, &str, bool)] = &[
            ("tion", "ʃən", true),
            ("sion", "ʒən", true),
            ("ture", "ʧəɹ", true),
            ("igh", "I", true),
            ("ee", "i", true),
            ("ea", "i", true),
            ("oo", "u", true),
            ("ou", "W", true),
            ("ow", "O", true),
            ("ai", "A", true),
            ("ay", "A", true),
            ("oi", "Y", true),
            ("oy", "Y", true),
            ("au", "ɔ", true),
            ("aw", "ɔ", true),
            ("th", "θ", false),
            ("sh", "ʃ", false),
            ("ch", "ʧ", false),
            ("ph", "f", false),
            ("wh", "w", false),
            ("ck", "k", false),
            ("ng", "ŋ", false),
            ("qu", "kw", false),
            ("gh", "", false),
        ];
        if let Some((p, s, v)) = pairs.iter().find(|(p, _, _)| at(i, p)) {
            if *v {
                vowel(&mut out, s);
            } else {
                out.push_str(s);
            }
            i += p.len();
            continue;
        }
        if i == 0 && (at(0, "kn") || at(0, "wr")) {
            i += 1;
            continue;
        }
        if i > 0 && cs[i - 1] == c && !is_v(c) {
            i += 1;
            continue;
        }
        let next = cs.get(i + 1).copied().unwrap_or(' ');
        let s: &str = match c {
            'a' | 'e' | 'i' | 'o' | 'u' => {
                let long = magic && i == n - 3;
                let s = match (c, long) {
                    ('a', true) => "A",
                    ('a', false) => "æ",
                    ('e', true) => "i",
                    ('e', false) => {
                        if i == n - 1 && n > 2 {
                            ""
                        } else {
                            "ɛ"
                        }
                    }
                    ('i', true) => "I",
                    ('i', false) => "ɪ",
                    ('o', true) => "O",
                    ('o', false) => "ɑ",
                    ('u', true) => "u",
                    _ => "ʌ",
                };
                if !s.is_empty() {
                    vowel(&mut out, s);
                }
                i += 1;
                continue;
            }
            'y' => {
                if i == 0 {
                    "j"
                } else {
                    vowel(&mut out, if i == n - 1 { "i" } else { "ɪ" });
                    i += 1;
                    continue;
                }
            }
            'c' => {
                if "eiy".contains(next) {
                    "s"
                } else {
                    "k"
                }
            }
            'g' => {
                if "eiy".contains(next) {
                    "ʤ"
                } else {
                    "ɡ"
                }
            }
            'j' => "ʤ",
            'r' => "ɹ",
            'x' => "ks",
            's' => {
                if i > 0 && is_v(cs[i - 1]) && is_v(next) {
                    "z"
                } else {
                    "s"
                }
            }
            'q' => "k",
            c => {
                out.push(c);
                i += 1;
                continue;
            }
        };
        out.push_str(s);
        i += 1;
    }
    out
}

/// Sayının İngilizce okunuşu.
pub fn number(n: u64) -> String {
    const ONES: [&str; 20] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    fn small(n: u64) -> String {
        if n < 20 {
            ONES[n as usize].into()
        } else if n % 10 == 0 {
            TENS[(n / 10) as usize].into()
        } else {
            format!("{}-{}", TENS[(n / 10) as usize], ONES[(n % 10) as usize])
        }
    }
    fn three(n: u64) -> String {
        match (n / 100, n % 100) {
            (0, r) => small(r),
            (h, 0) => format!("{} hundred", ONES[h as usize]),
            (h, r) => format!("{} hundred {}", ONES[h as usize], small(r)),
        }
    }
    if n == 0 {
        return "zero".into();
    }
    let mut parts = Vec::new();
    for (scale, name) in [(1_000_000_000_000u64, "trillion"), (1_000_000_000, "billion"), (1_000_000, "million"), (1_000, "thousand")] {
        let k = n / scale % 1000;
        if k > 0 {
            parts.push(format!("{} {name}", three(k)));
        }
    }
    if n % 1000 > 0 {
        parts.push(three(n % 1000));
    }
    parts.join(" ")
}

/// 1990 → nineteen ninety, 2025 → twenty twenty-five, 1905 → nineteen oh five.
fn year(n: u64) -> Option<String> {
    if !((1100..=1999).contains(&n) || (2010..=2099).contains(&n)) || n % 100 == 0 && n >= 2000 {
        return None;
    }
    let (a, b) = (n / 100, n % 100);
    Some(match b {
        0 => format!("{} hundred", number(a)),
        1..=9 => format!("{} oh {}", number(a), number(b)),
        _ => format!("{} {}", number(a), number(b)),
    })
}

/// Sayıları ve sembolleri sözcüğe açar.
pub fn normalize(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut pending_unit: Option<&str> = None;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_digit() {
            let mut int = String::new();
            let mut j = i;
            let mut grouped = false;
            loop {
                while j < cs.len() && cs[j].is_ascii_digit() {
                    int.push(cs[j]);
                    j += 1;
                }
                if j < cs.len() && cs[j] == ',' && cs[j + 1..].iter().take(3).filter(|c| c.is_ascii_digit()).count() == 3 && cs.get(j + 4).map_or(true, |c| !c.is_ascii_digit()) {
                    grouped = true;
                    j += 1;
                    continue;
                }
                break;
            }
            let v = int.parse::<u64>().ok();
            let mut words = match v {
                Some(v) if !grouped && int.len() == 4 => year(v).unwrap_or_else(|| number(v)),
                Some(v) => number(v),
                None => int.chars().map(|d| number(d.to_digit(10).unwrap() as u64)).collect::<Vec<_>>().join(" "),
            };
            if j + 1 < cs.len() && cs[j] == '.' && cs[j + 1].is_ascii_digit() {
                j += 1;
                words.push_str(" point");
                while j < cs.len() && cs[j].is_ascii_digit() {
                    words.push(' ');
                    words.push_str(&number(cs[j].to_digit(10).unwrap() as u64));
                    j += 1;
                }
            }
            // Time: 2:30 → "two thirty", 2:05 → "two oh five", 2:00 → "two o'clock"
            if j + 2 < cs.len() && cs[j] == ':' && cs[j + 1].is_ascii_digit() && cs[j + 2].is_ascii_digit() && cs.get(j + 3).map_or(true, |c| !c.is_ascii_digit()) {
                let m = cs[j + 1].to_digit(10).unwrap() * 10 + cs[j + 2].to_digit(10).unwrap();
                words.push_str(&match m {
                    0 => " o'clock".to_string(),
                    1..=9 => format!(" oh {}", number(m as u64)),
                    _ => format!(" {}", number(m as u64)),
                });
                j += 3;
            }
            if cs.get(j) == Some(&'%') {
                words.push_str(" percent");
                j += 1;
            }
            out.push(' ');
            out.push_str(&words);
            if let Some(u) = pending_unit.take() {
                out.push(' ');
                out.push_str(u);
            }
            out.push(' ');
            i = j;
            continue;
        }
        match c {
            '$' => pending_unit = Some("dollars"),
            '€' => pending_unit = Some("euros"),
            '£' => pending_unit = Some("pounds"),
            '%' => out.push_str(" percent "),
            '&' => out.push_str(" and "),
            '+' => out.push_str(" plus "),
            '=' => out.push_str(" equals "),
            '@' => out.push_str(" at "),
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Metni misaki IPA'ya çevirir (sözcükler boşlukla, noktalama korunur).
pub fn phonemize(lex: &Lexicon, text: &str) -> String {
    let norm = normalize(text);
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let w = word.trim_matches(|c| c == '\'' || c == '-');
        if !w.is_empty() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&lex.word(w));
        }
        word.clear();
    };
    for c in norm.chars() {
        if c.is_alphabetic() || ((c == '\'' || c == '’' || c == '-') && !word.is_empty()) {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        match c {
            '.' | ',' | '!' | '?' | ';' | ':' => {
                out.push(c);
                out.push(' ');
            }
            '—' | '–' | '…' => out.push_str(", "),
            _ => out.push(' '),
        }
    }
    flush(&mut word, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// misaki IPA → espeak IPA (Piper İngilizce sesleri için).
pub fn to_espeak(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            'A' => out.push_str("eɪ"),
            'I' => out.push_str("aɪ"),
            'W' => out.push_str("aʊ"),
            'Y' => out.push_str("ɔɪ"),
            'O' => out.push_str("oʊ"),
            'Q' => out.push_str("əʊ"),
            'ʤ' => out.push_str("dʒ"),
            'ʧ' => out.push_str("tʃ"),
            'ᵊ' => out.push('ə'),
            'ᵻ' => out.push('ɪ'),
            'T' => out.push('ɾ'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number(42), "forty-two");
        assert_eq!(number(1_205), "one thousand two hundred five");
        assert!(normalize("in 1990").contains("nineteen ninety"));
        assert!(normalize("in 2025").contains("twenty twenty-five"));
        assert!(normalize("$5").contains("five dollars"));
        assert!(normalize("3.14").contains("three point one four"));
        assert!(normalize("1,000,000 people").contains("one million"));
        assert!(normalize("50%").contains("fifty percent"));
        assert!(normalize("at 2:30").contains("two thirty"));
        assert!(normalize("at 7:00").contains("seven o'clock"));
    }

    #[test]
    fn words() {
        let lex = Lexicon::from(&[("hello", "həlˈO"), ("world", "wˈɜɹld"), ("cat", "kˈæt"), ("jump", "ʤˈʌmp"), ("F", "ˈɛf"), ("B", "bˈi"), ("I", "ˈI")]);
        assert_eq!(phonemize(&lex, "Hello, world!"), "həlˈO, wˈɜɹld!");
        assert_eq!(lex.word("cats"), "kˈæts");
        assert_eq!(lex.word("jumped"), "ʤˈʌmpt");
        assert_eq!(lex.word("jumping"), "ʤˈʌmpɪŋ");
        assert_eq!(lex.word("FBI"), "ˈɛf bˈi ˈI");
        assert_eq!(rules("make"), "mˈAk");
        assert_eq!(rules("ship"), "ʃˈɪp");
        assert_eq!(to_espeak("həlˈO"), "həlˈoʊ");
    }
}
