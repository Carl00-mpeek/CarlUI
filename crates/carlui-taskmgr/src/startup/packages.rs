//! Store / MSIX uygulamalarının başlangıç görevleri (WhatsApp, Terminal, Xbox…).
//! Bunlar Run anahtarında görünmez: paketin AppxManifest.xml dosyasında
//! `StartupTask` olarak tanımlıdır, açık/kapalı durumu ise kullanıcının
//! kaydında `SystemAppData\<paket ailesi>\<görev>\State` değerindedir
//! (Windows Ayarlar > Başlangıç ve Görev Yöneticisi de aynı yeri kullanır).
//! State: 0 kapalı (varsayılan), 1 kullanıcı kapattı, 2 açık,
//! 3 ilkeyle kapalı, 4 ilkeyle açık.

use super::{reg, Fail, Item, Toggle};
use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, REG_DWORD};

const APPMODEL: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\";
pub const STATE_BASE: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\SystemAppData";

#[derive(Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub enabled: bool,
    pub display: String,
    pub exe: String,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = format!(" {}=", name.to_ascii_lowercase());
    let i = lower.find(&key)? + key.len();
    let q = tag[i..].chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let rest = &tag[i + 1..];
    Some(rest[..rest.find(q)?].to_string())
}

/// Manifest metnindeki başlangıç görevleri.
pub fn parse_manifest(xml: &str) -> Vec<Task> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = xml[from..].find(":StartupTask").map(|i| i + from) {
        from = i + 1;
        // `<uap5:StartupTask …>` / `<desktop:StartupTask …>`; kapanış etiketlerini atla.
        let Some(lt) = xml[..i].rfind('<') else { continue };
        if xml[lt..].starts_with("</") {
            continue;
        }
        let end = xml[i..].find('>').map(|e| i + e).unwrap_or(xml.len());
        let tag = xml[lt..end].replace(['\r', '\n', '\t'], " ");
        let Some(id) = attr(&tag, "TaskId") else { continue };
        // Görevin çalıştırdığı dosya, onu saran Extension etiketindedir.
        let ext_tag = xml[..lt].rfind(":Extension ").and_then(|e| xml[..e].rfind('<').map(|s| (s, e))).map(|(s, e)| {
            let close = xml[e..].find('>').map(|c| e + c).unwrap_or(e);
            xml[s..close].replace(['\r', '\n', '\t'], " ")
        });
        // Extension'da yoksa uygulamanın kendi exe'si çalışır (Terminal böyle).
        let app_tag = || xml[..lt].rfind("<Application ").map(|s| xml[s..xml[s..].find('>').map(|c| s + c).unwrap_or(lt)].replace(['\r', '\n', '\t'], " "));
        let exe = ext_tag.as_deref().and_then(|t| attr(t, "Executable")).or_else(|| app_tag().and_then(|t| attr(&t, "Executable"))).unwrap_or_default();
        out.push(Task {
            enabled: attr(&tag, "Enabled").map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false),
            display: attr(&tag, "DisplayName").filter(|d| !d.starts_with("ms-resource:")).unwrap_or_default(),
            id,
            exe,
        });
    }
    out
}

/// Tam paket adından (`Ad_Sürüm_Mimari_Kaynak_Yayıncı`) paket ailesi adı (`Ad_Yayıncı`).
pub fn family(full: &str) -> Option<String> {
    let parts: Vec<&str> = full.split('_').collect();
    (parts.len() >= 5).then(|| format!("{}_{}", parts[0], parts[parts.len() - 1]))
}

pub fn state(root: HKEY, base: &str, family: &str, task: &str) -> Option<u32> {
    reg::value(root, &format!("{base}\\{family}\\{task}"), "State").and_then(|(_, d)| reg::dword(&d))
}

pub fn set_state(root: HKEY, base: &str, family: &str, task: &str, on: bool) -> std::result::Result<(), Fail> {
    if matches!(state(root, base, family, task), Some(3 | 4)) {
        return Err(Fail::Other(t!("Bu görev bir ilkeyle yönetiliyor", "This task is managed by a policy").into()));
    }
    let v: u32 = if on { 2 } else { 1 };
    reg::set_value(root, &format!("{base}\\{family}\\{task}"), "State", REG_DWORD.0, &v.to_le_bytes())
}

/// Durum değerine göre etkin mi; değer yoksa manifestteki varsayılan geçerli.
pub fn is_on(state: Option<u32>, default: bool) -> bool {
    match state {
        Some(2 | 4) => true,
        Some(_) => false,
        None => default,
    }
}

pub fn scan(out: &mut Vec<Item>) {
    let root = HKEY_CURRENT_USER;
    let repo = format!("{APPMODEL}Repository\\Packages");
    let mut seen = std::collections::HashSet::new();
    for full in super::subkeys(root, &repo) {
        let Some(fam) = family(&full) else { continue };
        let Some((_, dir)) = reg::value(root, &format!("{repo}\\{full}"), "PackageRootFolder") else { continue };
        let dir = reg::reg_string(&dir);
        let Ok(xml) = std::fs::read_to_string(std::path::Path::new(&dir).join("AppxManifest.xml")) else { continue };
        for task in parse_manifest(&xml) {
            if !seen.insert((fam.to_lowercase(), task.id.to_lowercase())) {
                continue;
            }
            let st = state(root, STATE_BASE, &fam, &task.id);
            let path = if task.exe.is_empty() { String::new() } else { format!("{}\\{}", dir.trim_end_matches('\\'), task.exe.replace('/', "\\")) };
            let app = fam.split('_').next().unwrap_or(&fam).to_string();
            out.push(Item {
                name: if task.display.is_empty() { app } else { task.display.clone() },
                source: t!("Mağaza uygulaması", "Store app"),
                windows: crate::util::is_windows_path(&path),
                command: if path.is_empty() { format!("{fam}!{}", task.id) } else { path.clone() },
                path,
                enabled: is_on(st, task.enabled),
                toggle: if matches!(st, Some(3 | 4)) { Toggle::None } else { Toggle::Package { family: fam.clone(), task: task.id } },
            });
        }
    }
}
