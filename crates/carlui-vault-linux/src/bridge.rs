//! Tarayıcı eklentisinin konuştuğu yerel köprü (`127.0.0.1:47321`). Windows'ta
//! bu köprü CarlUI adasındadır ve isteği kasaya boruyla aktarır; Linux'ta kasa
//! penceresi köprüyü kendisi açar. Yollar, başlıklar ve yanıtlar aynıdır, bu
//! yüzden eklenti iki sistemde de değişmeden çalışır.
//!
//! İndirme yakalama (`/offer`) Linux'ta yoktur: `/ping` `enabled: false` döner,
//! teklifler tarayıcıya bırakılır.
//!
//! Güvenlik katmanları: `X-CarlUI` başlığı ve eklenti kökeni; kasa isteklerinde
//! bağlantıyı açan sürecin bu kullanıcıya ait bir tarayıcı olması (`peer`);
//! eşleştirme belirteci (`pairing`); doldurulan kaydın adresinin sekme adresine
//! uyması (`autofill::body_matches`).

use crate::core::{AskKind, Shared, ASK_TTL};
use crate::reply::Reply;
use carlui_vault::t;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    time::{Duration, Instant},
};

pub const PORT: u16 = 47321;

pub fn port() -> u16 {
    std::env::var("CARLUI_BRIDGE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

fn respond(s: &mut TcpStream, code: &str, body: &str) {
    let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

/// Dinlemeye başlar. Port doluysa (başka bir kasa ya da CarlUI açık) hata döner.
pub fn start(shared: Arc<Shared>) -> std::io::Result<()> {
    let l = TcpListener::bind(("127.0.0.1", port()))?;
    std::thread::Builder::new().name("bridge".into()).spawn(move || {
        for s in l.incoming().flatten() {
            let sh = shared.clone();
            std::thread::spawn(move || handle(s, &sh));
        }
    })?;
    Ok(())
}

fn handle(mut s: TcpStream, shared: &Shared) {
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
    let (mut len, mut origin, mut marked, mut token) = (0usize, String::new(), false, String::new());
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().unwrap_or(0).min(1 << 20),
            "origin" => origin = v.trim().to_string(),
            "x-carlui" => {
                marked = true;
                token = v.trim().to_string();
            }
            _ => {}
        }
    }
    let from_ext = origin.is_empty() || origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://") || origin.starts_with("extension://");
    if method == "OPTIONS" || !marked || !from_ext {
        respond(&mut s, "403 Forbidden", "{}");
        return;
    }
    let mut browser = String::from("?");
    if path.starts_with("/vault/") {
        match crate::peer::check(&s) {
            Ok(b) => browser = b.rsplit('/').next().unwrap_or(&b).to_string(),
            Err(why) => {
                eprintln!("köprü: kasa isteği reddedildi ({why})");
                let msg = t!("Bu istek tarayıcı eklentisinden gelmedi.", "This request didn't come from the browser extension.");
                respond(&mut s, "403 Forbidden", &json!({ "ok": false, "message": msg }).to_string());
                return;
            }
        }
    }
    let mut body = vec![0u8; len];
    if len > 0 && r.read_exact(&mut body).is_err() {
        return;
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let fill_on = shared.core().settings.browser_fill;
    match (method.as_str(), path.as_str()) {
        ("GET", "/ping") => respond(&mut s, "200 OK", &format!("{{\"ok\":true,\"enabled\":false,\"lang\":\"{}\"}}", if carlui_vault::lang::is_tr() { "tr" } else { "en" })),
        ("POST", "/progress") => respond(&mut s, "200 OK", "{}"),
        ("POST", p) if p.starts_with("/vault/") && !fill_on => {
            respond(&mut s, "200 OK", &json!({ "ok": false, "off": true, "message": t!("Tarayıcıda doldurma Ayarlar'da kapalı.", "Browser filling is turned off in Settings.") }).to_string());
        }
        ("POST", "/vault/pair") => respond(&mut s, "200 OK", &crate::pairing::pair(shared, &origin).to_string()),
        ("POST", p) if p.starts_with("/vault/") && !crate::pairing::paired(&token) => {
            let msg = t!("Eklentinin Carl Vault'a bağlanması gerekiyor. Eklentiyi güncelleyip yeniden dene.", "The extension needs to connect to Carl Vault. Update the extension and try again.");
            respond(&mut s, "200 OK", &json!({ "ok": false, "pair": true, "message": msg }).to_string());
        }
        ("POST", "/vault/find") | ("POST", "/vault/fill") | ("POST", "/vault/save") => {
            let reply = {
                let mut c = shared.core();
                let r = match path.as_str() {
                    "/vault/find" => c.find_logins(&g("url")),
                    "/vault/fill" => c.fill_login(&g("url"), &g("id"), &browser),
                    _ => c.save_login(&g("url"), &g("user"), &g("password"), v.get("commit").and_then(|x| x.as_bool()).unwrap_or(false), &g("name")),
                };
                if r.ok {
                    c.touch();
                }
                r
            };
            shared.repaint();
            respond(&mut s, "200 OK", &reply.json());
        }
        ("POST", "/vault/passkey") => {
            let list = |k: &str| v.get(k).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).take(64).collect()).unwrap_or_default();
            let ask = match g("op").as_str() {
                "create" => {
                    let algs: Vec<i64> = v.get("algs").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|n| n.as_i64()).take(16).collect()).unwrap_or_default();
                    crate::core::passkey_create(&g("origin"), &g("rp_id"), g("rp_name"), g("user_name"), g("user_display"), &g("user_id"), g("challenge"), &algs, list("exclude"))
                }
                "get" => crate::core::passkey_get(&g("origin"), &g("rp_id"), g("challenge"), list("allow")),
                _ => {
                    respond(&mut s, "400 Bad Request", "{}");
                    return;
                }
            };
            let reply = match ask {
                Ok(ask) => passkey(shared, ask),
                Err(r) => r,
            };
            respond(&mut s, "200 OK", &reply.json());
        }
        ("POST", "/vault/open") => {
            shared.raise();
            let locked = !shared.core().is_open();
            respond(&mut s, "200 OK", &Reply { ok: true, locked, ..Default::default() }.json());
        }
        ("POST", "/offer") | ("POST", "/offer/wait") => respond(&mut s, "200 OK", "{\"action\":\"browser\"}"),
        _ => respond(&mut s, "404 Not Found", "{}"),
    }
}

/// Passkey isteği: kasa kilitliyse pencere öne gelir ve açılması beklenir,
/// sonra onay kartı gösterilir. Ret, süre ya da kilit: tarayıcıya bırakılır.
fn passkey(shared: &Shared, mut ask: crate::core::PasskeyAsk) -> Reply {
    let started = Instant::now();
    if !shared.core().is_open() {
        shared.asks().waiting_unlock = Some(ask.site().to_string());
        shared.raise();
        // Eklenti 130 sn bekler: açılış için en çok 80 sn, kart için kalan süre.
        while !shared.core().is_open() && started.elapsed() < Duration::from_secs(80) {
            std::thread::sleep(Duration::from_millis(250));
        }
        shared.asks().waiting_unlock = None;
        shared.repaint();
        if !shared.core().is_open() {
            return Reply::fallback("");
        }
    }
    if let Some(r) = shared.core().passkey_prepare(&mut ask) {
        return r;
    }
    let left = ASK_TTL.saturating_sub(started.elapsed());
    let (answer, kind) = shared.ask(AskKind::Passkey(ask), left);
    let (Some(i), AskKind::Passkey(ask)) = (answer, kind) else { return Reply::fallback("") };
    let reply = {
        let mut c = shared.core();
        c.touch();
        c.passkey_approve(&ask, i)
    };
    shared.repaint();
    reply
}
