//! Bu modül "tarayıcıda başlayan indirme CarlUI'ya nasıl gelir?" sorununu
//! şu yöntemle çözüyor: isteğe bağlı tarayıcı eklentisi (elle kurulur, bkz.
//! `extension/`) yalnızca bu bilgisayardan erişilebilen küçük bir yerel
//! adrese (127.0.0.1:47321) konuşur:
//! - `GET /ping` → yönetici açık mı?
//! - `POST /offer` → tarayıcı indirmeyi duraklatır; ada "İndirme bilgisi"
//!   kartıyla aşağı açılır (ad, boyut, kaynak, kayıt yeri). Kullanıcının
//!   seçimi (CarlUI indirir / tarayıcıya bırak) yanıt olarak döner;
//!   eklenti tarayıcıdaki indirmeyi buna göre iptal eder ya da sürdürür.
//! - `POST /vault/find`, `/vault/fill`, `/vault/open` → Carl Vault ile giriş
//!   sayfası doldurma; `/vault/save` → sayfada gönderilen giriş/kayıt formunu
//!   kasaya sorma ve (kullanıcı onaylayınca) kaydetme. İstek kasanın borusuna
//!   aktarılır (bkz. `vaultlink`).
//! - `POST /vault/passkey` → sitenin passkey isteği (oluşturma/giriş) kasaya
//!   sorulur; kasa onay isterken köprü sonucu bekler (bkz. `app/passkeys.rs`).
//! - `POST /progress` → yönetici kapalıyken tarayıcının kendi indirmeleri
//!   gerçek yüzde ve hızla adada görünür.
//!
//! Web sayfaları bu adrese istek atamaz: `X-CarlUI` başlığı ön kontrol
//! (CORS preflight) gerektirir ve buna izin verilmez; ayrıca köken yalnızca
//! eklenti kökeni olabilir. Yönetici kapalıysa teklif hemen "browser" döner.
//!
//! Başlık ve köken bu bilgisayardaki başka bir programca taklit edilebilir;
//! bu yüzden `/vault/` istekleri ayrıca bağlantıyı açan sürece bakılarak
//! kabul edilir: bilinen bir tarayıcı olmalı ve öndeki pencereyle aynı exe
//! olmalı (bkz. `browser_peer`).

use crate::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{atomic::AtomicBool, mpsc},
};

pub const PORT: u16 = 47321;
pub const WM_APP_OFFER: u32 = WM_APP + 30;

/// Ayarlardaki "İndirme yöneticisi".
pub static ENABLED: AtomicBool = AtomicBool::new(true);
/// Ayarlardaki Carl Vault → "Tarayıcıda doldurma" (kasa kapalıyken de eklenti
/// sayfa içi düğmeyi göstermesin).
pub static VAULT_FILL: AtomicBool = AtomicBool::new(true);

/// Karttaki seçim: eklentiye dönen yanıt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    CarlUI,
    Browser,
}

pub struct Offer {
    pub url: String,
    pub name: String,
    pub size: u64,
    pub host: String,
    pub headers: String,
    pub at: Instant,
    reply: Option<mpsc::Sender<Choice>>,
}

/// Köprü bu kadar bekler; sonra tarayıcı indirmeyi kendisi sürdürür.
pub const OFFER_TTL: Duration = Duration::from_secs(235);

impl Offer {
    pub fn answer(&mut self, c: Choice) {
        if let Some(tx) = self.reply.take() {
            let _ = tx.send(c);
        }
    }
}

impl Drop for Offer {
    fn drop(&mut self) {
        // Yanıtsız kalan teklif tarayıcıya bırakılır (duraklatılmış indirme sürer).
        self.answer(Choice::Browser);
    }
}

static OFFERS: Mutex<VecDeque<Offer>> = Mutex::new(VecDeque::new());

pub fn take_offers() -> Vec<Offer> {
    OFFERS.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

/// Tarayıcının kendi indirmeleri: (ad, inen, toplam, bayt/sn).
type Progress = (String, u64, u64, f64);
static BROWSER: Mutex<(Option<Instant>, Vec<Progress>)> = Mutex::new((None, Vec::new()));

pub fn browser() -> Vec<Progress> {
    match BROWSER.lock() {
        Ok(g) if g.0.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) => g.1.clone(),
        _ => Vec::new(),
    }
}

fn clean_header(s: &str) -> String {
    s.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

fn respond(s: &mut TcpStream, code: &str, body: &str) {
    let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

/// Bağlantıyı açan süreç bilinen bir tarayıcı mı ve öndeki pencereyle aynı
/// exe mi (Chrome/Edge'de ağ isteği tarayıcının yardımcı sürecinden gelir;
/// yolu ana pencereninkiyle aynıdır). Değilse nedeni.
fn browser_peer(s: &TcpStream) -> std::result::Result<(), String> {
    let std::net::SocketAddr::V4(peer) = s.peer_addr().map_err(|e| e.to_string())? else { return Err("IPv6".into()) };
    let port = s.local_addr().map_err(|e| e.to_string())?.port();
    let pid = carlui_core::win::tcp_client_pid(peer, port).ok_or("süreç bulunamadı")?;
    let exe = carlui_core::win::process_exe(pid);
    if !carlui_core::win::is_browser(&exe) {
        return Err(format!("tarayıcı değil: {}", exe.rsplit('\\').next().unwrap_or("?")));
    }
    let front = carlui_core::win::window_exe(unsafe { GetForegroundWindow() });
    if exe != front {
        return Err("istekte bulunan tarayıcı önde değil".into());
    }
    Ok(())
}

fn handle(mut s: TcpStream, hwnd: isize) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut r = BufReader::new(match s.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    });
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let (mut len, mut origin, mut marked) = (0usize, String::new(), false);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap_or(0).min(1 << 20),
            "origin" => origin = v.trim().to_string(),
            "x-carlui" => marked = true,
            _ => {}
        }
    }
    let from_ext = origin.is_empty() || origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") || origin.starts_with("extension://");
    if method == "OPTIONS" || !marked || !from_ext {
        respond(&mut s, "403 Forbidden", "{}");
        return;
    }
    // Kasa istekleri: bağlantıyı açan süreç öndeki tarayıcının kendisi olmalı.
    // Başlık ve köken başka bir yerel programca kolayca taklit edilir; süreç
    // ise Windows'un bağlantı tablosundan okunur.
    if path.starts_with("/vault/") {
        if let Err(why) = browser_peer(&s) {
            carlui_core::warn!("köprü: kasa isteği reddedildi ({why})");
            let msg = t!("Bu istek tarayıcı eklentisinden gelmedi.", "This request didn't come from the browser extension.");
            respond(&mut s, "403 Forbidden", &serde_json::json!({ "ok": false, "message": msg }).to_string());
            return;
        }
    }
    let mut body = vec![0u8; len];
    if len > 0 && r.read_exact(&mut body).is_err() {
        return;
    }
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let on = ENABLED.load(Ordering::Relaxed);
    let post = |m: u32, w: usize| unsafe {
        let _ = PostMessageW(HWND(hwnd as _), m, WPARAM(w), LPARAM(0));
    };
    match (method.as_str(), path.as_str()) {
        // `lang`: Ayarlar'da seçili arayüz dili; eklenti metinlerini buna göre yazar.
        ("GET", "/ping") => respond(&mut s, "200 OK", &format!("{{\"ok\":true,\"enabled\":{on},\"lang\":\"{}\"}}", if carlui_core::lang::is_tr() { "tr" } else { "en" })),
        ("POST", "/progress") => {
            let list = v.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default();
            let items: Vec<_> = list
                .iter()
                .map(|i| {
                    let n = |k: &str| i.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0).max(0.0);
                    let name = i.get("name").and_then(|x| x.as_str()).unwrap_or("").rsplit(['\\', '/']).next().unwrap_or("").to_string();
                    (name, n("got") as u64, n("total") as u64, n("speed"))
                })
                .collect();
            if let Ok(mut b) = BROWSER.lock() {
                *b = (Some(Instant::now()), items);
            }
            post(WM_TIMER, crate::dlman::TIMER_DL);
            respond(&mut s, "200 OK", "{}");
        }
        // Carl Vault: istek kasanın doğrulanmış borusuna aktarılır (kasa açık,
        // adres uyuyor ve tarayıcı önde olmalı; bkz. carlui_core::vaultlink).
        ("POST", p) if p.starts_with("/vault/") && !VAULT_FILL.load(Ordering::Relaxed) => {
            respond(&mut s, "200 OK", &serde_json::json!({ "ok": false, "off": true, "message": t!("Tarayıcıda doldurma Ayarlar'da kapalı.", "Browser filling is turned off in Settings.") }).to_string());
        }
        ("POST", "/vault/find") | ("POST", "/vault/fill") | ("POST", "/vault/save") => {
            use carlui_core::vaultlink::{ask, Request};
            let req = match path.as_str() {
                "/vault/find" => Request::Find { url: g("url") },
                "/vault/fill" => Request::Fill { url: g("url"), id: g("id") },
                _ => Request::Save { url: g("url"), user: g("user"), password: g("password"), commit: v.get("commit").and_then(|x| x.as_bool()).unwrap_or(false), name: g("name") },
            };
            let body = match ask(&req) {
                Ok(r) => serde_json::to_string(&r).unwrap_or_default(),
                Err(m) => serde_json::json!({ "ok": false, "closed": true, "message": m }).to_string(),
            };
            respond(&mut s, "200 OK", &body);
        }
        // Passkey: kasa hemen bir bilet döner ve onay ister; sonuç gelene dek
        // (en çok 2 dk) burada sorulur. Kasa kapalıysa eklenti tarayıcının
        // kendi passkey akışına düşer (`fallback`).
        ("POST", "/vault/passkey") => {
            use carlui_core::vaultlink::{ask, Reply, Request};
            let list = |k: &str| v.get(k).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).take(64).collect()).unwrap_or_default();
            let req = match g("op").as_str() {
                "create" => Request::PasskeyCreate {
                    origin: g("origin"),
                    rp_id: g("rp_id"),
                    rp_name: g("rp_name"),
                    user_name: g("user_name"),
                    user_display: g("user_display"),
                    user_id: g("user_id"),
                    challenge: g("challenge"),
                    algs: v.get("algs").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|n| n.as_i64()).take(16).collect()).unwrap_or_default(),
                    exclude: list("exclude"),
                },
                "get" => Request::PasskeyGet { origin: g("origin"), rp_id: g("rp_id"), challenge: g("challenge"), allow: list("allow") },
                _ => {
                    respond(&mut s, "400 Bad Request", "{}");
                    return;
                }
            };
            let started = Instant::now();
            let mut reply = ask(&req);
            while let Ok(Reply { pending, .. }) = &reply {
                if pending.is_empty() || started.elapsed() > Duration::from_secs(120) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
                reply = ask(&Request::PasskeyWait { ticket: pending.clone() });
            }
            let body = match reply {
                Ok(r) if !r.pending.is_empty() => serde_json::json!({ "ok": false, "fallback": true }).to_string(),
                Ok(r) => serde_json::to_string(&r).unwrap_or_default(),
                Err(m) => serde_json::json!({ "ok": false, "closed": true, "fallback": true, "message": m }).to_string(),
            };
            respond(&mut s, "200 OK", &body);
        }
        // Kasa öne gelir; kapalıysa başlatılır. Öndeki pencere (eklentiye basılan
        // tarayıcı) bildirilir ki kilit açılınca odak oraya dönsün.
        ("POST", "/vault/open") => {
            use carlui_core::vaultlink::{ask, Request};
            let req = Request::Open { back: unsafe { GetForegroundWindow() }.0 as isize };
            let mut reply = ask(&req);
            if reply.is_err() {
                let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("carlui-vault.exe")));
                if exe.is_some_and(|e| std::process::Command::new(e).spawn().is_ok()) {
                    for _ in 0..30 {
                        std::thread::sleep(Duration::from_millis(250));
                        reply = ask(&req);
                        if reply.is_ok() {
                            break;
                        }
                    }
                }
            }
            info!("eklentiden Carl Vault açıldı: {}", reply.is_ok());
            let body = match reply {
                Ok(r) => serde_json::to_string(&r).unwrap_or_default(),
                Err(m) => serde_json::json!({ "ok": false, "closed": true, "message": m }).to_string(),
            };
            respond(&mut s, "200 OK", &body);
        }
        ("POST", "/offer") if !on => respond(&mut s, "200 OK", "{\"action\":\"browser\"}"),
        ("POST", "/offer") => {
            let url = g("url");
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                respond(&mut s, "200 OK", "{\"action\":\"browser\"}");
                return;
            }
            let mut headers = Vec::new();
            for (k, h) in [("cookies", "Cookie"), ("referrer", "Referer"), ("ua", "User-Agent")] {
                let val = clean_header(&g(k));
                if !val.is_empty() {
                    headers.push(format!("{h}: {val}"));
                }
            }
            let headers = headers.join("\r\n");
            // Ad ve boyut sunucudan (IDM'deki gibi): tarayıcı çoğu zaman henüz bilmiyor.
            let (probed, psize) = crate::dlman::probe(&url, &headers);
            let given = g("filename").rsplit(['\\', '/']).next().unwrap_or("").to_string();
            let name = probed.or((!given.is_empty()).then_some(given)).unwrap_or_else(|| crate::dlman::name_from_url(&url));
            let size = if psize > 0 { psize } else { v.get("size").and_then(|x| x.as_f64()).unwrap_or(0.0).max(0.0) as u64 };
            let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("").to_string();
            let (tx, rx) = mpsc::channel();
            info!("tarayıcıdan indirme teklifi: {name} ({host})");
            if let Ok(mut q) = OFFERS.lock() {
                q.push_back(Offer { url, name, size, host, headers, at: Instant::now(), reply: Some(tx) });
            }
            post(WM_APP_OFFER, 0);
            // Tarayıcının servis çalışanı ~5 dk'da kapanabilir; ondan önce yanıtla.
            let c = rx.recv_timeout(Duration::from_secs(240)).unwrap_or(Choice::Browser);
            let a = match c {
                Choice::CarlUI => "carlui",
                Choice::Browser => "browser",
            };
            respond(&mut s, "200 OK", &format!("{{\"action\":\"{a}\"}}"));
        }
        _ => respond(&mut s, "404 Not Found", "{}"),
    }
}

pub fn start(hwnd: isize) {
    std::thread::Builder::new()
        .name("bridge".into())
        .spawn(move || {
            let l = match TcpListener::bind(("127.0.0.1", PORT)) {
                Ok(l) => l,
                Err(e) => {
                    carlui_core::warn!("eklenti köprüsü açılamadı (127.0.0.1:{PORT}): {e}");
                    return;
                }
            };
            for s in l.incoming().flatten() {
                std::thread::spawn(move || handle(s, hwnd));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_browser_peer_is_refused() {
        // Test süreci tarayıcı değil: curl/PowerShell gibi her yerel program böyle reddedilir.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let _c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let why = browser_peer(&s).unwrap_err();
        assert!(why.starts_with("tarayıcı değil"), "{why}");
    }

    /// Elle: `cargo test -p carlui-island real_browser -- --ignored --nocapture`,
    /// sonra yazdığı adresi öndeki tarayıcıda aç.
    #[test]
    #[ignore]
    fn real_browser_peer_is_accepted() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        println!("ADRES http://{}/", l.local_addr().unwrap());
        let (mut s, _) = l.accept().unwrap();
        let r = browser_peer(&s);
        respond(&mut s, "200 OK", &format!("{{\"sonuc\":\"{r:?}\"}}"));
        assert_eq!(r, Ok(()));
    }
}
