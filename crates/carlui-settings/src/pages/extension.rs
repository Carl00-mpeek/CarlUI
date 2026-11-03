//! Bu modül "tarayıcı eklentisi kurulmuş klasörde duruyor ama nasıl
//! yükleneceği hiçbir yerde anlatılmıyor" sorununu Arama sayfasındaki kısa bir
//! rehberle çözüyor: klasörün yeri, tarayıcı başına adımlar ve klasörü ya da
//! tarayıcının eklenti sayfasını doğrudan açan düğmeler.

use crate::{ui::Flow, ui::P, Act};
use carlui_ui::paint::R;
use std::{path::PathBuf, sync::OnceLock};
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
    },
};

pub struct Browsers {
    pub dir: Option<PathBuf>,
    pub firefox: Option<String>,
    /// Chrome yoksa Edge: (exe, eklenti sayfası, ad).
    pub chromium: Option<(String, &'static str, &'static str)>,
}

fn reg_default(hive: HKEY, path: &str) -> Option<String> {
    unsafe {
        let p = HSTRING::from(path);
        let mut buf = [0u16; 520];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(hive, &p, None, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]).trim_matches('"').to_string())
    }
}

fn app_path(exe: &str) -> Option<String> {
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|h| reg_default(h, &key)).filter(|p| std::path::Path::new(p).exists())
}

/// Kurulumda exe'lerin yanındaki `extension`, geliştirmede depo kökündeki klasör.
fn ext_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let here = exe.parent()?;
    [here.join(r"extension\carlui-downloads"), here.join(r"..\..\extension\carlui-downloads")].into_iter().find(|p| p.join("manifest.json").exists()).and_then(|p| p.canonicalize().ok()).map(|p| PathBuf::from(p.display().to_string().trim_start_matches(r"\\?\")))
}

pub fn browsers() -> &'static Browsers {
    static B: OnceLock<Browsers> = OnceLock::new();
    B.get_or_init(|| Browsers {
        dir: ext_dir(),
        firefox: app_path("firefox.exe"),
        chromium: app_path("chrome.exe").map(|p| (p, "chrome://extensions", "Chrome")).or_else(|| app_path("msedge.exe").map(|p| (p, "edge://extensions", "Edge"))),
    })
}

pub fn paint(p: &mut P, f: &mut Flow) {
    let b = browsers();
    f.section(p, "Tarayıcı eklentisi");
    f.note(p, "İsteğe bağlı. Tarayıcıda başlayan indirmeleri adaya taşır; yüzde ve hız adada görünür.");

    let dir = b.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "Eklenti klasörü bulunamadı (CarlUI kurucuyla kurulmamış olabilir)".into());
    let r = f.row(p, "Eklenti klasörü", &dir, 150.0);
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), Some('\u{E838}'), "Klasörü aç", Act::Ext(0), b.dir.is_some(), false);

    let (name, label) = match &b.chromium {
        Some((_, _, n)) => (format!("{n} (kalıcı)"), format!("{n}'de aç")),
        None => ("Chrome / Edge (kalıcı)".to_string(), "Tarayıcı yok".to_string()),
    };
    let r = f.row(p, &name, "Geliştirici modu → Paketlenmemiş öğe yükle → eklenti klasörünü seçin.", 150.0);
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), None, &label, Act::Ext(2), b.chromium.is_some() && b.dir.is_some(), false);

    let r = f.row(p, "Firefox (geçici)", "Geçici Eklenti Yükle → klasördeki manifest.json. Firefox kapanınca silinir.", 150.0);
    let ff = if b.firefox.is_some() { "Firefox'ta aç" } else { "Firefox yok" };
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), None, ff, Act::Ext(1), b.firefox.is_some() && b.dir.is_some(), false);
    f.note(p, "Firefox'ta kalıcı kurulum Mozilla imzası ister; adımlar klasördeki KURULUM.md dosyasında.");
}
