//! Yetki yetmeyen değişiklik için kendini yönetici olarak (UAC sorusuyla)
//! `--startup-set …` argümanlarıyla açar; yardımcı süreç değişikliği yapar ve
//! çıkış koduyla sonucu bildirir. Yardımcı hiçbir süreci kapatmaz/açmaz.

use super::{direct, Toggle, APPROVED_KEYS, MOVABLE};
use windows::{
    core::*,
    Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
    },
};

/// Yardımcıya geçirilecek argümanlar.
pub fn args(toggle: &Toggle, on: bool) -> Vec<String> {
    let on = if on { "1" } else { "0" }.to_string();
    match toggle {
        Toggle::Approved { hive, key, name } => vec!["approved".into(), on, hive.to_string(), key.to_string(), name.clone()],
        Toggle::Moved { hive, src, name } => vec!["moved".into(), on, hive.to_string(), src.to_string(), name.clone()],
        Toggle::Task(p) => vec!["task".into(), on, p.clone()],
        Toggle::Service(n) => vec!["service".into(), on, n.clone()],
        Toggle::Package { family, task } => vec!["package".into(), on, family.clone(), task.clone()],
        Toggle::None => vec![],
    }
}

/// `args`'ın tersi. Tanınmayan anahtar adı kabul edilmez (yalnızca bilinen
/// başlangıç anahtarlarına yazılabilir).
pub fn parse(a: &[String]) -> Option<(Toggle, bool)> {
    let on = a.get(1)? == "1";
    let t = match (a.first()?.as_str(), a.get(2), a.get(3), a.get(4)) {
        ("approved", Some(h), Some(k), Some(n)) => Toggle::Approved { hive: h.parse().ok()?, key: APPROVED_KEYS.into_iter().find(|x| x == k)?, name: n.clone() },
        ("moved", Some(h), Some(s), Some(n)) => Toggle::Moved { hive: h.parse().ok()?, src: MOVABLE.iter().map(|m| m.0).find(|x| x == s)?, name: n.clone() },
        ("task", Some(p), ..) => Toggle::Task(p.clone()),
        ("service", Some(n), ..) => Toggle::Service(n.clone()),
        ("package", Some(f), Some(t), ..) => Toggle::Package { family: f.clone(), task: t.clone() },
        _ => return None,
    };
    Some((t, on))
}

/// `--startup-set …` ile açılan yardımcı: 0 = başarılı, 1 = yapılamadı, 2 = hatalı argüman.
pub fn run_helper(a: &[String]) -> i32 {
    match parse(a) {
        Some((t, on)) => match direct(&t, on) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        None => 2,
    }
}

/// Bir argümanı Windows komut satırı kurallarına göre tırnaklar
/// (tırnaktan ve sondaki tırnaktan önceki ters bölüler ikilenir).
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in s.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat('\\').take(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
                continue;
            }
            _ => {}
        }
        if c != '\\' {
            out.extend(std::iter::repeat('\\').take(slashes));
            slashes = 0;
            out.push(c);
        }
    }
    out.extend(std::iter::repeat('\\').take(slashes * 2));
    out.push('"');
    out
}

/// Kendini yönetici olarak çalıştırıp değişikliği ona yaptırır.
pub fn elevated(toggle: &Toggle, on: bool) -> std::result::Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let params = std::iter::once("--startup-set".to_string()).chain(args(toggle, on).iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ");
    let (exe_w, params_w) = (HSTRING::from(exe.as_os_str()), HSTRING::from(params));
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe_w.as_ptr()),
            lpParameters: PCWSTR(params_w.as_ptr()),
            nShow: 0,
            ..Default::default()
        };
        if ShellExecuteExW(&mut info).is_err() || info.hProcess.is_invalid() {
            return Err(t!("Yönetici izni verilmedi", "Admin permission was not given").into());
        }
        WaitForSingleObject(info.hProcess, 30_000);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if code == 0 { Ok(()) } else { Err(t!("Yönetici olarak da değiştirilemedi (öğe korumalı olabilir)", "Couldn't change it even as admin (the item may be protected)").into()) }
    }
}
