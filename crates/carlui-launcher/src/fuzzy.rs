//! Bu modül "yanlış duyulmuş ya da kulaktan yazılmış ad ('yutub', 'spotifay',
//! 'krom', 'vatsap') hangi uygulama/site?" sorununu çözüyor: iki ad hem
//! yazımca (düzenleme uzaklığı) hem okunuşça (İngilizce yazımı Türkçe
//! okunuşa yaklaştıran kaba bir dönüşümden sonra) karşılaştırılır; en iyi
//! benzerlik 0..1 arası döner. Model yok, birkaç kural ve sayma.

/// Yalnızca harf ve rakam (normalleştirilmiş metinde boşluksuz biçim).
pub fn compact(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Okunuşa göre kaba biçim: "chrome" → "krom", "youtube" → "iutub",
/// "whatsapp" → "vatsap", "spotify" → "spotifi", "excel" → "eksel".
pub fn phon(s: &str) -> String {
    let mut t = compact(s);
    for (a, b) in [
        ("ph", "f"), ("wh", "v"), ("th", "t"), ("ck", "k"), ("sh", "s"), ("ch", "k"), ("x", "ks"), ("ce", "se"), ("ci", "si"),
        ("ou", "u"), ("oo", "u"), ("ee", "i"), ("ea", "i"), ("qu", "kv"),
    ] {
        t = t.replace(a, b);
    }
    // Sondaki "ay/ai/ey/y" İngilizcede çoğunlukla "i"ye yakın okunur (spotify, spotifay).
    for end in ["ay", "ai", "ey", "y"] {
        if t.len() > 3 && t.ends_with(end) {
            t.truncate(t.len() - end.len());
            t.push('i');
            break;
        }
    }
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        let c = match c {
            'c' => 'k',
            'q' => 'k',
            'w' => 'v',
            'y' => 'i',
            'z' => 's',
            'j' => 'c',
            c => c,
        };
        // Çift harfler tek okunur ("whatsapp" → "vatsap").
        if out.ends_with(c) {
            continue;
        }
        out.push(c);
    }
    // Sondaki sessiz "e" ("chrome", "youtube").
    if out.chars().count() > 3 && out.ends_with('e') {
        out.pop();
    }
    out
}

pub fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn lev_sim(a: &str, b: &str) -> f32 {
    let n = a.chars().count().max(b.chars().count());
    if n == 0 {
        return 0.0;
    }
    1.0 - lev(a, b) as f32 / n as f32
}

/// `said` (söylenen) ile `name` (aday ad) ne kadar benzer: 1 = aynı.
/// İkisi de normalleştirilmiş (`search::norm`) olmalı.
pub fn sim(said: &str, name: &str) -> f32 {
    let (a, b) = (compact(said), compact(name));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (pa, pb) = (phon(&a), phon(&b));
    if pa == pb {
        return 0.97;
    }
    let mut s = lev_sim(&a, &b).max(lev_sim(&pa, &pb));
    // Adın başı söylendi: "insta" → Instagram, "visual studio" → Visual Studio Code.
    let n = a.chars().count();
    if n >= 4 && (b.starts_with(&a) || pb.starts_with(&pa)) {
        s = s.max(0.8 + 0.15 * n as f32 / b.chars().count() as f32);
    }
    s
}

/// Söylenen adın uzunluğuna göre kabul eşiği: kısa adlarda tek harf bile çok şeyi değiştirir.
pub fn threshold(said: &str) -> f32 {
    match compact(said).chars().count() {
        0..=3 => 0.99,
        4..=5 => 0.79,
        _ => 0.72,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_alike() {
        assert_eq!(phon("chrome"), "krom");
        assert_eq!(phon("whatsapp"), phon("vatsap"));
        assert_eq!(phon("spotify"), phon("spotifay"));
        assert_eq!(phon("excel"), phon("eksel"));
        assert_eq!(phon("discord"), phon("diskord"));
        assert_eq!(phon("netflix"), phon("netfliks"));
        for (said, name) in [("yutub", "youtube"), ("krom", "chrome"), ("gugil", "google"), ("istim", "steam"), ("insta", "instagram"), ("vord", "word")] {
            assert!(sim(said, name) >= threshold(said), "{said} ~ {name}: {}", sim(said, name));
        }
        for (said, name) in [("spotify", "steam"), ("word", "edge"), ("youtube", "gmail"), ("notion", "netflix")] {
            assert!(sim(said, name) < threshold(said), "{said} !~ {name}: {}", sim(said, name));
        }
        assert_eq!(lev("kitten", "sitting"), 3);
    }
}
