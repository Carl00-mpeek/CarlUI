//! Uygulamanın kendi kaldırıcısını çalıştırır.

use std::path::Path;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Threading::{WaitForSingleObject, INFINITE},
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

/// Komut satırını program ve bağımsız değişkenlere ayırır. Kayıtlardaki yollar
/// çoğu zaman tırnaksızdır: `C:\Program Files\Foo\unins000.exe /S`.
pub fn split(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(r) = c.strip_prefix('"') {
        if let Some(e) = r.find('"') {
            return (r[..e].to_string(), r[e + 1..].trim().to_string());
        }
    }
    if let Some(i) = c.to_ascii_lowercase().find(".exe") {
        let e = i + 4;
        if e == c.len() || c.as_bytes()[e] == b' ' {
            return (c[..e].to_string(), c[e..].trim().to_string());
        }
    }
    match c.split_once(' ') {
        Some((a, b)) => (a.to_string(), b.trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// MSI kayıtlarının çoğu `/I{…}` (onar/değiştir) der; kaldırmak için `/X` gerekir.
pub fn msi_fix(cmd: &str) -> String {
    let low = cmd.to_ascii_lowercase();
    match low.find("/i{").filter(|_| low.contains("msiexec")) {
        Some(i) => format!("{}/X{}", &cmd[..i], &cmd[i + 2..]),
        None => cmd.to_string(),
    }
}

/// Komut tam yol gösteriyor ama dosya yok.
pub fn missing(cmd: &str) -> bool {
    let (exe, _) = split(cmd);
    exe.contains(":\\") && !Path::new(&exe).exists()
}

/// Kaldırıcıyı başlatır ve kapanmasını bekler. Yönetici izni isteyen kaldırıcılar
/// için Windows UAC penceresini kendisi açar.
pub fn wait(cmd: &str) -> std::result::Result<(), String> {
    let (exe, args) = split(cmd);
    let (exe, args) = (HSTRING::from(exe), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("open"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| e.message().to_string())?;
        if !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn commands() {
        assert_eq!(split(r#""C:\Program Files\Foo\unins000.exe" /SILENT"#), s(r"C:\Program Files\Foo\unins000.exe", "/SILENT"));
        assert_eq!(split(r"C:\Program Files\Foo\uninstall.exe /S"), s(r"C:\Program Files\Foo\uninstall.exe", "/S"));
        assert_eq!(split(r"C:\Program Files\Foo\uninstall.exe"), s(r"C:\Program Files\Foo\uninstall.exe", ""));
        assert_eq!(split("MsiExec.exe /X{1234}"), s("MsiExec.exe", "/X{1234}"));
        assert_eq!(split(r"cmd /c C:\x.bat"), s("cmd", r"/c C:\x.bat"));
    }

    #[test]
    fn msi() {
        assert_eq!(msi_fix("MsiExec.exe /I{ABC}"), "MsiExec.exe /X{ABC}");
        assert_eq!(msi_fix("MsiExec.exe /X{ABC}"), "MsiExec.exe /X{ABC}");
        assert_eq!(msi_fix(r"C:\foo\setup.exe /i{ABC}"), r"C:\foo\setup.exe /i{ABC}");
    }

    #[test]
    fn stale() {
        assert!(missing(r#""C:\yok\yok\unins000.exe" /S"#));
        assert!(!missing("MsiExec.exe /X{ABC}"));
    }
}
