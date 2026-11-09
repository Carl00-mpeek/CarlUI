//! Başlangıçta çalışan her şey. Windows Görev Yöneticisi yalnızca Run kayıtlarını,
//! Başlangıç klasörünü ve Mağaza uygulamalarını gösterir; burada şunların hepsi taranır:
//! - Kayıt defteri: HKCU/HKLM Run, RunOnce, 32 bit Run/RunOnce, Policies\Explorer\Run
//! - Başlangıç klasörleri (kullanıcı ve tüm kullanıcılar)
//! - Mağaza / MSIX uygulamalarının başlangıç görevleri
//! - Zamanlanmış görevler: oturum açılışında ya da sistem açılışında tetiklenenler
//! - Hizmetler: otomatik başlayanlar
//! - Winlogon: Shell ve Userinit (değiştirilmişse kötü amaçlı yazılım işaretidir)
//!
//! Açmak/kapatmak yalnızca başlangıç kaydını değiştirir; hiçbir süreç kapatılmaz,
//! başlatılmaz. Değişiklikten sonra durum yeniden okunur: uygulama ya da Windows
//! geri aldıysa "yapıldı" denmez.

mod elevate;
mod packages;
pub mod reg;
mod sources;
mod system;
#[cfg(test)]
mod tests;

pub use elevate::run_helper;
use windows::{
    core::*,
    Win32::System::{Com::*, Registry::*},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Toggle {
    /// StartupApproved altındaki anahtar ("Run", "Run32", "StartupFolder") ve değer adı.
    Approved { hive: isize, key: &'static str, name: String },
    /// StartupApproved'a bakmayan anahtar (RunOnce, ilke): kapatınca girdi
    /// CarlUI'nin yedek anahtarına taşınır, açınca birebir geri konur.
    Moved { hive: isize, src: &'static str, name: String },
    /// Zamanlanmış görevin tam yolu.
    Task(String),
    /// Windows dışı hizmet: kapatınca "elle başlat" olur, açınca yine otomatik.
    Service(String),
    /// Mağaza uygulamasının başlangıç görevi.
    Package { family: String, task: String },
    None,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub source: &'static str,
    pub command: String,
    pub path: String,
    pub enabled: bool,
    pub windows: bool,
    pub toggle: Toggle,
}

pub const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\";
pub const APPROVED_KEYS: [&str; 3] = ["Run", "Run32", "StartupFolder"];
/// Kapatılan RunOnce/ilke girdilerinin saklandığı yer (girdinin kendi kovanında).
pub const BACKUP: &str = "Software\\CarlUI\\DisabledStartup\\";
/// (kimlik, anahtar yolu)
pub const MOVABLE: [(&str, &str); 3] = [
    ("RunOnce", "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
    ("RunOnce32", "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
    ("Policy", "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer\\Run"),
];

fn movable_path(src: &str) -> &'static str {
    MOVABLE.iter().find(|m| m.0 == src).map(|m| m.1).unwrap_or("")
}

/// Anahtarın alt anahtar adları.
pub fn subkeys(root: HKEY, path: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(path), 0, KEY_READ, &mut k).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(k, i, PWSTR(name.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(k);
    }
    out
}

/// Bütün kaynakları tarar (yavaş: arka planda çağırın).
pub fn scan() -> Vec<Item> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut out = Vec::new();
    sources::run_keys(&mut out);
    sources::startup_folders(&mut out);
    packages::scan(&mut out);
    sources::winlogon(&mut out);
    system::tasks(&mut out);
    system::services(&mut out);
    out
}

/// Değiştirme denemesinin sonucu: yetki yoksa yönetici olarak yeniden denenir.
pub enum Fail {
    Denied,
    Other(String),
}

/// RunOnce/ilke girdisini açar ya da kapatır (`live`: asıl anahtar, `backup`: yedek).
pub fn set_moved(root: HKEY, live: &str, backup: &str, name: &str, on: bool) -> std::result::Result<(), Fail> {
    if on {
        reg::move_value(root, backup, live, name, false)
    } else {
        reg::move_value(root, live, backup, name, true)
    }
}

pub fn moved_on(root: HKEY, live: &str, name: &str) -> bool {
    reg::value(root, live, name).is_some()
}

fn direct(toggle: &Toggle, on: bool) -> std::result::Result<(), Fail> {
    match toggle {
        Toggle::Approved { hive, key, name } => reg::set_approved(reg::hkey(*hive), APPROVED, key, name, on),
        Toggle::Moved { hive, src, name } => set_moved(reg::hkey(*hive), movable_path(src), &format!("{BACKUP}{src}"), name, on),
        Toggle::Task(path) => system::set_task(path, on),
        Toggle::Service(name) => {
            let r = system::set_service(name, on);
            if r.is_ok() {
                mark_service(name, !on);
            }
            r
        }
        Toggle::Package { family, task } => packages::set_state(HKEY_CURRENT_USER, packages::STATE_BASE, family, task, on),
        Toggle::None => Err(Fail::Other(t!("Bu öğe buradan kapatılamaz (Winlogon)", "This item can't be turned off here (Winlogon)").into())),
    }
}

/// Öğenin şu anki gerçek durumu (doğrulama için).
fn current(toggle: &Toggle) -> Option<bool> {
    match toggle {
        Toggle::Approved { hive, key, name } => Some(reg::approved(reg::hkey(*hive), APPROVED, key, name)),
        Toggle::Moved { hive, src, name } => Some(moved_on(reg::hkey(*hive), movable_path(src), name)),
        Toggle::Task(path) => system::task_enabled(path),
        Toggle::Service(name) => system::service_auto(name),
        Toggle::Package { family, task } => packages::state(HKEY_CURRENT_USER, packages::STATE_BASE, family, task).map(|s| packages::is_on(Some(s), false)),
        Toggle::None => None,
    }
}

/// CarlUI'nin elle başlatmaya çevirdiği hizmetler; listede kalsınlar ki geri açılabilsinler.
const OFF_SERVICES: &str = "Software\\CarlUI\\DisabledServices";

fn mark_service(name: &str, off: bool) {
    // Yönetici olarak çalışan yardımcı yazsa da kayıt asıl kullanıcınındır (aynı oturum).
    let _ = if off { reg::set_value(HKEY_CURRENT_USER, OFF_SERVICES, name, REG_DWORD.0, &1u32.to_le_bytes()) } else { reg::delete_value(HKEY_CURRENT_USER, OFF_SERVICES, name) };
}

fn marked_services() -> Vec<String> {
    reg::values(HKEY_CURRENT_USER, OFF_SERVICES).into_iter().map(|(n, ..)| n).collect()
}

/// Öğeyi etkinleştir/devre dışı bırak. Yetki yetmezse UAC ile yönetici izni ister.
/// Sonra durumu yeniden okur: değişiklik tutmadıysa hata döner.
pub fn set_enabled(item: &Item, on: bool) -> std::result::Result<(), String> {
    match direct(&item.toggle, on) {
        Ok(()) => {}
        Err(Fail::Other(e)) => return Err(e),
        Err(Fail::Denied) => elevate::elevated(&item.toggle, on)?,
    }
    match current(&item.toggle) {
        Some(now) if now != on => Err(t!("Değiştirildi ama hemen geri alındı (uygulama ya da Windows kendi ayarını yeniden yazıyor)", "Changed, but it was reverted right away (the app or Windows rewrites its own setting)").into()),
        _ => Ok(()),
    }
}
