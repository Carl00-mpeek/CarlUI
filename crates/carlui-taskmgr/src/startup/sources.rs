//! Kayıt defteri Run/RunOnce/ilke anahtarları, Başlangıç klasörleri ve Winlogon.

use super::{reg, Item, Toggle, APPROVED, BACKUP, MOVABLE};
use crate::util::{exe_of, is_windows_path};
use windows::{
    core::*,
    Win32::{
        System::{Com::*, Registry::*},
        UI::Shell::*,
    },
};

fn item(name: String, data: &[u8], source: &'static str, enabled: bool, toggle: Toggle) -> Item {
    let command = reg::reg_string(data);
    let path = exe_of(&command);
    Item { windows: is_windows_path(&path), name, source, command, path, enabled, toggle }
}

fn is_text(ty: u32) -> bool {
    ty == REG_SZ.0 || ty == REG_EXPAND_SZ.0
}

pub fn run_keys(out: &mut Vec<Item>) {
    // StartupApproved ile açılıp kapatılanlar (Windows Görev Yöneticisi gibi).
    let approved: [(HKEY, &str, &'static str, &'static str); 3] = [
        (HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Kullanıcı Run", "Registry · User Run"), "Run"),
        (HKEY_LOCAL_MACHINE, "Software\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Sistem Run", "Registry · System Run"), "Run"),
        (HKEY_LOCAL_MACHINE, "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Sistem Run (32 bit)", "Registry · System Run (32-bit)"), "Run32"),
    ];
    for (hive, key, source, appr) in approved {
        for (name, data, ty) in reg::values(hive, key) {
            if !is_text(ty) {
                continue;
            }
            let enabled = reg::approved(hive, APPROVED, appr, &name);
            out.push(item(name.clone(), &data, source, enabled, Toggle::Approved { hive: hive.0 as isize, key: appr, name }));
        }
    }
    // RunOnce ve ilke anahtarları StartupApproved'a bakmaz: kapatınca girdi
    // CarlUI'nin yedek anahtarına birebir taşınır, açınca geri konur.
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for (src, path) in MOVABLE {
            if hive == HKEY_CURRENT_USER && src == "RunOnce32" {
                continue;
            }
            let label = movable_label(hive == HKEY_CURRENT_USER, src);
            let live = reg::values(hive, path);
            for (name, data, ty) in &live {
                if is_text(*ty) {
                    out.push(item(name.clone(), data, label, true, Toggle::Moved { hive: hive.0 as isize, src, name: name.clone() }));
                }
            }
            for (name, data, ty) in reg::values(hive, &format!("{BACKUP}{src}")) {
                if is_text(ty) && !live.iter().any(|(n, ..)| n.eq_ignore_ascii_case(&name)) {
                    out.push(item(name.clone(), &data, label, false, Toggle::Moved { hive: hive.0 as isize, src, name }));
                }
            }
        }
    }
}

fn movable_label(user: bool, src: &str) -> &'static str {
    match (user, src) {
        (true, "RunOnce") => t!("Kayıt · Kullanıcı RunOnce", "Registry · User RunOnce"),
        (false, "RunOnce") => t!("Kayıt · Sistem RunOnce", "Registry · System RunOnce"),
        (_, "RunOnce32") => t!("Kayıt · Sistem RunOnce (32 bit)", "Registry · System RunOnce (32-bit)"),
        (true, _) => t!("Kayıt · Kullanıcı ilkesi", "Registry · User policy"),
        (false, _) => t!("Kayıt · Sistem ilkesi", "Registry · System policy"),
    }
}

pub fn startup_folders(out: &mut Vec<Item>) {
    for (id, hive, source) in [(FOLDERID_Startup, HKEY_CURRENT_USER, t!("Başlangıç klasörü", "Startup folder")), (FOLDERID_CommonStartup, HKEY_LOCAL_MACHINE, t!("Başlangıç klasörü (tümü)", "Startup folder (all users)"))] {
        let Ok(dir) = (unsafe { SHGetKnownFolderPath(&id, KNOWN_FOLDER_FLAG(0), None) }) else { continue };
        let dir = unsafe {
            let s = dir.to_string().unwrap_or_default();
            CoTaskMemFree(Some(dir.0 as _));
            s
        };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if file.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let full = e.path().to_string_lossy().to_string();
            let target = if file.to_lowercase().ends_with(".lnk") { link_target(&full) } else { full.clone() };
            let name = file.trim_end_matches(".lnk").to_string();
            out.push(Item {
                enabled: reg::approved(hive, APPROVED, "StartupFolder", &file),
                toggle: Toggle::Approved { hive: hive.0 as isize, key: "StartupFolder", name: file },
                windows: is_windows_path(&target),
                name,
                source,
                command: target.clone(),
                path: target,
            });
        }
    }
}

fn link_target(lnk: &str) -> String {
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return String::new() };
        let Ok(pf) = link.cast::<IPersistFile>() else { return String::new() };
        if pf.Load(&HSTRING::from(lnk), STGM_READ).is_err() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
        String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
    }
}

/// Shell ve Userinit: değiştirilmişse kötü amaçlı yazılım işaretidir. Kapatmak
/// oturumu açılmaz hale getirebileceği için yalnızca gösterilir.
pub fn winlogon(out: &mut Vec<Item>) {
    let key = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon";
    for (hive, label) in [(HKEY_LOCAL_MACHINE, t!("Winlogon · Sistem", "Winlogon · System")), (HKEY_CURRENT_USER, t!("Winlogon · Kullanıcı", "Winlogon · User"))] {
        for (name, data, _) in reg::values(hive, key) {
            if !(name.eq_ignore_ascii_case("Shell") || name.eq_ignore_ascii_case("Userinit")) {
                continue;
            }
            let command = reg::reg_string(&data);
            for part in command.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let path = exe_of(part);
                out.push(Item { windows: is_windows_path(&path), name: format!("{name}: {}", crate::util::file_name(&path)), source: label, command: part.to_string(), path, enabled: true, toggle: Toggle::None });
            }
        }
    }
}
