//! Uygulamanın Windows'a bıraktığı hizmetler ve zamanlanmış görevler. Bunlar
//! dosya ya da kayıt anahtarı gibi silinmez; Windows'un kendi araçlarıyla
//! (`sc.exe`, `schtasks.exe`) kaldırılır.

use crate::{reg::Key, run};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::Duration,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

const SERVICES: &str = r"HKLM\SYSTEM\CurrentControlSet\Services";
const TREE: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree";
const TASKS: &str = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tasks";

/// Hizmet türünün "kendi süreci / paylaşılan süreç" bitleri; sürücüler sayılmaz.
const WIN32_SERVICE: u32 = 0x30;

/// `\??\C:\a\b.exe -k x` / `"C:\a b\c.exe" --x` → `C:\a b\c.exe`.
pub fn exe_of(cmd: &str) -> String {
    let c = cmd.trim();
    let c = c.strip_prefix(r"\??\").unwrap_or(c);
    run::split(c).0
}

/// Hizmetin çalıştırdığı dosya. Sürücüler ve okunamayanlar `None`.
pub fn image(name: &str) -> Option<String> {
    let k = Key::open(&format!(r"{SERVICES}\{name}"))?;
    if k.dword("Type").map_or(true, |t| t & WIN32_SERVICE == 0) {
        return None;
    }
    k.string("ImagePath").map(|p| exe_of(&p))
}

/// Tüm hizmetler: (ad, dosya).
pub fn services() -> Vec<(String, String)> {
    let Some(k) = Key::open(SERVICES) else { return Vec::new() };
    k.subkeys().into_iter().filter_map(|n| image(&n).map(|e| (n, e))).collect()
}

pub fn service_exists(name: &str) -> bool {
    Key::open(&format!(r"{SERVICES}\{name}")).is_some()
}

fn tool(exe: &str, args: &[&str]) -> bool {
    Command::new(exe).args(args).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW.0).status().is_ok_and(|s| s.success())
}

/// Hizmeti durdurup siler. Yönetici izni ister; yoksa `false` döner ve öğe
/// yönetici geçişinde yeniden denenir.
pub fn delete_service(name: &str) -> bool {
    if tool("sc.exe", &["stop", name]) {
        // Durması birkaç saniye sürebilir; çalışırken silinen hizmetin kaydı
        // ancak durunca kalkar.
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(500));
            if !running(name) {
                break;
            }
        }
    }
    tool("sc.exe", &["delete", name])
}

fn running(name: &str) -> bool {
    // `sc query` çıktısı dile göre değişir; durum kodu (4 = RUNNING, 3 = STOP_PENDING) değişmez.
    let Ok(out) = Command::new("sc.exe").args(["query", name]).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW.0).output() else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().any(|l| l.contains(':') && l.split(':').nth(1).is_some_and(|v| matches!(v.split_whitespace().next(), Some("3" | "4"))))
}

/// Ham baytlardaki UTF-16 metin (görev eylemleri böyle saklanır). İki hizada
/// da okunur; ortam değişkenleri açılır, küçük harfe çevrilir.
fn blob_text(b: &[u8]) -> String {
    let mut text = String::new();
    for off in [0, 1] {
        let units: Vec<u16> = b.get(off..).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        text.extend(char::decode_utf16(units).map(|c| c.ok().filter(|c| !c.is_control()).unwrap_or('\n')));
        text.push('\n');
    }
    let mut low = text.to_lowercase();
    for var in ["LOCALAPPDATA", "APPDATA", "ProgramFiles(x86)", "ProgramFiles", "ProgramW6432", "ProgramData", "USERPROFILE", "PUBLIC", "SystemDrive"] {
        if let Some(v) = std::env::var_os(var) {
            low = low.replace(&format!("%{}%", var.to_lowercase()), &v.to_string_lossy().to_lowercase());
        }
    }
    low
}

fn walk(key: &str, rel: &str, depth: usize, out: &mut Vec<(String, String)>) {
    let Some(k) = Key::open(key) else { return };
    for sub in k.subkeys() {
        let path = format!(r"{key}\{sub}");
        let rel = format!(r"{rel}\{sub}");
        if let Some(id) = Key::open(&path).and_then(|t| t.string("Id")) {
            if let Some(blob) = Key::open(&format!(r"{TASKS}\{id}")).and_then(|t| t.binary("Actions")) {
                out.push((rel.clone(), blob_text(&blob)));
            }
        }
        if depth < 8 {
            walk(&path, &rel, depth + 1, out);
        }
    }
}

/// Zamanlanmış görevler: (`\Klasör\Ad`, eylemlerin metni). Görev önbelleği
/// okunamazsa (yetki) boş döner.
pub fn tasks() -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk(TREE, "", 0, &mut out);
    out
}

pub fn task_exists(path: &str) -> bool {
    Key::open(&format!("{TREE}{path}")).is_some()
}

pub fn delete_task(path: &str) -> bool {
    tool("schtasks.exe", &["/Delete", "/TN", path, "/F"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_commands() {
        assert_eq!(exe_of(r#""C:\Program Files\Foo\svc.exe" --run"#), r"C:\Program Files\Foo\svc.exe");
        assert_eq!(exe_of(r"\??\C:\Foo\drv.sys"), r"C:\Foo\drv.sys");
        assert_eq!(exe_of(r"C:\Foo Bar\svc.exe -k x"), r"C:\Foo Bar\svc.exe");
    }

    #[test]
    fn task_blob() {
        let mut b = vec![3u8, 0, 0];
        b.extend(r"C:\Foo\Up.exe".encode_utf16().flat_map(u16::to_le_bytes));
        assert!(blob_text(&b).contains(r"c:\foo\up.exe"));
    }
}
