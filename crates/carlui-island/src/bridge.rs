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
//!   sayfası doldurma; istek kasanın borusuna aktarılır (bkz. `vaultlink`).
//! - `POST /progress` → yönetici kapalıyken tarayıcının kendi indirmeleri
//!   gerçek yüzde ve hızla adada görünür.
//!
//! Web sayfaları bu adrese istek atamaz: `X-CarlUI` başlığı ön kontrol
//! (CORS preflight) gerektirir ve buna izin verilmez; ayrıca köken yalnızca
//! eklenti kökeni olabilir. Yönetici kapalıysa teklif hemen "browser" döner.

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
static BROWSER: Mutex<(Option<Instant>, Vec<(String, u64, u64, f64)>)> = Mutex::new((None, Vec::new()));

pub fn browser() -> Vec<(String, u64, u64, f64)> {
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
        ("GET", "/ping") => respond(&mut s, "200 OK", &format!("{{\"ok\":true,\"enabled\":{on}}}")),
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
        ("POST", "/vault/find") | ("POST", "/vault/fill") => {
            use carlui_core::vaultlink::{ask, Request};
            let req = if path == "/vault/find" { Request::Find { url: g("url") } } else { Request::Fill { url: g("url"), id: g("id") } };
            let body = match ask(&req) {
                Ok(r) => serde_json::to_string(&r).unwrap_or_default(),
                Err(m) => serde_json::json!({ "ok": false, "closed": true, "message": m }).to_string(),
            };
            respond(&mut s, "200 OK", &body);
        }
        ("POST", "/vault/open") => {
            let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("carlui-vault.exe")));
            let ok = exe.is_some_and(|e| std::process::Command::new(e).spawn().is_ok());
            info!("eklentiden Carl Vault açıldı: {ok}");
            respond(&mut s, "200 OK", &format!("{{\"ok\":{ok}}}"));
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
