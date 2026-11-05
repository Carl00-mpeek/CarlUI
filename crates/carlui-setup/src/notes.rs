//! Bu modül güncellemeden sonra "Yenilikler" ekranında ne yazacağı sorununu
//! çözüyor. Kurucuya derlenirken GitHub sürüm notu gömülür (`payload::NOTES`);
//! not "## Türkçe" … "---" … "## English" biçiminde iki dillidir. Burada
//! kullanıcının diline uyan bölüm seçilir, ekranda gereksiz satırlar (SHA-256,
//! kurulum talimatı, ayraçlar) atılır, Markdown işaretleri (`**`, `` ` ``)
//! temizlenir ve satırlar başlık / madde / paragraf olarak sınıflanır.
//! Ekranın genişliğine göre sözcük kaydırma da burada (`wrap`).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Head,
    Bullet,
    /// Kaydırılan maddenin devam satırı (yalnızca çizimde).
    More,
    Para,
}

/// Notun dile uyan bölümü, gösterilecek satırlar.
pub fn parse(src: &str, tr: bool) -> Vec<(Kind, String)> {
    let want = if tr { "türkçe" } else { "english" };
    let lines: Vec<&str> = src.lines().map(str::trim).collect();
    let is_lang = |l: &str| l.starts_with("## ");
    // İstenen dilin başlığı yoksa (tek dilli not) tamamı kullanılır.
    let start = lines.iter().position(|l| is_lang(l) && l[3..].trim().to_lowercase() == want);
    let body: &[&str] = match start {
        Some(i) => {
            let rest = &lines[i + 1..];
            let end = rest.iter().position(|l| is_lang(l) || *l == "---").unwrap_or(rest.len());
            &rest[..end]
        }
        None => &lines,
    };
    let mut out = Vec::new();
    let mut skip_hash = false;
    for &l in body {
        if l.is_empty() || l == "---" || is_lang(l) {
            continue;
        }
        let low = l.to_lowercase();
        if low.starts_with("**sha-256") {
            skip_hash = true;
            continue;
        }
        // SHA satırının altındaki `abc…` karması.
        if skip_hash && l.starts_with('`') && l.ends_with('`') {
            skip_hash = false;
            continue;
        }
        skip_hash = false;
        if low.starts_with("**kurulum:**") || low.starts_with("**install:**") {
            continue;
        }
        let (kind, text) = if let Some(b) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            (Kind::Bullet, b)
        } else if l.starts_with("**") && l.ends_with("**") && l.len() > 4 && !l[2..l.len() - 2].contains("**") {
            (Kind::Head, l[2..l.len() - 2].trim_end_matches(':'))
        } else {
            (Kind::Para, l)
        };
        let text = text.replace("**", "").replace('`', "");
        if !text.trim().is_empty() {
            out.push((kind, text.trim().to_string()));
        }
    }
    out
}

/// Metni `max` genişliğe sığan satırlara böler (sözcük sınırından).
/// Tek başına sığmayan uzun sözcük kendi satırında kalır.
pub fn wrap(text: &str, max: f32, width: impl Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        let cand = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
        if cur.is_empty() || width(&cand) <= max {
            cur = cand;
        } else {
            out.push(std::mem::take(&mut cur));
            cur = w.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "## Türkçe\n\n**Yeni: Carl Office**, hafif ofis modülü.\n- Excel açar.\n\n**Carl Vault:**\n- **Passkey desteği:** kasada.\n\n**SHA-256 (CarlUI-Setup.exe):**\n`eef8`\n\n**Kurulum:** `CarlUI-Setup.exe` dosyasını çalıştır.\n\n---\n\n## English\n\n**Carl Vault:**\n- The list shows `2FA`.\n\n**SHA-256 (CarlUI-Setup.exe):**\n`eef8`\n\n**Install:** Download it.\n";

    #[test]
    fn turkish_section() {
        let v = parse(SAMPLE, true);
        assert_eq!(
            v,
            vec![
                (Kind::Para, "Yeni: Carl Office, hafif ofis modülü.".into()),
                (Kind::Bullet, "Excel açar.".into()),
                (Kind::Head, "Carl Vault".into()),
                (Kind::Bullet, "Passkey desteği: kasada.".into()),
            ]
        );
    }

    #[test]
    fn english_section() {
        let v = parse(SAMPLE, false);
        assert_eq!(v, vec![(Kind::Head, "Carl Vault".into()), (Kind::Bullet, "The list shows 2FA.".into())]);
    }

    #[test]
    fn single_language_and_empty() {
        assert_eq!(parse("- one\n- two", false), vec![(Kind::Bullet, "one".into()), (Kind::Bullet, "two".into())]);
        assert!(parse("", true).is_empty());
    }

    #[test]
    fn wraps_on_words() {
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(wrap("aa bb cc dd", 5.0, w), vec!["aa bb", "cc dd"]);
        assert_eq!(wrap("uzunsözcük x", 4.0, w), vec!["uzunsözcük", "x"]);
        assert!(wrap("   ", 10.0, w).is_empty());
    }
}
