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

/// Sayfanın kökeni ("https://www.roblox.com"); kayda yol ve sorgu (oturum
/// belirteci içerebilir) yazılmasın diye yalnızca bu saklanır.
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let auth = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.to_ascii_lowercase();
    (!auth.is_empty()).then(|| format!("{scheme}://{auth}"))
}

/// Kayıt adı: alan adının asıl kısmı ("accounts.google.com" → "Google",
/// "www.hepsiburada.com.tr" → "Hepsiburada"); IP ya da tek parçalı adda makinenin kendisi.
pub fn site_name(url: &str) -> String {
    let Some((_, host, _)) = parts(url, false) else { return String::new() };
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host;
    }
    let n = labels.len();
    // "com.tr", "co.uk" gibi ikili uzantılar.
    let second = labels[n - 2];
    let pick = if n >= 3 && labels[n - 1].len() == 2 && ["com", "co", "org", "net", "gov", "edu", "ac", "gen", "web", "bel"].contains(&second) { labels[n - 3] } else { second };
    let mut c = pick.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Tarayıcıda gönderilen giriş/kayıt formunun kasadaki karşılığı.
#[derive(Debug, PartialEq, Eq)]
pub enum Capture {
    /// Bu site ve kullanıcı adıyla kayıt yok.
    New,
    /// Kayıt var, parola farklı (kaydın kimliği).
    Changed(String),
    /// Aynısı zaten kayıtlı.
    Same,
}

/// Yakalanan bilgiyi kasadaki kayıtlarla karşılaştırır. Kullanıcı adı büyük/küçük
/// harf duyarsız eşlenir; form kullanıcı adı vermediyse sitede tek kayıt varsa o sayılır.
pub fn classify<'a>(items: impl IntoIterator<Item = (&'a str, &'a Body)>, url: &str, user: &str, password: &str) -> Capture {
    let user = user.trim();
    let site: Vec<(&str, String, String)> = items
        .into_iter()
        .filter(|(_, b)| body_matches(b, url))
        .map(|(id, b)| {
            let (u, p) = credentials(b);
            (id, u, p)
        })
        .collect();
    let same_user: Vec<_> = if user.is_empty() && site.len() == 1 { site.iter().collect() } else { site.iter().filter(|(_, u, _)| u.eq_ignore_ascii_case(user)).collect() };
    if same_user.iter().any(|(_, _, p)| p == password) {
        return Capture::Same;
    }
    match same_user.first() {
        Some((id, _, _)) => Capture::Changed(id.to_string()),
        None => Capture::New,
    }
}

/// Kaydın parola alanını değiştirir (yoksa ekler).
pub fn set_password(body: &mut Body, password: &str) {
    match body.fields.iter_mut().find(|f| f.kind == FieldKind::Secret) {
        Some(f) => f.value = password.into(),
        None => body.fields.push(crate::entry::Field { label: carlui_core::t!("Parola", "Password").into(), value: password.into(), kind: FieldKind::Secret, fmt: Default::default() }),
    }
}

/// Yakalanan bilgiden yeni giriş kaydı.
pub fn new_login(url: &str, user: &str, password: &str) -> (crate::entry::Meta, Body) {
    let origin = origin(url).unwrap_or_default();
    let name = site_name(url);
    let i = crate::import::login(crate::import::Login { name: &name, url: &origin, user, password, totp: "", notes: "" });
    (i.meta, i.body)
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
        let f = |label: &str, value: &str, kind| Field { label: label.into(), value: value.into(), kind, fmt: Default::default() };
        let body = Body {
            fields: vec![f("Not", "x", FieldKind::Text), f("E-posta", "carl@ornek.com", FieldKind::Text), f("Parola", "s3cret", FieldKind::Secret), f("Adres", "https://github.com", FieldKind::Url)],
            file: None,
        };
        assert_eq!(credentials(&body), ("carl@ornek.com".into(), "s3cret".into()));
        assert!(body_matches(&body, "https://github.com/login"));
        assert!(!body_matches(&body, "https://gitlab.com/login"));
    }
    #[test]
    fn captured_logins() {
        assert_eq!(origin("https://www.Roblox.com/signup?ref=abc#x").as_deref(), Some("https://www.roblox.com"));
        assert_eq!(origin("http://localhost:8765/login").as_deref(), Some("http://localhost:8765"));
        assert_eq!(origin("https://user:pw@site.com/").as_deref(), Some("https://site.com"));
        assert_eq!(origin("file:///C:/x.html"), None);
        assert_eq!(site_name("https://www.roblox.com/signup"), "Roblox");
        assert_eq!(site_name("https://accounts.google.com/"), "Google");
        assert_eq!(site_name("https://giris.hepsiburada.com.tr/"), "Hepsiburada");
        assert_eq!(site_name("https://www.bbc.co.uk/"), "Bbc");
        assert_eq!(site_name("http://192.168.1.1/"), "192.168.1.1");
        assert_eq!(site_name("http://localhost:3000/"), "localhost");

        let (meta, body) = new_login("https://www.roblox.com/signup?ref=abc", "carl", "Parola-1");
        assert_eq!(meta.name, "Roblox");
        assert_eq!(credentials(&body), ("carl".into(), "Parola-1".into()));
        assert!(body.fields.iter().any(|f| f.kind == FieldKind::Url && f.value == "https://www.roblox.com"), "yol ve sorgu saklanmaz");
        assert!(body_matches(&body, "https://www.roblox.com/login"));

        let (_, other) = new_login("https://github.com/login", "carl", "x");
        let items = [("r", &body), ("g", &other)];
        assert_eq!(classify(items, "https://www.roblox.com/login", "Carl", "Parola-1"), Capture::Same);
        assert_eq!(classify(items, "https://www.roblox.com/login", "carl", "Yeni-2"), Capture::Changed("r".into()));
        assert_eq!(classify(items, "https://www.roblox.com/login", "baska", "Parola-1"), Capture::New);
        assert_eq!(classify(items, "https://www.roblox.com/login", "", "Yeni-2"), Capture::Changed("r".into()), "kullanıcı adı yoksa tek kayıt");
        assert_eq!(classify(items, "https://roblox.com.evil.io/login", "carl", "Parola-1"), Capture::New, "benzer alan adı eşlenmez");

        let mut b = body.clone();
        set_password(&mut b, "Yeni-2");
        assert_eq!(credentials(&b).1, "Yeni-2");
    }
}
