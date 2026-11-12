//! CarlUI güncellemesinin ortak kısmı: GitHub'daki son sürümü sormak,
//! kurucuyu indirip SHA-256'sını doğrulamak ve kurucuyu `--update` ile açmak.
//! Sorun: aynı iş hem adada (günlük denetim, bildirim) hem ayarlardaki
//! Güncelleme sayfasında gerekiyordu; iki ayrı kopya ileride birbirinden
//! ayrılırdı (özellikle doğrulama). Çözüm: ağ ve doğrulama yalnızca burada.
//! Buradaki ağ çağrıları beklemeli; pencere iş parçacığından çağrılmamalı.

use std::path::PathBuf;
use windows::{core::HSTRING, Foundation::Uri, Storage::Streams::DataReader, Web::Http::HttpClient};

pub const API: &str = "https://api.github.com/repos/Carl00-mpeek/CarlUI/releases/latest";
pub const ASSET: &str = "CarlUI-Setup.exe";
const RELEASES: &str = "https://github.com/Carl00-mpeek/CarlUI/releases";

/// GitHub'daki bir sürüm: etiket, sayfa, kurucunun adresi, SHA-256'sı ve
/// sürüm notunun metni.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    pub page: String,
    pub url: String,
    pub sha: String,
    pub notes: String,
}

impl Release {
    /// "v1.2.0" → "1.2.0"
    pub fn version(&self) -> &str {
        self.tag.trim_start_matches(['v', 'V'])
    }

    /// Kurucu ve özeti varsa uygulama içinden kurulabilir; yoksa yalnızca
    /// sayfası açılabilir.
    pub fn installable(&self) -> bool {
        !self.url.is_empty() && self.sha.len() == 64
    }
}

/// Kurulu sürüm. CARLUI_FAKE_VERSION (deneme) kurulu sürümü eski gösterir,
/// akış uçtan uca denenebilsin diye.
pub fn current() -> String {
    std::env::var("CARLUI_FAKE_VERSION").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// "v0.2.10" → [0, 2, 10]
fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

/// `latest`, `current`'tan yeni mi? Sayıya çevrilemeyen etiket yeni sayılmaz.
pub fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parse(latest), parse(current));
    !a.is_empty() && a > b
}

/// Metindeki ilk 64 haneli onaltılık değer (sürüm notundaki SHA-256).
pub fn find_sha(text: &str) -> Option<String> {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(63)).find_map(|i| {
        let w = text.get(i..i + 64)?;
        let edge = |j: usize| b.get(j).is_none_or(|c| !c.is_ascii_hexdigit());
        (w.bytes().all(|c| c.is_ascii_hexdigit()) && (i == 0 || edge(i - 1)) && edge(i + 64)).then(|| w.to_lowercase())
    })
}

/// GitHub yanıtından sürüm bilgisi. SHA önce dosyanın kendi özetinden
/// (`digest`), yoksa sürüm notundan alınır.
pub fn release_from(v: &serde_json::Value) -> Option<Release> {
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let page = v.get("html_url").and_then(|u| u.as_str()).unwrap_or(RELEASES).to_string();
    let body = v.get("body").and_then(|b| b.as_str()).unwrap_or_default();
    let asset = v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET));
    let url = asset.and_then(|a| a.get("browser_download_url")?.as_str()).unwrap_or_default().to_string();
    let sha = asset.and_then(|a| a.get("digest")?.as_str()?.strip_prefix("sha256:").map(str::to_lowercase)).or_else(|| find_sha(body)).unwrap_or_default();
    Some(Release { tag, page, url, sha, notes: body.to_string() })
}

/// Sürüm notundan kısa bir özet: Markdown işaretleri, başlıklar, SHA ve
/// bağlantı satırları atılır; ilk `max_lines` madde, en çok `max_chars` harf.
pub fn notes_excerpt(body: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut total = 0;
    for raw in body.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with("```") || l.starts_with('|') || l.starts_with("---") {
            continue;
        }
        if find_sha(l).is_some() || l.to_lowercase().contains("sha-256") || l.contains("http://") || l.contains("https://") {
            continue;
        }
        let l = l.trim_start_matches(['-', '*', '+', '>', ' ']).trim();
        let clean: String = l.chars().filter(|c| !matches!(c, '*' | '`' | '_')).collect();
        let clean = clean.trim().to_string();
        if clean.is_empty() {
            continue;
        }
        if out.len() >= max_lines || total >= max_chars {
            break;
        }
        let room = max_chars - total;
        let line = if clean.chars().count() > room {
            let cut: String = clean.chars().take(room.saturating_sub(1)).collect();
            format!("{}…", cut.trim_end())
        } else {
            clean
        };
        total += line.chars().count();
        out.push(line);
    }
    out
}

fn client() -> Option<HttpClient> {
    let client = HttpClient::new().ok()?;
    // GitHub, User-Agent'sız istekleri reddeder.
    client.DefaultRequestHeaders().ok()?.UserAgent().ok()?.TryParseAdd(&HSTRING::from("CarlUI")).ok()?;
    Some(client)
}

/// Son sürümü sorar (bekler). Hata metni kullanıcıya gösterilebilir.
pub fn latest(api: &str) -> Result<Release, String> {
    let offline = || t!("GitHub'a bağlanılamadı. İnternet bağlantını denetleyip yeniden dene.", "Couldn't reach GitHub. Check your internet connection and try again.").to_string();
    let c = client().ok_or_else(offline)?;
    let uri = Uri::CreateUri(&HSTRING::from(api)).map_err(|e| e.to_string())?;
    let text = c.GetStringAsync(&uri).and_then(|op| op.get()).map_err(|e| {
        crate::warn!("güncelleme denetimi: {e}");
        offline()
    })?;
    let v: serde_json::Value = serde_json::from_str(&text.to_string()).map_err(|_| t!("GitHub'ın yanıtı okunamadı.", "GitHub's reply couldn't be read.").to_string())?;
    release_from(&v).ok_or_else(|| t!("Yayımlanmış bir sürüm bulunamadı.", "No published release was found.").to_string())
}

fn checked_file() -> PathBuf {
    crate::paths::data_dir().join("update-checked.txt")
}

/// Başarılı bir denetimin yerel saatini yazar ("2026-11-12 14:05").
pub fn record_check() {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let s = format!("{:04}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute);
    let _ = std::fs::write(checked_file(), s);
}

/// Son başarılı denetimin zamanı, dile göre biçimlenmiş; hiç yoksa `None`.
pub fn last_check() -> Option<String> {
    let s = std::fs::read_to_string(checked_file()).ok()?;
    format_stamp(s.trim(), crate::lang::is_tr())
}

/// "2026-11-12 14:05" → Türkçede "12.11.2026 14:05", İngilizcede olduğu gibi.
pub fn format_stamp(s: &str, tr: bool) -> Option<String> {
    let (date, time) = s.split_once(' ')?;
    let p: Vec<&str> = date.split('-').collect();
    let [y, m, d] = p[..] else { return None };
    if [y, m, d, &time.replace(':', "")].iter().any(|x| x.is_empty() || !x.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(if tr { format!("{d}.{m}.{y} {time}") } else { format!("{y}-{m}-{d} {time}") })
}

pub fn sha256(data: &[u8]) -> String {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if st.is_err() {
        return String::new();
    }
    out.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn download(url: &str) -> Option<Vec<u8>> {
    let buf = client()?.GetBufferAsync(&Uri::CreateUri(&HSTRING::from(url)).ok()?).ok()?.get().ok()?;
    let n = buf.Length().ok()? as usize;
    let reader = DataReader::FromBuffer(&buf).ok()?;
    let mut out = vec![0u8; n];
    reader.ReadBytes(&mut out).ok()?;
    Some(out)
}

/// Kurulumun aşamaları (ilerleme göstermek için).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Downloading,
    Verifying,
    Launching,
}

/// İndirilen veri beklenen özetle tam eşleşmiyorsa hata. Beklenen özet
/// boşsa ya da 64 hane değilse de hata: doğrulanamayan dosya asla açılmaz.
pub fn verify(data: &[u8], expected: &str) -> Result<(), String> {
    let got = sha256(data);
    if expected.len() != 64 || got.is_empty() || got != expected.to_lowercase() {
        crate::warn!("güncelleme: SHA uyuşmuyor ({got} ≠ {expected})");
        return Err(t!("indirilen dosya doğrulanamadı", "the downloaded file failed verification").into());
    }
    Ok(())
}

/// İndir, doğrula, kurucuyu `--update` ile aç. `stage` her aşamada çağrılır.
/// Kurucu CarlUI'yi kapatır, dosyaları yeniler ve yeniden açar.
pub fn install(r: &Release, stage: impl Fn(Stage)) -> Result<(), String> {
    stage(Stage::Downloading);
    let data = download(&r.url).ok_or(t!("kurucu indirilemedi", "couldn't download the installer"))?;
    stage(Stage::Verifying);
    verify(&data, &r.sha)?;
    let path = std::env::temp_dir().join(format!("CarlUI-Setup-{}.exe", r.tag));
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    crate::info!("güncelleme doğrulandı, kuruluyor: {}", path.display());
    stage(Stage::Launching);
    // Kurucu CarlUI'yi kapatır; CarlUI'nin iş nesnesinden ayrıldığı için kendisi kapanmaz.
    std::process::Command::new(&path).arg("--update").spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Önceki güncellemenin kurucusu (o çalışırken silinemezdi).
pub fn cleanup_old() {
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("CarlUI-Setup-") && n.ends_with(".exe") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(newer("v1.0.1", "1.0.0"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.0.9", "0.1.0"));
        assert!(!newer("nightly", "0.1.0"));
        assert!(!newer("v1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn release_json() {
        let h = "66a219252af6efd700b2dc04fbbb90eee1ee0f8d9edde94c239dbb6b035fab04";
        let with_digest = serde_json::json!({"tag_name":"v0.2.0","html_url":"https://x/r","body":"",
            "assets":[{"name":"CarlUI-Setup.exe","browser_download_url":"https://x/s.exe","digest":format!("sha256:{h}")}]});
        let r = release_from(&with_digest).unwrap();
        assert_eq!((r.tag.as_str(), r.url.as_str(), r.sha.as_str()), ("v0.2.0", "https://x/s.exe", h));
        assert!(r.installable());
        assert_eq!(r.version(), "0.2.0");
        let from_body = serde_json::json!({"tag_name":"v0.2.0","body":format!("**SHA-256:** `{h}`"),"assets":[{"name":"CarlUI-Setup.exe","browser_download_url":"u"}]});
        assert_eq!(release_from(&from_body).unwrap().sha, h);
        let no_asset = serde_json::json!({"tag_name":"v0.2.0","assets":[]});
        assert!(!release_from(&no_asset).unwrap().installable());
        assert_eq!(find_sha("x 1234"), None);
        // Çok baytlı harfler dilimlemeyi bozmasın.
        assert_eq!(find_sha("çğüşöı İ"), None);
    }

    #[test]
    fn hash_and_verify() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256(b"abc"), abc);
        assert!(verify(b"abc", abc).is_ok());
        assert!(verify(b"abc", &abc.to_uppercase()).is_ok());
        assert!(verify(b"abd", abc).is_err());
        // Özeti olmayan dosya doğrulanmış sayılmaz.
        assert!(verify(b"abc", "").is_err());
    }

    #[test]
    fn excerpt() {
        let h = "66a219252af6efd700b2dc04fbbb90eee1ee0f8d9edde94c239dbb6b035fab04";
        let body = format!("## Yenilikler\n\n- **Hayvan** tepkileri\n* Güvenli iç haberleşme\n\n**SHA-256:** `{h}`\nhttps://x/y\n- Üçüncü\n- Dördüncü");
        assert_eq!(notes_excerpt(&body, 3, 200), vec!["Hayvan tepkileri", "Güvenli iç haberleşme", "Üçüncü"]);
        let cut = notes_excerpt("- abcdefghij\n- klm", 5, 6);
        assert_eq!(cut, vec!["abcde…"]);
        assert!(notes_excerpt("", 3, 100).is_empty());
    }

    #[test]
    fn stamps() {
        assert_eq!(format_stamp("2026-11-12 14:05", true).as_deref(), Some("12.11.2026 14:05"));
        assert_eq!(format_stamp("2026-11-12 14:05", false).as_deref(), Some("2026-11-12 14:05"));
        assert_eq!(format_stamp("bozuk", true), None);
        assert_eq!(format_stamp("2026-11 14:05", true), None);
    }

    /// İnternet ister: `cargo test -p carlui-core -- --ignored github`
    #[test]
    #[ignore]
    fn github() {
        let r = latest(API).expect("yanıt yok");
        println!("{r:?}");
        assert!(r.url.ends_with(ASSET), "{r:?}");
        let data = download(&r.url).expect("indirilemedi");
        assert_eq!(sha256(&data), r.sha);
    }
}
