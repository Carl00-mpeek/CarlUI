//! Kurulu uygulamalar: Windows'un kaldırma kayıtları ve Mağaza paketleri.

use crate::{
    names::norm,
    reg::{self, Key},
    run,
};
use std::path::Path;
use windows::{
    core::HSTRING,
    ApplicationModel::{Package, PackageSignatureKind},
    Management::Deployment::PackageManager,
};

const ROOTS: [&str; 3] = [r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall"];

#[derive(Clone, Debug, Default)]
pub struct App {
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// `YYYYMMDD`; bilinmiyorsa boş.
    pub date: String,
    pub size: u64,
    pub location: String,
    pub icon: String,
    pub uninstall: String,
    /// Kaldırma kaydının tam yolu; Mağaza uygulamalarında boş.
    pub key: String,
    /// Mağaza paketinin tam adı; masaüstü uygulamalarında boş.
    pub package: String,
    /// Kaldırıcı dosyası diskte yok: kayıt ölü.
    pub stale: bool,
}

impl App {
    pub fn is_store(&self) -> bool {
        !self.package.is_empty()
    }

    pub fn still_there(&self) -> bool {
        !self.key.is_empty() && reg::exists(&self.key)
    }
}

/// `"C:\a.exe",0` → `C:\a.exe`.
fn clean_icon(s: &str) -> String {
    let s = s.trim().trim_matches('"');
    let s = match s.rsplit_once(',') {
        Some((p, n)) if n.trim().parse::<i32>().is_ok() => p,
        _ => s,
    };
    s.trim().trim_matches('"').to_string()
}

/// FILETIME değerini `YYYYMMDD` metnine çevirir (UTC).
fn day(filetime: u64) -> String {
    // 1601-01-01 ile 1970-01-01 arası 134774 gün.
    let z = (filetime / 864_000_000_000) as i64 - 134_774 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

/// Kurulum günü. Birçok kurucu `InstallDate` yazmaz; o zaman kaydın yazıldığı gün
/// sayılır, yoksa yeni kurulan uygulama listenin dibinde kalırdı.
fn install_day(k: &Key) -> String {
    k.string("InstallDate").filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())).or_else(|| k.written().map(day)).unwrap_or_default()
}

fn read(root: &str, sub: &str) -> Option<App> {
    let path = format!(r"{root}\{sub}");
    let k = Key::open(&path)?;
    let name = k.string("DisplayName")?;
    if k.dword("SystemComponent") == Some(1) || k.dword("NoRemove") == Some(1) || k.string("ParentKeyName").is_some() {
        return None;
    }
    if k.string("ReleaseType").is_some_and(|t| t.contains("Update") || t.contains("Hotfix")) {
        return None;
    }
    let msi = k.dword("WindowsInstaller") == Some(1) && sub.starts_with('{');
    let uninstall = if msi { format!("MsiExec.exe /X{sub}") } else { run::msi_fix(&k.string("UninstallString")?) };
    let icon = k.string("DisplayIcon").map(|s| clean_icon(&s)).filter(|p| Path::new(p).exists()).unwrap_or_default();
    Some(App {
        publisher: k.string("Publisher").unwrap_or_default(),
        version: k.string("DisplayVersion").unwrap_or_default(),
        date: install_day(&k),
        size: k.dword("EstimatedSize").unwrap_or(0) as u64 * 1024,
        location: k.string("InstallLocation").map(|s| s.trim_matches('"').trim_end_matches('\\').to_string()).unwrap_or_default(),
        stale: run::missing(&uninstall),
        icon,
        uninstall,
        key: path,
        name,
        package: String::new(),
    })
}

fn desktop() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for root in ROOTS {
        let Some(k) = Key::open(root) else { continue };
        for sub in k.subkeys() {
            let Some(a) = read(root, &sub) else { continue };
            // Hem 32 hem 64 bit görünümde kayıtlı olanlar tek satır.
            if !norm(&a.name).starts_with("carlui") && !out.iter().any(|o| o.name == a.name && o.version == a.version) {
                out.push(a);
            }
        }
    }
    out
}

/// `file:///C:/a%20b/logo.png` → `C:\a b\logo.png`.
fn uri_path(uri: &str) -> String {
    let s = uri.strip_prefix("file:///").unwrap_or("");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%').then(|| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(if b[i] == b'/' { b'\\' } else { b[i] });
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn package(p: &Package) -> windows::core::Result<Option<App>> {
    if p.IsFramework()? || p.IsResourcePackage()? || p.IsBundle()? || p.IsOptional()? || p.SignatureKind()? == PackageSignatureKind::System {
        return Ok(None);
    }
    let name = p.DisplayName()?.to_string();
    if name.is_empty() {
        return Ok(None);
    }
    let id = p.Id()?;
    let v = id.Version()?;
    Ok(Some(App {
        name,
        publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
        version: format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision),
        location: p.InstalledPath().map(|s| s.to_string()).unwrap_or_default(),
        icon: p.Logo().and_then(|u| u.AbsoluteUri()).map(|u| uri_path(&u.to_string())).unwrap_or_default(),
        package: id.FullName()?.to_string(),
        date: p.InstalledDate().map(|d| day(d.UniversalTime as u64)).unwrap_or_default(),
        ..Default::default()
    }))
}

fn store() -> Vec<App> {
    let found = PackageManager::new().and_then(|m| m.FindPackagesByUserSecurityId(&HSTRING::new()));
    match found {
        Ok(list) => list.into_iter().filter_map(|p| package(&p).ok().flatten()).collect(),
        Err(e) => {
            carlui_core::warn!("Mağaza paketleri okunamadı: {e}");
            Vec::new()
        }
    }
}

/// Arka plan iş parçacığında çağrılır.
pub fn list() -> Vec<App> {
    let mut v = desktop();
    v.extend(store());
    v
}

/// Mağaza paketini kaldırır. Hata metni kullanıcıya gösterilir.
pub fn remove_package(full: &str) -> Result<(), String> {
    let m = PackageManager::new().map_err(|e| e.to_string())?;
    let op = m.RemovePackageAsync(&HSTRING::from(full)).map_err(|e| e.to_string())?;
    op.get().map(|_| ()).map_err(|e| e.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_path() {
        assert_eq!(clean_icon(r#""C:\a b\x.exe",0"#), r"C:\a b\x.exe");
        assert_eq!(clean_icon(r"C:\a\x.ico"), r"C:\a\x.ico");
        assert_eq!(clean_icon(r"C:\a,b\x.exe,-101"), r"C:\a,b\x.exe");
    }

    #[test]
    fn filetime_day() {
        assert_eq!(day(116_444_736_000_000_000), "19700101");
        // 2024 artık yıl: 29 Şubat.
        assert_eq!(day((19_782 + 134_774) * 864_000_000_000), "20240229");
        assert_eq!(day((19_783 + 134_774) * 864_000_000_000), "20240301");
    }

    #[test]
    fn file_uri() {
        assert_eq!(uri_path("file:///C:/Program%20Files/a/logo.png"), r"C:\Program Files\a\logo.png");
        assert_eq!(uri_path("ms-appx:///logo.png"), "");
    }
}
