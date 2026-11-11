//! Köprü eşleştirmesi: kasa istekleri (`/vault/...`) yalnızca kullanıcının
//! onayladığı eklentiden kabul edilir.
//!
//! Neden: köken (`chrome-extension://…`) bilgisini tarayıcı koyar ama
//! tarayıcıdaki *her* eklentinin kökeni böyle başlar. Yerel adrese erişimi olan
//! başka bir eklenti, kasa açıkken istediği sitenin parolasını isteyebilirdi.
//!
//! Akış: eklenti kasa isteğinde `X-CarlUI` başlığına sakladığı belirteci koyar.
//! Belirteç tanınmazsa yanıt `pair: true` olur; eklenti `/vault/pair` ister,
//! CarlUI kullanıcıya eklentinin kökenini gösterip onay sorar, onaylanırsa
//! rastgele bir belirteç döner. Belirteçler `%APPDATA%\CarlUI\bridge-tokens`
//! dosyasında tutulur (yerel programlar zaten `browser_peer` ile reddedilir).
//! Reddedilen köken bu oturumda bir daha sorulmaz.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use windows::{core::HSTRING, Win32::UI::WindowsAndMessaging::*};

static ASKING: AtomicBool = AtomicBool::new(false);
static DENIED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn file() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("bridge-tokens")
}

/// Belirteç biçimi: 64 onaltılık karakter.
fn well_formed(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Bu belirteç daha önce onaylanmış mı?
pub fn paired(token: &str) -> bool {
    well_formed(token) && std::fs::read_to_string(file()).is_ok_and(|t| t.lines().any(|l| l.trim() == token))
}

fn new_token() -> Option<String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn remember(token: &str) -> bool {
    use std::io::Write;
    let path = file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| writeln!(f, "{token}")).is_ok()
}

/// Kullanıcıya sorar; onaylanırsa yeni belirteçle yanıt verir.
pub fn pair(origin: &str) -> serde_json::Value {
    let origin = origin.trim();
    if DENIED.lock().is_ok_and(|d| d.iter().any(|o| o == origin)) {
        return serde_json::json!({ "ok": false, "denied": true, "message": t!("Bu eklentinin bağlantısı reddedildi.", "This extension's connection was declined.") });
    }
    if ASKING.swap(true, Ordering::AcqRel) {
        return serde_json::json!({ "ok": false, "busy": true });
    }
    let who = if origin.is_empty() { t!("(kökeni bildirilmedi)", "(origin not given)").to_string() } else { origin.to_string() };
    let text = t!(
        format!("Bir tarayıcı eklentisi Carl Vault'a bağlanmak istiyor:\n\n{who}\n\nYalnızca CarlUI eklentisini kullanıyorsan onayla. Tanımadığın bir eklentiyse Hayır'a bas; onaylanan eklenti kasa açıkken site parolalarını isteyebilir."),
        format!("A browser extension wants to connect to Carl Vault:\n\n{who}\n\nOnly approve if you are using the CarlUI extension. If you don't recognise it, choose No; an approved extension can request site passwords while the vault is open.")
    );
    let answer = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(t!("Carl Vault — eklenti bağlantısı", "Carl Vault — extension connection")),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    ASKING.store(false, Ordering::Release);
    if answer != IDYES {
        carlui_core::warn!("köprü: eklenti eşleştirmesi reddedildi ({who})");
        if let Ok(mut d) = DENIED.lock() {
            d.push(origin.to_string());
        }
        return serde_json::json!({ "ok": false, "denied": true, "message": t!("Bağlantı onaylanmadı.", "The connection wasn't approved.") });
    }
    match new_token().filter(|t| remember(t)) {
        Some(token) => {
            carlui_core::info!("köprü: eklenti eşleştirildi ({who})");
            serde_json::json!({ "ok": true, "token": token })
        }
        None => serde_json::json!({ "ok": false, "message": t!("Bağlantı kaydedilemedi.", "Couldn't save the connection.") }),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens() {
        let t = super::new_token().unwrap();
        assert!(super::well_formed(&t));
        assert!(!super::well_formed("1"));
        assert!(!super::paired("1"), "eski eklentinin \"1\" başlığı eşleşmiş sayılmaz");
        assert!(!super::paired(&"g".repeat(64)));
    }
}
