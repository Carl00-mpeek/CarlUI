//! Carl Vault ↔ CarlUI panosu bağlantısı (Vault Aşama 2). Sorun: kasadan
//! kopyalanan parola panoya girerse herhangi bir uygulama okuyabilir. Çözüm:
//! kasa panoya yalnızca kısa ömürlü, tek kullanımlık bir belirteç koyar
//! (`FORMAT` biçiminde `Offer`). CarlUI panosu (Win+V) bunu geçmişe yazmadan
//! "kasa kaydı" olarak gösterir; seçilince ada, hedef pencereyle birlikte
//! belirteci kasanın yerel borusuna (`pipe_name`) yollar. Kasa belirteci,
//! isteyen sürecin aynı klasördeki `carlui-island.exe` olduğunu ve hedef
//! pencereyi doğrular; değeri doğrudan o pencereye yazar. Gerçek değer panoya
//! hiç girmez. Ada da karşıdaki sunucunun `carlui-vault.exe` olduğuna bakar.
//!
//! Tarayıcı eklentisi de (indirme eklentisiyle aynı eklenti) bu boruyu adanın
//! yerel köprüsü üzerinden kullanır: `Find` sayfa adresine uyan kayıtların
//! adlarını, `Fill` kullanıcının seçtiği kaydın giriş bilgilerini ister. Kasa
//! yalnızca açıkken, adres kayıttaki adrese sıkı biçimde uyuyorsa ve öndeki
//! pencere bir tarayıcıysa yanıt verir.
//!
//! Mesajlar satır başına bir JSON'dur; bağlantı başına tek istek/yanıt.

use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    os::windows::io::FromRawHandle,
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::GetCurrentProcessId},
    },
};

/// Panodaki belirteç biçiminin adı (içerik: UTF-8 JSON `Offer`).
pub const FORMAT: &str = "CarlUI.VaultToken";
/// Belirtecin ömrü.
pub const TTL_MS: i64 = 60_000;
const MAX_LINE: u64 = 8 * 1024;

/// Panoya konan teklif: belirteç ve listede görünecek ad ("GitHub · Parola").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Offer {
    pub token: String,
    pub label: String,
    /// Unix zamanı (ms); bu andan sonra geçersiz.
    pub expires: i64,
}

/// Adanın kasaya isteği.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    /// Pano belirtecinin değerini `hwnd` penceresine yaz.
    Type { token: String, hwnd: isize },
    /// Tarayıcı eklentisi (ada köprüsü üzerinden): bu sayfa adresine uyan kayıtlar.
    Find { url: String },
    /// Tarayıcı eklentisi: bu kaydın giriş bilgileri; kayıt adresi sayfaya uymalı.
    Fill { url: String, id: String },
    /// Ekran görüntüsü aracının okuduğu 2FA QR'ı (otpauth adresi): kasa
    /// doldurulmuş yeni bir 2FA kaydı açar; kaydetmek kullanıcıya kalır.
    ImportTotp { uri: String },
    /// Tarayıcı eklentisinin sayfa içi düğmesi: kasa öne gelir (kilitliyse
    /// parola kendi penceresine yazılır); açılınca odak `back` penceresine
    /// (istek anındaki tarayıcı) döner ki eklenti doldurmayı sürdürsün.
    Open { back: isize },
    /// Tarayıcı eklentisi: sayfada gönderilen giriş/kayıt formunun bilgileri.
    /// `commit` false ise yalnızca durum sorulur (`Reply::status`: "new",
    /// "changed", "same"); true ise kullanıcı sayfadaki "Kaydet"e basmıştır,
    /// kayıt eklenir ya da parolası güncellenir.
    Save {
        url: String,
        user: String,
        password: String,
        commit: bool,
        /// Kullanıcının şeritte yazdığı kayıt adı (boşsa site adı).
        #[serde(default)]
        name: String,
    },
}

/// Bir sayfaya uyan kayıt (yalnızca ad ve kullanıcı adı; parola yok).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Login {
    pub id: String,
    pub name: String,
    pub username: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Reply {
    pub ok: bool,
    /// Kullanıcıya gösterilecek kısa açıklama.
    pub message: String,
    /// `Find` yanıtı.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logins: Vec<Login>,
    /// `Fill` yanıtı: kullanıcı adı ve parola.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<(String, String)>,
    /// Kasa kilitli (eklenti "Carl Vault'u aç" gösterir).
    #[serde(default)]
    pub locked: bool,
    /// `Save` yanıtı: "new", "changed" ya da "same".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// `Save` yanıtı: önerilen kayıt adı (yeni) ya da mevcut kaydın adı (değişmiş).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Reply {
    pub fn no(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), ..Default::default() }
    }
}

/// Oturuma özel boru adı (aynı makinedeki başka oturumlar karışmasın).
pub fn pipe_name() -> String {
    let mut session = 0;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
    }
    format!(r"\\.\pipe\carlui-vault-{session}")
}

/// Bu exe ile aynı klasördeki `name` (küçük harf).
fn sibling(name: &str) -> String {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).unwrap_or_else(|| PathBuf::from(name)).to_string_lossy().to_lowercase()
}

fn read_line(f: &File) -> Option<String> {
    let mut s = String::new();
    BufReader::new(f.take(MAX_LINE)).read_line(&mut s).ok()?;
    (!s.trim().is_empty()).then_some(s)
}

fn write_line<T: Serialize>(mut f: &File, v: &T) -> bool {
    let mut line = serde_json::to_vec(v).unwrap_or_default();
    line.push(b'\n');
    f.write_all(&line).is_ok() && f.flush().is_ok()
}

/// Adadan: isteği kasaya yollar ve yanıtı bekler.
pub fn ask(req: &Request) -> std::result::Result<Reply, String> {
    let name = HSTRING::from(pipe_name());
    let started = Instant::now();
    let h = loop {
        match unsafe { CreateFileW(&name, (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) } {
            Ok(h) => break h,
            Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() && started.elapsed() < Duration::from_secs(2) => unsafe {
                let _ = WaitNamedPipeW(&name, 500);
            },
            Err(_) => return Err(t!("Carl Vault açık değil ya da kilitli.", "Carl Vault isn't open or is locked.").into()),
        }
    };
    let f = unsafe { File::from_raw_handle(h.0 as _) };
    // Karşıdaki sunucu gerçekten kasa mı?
    let mut pid = 0;
    let ok = unsafe { GetNamedPipeServerProcessId(HANDLE(h.0), &mut pid).is_ok() };
    if !ok || crate::win::process_exe(pid) != sibling("carlui-vault.exe") {
        crate::warn!("kasa borusundaki sunucu Carl Vault değil; istek yollanmadı");
        return Err(t!("Kasa bağlantısı doğrulanamadı.", "Couldn't verify the vault connection.").into());
    }
    if !write_line(&f, req) {
        return Err(t!("Kasaya ulaşılamadı.", "Couldn't reach the vault.").into());
    }
    read_line(&f).and_then(|l| serde_json::from_str(&l).ok()).ok_or_else(|| t!("Kasa yanıt vermedi.", "The vault didn't answer.").into())
}

/// Kasadan: boruyu dinler. Yalnızca aynı klasördeki `carlui-island.exe`'nin
/// isteklerini `on`'a verir; `on` istekçinin süreç kimliğini de alır.
pub fn serve(on: impl Fn(Request, u32) -> Reply + Send + 'static) {
    let _ = std::thread::Builder::new().name("vault-link".into()).spawn(move || {
        let name = HSTRING::from(pipe_name());
        let island = sibling("carlui-island.exe");
        loop {
            let h = unsafe {
                CreateNamedPipeW(
                    &name,
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    4096,
                    4096,
                    0,
                    None,
                )
            };
            if h.is_invalid() {
                crate::warn!("kasa borusu açılamadı: {}", Error::from_win32());
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            let connected = unsafe { ConnectNamedPipe(h, None).is_ok() || GetLastError() == ERROR_PIPE_CONNECTED };
            let f = unsafe { File::from_raw_handle(h.0 as _) };
            if !connected {
                continue;
            }
            let mut pid = 0;
            let _ = unsafe { GetNamedPipeClientProcessId(h, &mut pid) };
            let exe = crate::win::process_exe(pid);
            let reply = if exe != island {
                crate::warn!("kasa: tanınmayan süreçten istek reddedildi ({exe})");
                Reply::no(t!("İstek reddedildi.", "Request refused."))
            } else {
                match read_line(&f).and_then(|l| serde_json::from_str::<Request>(&l).ok()) {
                    Some(r) => on(r, pid),
                    None => Reply::no(t!("Geçersiz istek.", "Invalid request.")),
                }
            };
            write_line(&f, &reply);
            unsafe {
                let _ = FlushFileBuffers(h);
                let _ = DisconnectNamedPipe(h);
            }
            drop(f);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_roundtrip() {
        let o = Offer { token: "ab".into(), label: "GitHub · Parola".into(), expires: 5 };
        let j = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<Offer>(&j).unwrap(), o);
        assert!(pipe_name().starts_with(r"\\.\pipe\carlui-vault-"));
    }

    #[test]
    fn request_tags() {
        let r = Request::Fill { url: "https://github.com/login".into(), id: "e1".into() };
        let j = serde_json::to_string(&r).unwrap();
        assert!(j.contains("\"op\":\"fill\""));
        assert_eq!(serde_json::from_str::<Request>(&j).unwrap(), r);
        let r = Request::ImportTotp { uri: "otpauth://totp/X?secret=AB".into() };
        assert!(serde_json::to_string(&r).unwrap().contains("\"op\":\"import-totp\""));
        let r = Request::Save { url: "https://roblox.com".into(), user: "carl".into(), password: "p".into(), commit: true, name: "Roblox".into() };
        assert_eq!(serde_json::from_str::<Request>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
        let r = Request::Open { back: 42 };
        assert!(serde_json::to_string(&r).unwrap().contains("\"op\":\"open\""));
        let reply = Reply::no("x");
        assert!(!serde_json::to_string(&reply).unwrap().contains("fill"), "boş alanlar yazılmaz");
    }
}
