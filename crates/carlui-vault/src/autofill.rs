//! Tarayıcı eklentisiyle doldurma: hangi kayıt hangi sayfaya aittir ve
//! kayıttan hangi alanlar kullanıcı adı / parola olarak gider. Eşleme sıkıdır,
//! çünkü yanlış siteye parola vermek (oltalama) asıl tehlikedir:
//! - Sayfanın ana makinesi kayıttaki adresin makinesiyle aynı olmalı ya da onun
//!   alt alanı olmalı ("github.com" kaydı "gist.github.com"a uyar; tersi uymaz).
//!   Baştaki "www." yok sayılır. Benzer görünen alan adları (githuh.com,
//!   github.com.evil.io) uymaz.
//! - Kayıt https ise (şeması yazılmamışsa da https sayılır) sayfa da https olmalı.
//! - Kayıtta bağlantı noktası yazılıysa aynı olmalı.

use crate::entry::{Body, FieldKind};

/// Adresin (şema, makine, bağlantı noktası) parçaları; makine küçük harf, "www."suz.
fn parts(url: &str, default_https: bool) -> Option<(String, String, Option<u16>)> {
    let url = url.trim();
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None if default_https => ("https".to_string(), url),
        None => return None,
    };
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let auth = rest.split(['/', '?', '#']).next()?;
    // Kullanıcı bilgisi (user@host) atlanır.
    let auth = auth.rsplit('@').next()?;
    let (host, port) = match auth.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => (h, Some(p.parse::<u16>().ok()?)),
        _ => (auth, None),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.contains(|c: char| c.is_whitespace()) {
        return None;
    }
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    Some((scheme, host, port))
}

/// Kayıttaki adres (`entry`) bu sayfaya (`page`) uyuyor mu.
pub fn url_matches(entry: &str, page: &str) -> bool {
    let (Some((es, eh, ep)), Some((ps, ph, pp))) = (parts(entry, true), parts(page, false)) else { return false };
    if es == "https" && ps != "https" {
        return false;
    }
    if ep.is_some() && ep != pp {
        return false;
    }
    ph == eh || ph.ends_with(&format!(".{eh}"))
}

/// Kayıt bu sayfaya uyuyor mu (adres alanlarından herhangi biri).
pub fn body_matches(body: &Body, page: &str) -> bool {
    body.fields.iter().any(|f| f.kind == FieldKind::Url && url_matches(&f.value, page))
}

fn is_user_label(l: &str) -> bool {
    let l = l.to_lowercase();
    ["kullanıcı", "kullanici", "user", "e-posta", "eposta", "email", "e-mail", "posta", "giriş", "login", "hesap"].iter().any(|k| l.contains(k))
}

/// (kullanıcı adı, parola). Kullanıcı adı: etiketi kullanıcı/e-posta benzeri ilk
/// metin alanı, yoksa ilk metin alanı. Parola: ilk gizli alan.
pub fn credentials(body: &Body) -> (String, String) {
    let texts = || body.fields.iter().filter(|f| f.kind == FieldKind::Text && !f.value.trim().is_empty());
    let user = texts().find(|f| is_user_label(&f.label)).or_else(|| texts().next()).map(|f| f.value.trim().to_string()).unwrap_or_default();
    let pass = body.fields.iter().find(|f| f.kind == FieldKind::Secret && !f.value.is_empty()).map(|f| f.value.clone()).unwrap_or_default();
    (user, pass)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Field;

    #[test]
    fn strict_matching() {
        assert!(url_matches("https://github.com", "https://github.com/login"));
        assert!(url_matches("github.com", "https://github.com/login?x=1"));
        assert!(url_matches("https://www.github.com/", "https://github.com/"));
        assert!(url_matches("github.com", "https://gist.github.com/"));
        assert!(url_matches("GitHub.com", "https://WWW.GITHUB.COM/"));
        assert!(!url_matches("gist.github.com", "https://github.com/"), "üst alan uymaz");
        assert!(!url_matches("github.com", "https://githuh.com/"));
        assert!(!url_matches("github.com", "https://github.com.evil.io/login"));
        assert!(!url_matches("github.com", "https://evilgithub.com/"));
        assert!(!url_matches("github.com", "https://evil.io/?u=github.com"));
        assert!(!url_matches("github.com", "https://github.com@evil.io/"));
        assert!(!url_matches("https://github.com", "http://github.com/"), "https kaydı http sayfaya gitmez");
        assert!(url_matches("http://192.168.1.1", "http://192.168.1.1/admin"));
        assert!(url_matches("http://localhost:8765", "http://localhost:8765/login"));
        assert!(!url_matches("http://localhost:8765", "http://localhost:9000/login"));
        assert!(!url_matches("github.com", "file:///C:/github.com/x.html"));
        assert!(!url_matches("", "https://github.com/"));
    }

    #[test]
    fn picks_fields() {
        let f = |label: &str, value: &str, kind| Field { label: label.into(), value: value.into(), kind };
        let body = Body {
            fields: vec![f("Not", "x", FieldKind::Text), f("E-posta", "carl@ornek.com", FieldKind::Text), f("Parola", "s3cret", FieldKind::Secret), f("Adres", "https://github.com", FieldKind::Url)],
            file: None,
        };
        assert_eq!(credentials(&body), ("carl@ornek.com".into(), "s3cret".into()));
        assert!(body_matches(&body, "https://github.com/login"));
        assert!(!body_matches(&body, "https://gitlab.com/login"));
    }
}
