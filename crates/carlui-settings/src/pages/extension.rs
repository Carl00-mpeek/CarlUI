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
    f.section(p, t!("Tarayıcı eklentisi", "Browser extension"));
    f.note(p, t!("İsteğe bağlı. Tarayıcıda başlayan indirmeleri adaya taşır (yüzde ve hız adada görünür) ve giriş sayfalarını Carl Vault kasasından doldurur (eklenti düğmesi ya da Ctrl+Shift+L).", "Optional. Shows downloads started in the browser on the island (with progress and speed) and fills sign-in pages from the Carl Vault (extension button or Ctrl+Shift+L)."));

    let dir = b.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| t!("Eklenti klasörü bulunamadı (CarlUI kurucuyla kurulmamış olabilir)", "Extension folder not found (CarlUI may not have been installed with the installer)").into());
    let r = f.row(p, t!("Eklenti klasörü", "Extension folder"), &dir, 150.0);
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), Some('\u{E838}'), t!("Klasörü aç", "Open folder"), Act::Ext(0), b.dir.is_some(), false);

    let (name, label) = match &b.chromium {
        Some((_, _, n)) => (t!(format!("{n} (kalıcı)"), format!("{n} (permanent)")), t!(format!("{n}'de aç"), format!("Open in {n}"))),
        None => (t!("Chrome / Edge (kalıcı)", "Chrome / Edge (permanent)").to_string(), t!("Tarayıcı yok", "No browser").to_string()),
    };
    let r = f.row(p, &name, t!("Geliştirici modu → Paketlenmemiş öğe yükle → eklenti klasörünü seçin.", "Developer mode → Load unpacked → choose the extension folder."), 150.0);
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), None, &label, Act::Ext(2), b.chromium.is_some() && b.dir.is_some(), false);

    let r = f.row(p, t!("Firefox (geçici)", "Firefox (temporary)"), t!("Geçici Eklenti Yükle → klasördeki manifest.json. Firefox kapanınca silinir.", "Load Temporary Add-on → manifest.json in the folder. Removed when Firefox closes."), 150.0);
    let ff = if b.firefox.is_some() { t!("Firefox'ta aç", "Open in Firefox") } else { t!("Firefox yok", "No Firefox") };
    p.button(R::new(r.right() - 16.0 - 134.0, r.y + (r.h - 32.0) / 2.0, 134.0, 32.0), None, ff, Act::Ext(1), b.firefox.is_some() && b.dir.is_some(), false);
    f.note(p, t!("Firefox'ta kalıcı kurulum Mozilla imzası ister; adımlar klasördeki KURULUM.md dosyasında.", "A permanent Firefox install needs Mozilla signing; steps are in KURULUM.md in the folder."));
}
