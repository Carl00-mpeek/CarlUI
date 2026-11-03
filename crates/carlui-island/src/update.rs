//! Bu modül "yeni sürüm çıktığını nereden bileceğim?" sorununu çözüyor:
//! açılıştan bir dakika sonra ve sonra günde bir kez GitHub'daki son
//! sürüm (`releases/latest`) sorulur. Kurulu sürümden yeniyse adada bir
//! bildirim çıkar; tıklayınca sürüm sayfası açılır. Hiçbir şey kendiliğinden
//! indirilmez ya da kurulmaz. Aynı sürüm için bildirim bir kez gösterilir.
//! Ayarlar → Genel → Güncellemeler'den kapatılabilir.

use crate::notify::Note;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use windows::{
    core::HSTRING,
    Foundation::Uri,
    Web::Http::HttpClient,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

const API: &str = "https://api.github.com/repos/Carl00-mpeek/CarlUI/releases/latest";

/// "v0.2.10" → [0, 2, 10]
fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parse(latest), parse(current));
    !a.is_empty() && a > b
}

/// Son sürümün etiketi ve sayfa adresi.
fn latest(api: &str) -> Option<(String, String)> {
    let client = HttpClient::new().ok()?;
    // GitHub, User-Agent'sız istekleri reddeder.
    client.DefaultRequestHeaders().ok()?.UserAgent().ok()?.TryParseAdd(&HSTRING::from("CarlUI")).ok()?;
    let text = client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(api)).ok()?).ok()?.get().ok()?.to_string();
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let url = v.get("html_url").and_then(|u| u.as_str()).unwrap_or("https://github.com/Carl00-mpeek/CarlUI/releases").to_string();
    Some((tag, url))
}

fn seen_file() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("update-seen.txt")
}

fn check(hwnd: isize, msg: u32, notes: &Mutex<VecDeque<Note>>) {
    if !carlui_core::config::load().general.check_updates {
        return;
    }
    let Some((tag, url)) = latest(API) else {
        carlui_core::info!("güncelleme denetimi: yanıt alınamadı");
        return;
    };
    let current = env!("CARGO_PKG_VERSION");
    if !newer(&tag, current) || std::fs::read_to_string(seen_file()).is_ok_and(|s| s.trim() == tag) {
        return;
    }
    carlui_core::info!("yeni sürüm: {tag} (kurulu {current})");
    let _ = std::fs::write(seen_file(), &tag);
    let ver = tag.trim_start_matches(['v', 'V']);
    let note = Note {
        app: "CarlUI".into(),
        title: t!(format!("CarlUI {ver} çıktı"), format!("CarlUI {ver} is out")),
        body: t!("İndirme sayfasını açmak için tıkla", "Click to open the download page").into(),
        file: Some(url),
        ..Default::default()
    };
    if let Ok(mut q) = notes.lock() {
        q.push_back(note);
    }
    unsafe {
        let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(0), LPARAM(0));
    }
}

/// Denetimi arka planda başlatır; yeni sürüm `notes`'a eklenip `msg` gönderilir.
pub fn start(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            std::thread::sleep(Duration::from_secs(60));
            loop {
                check(hwnd, msg, &notes);
                std::thread::sleep(Duration::from_secs(24 * 3600));
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

    /// İnternet ister: `cargo test -p carlui-island -- --ignored github`
    #[test]
    #[ignore]
    fn github() {
        let (tag, url) = latest("https://api.github.com/repos/microsoft/terminal/releases/latest").expect("yanıt yok");
        assert!(!parse(&tag).is_empty(), "{tag}");
        assert!(url.starts_with("https://github.com/"), "{url}");
        println!("{tag} {url}");
    }
}
