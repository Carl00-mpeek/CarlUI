//! Bu modül "Türkçe metni ses modelinin anladığı seslere nasıl çeviririz?"
//! sorusunu çözüyor. Türkçe yazıldığı gibi okunur; bu yüzden sözlük yerine
//! kurallar yeter: harf → IPA (espeak "tr" işaretleriyle), ince ünlü yanında
//! k/g/l inceltilir, ğ önceki ünlüyü uzatır, vurgu çoğunlukla son hecededir.
//! Sayılar, yüzde ve kısaltmalar önce yazıya açılır.

const VOWELS: &str = "aeıioöuüâîû";
const FRONT: &str = "eiöüâîû";

fn lower(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'I' => 'ı',
            'İ' => 'i',
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// 0 … 999 999 999 999 arası sayının Türkçe okunuşu.
pub fn number(n: u64) -> String {
    const ONES: [&str; 10] = ["", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz"];
    const TENS: [&str; 10] = ["", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan"];
    if n == 0 {
        return "sıfır".into();
    }
    fn three(n: u64, out: &mut Vec<String>) {
        let (h, t, o) = (n / 100, n / 10 % 10, n % 10);
        if h > 1 {
            out.push(ONES[h as usize].into());
        }
        if h > 0 {
            out.push("yüz".into());
        }
        if t > 0 {
            out.push(TENS[t as usize].into());
        }
        if o > 0 {
            out.push(ONES[o as usize].into());
        }
    }
    let mut out = Vec::new();
    for (scale, name) in [(1_000_000_000_000u64, "trilyon"), (1_000_000_000, "milyar"), (1_000_000, "milyon"), (1_000, "bin")] {
        let k = n / scale % 1000;
        if k > 0 {
            if !(k == 1 && name == "bin") {
                three(k, &mut out);
            }
            out.push(name.into());
        }
    }
    three(n % 1000, &mut out);
    out.join(" ")
}

fn letter_name(c: char) -> &'static str {
    match c {
        'a' => "a",
        'b' => "be",
        'c' => "ce",
        'ç' => "çe",
        'd' => "de",
        'e' => "e",
        'f' => "fe",
        'g' => "ge",
        'ğ' => "yumuşak ge",
        'h' => "he",
        'ı' => "ı",
        'i' => "i",
        'j' => "je",
        'k' => "ke",
        'l' => "le",
        'm' => "me",
        'n' => "ne",
        'o' => "o",
        'ö' => "ö",
        'p' => "pe",
        'q' => "kü",
        'r' => "re",
        's' => "se",
        'ş' => "şe",
        't' => "te",
        'u' => "u",
        'ü' => "ü",
        'v' => "ve",
        'w' => "dabılyu",
        'x' => "iks",
        'y' => "ye",
        'z' => "ze",
        _ => "",
    }
}

/// Sayıları, sembolleri ve kısaltmaları sözcüğe açar; sonuç küçük harflidir.
pub fn normalize(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_digit() {
            // 1.250.000 (binlik nokta) ve 3,5 (ondalık virgül)
            let mut int = String::new();
            let mut j = i;
            loop {
                while j < cs.len() && cs[j].is_ascii_digit() {
                    int.push(cs[j]);
                    j += 1;
                }
                if j + 3 < cs.len() + 1 && j < cs.len() && cs[j] == '.' && cs[j + 1..].iter().take(3).filter(|c| c.is_ascii_digit()).count() == 3 && cs.get(j + 4).map_or(true, |c| !c.is_ascii_digit()) {
                    j += 1;
                    continue;
                }
                break;
            }
            let mut words = int.parse::<u64>().map(number).unwrap_or_else(|_| int.chars().map(|d| number(d.to_digit(10).unwrap() as u64)).collect::<Vec<_>>().join(" "));
            if j + 1 < cs.len() && (cs[j] == ',' || cs[j] == '.') && cs[j + 1].is_ascii_digit() {
                j += 1;
                let mut frac = String::new();
                while j < cs.len() && cs[j].is_ascii_digit() {
                    frac.push(cs[j]);
                    j += 1;
                }
                words.push_str(" virgül ");
                words.push_str(&if frac.starts_with('0') { frac.chars().map(|d| number(d.to_digit(10).unwrap() as u64)).collect::<Vec<_>>().join(" ") } else { frac.parse::<u64>().map(number).unwrap_or_default() });
            }
            // Saat: 14:30 → "on dört otuz", 09:05 → "dokuz sıfır beş"
            if j + 2 < cs.len() && cs[j] == ':' && cs[j + 1].is_ascii_digit() && cs[j + 2].is_ascii_digit() && cs.get(j + 3).map_or(true, |c| !c.is_ascii_digit()) {
                let m = cs[j + 1].to_digit(10).unwrap() * 10 + cs[j + 2].to_digit(10).unwrap();
                if m > 0 {
                    words.push(' ');
                    if m < 10 {
                        words.push_str("sıfır ");
                    }
                    words.push_str(&number(m as u64));
                }
                j += 3;
            }
            out.push(' ');
            out.push_str(&words);
            // 14'te, 2026'da: ek sayıya bitişik okunur.
            if matches!(cs.get(j), Some('\'') | Some('’')) && cs.get(j + 1).map_or(false, |c| c.is_alphabetic()) {
                j += 1;
            } else {
                out.push(' ');
            }
            i = j;
            continue;
        }
        if c.is_alphabetic() {
            let mut j = i;
            while j < cs.len() && (cs[j].is_alphabetic() || cs[j] == '\'' || cs[j] == '’') {
                j += 1;
            }
            let w: String = cs[i..j].iter().collect();
            let core: String = w.chars().take_while(|c| c.is_alphabetic()).collect();
            let n = core.chars().count();
            let has_vowel = lower(&core).chars().any(|c| VOWELS.contains(c));
            // TBMM, AB, PTT: harf harf okunur (TÜRKİYE gibi başlıklar değil)
            if n >= 2 && n <= 4 && core.chars().all(|c| c.is_uppercase()) && (!has_vowel || n <= 3) {
                let spelled: Vec<&str> = lower(&core).chars().map(letter_name).collect();
                out.push_str(&spelled.join(" "));
                out.push_str(&lower(&w[core.len()..]).replace(['\'', '’'], ""));
            } else {
                out.push_str(&lower(&w).replace(['\'', '’'], ""));
            }
            i = j;
            continue;
        }
        match c {
            '%' => out.push_str(" yüzde "),
            '&' => out.push_str(" ve "),
            '+' => out.push_str(" artı "),
            '=' => out.push_str(" eşittir "),
            '°' => out.push_str(" derece "),
            '€' => out.push_str(" avro "),
            '$' => out.push_str(" dolar "),
            '₺' => out.push_str(" lira "),
            '@' => out.push_str(" et "),
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
        i += 1;
    }
    out.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Vurgusu son hecede olmayan sık sözcükler: vurgulu ünlünün sırası (0'dan).
fn stress_exception(w: &str) -> Option<usize> {
    Some(match w {
        "evet" | "hayır" | "şimdi" | "sonra" | "belki" | "ama" | "fakat" | "çünkü" | "yani" | "hani" | "nasıl" | "neden" | "niçin" | "nerede" | "nereye" | "nereden" | "ankara" | "türkiye" | "avrupa" | "asya" | "lütfen" | "tamam" | "ancak" | "sadece" | "yarın" | "bugün" | "dün" | "hepsi" | "herkes" => 0,
        "merhaba" | "istanbul" | "izmir" | "almanya" | "amerika" | "ingiltere" | "teşekkürler" | "anneanne" | "babaanne" => 1,
        _ => return None,
    })
}

/// Sözcükleri boşlukla ayrılmış espeak "tr" fonem dizisine çevirir.
pub fn phonemize(text: &str) -> String {
    let norm = normalize(text);
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&word_ipa(word));
            word.clear();
        }
    };
    for c in norm.chars() {
        if c.is_alphabetic() {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        match c {
            '.' | ',' | '!' | '?' | ';' | ':' => {
                out.push(c);
                out.push(' ');
            }
            '-' | '—' | '–' | '…' => {
                out.push(',');
                out.push(' ');
            }
            _ => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
        }
    }
    flush(&mut word, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn word_ipa(w: &str) -> String {
    let cs: Vec<char> = w.chars().collect();
    let is_v = |c: char| VOWELS.contains(c);
    // Ünsüz için belirleyici ünlü: sonraki harf ünlüyse o, değilse önceki ünlü.
    let near = |i: usize| -> Option<char> {
        if let Some(&n) = cs.get(i + 1) {
            if is_v(n) {
                return Some(n);
            }
        }
        cs[..i].iter().rev().find(|c| is_v(**c)).copied().or_else(|| cs[i..].iter().find(|c| is_v(**c)).copied())
    };
    let front = |i: usize| near(i).map_or(false, |v| FRONT.contains(v));
    // k/g yalnızca ardından ince ünlü gelince incelir; hece sonunda kalın kalır
    // (yoksa "örnek" "örneş" gibi duyuluyor).
    let front_next = |i: usize| cs.get(i + 1).map_or(false, |c| FRONT.contains(*c));
    let vowel_count = cs.iter().filter(|c| is_v(**c)).count();
    let stressed = if vowel_count == 0 { usize::MAX } else { stress_exception(w).filter(|&k| k < vowel_count).unwrap_or(vowel_count - 1) };
    let mut out = String::new();
    let mut vi = 0;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if is_v(c) {
            if vi == stressed && vowel_count > 0 {
                out.push('ˈ');
            }
            vi += 1;
            out.push_str(match c {
                'a' | 'â' => "a",
                'e' => "e",
                'ı' => "ɯ",
                'i' | 'î' => "i",
                'o' => "o",
                'ö' => "œ",
                'u' | 'û' => "u",
                'ü' => "y",
                _ => "",
            });
            i += 1;
            continue;
        }
        // Çift ünsüz (anne, Allah) uzun okunur.
        let s: &str = match c {
            'b' => "b",
            'c' => "dʒ",
            'ç' => "tʃ",
            'd' => "d",
            'f' => "f",
            'g' => {
                if front_next(i) {
                    "ɟ"
                } else {
                    "ɡ"
                }
            }
            'ğ' => {
                let prev_v = i > 0 && is_v(cs[i - 1]);
                let next_v = cs.get(i + 1).map_or(false, |c| is_v(*c));
                if !prev_v {
                    ""
                } else if !next_v {
                    "ː"
                } else if FRONT.contains(cs[i - 1]) {
                    "j"
                } else {
                    ""
                }
            }
            'h' => "h",
            'j' => "ʒ",
            'k' | 'q' => {
                if front_next(i) {
                    "c"
                } else {
                    "k"
                }
            }
            'l' => {
                if front(i) {
                    "l"
                } else {
                    "ɫ"
                }
            }
            'm' => "m",
            'n' => "n",
            'p' => "p",
            'r' => "ɾ",
            's' => "s",
            'ş' => "ʃ",
            't' => "t",
            'v' | 'w' => "v",
            'x' => "ks",
            'y' => "j",
            'z' => "z",
            _ => "",
        };
        out.push_str(s);
        if cs.get(i + 1) == Some(&c) && !s.is_empty() && c != 'ğ' {
            out.push('ː');
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number(0), "sıfır");
        assert_eq!(number(1), "bir");
        assert_eq!(number(19), "on dokuz");
        assert_eq!(number(100), "yüz");
        assert_eq!(number(1453), "bin dört yüz elli üç");
        assert_eq!(number(2026), "iki bin yirmi altı");
        assert_eq!(number(1_250_000), "bir milyon iki yüz elli bin");
        assert!(normalize("%25").contains("yüzde yirmi beş"));
        assert!(normalize("3,5 kg").contains("üç virgül beş"));
        assert!(normalize("1.000.000 kişi").contains("bir milyon"));
        assert_eq!(normalize("saat 14:30'da"), "saat on dört otuzda");
        assert_eq!(normalize("09:05"), "dokuz sıfır beş");
    }

    #[test]
    fn words() {
        assert_eq!(phonemize("Merhaba"), "meɾhˈaba");
        assert_eq!(phonemize("kedi"), "cedˈi");
        assert_eq!(phonemize("göz"), "ɟˈœz");
        assert_eq!(phonemize("dağ"), "dˈaː");
        assert_eq!(phonemize("çiçek"), "tʃitʃˈek");
        assert_eq!(phonemize("Işık, şu."), "ɯʃˈɯk, ʃˈu.");
        assert_eq!(phonemize("anne"), "anːˈe");
        assert_eq!(phonemize("okul"), "okˈuɫ");
        assert!(phonemize("TBMM").starts_with("tˈe"));
    }
}
