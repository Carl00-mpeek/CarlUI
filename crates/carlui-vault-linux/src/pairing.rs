//! Eklenti eşleştirmesi (Windows'taki `carlui-island/src/pairing.rs` ile aynı
//! akış): kasa istekleri yalnızca kullanıcının onayladığı eklentiden kabul
//! edilir. Her eklentinin kökeni "…-extension://" ile başladığından köken tek
//! başına yetmez; onaylanan eklentiye rastgele bir belirteç verilir ve
//! `~/.local/share/CarlUI/bridge-tokens` dosyasında (yalnızca bu kullanıcı
//! okuyabilir) tutulur. Soru, ileti kutusu yerine kasa penceresinde kart olarak
//! gösterilir. Reddedilen köken bu oturumda bir daha sorulmaz.

use crate::core::{AskKind, Shared, ASK_TTL};
use carlui_vault::t;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

static ASKING: AtomicBool = AtomicBool::new(false);
static DENIED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn file() -> std::path::PathBuf {
    crate::settings::dir().join("bridge-tokens")
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
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(&path).and_then(|mut f| writeln!(f, "{token}")).is_ok()
}

/// Onay kartındaki metin.
pub fn prompt(who: &str) -> String {
    t!(
        format!("Bir tarayıcı eklentisi Carl Vault'a bağlanmak istiyor:\n\n{who}\n\nYalnızca CarlUI eklentisini kullanıyorsan onayla. Tanımadığın bir eklentiyse Hayır'a bas; onaylanan eklenti kasa açıkken site parolalarını isteyebilir."),
        format!("A browser extension wants to connect to Carl Vault:\n\n{who}\n\nOnly approve if you are using the CarlUI extension. If you don't recognise it, choose No; an approved extension can request site passwords while the vault is open.")
    )
}

/// Kullanıcıya sorar; onaylanırsa yeni belirteçle yanıt verir.
pub fn pair(shared: &Shared, origin: &str) -> Value {
    let origin = origin.trim();
    if DENIED.lock().is_ok_and(|d| d.iter().any(|o| o == origin)) {
        return json!({ "ok": false, "denied": true, "message": t!("Bu eklentinin bağlantısı reddedildi.", "This extension's connection was declined.") });
    }
    if ASKING.swap(true, Ordering::AcqRel) {
        return json!({ "ok": false, "busy": true });
    }
    let who = if origin.is_empty() { t!("(kökeni bildirilmedi)", "(origin not given)").to_string() } else { origin.to_string() };
    let (answer, _) = shared.ask(AskKind::Pair(who.clone()), ASK_TTL);
    ASKING.store(false, Ordering::Release);
    if answer.is_none() {
        eprintln!("köprü: eklenti eşleştirmesi reddedildi ({who})");
        if let Ok(mut d) = DENIED.lock() {
            d.push(origin.to_string());
        }
        return json!({ "ok": false, "denied": true, "message": t!("Bağlantı onaylanmadı.", "The connection wasn't approved.") });
    }
    match new_token().filter(|t| remember(t)) {
        Some(token) => {
            eprintln!("köprü: eklenti eşleştirildi ({who})");
            json!({ "ok": true, "token": token })
        }
        None => json!({ "ok": false, "message": t!("Bağlantı kaydedilemedi.", "Couldn't save the connection.") }),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens() {
        let t = super::new_token().unwrap();
        assert!(super::well_formed(&t));
        assert!(!super::well_formed("1"));
        assert!(!super::paired("1"));
        assert!(!super::paired(&"g".repeat(64)));
    }
}
