//! Bu modül "Steam, Epic, torrent gibi başka programların indirmeleri de
//! adada görünsün" sorununu yönetici izni gerektirmeden şu yöntemle çözüyor:
//! saniyede bir fiziksel ağ bağdaştırıcılarının aldığı bayt ile her sürecin
//! diske yazdığı bayt okunur. İnternetten hızla veri gelirken aynı anda diske
//! de yazan süreç büyük olasılıkla o veriyi indirendir; üç saniye üst üste
//! aynı süreç çıkarsa hapta "↓ Steam · 1,2 GB · 48 MB/sn" görünür. Bu bir
//! tahmindir: dosya adı ve yüzde bilinmez, hız tüm bilgisayarın alım hızıdır.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_TIMER},
};

const MB: f64 = 1024.0 * 1024.0;
/// Başlamak için en az bu alım hızı…
const START_RX: f64 = 1.0 * MB;
/// …ve en az bu yazma hızı.
const START_W: f64 = 0.5 * MB;
/// Başladıktan sonra bu hızın altı "durdu" sayılır.
const KEEP_RX: f64 = 0.125 * MB;
/// Bu kadar saniye üst üste aynı süreç çıkınca gösterilir.
const CONFIRM: u8 = 3;
/// Bu kadar saniye iz kalmazsa biter.
const GRACE: u8 = 6;

/// Diske yazsa da indirici olamayacak süreçler.
const SKIP: &[&str] = &["system", "registry", "memory compression", "msmpeng.exe", "searchindexer.exe", "mssense.exe"];

/// Gösterilen indirme: (ad, inen bayt, toplam (0 = bilinmiyor), bayt/sn).
static CUR: Mutex<Option<(String, u64, u64, f64)>> = Mutex::new(None);

pub fn current() -> Option<(String, u64, u64, f64)> {
    CUR.lock().ok().and_then(|c| c.clone())
}

/// Steam'in son yazdığı (oyun, inen) ve o andan beri ölçülen alım.
static STEAM: Mutex<Option<(String, u64, f64)>> = Mutex::new(None);

/// Program kendi ilerlemesini bir dosyaya yazıyorsa oradan ayrıntı alınır
/// (şimdilik Steam: oyunun adı, inen ve toplam bayt). Steam inen miktarı
/// yalnızca başlarken/dururken yazar; aradaki ilerleme ağ alımından eklenir.
fn detail(app: &str, got: u64, rx: f64, dt: f64) -> (String, u64, u64, f64) {
    match app {
        "Steam" => match crate::steam::downloading() {
            Some((game, base, total)) => {
                let Ok(mut s) = STEAM.lock() else { return (game, base, total, rx) };
                let extra = match s.as_mut() {
                    Some((g, b, e)) if *g == game && *b == base => {
                        *e += rx * dt;
                        *e
                    }
                    _ => {
                        *s = Some((game.clone(), base, 0.0));
                        0.0
                    }
                };
                (game, (base + extra as u64).min(total.saturating_sub(1)), total, rx)
            }
            None => (app.into(), got, 0, rx),
        },
        _ => (app.into(), got, 0, rx),
    }
}

/// Exe adından okunur program adı.
fn label(exe: &str) -> String {
    let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe);
    let known = match stem.to_lowercase().as_str() {
        "steam" | "steamservice" => "Steam",
        "epicgameslauncher" | "epicwebhelper" => "Epic Games",
        "utorrent" => "µTorrent",
        "utweb" => "µTorrent Web",
        "bittorrent" => "BitTorrent",
        "qbittorrent" => "qBittorrent",
        "battle.net" | "agent" => "Battle.net",
        "eadesktop" | "eabackgroundservice" => "EA app",
        "upc" | "ubisoftconnect" => "Ubisoft Connect",
        "galaxyclient" | "galaxycommunication" => "GOG Galaxy",
        "riotclientservices" => "Riot Client",
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "msedge" => "Edge",
        "svchost" | "tiworker" | "mousocoreworker" | "wuauclt" => "Windows Update",
        "winstore.app" | "wsappx" => "Microsoft Store",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    let mut c = stem.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// En çok yazan süreç, alımla birlikte indirme gibi görünüyorsa.
fn pick(rx: f64, writes: &[(u32, String, f64)]) -> Option<usize> {
    if rx < START_RX {
        return None;
    }
    let (i, w) = writes.iter().enumerate().max_by(|a, b| a.1 .2.total_cmp(&b.1 .2))?;
    // Tarayıcı video izlerken de önbelleğe yazar; 4K akış ~3 MB/sn'yi geçmez.
    let browser = BROWSERS.contains(&w.1.to_lowercase().as_str());
    if browser && rx < BROWSER_RX {
        return None;
    }
    (w.2 >= START_W && w.2 >= rx * 0.2).then_some(i)
}

const BROWSERS: &[&str] = &["firefox.exe", "chrome.exe", "msedge.exe", "opera.exe", "brave.exe", "vivaldi.exe"];
const BROWSER_RX: f64 = 5.0 * MB;

struct Active {
    pid: u32,
    name: String,
    got: f64,
    miss: u8,
}

#[derive(Default)]
struct Watch {
    rx: u64,
    w: HashMap<u32, u64>,
    cand: Option<(u32, u8)>,
    active: Option<Active>,
}

impl Watch {
    /// Bir ölçüm; `dt` saniye. Gösterilecek (program, inen, hız) döner.
    fn step(&mut self, dt: f64) -> Option<(String, u64, f64)> {
        let rx_now = carlui_sys::net::rx_total();
        let rx = if self.rx == 0 { 0.0 } else { rx_now.saturating_sub(self.rx) as f64 / dt };
        self.rx = rx_now;
        let me = std::process::id();
        let mut writes = Vec::new();
        let mut seen = HashMap::new();
        for p in carlui_sys::process::list() {
            if p.pid <= 4 || p.pid == me {
                continue;
            }
            let lower = p.name.to_lowercase();
            if lower.starts_with("carlui") || SKIP.contains(&lower.as_str()) {
                continue;
            }
            if let Some(&old) = self.w.get(&p.pid) {
                writes.push((p.pid, p.name.clone(), p.write_bytes.saturating_sub(old) as f64 / dt));
            }
            seen.insert(p.pid, p.write_bytes);
        }
        self.w = seen;

        if let Some(a) = &mut self.active {
            let wrote = writes.iter().any(|w| w.0 == a.pid && w.2 > 0.0);
            if rx >= KEEP_RX && wrote {
                a.miss = 0;
            } else {
                a.miss += 1;
            }
            if a.miss >= GRACE || !self.w.contains_key(&a.pid) {
                self.active = None;
                self.cand = None;
                return None;
            }
            a.got += rx * dt;
            return Some((a.name.clone(), a.got as u64, rx));
        }
        let Some(i) = pick(rx, &writes) else {
            self.cand = None;
            return None;
        };
        let pid = writes[i].0;
        let n = match self.cand {
            Some((p, n)) if p == pid => n + 1,
            _ => 1,
        };
        self.cand = Some((pid, n));
        if n < CONFIRM {
            return None;
        }
        let name = label(&writes[i].1);
        carlui_core::info!("indirme sezildi: {name} ({:.1} MB/sn)", rx / MB);
        self.active = Some(Active { pid, name: name.clone(), got: rx * dt * CONFIRM as f64, miss: 0 });
        Some((name, (rx * dt * CONFIRM as f64) as u64, rx))
    }
}

/// Saniyede bir ölçer; bir şey göründükçe adaya `TIMER_DL` gönderir ki hap tazelensin.
pub fn start(hwnd: isize) {
    std::thread::Builder::new()
        .name("netwatch".into())
        .spawn(move || {
            let mut w = Watch::default();
            let mut last = Instant::now();
            let mut was = false;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let dt = last.elapsed().as_secs_f64().max(0.1);
                last = Instant::now();
                let now = w.step(dt).map(|(app, got, rx)| detail(&app, got, rx, dt));
                let on = now.is_some();
                if let Ok(mut c) = CUR.lock() {
                    *c = now;
                }
                if on || was {
                    unsafe {
                        let _ = PostMessageW(HWND(hwnd as _), WM_TIMER, WPARAM(crate::dlman::TIMER_DL), LPARAM(0));
                    }
                }
                was = on;
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label("steam.exe"), "Steam");
        assert_eq!(label("EpicGamesLauncher.exe"), "Epic Games");
        assert_eq!(label("qbittorrent.exe"), "qBittorrent");
        assert_eq!(label("aria2c.exe"), "Aria2c");
    }

    #[test]
    fn picks() {
        let w = |v: &[(u32, f64)]| v.iter().map(|&(p, r)| (p, format!("p{p}.exe"), r * MB)).collect::<Vec<_>>();
        // Hızlı alım + hızlı yazım: en çok yazan seçilir.
        assert_eq!(pick(40.0 * MB, &w(&[(1, 0.2), (2, 38.0)])), Some(1));
        // Alım yok: yazım ne kadar çok olursa olsun indirme değil.
        assert_eq!(pick(0.1 * MB, &w(&[(2, 300.0)])), None);
        // Video izlerken: alım var ama kimse kayda değer yazmıyor.
        assert_eq!(pick(8.0 * MB, &w(&[(2, 0.3)])), None);
        // Yazım alımın çok altında (başka bir şey yazıyor).
        assert_eq!(pick(50.0 * MB, &w(&[(2, 4.0)])), None);
        // Tarayıcıda 4K video: önbelleğe yazsa da indirme sayılmaz.
        let ff = vec![(9, "firefox.exe".to_string(), 3.0 * MB)];
        assert_eq!(pick(3.0 * MB, &ff), None);
        assert_eq!(pick(30.0 * MB, &[(9, "firefox.exe".to_string(), 30.0 * MB)]), Some(0));
    }

    /// Canlı: `cargo test -p carlui-island -- --ignored live --nocapture`
    #[test]
    #[ignore]
    fn live() {
        let mut w = Watch::default();
        for _ in 0..12 {
            std::thread::sleep(Duration::from_secs(1));
            println!("{:?}", w.step(1.0));
        }
    }
}
