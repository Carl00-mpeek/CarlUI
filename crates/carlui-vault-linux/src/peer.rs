//! Köprüye bağlanan yerel süreç kim? Windows'ta bağlantı tablosundan süreç
//! bulunur ve öndeki pencereyle karşılaştırılır. Linux'ta:
//!
//! 1. `/proc/net/tcp`: istemci soketinin satırı (yerel = istemci adresi,
//!    uzak = köprü portu) → soketin sahibi uid ve inode.
//! 2. `/proc/<pid>/fd/*` içinde `socket:[inode]` → süreç.
//! 3. `/proc/<pid>/exe` adı bilinen bir tarayıcı olmalı.
//!
//! Wayland'de "öndeki pencere hangi uygulama" sorulamaz; o denetim yoktur.
//! Chromium'un ağ süreci bazen kendini döküme kapatır (`/proc/<pid>/fd`
//! okunamaz). O zaman süreç bulunamazsa yalnızca soket sahibinin bu kullanıcı
//! olduğu denetlenir; asıl koruma eşleştirme belirtecidir (`pairing`).
//! `CARLUI_STRICT_PEER=1` ile bu durumda da reddedilir.

use std::net::TcpStream;

/// Bilinen tarayıcı exe adları (küçük harf). `CARLUI_BROWSERS` (virgülle) eklenir.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const BROWSERS: &[&str] = &[
    "helium", "chrome", "chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "brave", "brave-browser", "vivaldi-bin", "msedge", "opera", "thorium",
    "firefox", "firefox-bin", "firefox-esr", "librewolf", "zen", "zen-bin", "floorp", "waterfox", "mullvadbrowser",
];

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn is_browser(exe: &str) -> bool {
    let name = exe.rsplit('/').next().unwrap_or(exe).trim_end_matches(" (deleted)").to_ascii_lowercase();
    if BROWSERS.contains(&name.as_str()) {
        return true;
    }
    std::env::var("CARLUI_BROWSERS").is_ok_and(|v| v.split(',').map(|s| s.trim().to_ascii_lowercase()).any(|s| !s.is_empty() && s == name))
}

/// Bağlantıyı açan süreç tarayıcı mı; değilse nedeni. Başarıda süreç adı.
#[cfg(target_os = "linux")]
pub fn check(s: &TcpStream) -> Result<String, String> {
    let peer = s.peer_addr().map_err(|e| e.to_string())?;
    let ours = s.local_addr().map_err(|e| e.to_string())?.port();
    let std::net::SocketAddr::V4(peer) = peer else { return Err("IPv6".into()) };
    let (uid, inode) = socket_of(peer.port(), ours).ok_or("soket bulunamadı")?;
    let me = my_uid();
    if me.is_some_and(|m| m != uid) {
        return Err(format!("başka kullanıcının bağlantısı (uid {uid})"));
    }
    match pid_of(inode) {
        Some(pid) => {
            let exe = std::fs::read_link(format!("/proc/{pid}/exe")).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_string();
            if is_browser(&exe) || (exe.is_empty() && is_browser(&comm)) {
                Ok(if exe.is_empty() { comm } else { exe })
            } else {
                Err(format!("tarayıcı değil: {}", if exe.is_empty() { comm } else { exe.rsplit('/').next().unwrap_or("?").to_string() }))
            }
        }
        None if std::env::var("CARLUI_STRICT_PEER").is_ok_and(|v| v == "1") => Err("süreç doğrulanamadı".into()),
        None => Ok("(süreç okunamadı, aynı kullanıcı)".into()),
    }
}

/// Linux dışında (geliştirme için Windows'ta) süreç denetimi yok.
#[cfg(not(target_os = "linux"))]
pub fn check(_s: &TcpStream) -> Result<String, String> {
    Ok("(denetim yok)".into())
}

/// "0100007F:1F90" → port.
fn port_of(addr: &str) -> Option<u16> {
    u16::from_str_radix(addr.rsplit(':').next()?, 16).ok()
}

/// İstemci soketinin (uid, inode) çifti.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn socket_of(client_port: u16, server_port: u16) -> Option<(u32, u64)> {
    let table = std::fs::read_to_string("/proc/net/tcp").ok()?;
    parse_tcp(&table, client_port, server_port)
}

fn parse_tcp(table: &str, client_port: u16, server_port: u16) -> Option<(u32, u64)> {
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        if port_of(f[1]) == Some(client_port) && port_of(f[2]) == Some(server_port) {
            let uid = f[7].parse().ok()?;
            let inode = f[9].parse().ok()?;
            if inode != 0 {
                return Some((uid, inode));
            }
        }
    }
    None
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pid_of(inode: u64) -> Option<u32> {
    let want = format!("socket:[{inode}]");
    for p in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = p.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|l| l.to_string_lossy() == want) {
                return Some(pid);
            }
        }
    }
    None
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn my_uid() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find(|l| l.starts_with("Uid:"))?.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsers() {
        assert!(is_browser("/usr/lib/helium/helium"));
        assert!(is_browser("/usr/lib/firefox/firefox"));
        assert!(is_browser("/opt/google/chrome/chrome (deleted)"));
        assert!(!is_browser("/usr/bin/curl"));
        assert!(!is_browser("/usr/bin/python3"));
    }

    #[test]
    fn tcp_table() {
        let t = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:B8C9 0100007F:00 0A 00000000:00000000 00:00000000 00000000  1000        0 11111 1\n   1: 0100007F:D431 0100007F:B8C9 01 00000000:00000000 00:00000000 00000000  1000        0 22222 1\n   2: 0100007F:B8C9 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 33333 1\n";
        // İstemci 54321 (D431), köprü 47305 (B8C9).
        assert_eq!(parse_tcp(t, 0xD431, 0xB8C9), Some((1000, 22222)));
        assert_eq!(parse_tcp(t, 1, 0xB8C9), None);
    }
}
