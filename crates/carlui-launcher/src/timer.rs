//! Bu modül "ayrı bir uygulama açmadan zamanlayıcı/kronometre kur" sorununu
//! aramada şu yöntemle çözüyor: "5dk", "25 dk çalış" (Pomodoro), "90sn",
//! "1 saat", "timer 10", "kronometre" ve "zamanlayıcı durdur" yazılınca
//! ilgili sonuç en üste çıkar. Sayımın kendisi adada yapılır (saat yerine
//! kalan süre görünür, bitince bildirim gelir); burada yalnızca ayrıştırma var.

use crate::search::{norm, score, Act, Icon, Item};

const KEYWORDS: &[&str] = &["timer", "zamanlayici", "sayac", "alarm"];
const UNITS: &[(&str, u32)] = &[
    ("dakika", 60),
    ("dakikalik", 60),
    ("dak", 60),
    ("dk", 60),
    ("min", 60),
    ("m", 60),
    ("saniye", 1),
    ("sn", 1),
    ("sec", 1),
    ("s", 1),
    ("saat", 3600),
    ("sa", 3600),
    ("hour", 3600),
    ("h", 3600),
];

/// 1500 → "25 dk", 90 → "1 dk 30 sn", 3600 → "1 sa".
pub fn human(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} sa"));
    }
    if m > 0 {
        parts.push(format!("{m} dk"));
    }
    if s > 0 {
        parts.push(format!("{s} sn"));
    }
    parts.join(" ")
}

/// "5dk çalış" → (300, "çalış"). `raw` kullanıcının yazdığı metin (etiket için).
fn parse(raw: &str) -> Option<(u32, String)> {
    let t = raw.trim();
    let n = norm(t);
    let mut rest_n = n.as_str();
    let mut rest_raw = t;
    let mut had_kw = false;
    if let Some(k) = KEYWORDS.iter().find(|k| rest_n.starts_with(*k) && rest_n[k.len()..].starts_with(' ')) {
        // norm Türkçe harfleri tek baytlık harflere çevirir; ham metinde kelime sınırını ayrıca bul.
        rest_n = rest_n[k.len()..].trim_start();
        rest_raw = rest_raw.split_once(' ').map(|(_, r)| r.trim_start()).unwrap_or("");
        had_kw = true;
    }
    let num_len = rest_n.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(rest_n.len());
    if num_len == 0 {
        return None;
    }
    let value: f64 = rest_n[..num_len].replace(',', ".").parse().ok()?;
    let after = rest_n[num_len..].trim_start();
    let (mult, unit_len) = match UNITS.iter().find(|(u, _)| after.starts_with(u) && after[u.len()..].chars().next().is_none_or(|c| c == ' ')) {
        Some((u, m)) => (*m, u.len()),
        None if had_kw && after.is_empty() => (60, 0),
        None => return None,
    };
    let secs = (value * mult as f64).round() as u32;
    if !(1..=24 * 3600).contains(&secs) {
        return None;
    }
    // Etiket: birimden sonraki kelimeler (ham metinden, Türkçe harfleriyle).
    let label_n = after[unit_len..].trim();
    let label = if label_n.is_empty() { String::new() } else { rest_raw.split_whitespace().rev().take(label_n.split_whitespace().count()).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ") };
    Some((secs, label))
}

pub(crate) fn items(raw: &str) -> Vec<Item> {
    let q = norm(raw.trim());
    if let Some((secs, label)) = parse(raw) {
        let title = if label.is_empty() { format!("{} zamanlayıcı başlat", human(secs)) } else { format!("{} zamanlayıcı: {label}", human(secs)) };
        let mut it = Item::new(title, "Adada geri sayım · bitince bildirim", Icon::Glyph('\u{E916}'), Act::Timer { secs, label }, "Zaman");
        it.key = "timer".into();
        return vec![it];
    }
    let mut out = Vec::new();
    let stop = ["zamanlayici durdur", "kronometre durdur", "sayaci durdur", "timer stop", "durdur zamanlayici"];
    let stopping = q.contains("durdur") || q.contains("stop") || q.contains("iptal");
    if stopping && stop.iter().any(|s| score(s, &q) >= 700) {
        out.push(Item::new("Zamanlayıcıyı durdur", "Adadaki sayım kapanır", Icon::Glyph('\u{E71A}'), Act::TimerStop, "Zaman"));
    }
    if q.chars().count() >= 4 && ["kronometre", "stopwatch"].iter().any(|s| score(s, &q) >= 700) && !stopping {
        let mut it = Item::new("Kronometre başlat", "Adada ileri sayım", Icon::Glyph('\u{E916}'), Act::Stopwatch, "Zaman");
        it.key = "stopwatch".into();
        out.push(it);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(q: &str) -> Option<u32> {
        parse(q).map(|p| p.0)
    }

    #[test]
    fn parsing() {
        assert_eq!(secs("5dk"), Some(300));
        assert_eq!(secs("5 dk"), Some(300));
        assert_eq!(secs("90sn"), Some(90));
        assert_eq!(secs("1 saat"), Some(3600));
        assert_eq!(secs("1,5 saat"), Some(5400));
        assert_eq!(secs("timer 10"), Some(600));
        assert_eq!(secs("zamanlayıcı 3 dakika"), Some(180));
        assert_eq!(parse("25dk çalış"), Some((1500, "çalış".into())));
        assert_eq!(parse("10 dk çay demle"), Some((600, "çay demle".into())));
        assert_eq!(secs("5"), None);
        assert_eq!(secs("5 masa"), None);
        assert_eq!(secs("3dmax"), None);
        assert_eq!(human(90), "1 dk 30 sn");
        assert!(matches!(items("kronometre")[0].act, Act::Stopwatch));
        assert!(matches!(items("zamanlayıcı durdur")[0].act, Act::TimerStop));
    }
}
