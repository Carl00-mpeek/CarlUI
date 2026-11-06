//! Artık aramasında kullanılan adlar. Yanlış şeyi silmemek için benzerlik değil
//! tam eşleşme aranır: `7-Zip 23.01 (x64)` için yalnızca `7zip` adlı klasör
//! sayılır, `7zip-yedek` sayılmaz.

use crate::apps::App;
use std::collections::HashSet;

/// En kısa genel ad. Daha kısası (`vlc`) başka programlarla çakışabilir; böyle
/// adlar yalnızca yayıncı klasörünün içinde aranır.
const MIN_TOP: usize = 4;
const MIN_SCOPED: usize = 3;

/// Hiçbir uygulamanın adı sayılmayan, ortak kullanılan klasör adları.
const DENY: &[&str] = &[
    "microsoft", "windows", "office", "commonfiles", "programfiles", "programfilesx86", "programdata", "system32", "syswow64", "temp", "tmp", "users", "public", "default", "packages", "google", "mozilla", "nvidia", "intel", "amd", "adobe", "apple", "update", "updater", "updates", "installer", "installers", "install", "setup", "uninstall", "app", "apps", "application", "applications", "software", "data", "cache", "caches", "logs", "config", "configs", "settings", "local", "roaming", "locallow", "desktop", "documents", "downloads", "music", "pictures", "videos", "programs", "startup", "startmenu", "crashdumps", "crashreports", "accessories", "accessibility", "administrativetools", "systemtools", "maintenance", "windowspowershell", "windowstools", "drivers", "bin", "common", "shared", "runtime", "tools", "utilities", "plugins", "carlui",
];

/// Birden çok programın ortak klasörü olduğu için içinde arama yapılmayan yayıncı.
const DENY_PUB: &[&str] = &["microsoft", "windows"];

/// Şirket adlarının sonundaki ekler.
const SUFFIX: &[&str] = &["inc", "llc", "ltd", "limited", "gmbh", "corp", "corporation", "co", "company", "sa", "ag", "bv", "srl", "pty", "as", "aş", "ab", "software"];

/// Karşılaştırma biçimi: yalnızca küçük harf ve rakam.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn versionish(t: &str) -> bool {
    let l = t.to_lowercase();
    let v = l.strip_prefix('v').unwrap_or(&l);
    v.starts_with(|c: char| c.is_ascii_digit()) && !v.contains(|c: char| c.is_alphabetic() && c != 'x' && c != 'b')
        || matches!(l.as_str(), "x64" | "x86" | "x32" | "amd64" | "arm64" | "64-bit" | "32-bit" | "64bit" | "32bit" | "version" | "build" | "-")
}

/// Sürüm ve mimari eklerini atar: `7-Zip 23.01 (x64)` → `7-Zip`.
pub fn base(name: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let mut parts: Vec<&str> = plain.split_whitespace().collect();
    while parts.len() > 1 && parts.last().is_some_and(|t| versionish(t)) {
        parts.pop();
    }
    parts.join(" ")
}

fn company(publisher: &str) -> String {
    let clean: String = publisher.chars().map(|c| if c == ',' || c == '.' { ' ' } else { c }).collect();
    let mut parts: Vec<&str> = clean.split_whitespace().collect();
    while parts.len() > 1 && parts.last().is_some_and(|t| SUFFIX.contains(&t.to_lowercase().as_str())) {
        parts.pop();
    }
    parts.join(" ")
}

pub fn leaf(path: &str) -> &str {
    path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("")
}

#[derive(Debug, Default, Clone)]
pub struct Names {
    /// Doğrudan `%APPDATA%` gibi köklerde aranır.
    pub top: HashSet<String>,
    /// Yalnızca yayıncı klasörünün içinde aranır.
    pub scoped: HashSet<String>,
    pub pubs: HashSet<String>,
}

impl Names {
    pub fn of(app: &App) -> Self {
        let mut n = Self::default();
        let short = norm(&base(&app.name));
        let company = norm(&company(&app.publisher));
        // `Google Chrome` → yayıncı klasöründe `Chrome`.
        if !company.is_empty() {
            if let Some(rest) = short.strip_prefix(&company) {
                n.scoped.insert(rest.to_string());
            }
        }
        for s in [norm(&app.name), short, norm(leaf(&app.location))] {
            n.scoped.insert(s.clone());
            n.top.insert(s);
        }
        n.top.retain(|s| s.chars().count() >= MIN_TOP && !DENY.contains(&s.as_str()));
        n.scoped.retain(|s| s.chars().count() >= MIN_SCOPED && !DENY.contains(&s.as_str()));
        for s in [company, norm(&app.publisher)] {
            if s.chars().count() >= MIN_SCOPED && !DENY_PUB.iter().any(|d| s.starts_with(d)) {
                n.pubs.insert(s);
            }
        }
        n
    }

    /// Başka bir kurulu uygulamanın da kullandığı adlar bu uygulamanın sayılmaz.
    pub fn without(mut self, taken: &HashSet<String>) -> Self {
        self.top.retain(|s| !taken.contains(s));
        self.scoped.retain(|s| !taken.contains(s));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, publisher: &str, location: &str) -> App {
        App { name: name.into(), publisher: publisher.into(), location: location.into(), ..Default::default() }
    }

    #[test]
    fn strips_version_and_arch() {
        assert_eq!(base("7-Zip 23.01 (x64)"), "7-Zip");
        assert_eq!(base("Foo Bar v2.1 x64"), "Foo Bar");
        assert_eq!(base("Notepad++ (64-bit x64)"), "Notepad++");
        assert_eq!(base("Paint 3D"), "Paint 3D");
        assert_eq!(base("1Password"), "1Password");
    }

    #[test]
    fn publisher_prefix_is_scoped_only() {
        let n = Names::of(&app("Google Chrome", "Google LLC", r"C:\Program Files\Google\Chrome\Application"));
        assert!(n.top.contains("googlechrome"));
        assert!(n.scoped.contains("chrome"));
        assert!(!n.top.contains("chrome"));
        assert!(n.pubs.contains("google"));
    }

    #[test]
    fn generic_names_never_match() {
        let n = Names::of(&app("Update", "Microsoft Corporation", r"C:\Program Files\Common"));
        assert!(n.top.is_empty() && n.scoped.is_empty() && n.pubs.is_empty());
    }

    #[test]
    fn short_name_stays_in_publisher_folder() {
        let n = Names::of(&app("VLC", "VideoLAN", r"C:\Program Files\VideoLAN\VLC"));
        assert!(n.top.is_empty());
        assert!(n.scoped.contains("vlc") && n.pubs.contains("videolan"));
    }

    #[test]
    fn names_shared_with_other_apps_are_dropped() {
        let taken: HashSet<String> = ["sharedthing".to_string()].into();
        let n = Names::of(&app("Shared Thing", "", "")).without(&taken);
        assert!(n.top.is_empty());
    }
}
