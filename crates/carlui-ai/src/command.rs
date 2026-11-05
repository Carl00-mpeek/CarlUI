//! Bu modül "tanınan konuşma bir kayıt komutu mu, kaç saniye isteniyor?"
//! sorusunu çözüyor. Tanıyıcıdan (Vosk ya da Whisper) gelen metin küçük
//! harfe çevrilir, sözcüklere ayrılır; içinde bir kaydetme fiili ile bir nesne
//! ("klip", "tekrar") ya da süre varsa komut sayılır. Süre Türkçe/İngilizce
//! sayı sözcükleri veya rakamla verilebilir: "son iki dakikayı kaydet",
//! "kırk beş saniye kaydet", "bir buçuk dakika", "save the last 30 seconds".
//! Gündelik konuşmada geçen tek başına "kaydet" tetiklemez.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cmd {
    /// İstenen süre (sn); yoksa tutulan sürenin tamamı.
    pub secs: Option<u32>,
}

/// Vosk dilbilgisi: yalnızca bu sözcükler tanınır, gerisi "[unk]" olur.
/// Böylece küçük model komut dışındaki konuşmayı yanlışlıkla komuta çevirmez.
pub const WORDS_TR: &[&str] = &[
    "son", "kaydet", "klip", "klibi", "tekrar", "tekrarı", "dakika", "dakikayı", "saniye", "saniyeyi", "yarım", "buçuk", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz",
    "dokuz", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan",
];

fn lower(s: &str) -> String {
    // Türkçe büyük I/İ'yi doğru küçült.
    s.chars()
        .flat_map(|c| match c {
            'I' => vec!['ı'],
            'İ' => vec!['i'],
            c => c.to_lowercase().collect(),
        })
        .collect()
}

fn number(w: &str) -> Option<f32> {
    if let Ok(v) = w.replace(',', ".").parse::<f32>() {
        return Some(v);
    }
    Some(match w {
        "bir" | "one" | "a" | "an" => 1.0,
        "iki" | "two" => 2.0,
        "üç" | "three" => 3.0,
        "dört" | "four" => 4.0,
        "beş" | "five" => 5.0,
        "altı" | "six" => 6.0,
        "yedi" | "seven" => 7.0,
        "sekiz" | "eight" => 8.0,
        "dokuz" | "nine" => 9.0,
        "on" | "ten" => 10.0,
        "eleven" => 11.0,
        "twelve" => 12.0,
        "fifteen" => 15.0,
        "yirmi" | "twenty" => 20.0,
        "otuz" | "thirty" => 30.0,
        "kırk" | "forty" => 40.0,
        "elli" | "fifty" => 50.0,
        "altmış" | "sixty" => 60.0,
        "yetmiş" | "seventy" => 70.0,
        "seksen" | "eighty" => 80.0,
        "doksan" | "ninety" => 90.0,
        "yüz" | "hundred" => 100.0,
        _ => return None,
    })
}

/// Birim sözcüğü → saniye.
fn unit(w: &str) -> Option<f32> {
    if w.starts_with("saniye") || w.starts_with("second") || w == "sn" || w == "sec" || w == "secs" {
        Some(1.0)
    } else if w.starts_with("dakika") || w.starts_with("minute") || w == "dk" || w == "min" || w == "mins" {
        Some(60.0)
    } else {
        None
    }
}

fn is_verb(w: &str) -> bool {
    w.starts_with("kaydet") || w.starts_with("save") || w == "clip"
}

fn is_object(w: &str) -> bool {
    // "son" / "last": "son iki dakikayı kaydet"ta süre duyulamasa da komut belli.
    let fuzzy = ["tekrarı", "tekrar", "klibi", "replay"].iter().any(|k| lev(w, k) <= 1);
    fuzzy || w.starts_with("klib") || w.starts_with("klip") || w.starts_with("tekrar") || ["clip", "that", "it", "son", "last"].contains(&w)
}

/// Düzenleme uzaklığı (Whisper'ın küçük modelleri "çekrarı" gibi tek harf kaçırır).
fn lev(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = cur;
        }
    }
    row[b.len()]
}

/// Metin bir kayıt komutuysa süresiyle döner.
pub fn parse(text: &str) -> Option<Cmd> {
    let t = lower(text);
    let words: Vec<&str> = t.split(|c: char| !(c.is_alphanumeric() || c == ',' || c == '.')).map(|w| w.trim_matches(|c| c == ',' || c == '.')).filter(|w| !w.is_empty()).collect();
    let verbs = words.iter().filter(|w| is_verb(w)).count();
    let objects = words.iter().filter(|w| is_object(w)).count();
    // "clip" hem fiil hem nesne olabilir; tek sözcük yetmez ("clip that" gerekir).
    let cmdish = words.iter().filter(|w| is_verb(w) || is_object(w)).count();
    let mut total = 0f32;
    let mut acc: Option<f32> = None;
    for w in &words {
        if let Some(n) = number(w) {
            acc = Some(acc.unwrap_or(0.0) + n);
        } else if *w == "buçuk" || *w == "half" {
            acc = Some(acc.unwrap_or(0.0) + 0.5);
        } else if *w == "yarım" {
            acc = Some(0.5);
        } else if let Some(u) = unit(w) {
            total += acc.unwrap_or(1.0) * u;
            acc = None;
        } else if *w != "and" && *w != "ve" {
            acc = None;
        }
    }
    let secs = (total >= 1.0).then(|| total.round() as u32);
    let ok = verbs > 0 && (secs.is_some() || objects > 0 && cmdish >= 2);
    ok.then_some(Cmd { secs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> Option<Option<u32>> {
        parse(t).map(|c| c.secs)
    }

    #[test]
    fn commands() {
        assert_eq!(s("klibi kaydet"), Some(None));
        assert_eq!(s("Tekrarı kaydet."), Some(None));
        assert_eq!(s("son iki dakikayı kaydet"), Some(Some(120)));
        assert_eq!(s("son otuz saniyeyi kaydet"), Some(Some(30)));
        assert_eq!(s("kırk beş saniye kaydet"), Some(Some(45)));
        assert_eq!(s("bir buçuk dakika kaydet"), Some(Some(90)));
        assert_eq!(s("yarım dakika kaydet"), Some(Some(30)));
        assert_eq!(s("Son 2 dakikayı kaydet."), Some(Some(120)));
        assert_eq!(s("iki dakika otuz saniye kaydet"), Some(Some(150)));
        assert_eq!(s("Save the last 30 seconds."), Some(Some(30)));
        assert_eq!(s("save the last two minutes"), Some(Some(120)));
        assert_eq!(s("Clip that!"), Some(None));
        assert_eq!(s("Saved last 30 seconds."), Some(Some(30)));
        assert_eq!(s("save a minute"), Some(Some(60)));
        assert_eq!(s("İKİ DAKİKA KAYDET"), Some(Some(120)));
        // Küçük Whisper modellerinin tipik hataları.
        assert_eq!(s("Çekrarı kaydet."), Some(None));
        assert_eq!(s("Son ikida kikayı kaydets."), Some(None));
    }

    #[test]
    fn not_commands() {
        assert_eq!(s("dosyayı kaydet"), None);
        assert_eq!(s("kaydet"), None);
        assert_eq!(s("Clip."), None);
        assert_eq!(s("iki dakika bekle"), None);
        assert_eq!(s("Teşekkürler."), None);
        assert_eq!(s(""), None);
        assert_eq!(s("[unk] [unk]"), None);
    }
}
