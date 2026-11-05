//! Bu modül "yeni sürüm çıktığını nereden bileceğim, nasıl kuracağım?"
//! sorununu çözüyor: açılıştan bir dakika sonra ve sonra günde bir kez
//! GitHub'daki son sürüm (`releases/latest`) sorulur. Kurulu sürümden yeniyse
//! adada Güncelle / Sonra düğmeli bildirim çıkar (aynı sürüm için bir kez).
//! Güncelle'ye basınca `CarlUI-Setup.exe` indirilir, SHA-256'sı GitHub'ın
//! bildirdiğiyle karşılaştırılır ve kurucu `--update` ile açılır: penceresinde
//! ilerleme görünür, CarlUI'yi kapatır, dosyaları yeniler, ayarlara dokunmadan
//! CarlUI'yi yeniden açar ve sonunda Yenilikler'i gösterir.
//! Kendiliğinden hiçbir şey kurulmaz. Ayarlar → Genel → Güncellemeler'den
//! denetim kapatılabilir.

use crate::notify::Note;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use windows::{
    core::HSTRING,
    Foundation::Uri,
    Storage::Streams::DataReader,
    Web::Http::HttpClient,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

const API: &str = "https://api.github.com/repos/Carl00-mpeek/CarlUI/releases/latest";
const ASSET: &str = "CarlUI-Setup.exe";

/// GitHub'daki bir sürüm: etiket, sayfa, kurucunun adresi ve SHA-256'sı.
#[derive(Clone, Debug, Default)]
pub struct Release {
    pub tag: String,
    pub page: String,
    pub url: String,
    pub sha: String,
}

/// "v0.2.10" → [0, 2, 10]
fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parse(latest), parse(current));
    !a.is_empty() && a > b
}

fn client() -> Option<HttpClient> {
    let client = HttpClient::new().ok()?;
    // GitHub, User-Agent'sız istekleri reddeder.
    client.DefaultRequestHeaders().ok()?.UserAgent().ok()?.TryParseAdd(&HSTRING::from("CarlUI")).ok()?;
    Some(client)
}

/// Metindeki ilk 64 haneli onaltılık değer (sürüm notundaki SHA-256).
fn find_sha(text: &str) -> Option<String> {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(63)).find_map(|i| {
        let w = &text[i..i + 64];
        let edge = |j: usize| b.get(j).is_none_or(|c| !c.is_ascii_hexdigit());
        (w.bytes().all(|c| c.is_ascii_hexdigit()) && (i == 0 || edge(i - 1)) && edge(i + 64)).then(|| w.to_lowercase())
    })
}

/// GitHub yanıtından sürüm bilgisi. SHA önce dosyanın kendi özetinden
/// (`digest`), yoksa sürüm notundan alınır.
fn release_from(v: &serde_json::Value) -> Option<Release> {
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let page = v.get("html_url").and_then(|u| u.as_str()).unwrap_or("https://github.com/Carl00-mpeek/CarlUI/releases").to_string();
    let asset = v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(ASSET));
    let url = asset.and_then(|a| a.get("browser_download_url")?.as_str()).unwrap_or_default().to_string();
    let sha = asset
        .and_then(|a| a.get("digest")?.as_str()?.strip_prefix("sha256:").map(str::to_lowercase))
        .or_else(|| v.get("body").and_then(|b| b.as_str()).and_then(find_sha))
        .unwrap_or_default();
    Some(Release { tag, page, url, sha })
}

fn latest(api: &str) -> Option<Release> {
    let text = client()?.GetStringAsync(&Uri::CreateUri(&HSTRING::from(api)).ok()?).ok()?.get().ok()?.to_string();
    release_from(&serde_json::from_str(&text).ok()?)
}

fn seen_file() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("update-seen.txt")
}

fn push(hwnd: isize, msg: u32, notes: &Mutex<VecDeque<Note>>, n: Note) {
    if let Ok(mut q) = notes.lock() {
        q.push_back(n);
    }
    unsafe {
        let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(0), LPARAM(0));
    }
}

fn check(hwnd: isize, msg: u32, notes: &Mutex<VecDeque<Note>>) {
    if !carlui_core::config::load().general.check_updates {
        return;
    }
    let Some(r) = latest(API) else {
        carlui_core::info!("güncelleme denetimi: yanıt alınamadı");
        return;
    };
    // CARLUI_FAKE_VERSION (deneme): kurulu sürümü eski göster, akış uçtan uca denensin.
    let fake = std::env::var("CARLUI_FAKE_VERSION").unwrap_or_default();
    let current = if fake.is_empty() { env!("CARGO_PKG_VERSION") } else { fake.as_str() };
    if !newer(&r.tag, current) || std::fs::read_to_string(seen_file()).is_ok_and(|s| s.trim() == r.tag) {
        return;
    }
    carlui_core::info!("yeni sürüm: {} (kurulu {current})", r.tag);
    let _ = std::fs::write(seen_file(), &r.tag);
    let ver = r.tag.trim_start_matches(['v', 'V']).to_string();
    // Kurucu ya da özeti yoksa kurulamaz: eskisi gibi sayfayı aç.
    let ready = !r.url.is_empty() && r.sha.len() == 64;
    push(
        hwnd,
        msg,
        notes,
        Note {
            app: "CarlUI".into(),
            title: t!(format!("CarlUI {ver} çıktı"), format!("CarlUI {ver} is out")),
            body: if ready { t!("Yeni sürüm kurulmaya hazır", "A new version is ready to install") } else { t!("İndirme sayfasını açmak için tıkla", "Click to open the download page") }.into(),
            file: (!ready).then(|| r.page.clone()),
            update: ready.then_some(r),
            ..Default::default()
        },
    );
}

/// Denetimi arka planda başlatır; yeni sürüm `notes`'a eklenip `msg` gönderilir.
pub fn start(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            // Önceki güncellemenin kurucusu (o çalışırken silinemezdi).
            if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with("CarlUI-Setup-") && n.ends_with(".exe") {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(60));
            loop {
                check(hwnd, msg, &notes);
                std::thread::sleep(Duration::from_secs(24 * 3600));
            }
        })
        .ok();
}

fn sha256(data: &[u8]) -> String {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if st.is_err() {
        return String::new();
    }
    out.iter().map(|b| format!("{b:02x}")).collect()
}

fn download(url: &str) -> Option<Vec<u8>> {
    let buf = client()?.GetBufferAsync(&Uri::CreateUri(&HSTRING::from(url)).ok()?).ok()?.get().ok()?;
    let n = buf.Length().ok()? as usize;
    let reader = DataReader::FromBuffer(&buf).ok()?;
    let mut out = vec![0u8; n];
    reader.ReadBytes(&mut out).ok()?;
    Some(out)
}

/// İndir, doğrula, kur. Hata olursa bildirim gelir ve CarlUI açık kalır.
fn install(r: &Release) -> Result<(), String> {
    let data = download(&r.url).ok_or(t!("kurucu indirilemedi", "couldn't download the installer"))?;
    let got = sha256(&data);
    if got != r.sha {
        carlui_core::warn!("güncelleme: SHA uyuşmuyor ({got} ≠ {})", r.sha);
        return Err(t!("indirilen dosya doğrulanamadı", "the downloaded file failed verification").into());
    }
    let path = std::env::temp_dir().join(format!("CarlUI-Setup-{}.exe", r.tag));
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    carlui_core::info!("güncelleme doğrulandı, kuruluyor: {}", path.display());
    // Kurucu CarlUI'yi kapatır; CarlUI'nin iş nesnesinden ayrıldığı için kendisi kapanmaz.
    std::process::Command::new(&path).arg("--update").spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Güncelle'ye basınca: arka planda indirip kurucuyu güncelleme kipinde açar.
pub fn apply(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>, r: Release) {
    let ver = r.tag.trim_start_matches(['v', 'V']).to_string();
    push(hwnd, msg, &notes, Note { app: "CarlUI".into(), title: t!(format!("CarlUI {ver} indiriliyor"), format!("Downloading CarlUI {ver}")), body: t!("Kurulunca CarlUI yeniden açılacak", "CarlUI will restart when it's installed").into(), ..Default::default() });
    std::thread::Builder::new()
        .name("update-apply".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            if let Err(e) = install(&r) {
                carlui_core::warn!("güncelleme kurulamadı: {e}");
                push(hwnd, msg, &notes, Note { app: "CarlUI".into(), title: t!("Güncelleme kurulamadı", "Update failed").into(), body: format!("{e} · {}", t!("sayfayı açmak için tıkla", "click to open the page")), file: Some(r.page.clone()), ..Default::default() });
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.0.9", "0.1.0"));
        assert!(!newer("nightly", "0.1.0"));
    }

    #[test]
    fn release_json() {
        let h = "66a219252af6efd700b2dc04fbbb90eee1ee0f8d9edde94c239dbb6b035fab04";
        let with_digest = serde_json::json!({"tag_name":"v0.2.0","html_url":"https://x/r","body":"",
            "assets":[{"name":"CarlUI-Setup.exe","browser_download_url":"https://x/s.exe","digest":format!("sha256:{h}")}]});
        let r = release_from(&with_digest).unwrap();
        assert_eq!((r.tag.as_str(), r.url.as_str(), r.sha.as_str()), ("v0.2.0", "https://x/s.exe", h));
        let from_body = serde_json::json!({"tag_name":"v0.2.0","body":format!("**SHA-256:** `{h}`"),"assets":[{"name":"CarlUI-Setup.exe","browser_download_url":"u"}]});
        assert_eq!(release_from(&from_body).unwrap().sha, h);
        assert_eq!(find_sha("x 1234"), None);
    }

    #[test]
    fn hash() {
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// İnternet ister: `cargo test -p carlui-island -- --ignored github`
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
