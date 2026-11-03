//! Bu modül "bağlantıyı tarayıcı açmadan indir" önerisini çözüyor: arama
//! kutusuna dosya bağlantısı yapıştırılınca (".zip", ".exe", ".pdf" …) ya da
//! "indir <adres>" yazılınca "İndir" sonucu çıkar; seçilince adanın indirme
//! yöneticisi İndirilenler'e indirir ve ilerlemeyi hapta gösterir.
//! "indirmeyi iptal et" süren indirmeleri durdurur. Ayarlardan kapatılabilir.

use crate::search::{norm, Act, Icon, Item};

const PAGES: &[&str] = &["html", "htm", "php", "asp", "aspx", "jsp", "cgi"];

/// Adresin son parçası ("…/setup.exe?v=1" → "setup.exe").
fn last_part(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let after = path.split_once("://").map(|p| p.1).unwrap_or(path);
    after.split('/').skip(1).filter(|s| !s.is_empty()).last().unwrap_or("")
}

/// Bağlantı bir dosyaya mı benziyor (sayfaya değil)?
pub fn is_file_link(url: &str) -> bool {
    let last = last_part(url);
    match last.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !PAGES.contains(&ext.to_lowercase().as_str()),
        None => false,
    }
}

fn item(url: &str) -> Item {
    let name = match last_part(url) {
        "" => url.to_string(),
        n => n.replace("%20", " "),
    };
    let mut it = Item::new(t!(format!("İndir: {name}"), format!("Download: {name}")), t!("İndirilenler'e · ilerleme adada", "To Downloads · progress on the island"), Icon::Glyph('\u{E896}'), Act::Download(url.to_string()), "İndir");
    it.key = "download".into();
    it
}

/// "indir <adres>" ya da "indirmeyi iptal et": yalnızca bunlar gösterilir.
pub fn command(raw: &str) -> Option<Vec<Item>> {
    let t = raw.trim();
    let q = norm(t);
    if ["indirme iptal", "indirmeyi iptal", "indirmeyi durdur", "download cancel", "cancel download"].iter().any(|k| q.starts_with(k)) {
        return Some(vec![Item::new(t!("İndirmeyi iptal et", "Cancel download"), t!("Adadaki süren indirmeler durur", "Stops running downloads on the island"), Icon::Glyph('\u{E711}'), Act::DownloadCancel, "İndir")]);
    }
    let rest = q.strip_prefix("indir ").or_else(|| q.strip_prefix("download "))?;
    // Büyük/küçük harf korunsun diye adres asıl metinden alınır.
    let url = t.get(t.len().checked_sub(rest.len())?..)?.trim();
    let url = if url.starts_with("http://") || url.starts_with("https://") { url.to_string() } else { format!("https://{url}") };
    (!url.contains(' ') && url.len() > 10).then(|| vec![item(&url)])
}

/// Yapıştırılan adres dosya bağlantısıysa "İndir" önerisi.
pub fn suggest(url: &str) -> Option<Item> {
    is_file_link(url).then(|| item(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert!(is_file_link("https://x.com/a/setup.exe?v=2"));
        assert!(is_file_link("https://x.com/dosya.tar.gz"));
        assert!(!is_file_link("https://x.com/index.html"));
        assert!(!is_file_link("https://github.com/rust-lang"));
        assert!(!is_file_link("https://x.com/"));
        assert!(matches!(&command("indir https://x.com/a.zip").unwrap()[0].act, Act::Download(u) if u == "https://x.com/a.zip"));
        assert!(matches!(&command("indirmeyi iptal et").unwrap()[0].act, Act::DownloadCancel));
        assert!(command("indirilenler").is_none());
    }
}
