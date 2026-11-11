//! Hızlı Erişim yığını: kullanıcının seçtiği klasör, dosya ve uygulamalar tek
//! bir dock simgesinde toplanır; tıklayınca cam ızgarada açılır. Öğeler
//! `%APPDATA%\CarlUI\Hızlı Erişim` klasöründe kısayol (.lnk) olarak durur,
//! böylece Gezgin'den sürükleyip bırakarak da düzenlenebilir. İlk açılışta
//! Masaüstü, Belgeler, İndirilenler ve Resimler kısayollarıyla dolar.

use std::path::{Path, PathBuf};
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        System::Com::*,
        UI::Shell::{Common::ITEMIDLIST, *},
    },
};

/// Ayarlardaki `stacks` listesinde Hızlı Erişim'in adı.
pub const KEY: &str = "quick";
/// Dock simgesi: Gezgin'in Hızlı Erişim (yıldız) simgesi.
pub const ICON: &str = "shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}";

/// Hızlı Erişim klasörü; yoksa oluşturulur ve varsayılan klasörlerle doldurulur.
pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?.join("CarlUI");
    let d = base.join(carlui_core::t!("Hızlı Erişim", "Quick Access"));
    if !d.is_dir() {
        std::fs::create_dir_all(&d).ok()?;
        for name in ["desktop", "documents", "downloads", "pictures"] {
            if let Some(p) = carlui_core::win::stack_dir(name) {
                add_to(&d, &p.display().to_string());
            }
        }
    }
    Some(d)
}

/// Bir klasörü, dosyayı ya da uygulamayı (exe, .lnk ya da
/// `shell:AppsFolder\<AUMID>`) Hızlı Erişim'e ekler. Başarılıysa true.
pub fn add(target: &str) -> bool {
    dir().is_some_and(|d| add_to(&d, target))
}

fn add_to(d: &Path, target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    // Hazır kısayol: kopyalanır (simgesi ve argümanlarıyla).
    if target.to_lowercase().ends_with(".lnk") {
        let p = Path::new(target);
        let Some(name) = p.file_name() else { return false };
        return std::fs::copy(p, unique(d, &name.to_string_lossy())).is_ok();
    }
    unsafe {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if SHParseDisplayName(&HSTRING::from(target), None, &mut pidl, 0, None).is_err() || pidl.is_null() {
            return false;
        }
        let name = SHCreateItemFromIDList::<IShellItem>(pidl)
            .and_then(|i| i.GetDisplayName(SIGDN_NORMALDISPLAY))
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                s
            })
            .unwrap_or_default();
        let ok = (|| -> Result<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetIDList(pidl)?;
            let name = if name.is_empty() { Path::new(target).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Kısayol".into()) } else { name };
            let file = unique(d, &format!("{}.lnk", clean(&name)));
            link.cast::<IPersistFile>()?.Save(&HSTRING::from(file.as_os_str()), true)
        })()
        .is_ok();
        CoTaskMemFree(Some(pidl as _));
        ok
    }
}

/// Dosya adında olamayacak karakterleri atar.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect::<String>().trim().to_string()
}

/// Aynı adlı dosya varsa "ad (2).lnk" gibi boş bir ad bulur.
fn unique(d: &Path, file: &str) -> PathBuf {
    let p = d.join(file);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file.to_string(), String::new()),
    };
    (2..100).map(|n| d.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

/// Hızlı Erişim'deki öğeler, ada göre (kullanıcının düzeni; zamana göre değil).
pub fn items(d: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| !n.to_string_lossy().eq_ignore_ascii_case("desktop.ini")))
        .collect();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    v.truncate(40);
    v
}

/// Klasör ya da dosya seçtirir (Windows'un kendi penceresi).
pub fn pick(owner: HWND, folder: bool) -> Option<String> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let o = FOS_PATHMUSTEXIST | if folder { FOS_PICKFOLDERS } else { FOS_FILEMUSTEXIST | FOS_NODEREFERENCELINKS };
        let _ = dlg.SetOptions(o);
        let _ = dlg.SetTitle(&HSTRING::from(if folder { carlui_core::t!("Hızlı Erişim'e klasör ekle", "Add a folder to Quick Access") } else { carlui_core::t!("Hızlı Erişim'e uygulama ya da dosya ekle", "Add an app or file to Quick Access") }));
        dlg.Show(owner).ok()?;
        let p = dlg.GetResult().ok()?.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}
