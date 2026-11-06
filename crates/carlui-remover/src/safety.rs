//! Silme öncesi son denetim. Tarama da yönetici geçişi de buradan geçer, yani
//! dosyadan okunan yol da aynı kurala tabidir.

use std::path::PathBuf;
use windows::{
    core::GUID,
    Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG},
    },
};

const SEALED_KEYS: &[&str] = &["MICROSOFT", "CLASSES", "POLICIES", "CLIENTS", "REGISTEREDAPPLICATIONS", "ODBC", "WOW6432NODE", "CARLUI"];

fn low(p: &str) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn lows(v: Vec<PathBuf>) -> Vec<String> {
    v.iter().map(|p| low(&p.to_string_lossy())).collect()
}

fn under(p: &str, root: &str) -> bool {
    p.len() > root.len() && p.starts_with(root) && p.as_bytes()[root.len()] == b'\\'
}

/// `a` ile `b` aynı klasör mü, biri ötekinin içinde mi?
pub fn overlaps(a: &str, b: &str) -> bool {
    let (a, b) = (low(a), low(b));
    a == b || under(&a, &b) || under(&b, &a)
}

fn env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

pub fn desktops() -> Vec<PathBuf> {
    known(&FOLDERID_Desktop).into_iter().chain(env("PUBLIC").map(|p| p.join("Desktop"))).collect()
}

pub fn menus() -> Vec<PathBuf> {
    let tail = r"Microsoft\Windows\Start Menu\Programs";
    [env("APPDATA"), env("ProgramData")].into_iter().flatten().map(|d| d.join(tail)).collect()
}

pub struct Guard {
    /// Kendisi ve üst klasörleri silinemez, içindekiler silinebilir.
    keep: Vec<String>,
    /// İçindeki hiçbir şey silinemez.
    sealed: Vec<String>,
    /// Mühürlü klasörün içinde kalsa da kısayollarına dokunulabilen yerler.
    menus: Vec<String>,
}

impl Guard {
    pub fn system() -> Self {
        let pf: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|n| env(n)).collect();
        let (appdata, local, data) = (env("APPDATA"), env("LOCALAPPDATA"), env("ProgramData"));
        let own = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from));
        let menus = menus();

        let mut keep: Vec<PathBuf> = pf.clone();
        keep.extend(["USERPROFILE", "PUBLIC", "SystemRoot"].iter().filter_map(|n| env(n)));
        keep.extend([appdata.clone(), local.clone(), data.clone(), own.clone()].into_iter().flatten());
        keep.extend(local.as_ref().map(|l| l.with_file_name("LocalLow")));
        keep.extend(local.as_ref().map(|l| l.join("Programs")));
        keep.extend(desktops());
        keep.extend([&FOLDERID_Documents, &FOLDERID_Downloads, &FOLDERID_Music, &FOLDERID_Pictures, &FOLDERID_Videos].into_iter().filter_map(known));
        keep.extend(menus.iter().cloned());
        keep.extend(menus.iter().map(|m| m.join("Startup")));

        let mut sealed: Vec<PathBuf> = env("SystemRoot").into_iter().collect();
        for p in &pf {
            sealed.push(p.join("Common Files"));
            sealed.push(p.join("WindowsApps"));
        }
        for d in [&appdata, &local, &data].into_iter().flatten() {
            sealed.push(d.join("Microsoft"));
            sealed.push(d.join("CarlUI"));
        }
        sealed.extend(own);

        Self { keep: lows(keep), sealed: lows(sealed), menus: lows(menus) }
    }

    pub fn path_ok(&self, path: &str) -> bool {
        let p = low(path);
        let b = p.as_bytes();
        if b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
            return false;
        }
        // Joker karakterler: Windows'un silme işlemi bunları genişletir.
        if p[2..].contains(['*', '?', '"', '<', '>', '|', ':']) || p.split('\\').any(|c| c.is_empty() || c == "." || c == "..") {
            return false;
        }
        if self.keep.iter().any(|k| *k == p || under(k, &p)) {
            return false;
        }
        self.menus.iter().any(|m| under(&p, m)) || !self.sealed.iter().any(|s| *s == p || under(&p, s))
    }
}

/// Yalnızca `Software` altındaki uygulama anahtarları ve bir uygulamanın kendi
/// kaldırma kaydı silinebilir.
pub fn key_ok(key: &str) -> bool {
    let up = key.to_uppercase();
    let parts: Vec<&str> = up.split('\\').collect();
    if parts.len() < 3 || parts.iter().any(|s| s.is_empty()) || !matches!(parts[0], "HKCU" | "HKLM") || parts[1] != "SOFTWARE" {
        return false;
    }
    let rest = if parts[2] == "WOW6432NODE" { &parts[3..] } else { &parts[2..] };
    match rest {
        [] => false,
        ["MICROSOFT", "WINDOWS", "CURRENTVERSION", "UNINSTALL", entry] => !entry.starts_with("CARLUI"),
        [first, ..] => !SEALED_KEYS.contains(first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> Guard {
        let menu = r"C:\Users\A\AppData\Roaming\Microsoft\Windows\Start Menu\Programs";
        let v = |l: &[&str]| l.iter().map(|s| low(s)).collect();
        Guard {
            keep: v(&[r"C:\Program Files", r"C:\Users\A", r"C:\Users\A\AppData\Roaming", r"C:\Users\A\Desktop", r"C:\Windows", menu]),
            sealed: v(&[r"C:\Windows", r"C:\Program Files\Common Files", r"C:\Users\A\AppData\Roaming\Microsoft", r"C:\CarlUI"]),
            menus: v(&[menu]),
        }
    }

    #[test]
    fn app_folders_pass() {
        let g = guard();
        assert!(g.path_ok(r"C:\Program Files\Foo"));
        assert!(g.path_ok(r"C:\Users\A\AppData\Roaming\Foo\"));
        assert!(g.path_ok(r"D:\Games\Foo"));
        assert!(g.path_ok(r"C:\Users\A\Desktop\Foo.lnk"));
        assert!(g.path_ok(r"C:\Users\A\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Foo"));
    }

    #[test]
    fn roots_and_their_parents_are_refused() {
        let g = guard();
        for p in [r"C:\", "C:", r"D:\", r"C:\Program Files", r"C:\Users", r"C:\Users\A", r"C:\Users\A\AppData", r"C:\USERS\a\Desktop\", r"C:\Windows", r"C:\Users\A\AppData\Roaming\Microsoft\Windows\Start Menu\Programs", r"C:\Users\A\AppData\Roaming\Microsoft\Windows\Start Menu"] {
            assert!(!g.path_ok(p), "{p}");
        }
    }

    #[test]
    fn sealed_trees_are_refused() {
        let g = guard();
        for p in [r"C:\Windows\System32\drivers", r"C:\Program Files\Common Files\Foo", r"C:\Users\A\AppData\Roaming\Microsoft\Teams", r"C:\CarlUI\carlui.exe"] {
            assert!(!g.path_ok(p), "{p}");
        }
    }

    #[test]
    fn tricky_paths_are_refused() {
        let g = guard();
        for p in [r"\\server\share\Foo", r"C:\Program Files\Foo\..\..\Windows", r"C:\Program Files\*", r"C:\Program Files\Fo?", "Foo", "", r"C:\Program Files\Foo:stream", r"C:\Foo\\Bar"] {
            assert!(!g.path_ok(p), "{p}");
        }
    }

    #[test]
    fn keys() {
        for k in [r"HKCU\Software\Foo", r"HKLM\SOFTWARE\Foo\Bar", r"HKLM\SOFTWARE\WOW6432Node\Foo", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Foo"] {
            assert!(key_ok(k), "{k}");
        }
        for k in [r"HKCU\Software", r"HKLM\SOFTWARE\Microsoft", r"HKLM\SOFTWARE\Microsoft\Windows", r"HKLM\SYSTEM\CurrentControlSet", r"HKCU\Software\Classes\Foo", r"HKLM\SOFTWARE\WOW6432Node", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Foo", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Foo\Bar", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlUI", r"HKCR\Foo", r"HKCU\Software\Foo\\"] {
            assert!(!key_ok(k), "{k}");
        }
    }

    #[test]
    fn overlap() {
        assert!(overlaps(r"C:\A\B", r"c:\a\b\"));
        assert!(overlaps(r"C:\A", r"C:\A\B"));
        assert!(overlaps(r"C:\A\B", r"C:\A"));
        assert!(!overlaps(r"C:\A\B", r"C:\A\BC"));
    }
}
