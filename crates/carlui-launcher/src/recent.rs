//! Bu modül "az önce açtığım dosya neredeydi?" sorununu şu yöntemle çözüyor:
//! Windows'un kendi "son kullanılanlar" klasöründeki (%APPDATA%\Microsoft\
//! Windows\Recent) kısayollar değişiklik tarihine göre sıralanır. "son"
//! yazınca en son açılan dosyalar listelenir, normal aramada da adı uyan son
//! dosyalar çıkar. Kısayolun kendisi açılır (Windows hedefi bulur); liste
//! 20 saniyede bir yenilenir, her tuşta disk okunmaz.

use crate::search::{norm, score, Act, Icon, Item};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

struct Entry {
    name: String,
    norm: String,
    lnk: PathBuf,
}

static CACHE: Mutex<Option<(Instant, Vec<Entry>)>> = Mutex::new(None);
const MAX: usize = 8;

fn load() -> Vec<Entry> {
    let Ok(appdata) = std::env::var("APPDATA") else { return Vec::new() };
    let dir = PathBuf::from(appdata).join("Microsoft\\Windows\\Recent");
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(SystemTime, Entry)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                return None;
            }
            let name = p.file_stem()?.to_string_lossy().to_string();
            // Uzantısız adlar çoğunlukla klasör ya da sürücü kısayoludur.
            if !name.contains('.') {
                return None;
            }
            let t = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((t, Entry { norm: norm(&name), name, lnk: p }))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.truncate(200);
    v.into_iter().map(|(_, e)| e).collect()
}

fn with<R>(f: impl FnOnce(&[Entry]) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map(|(t, _)| t.elapsed() > Duration::from_secs(20)).unwrap_or(true) {
        *g = Some((Instant::now(), load()));
    }
    f(&g.as_ref().map(|(_, v)| v.as_slice()).unwrap_or(&[]))
}

fn item(e: &Entry, n: usize) -> Item {
    let lnk = e.lnk.display().to_string();
    let sub = if n == 0 { t!("Son açılan dosya", "Last opened file").to_string() } else { t!("Son kullanılanlar", "Recent").to_string() };
    let mut it = Item::new(&e.name, sub, Icon::Shell(lnk.clone()), Act::Open(lnk.clone()), "Son");
    it.key = format!("recent:{}", e.norm);
    it
}

/// "son" / "son dosyalar" / "recent": en son açılan dosyalar; "son rapor": adı uyanlar.
pub(crate) fn listing(q: &str) -> Option<Vec<Item>> {
    let rest = ["son dosyalar", "recent", "son"].iter().find_map(|k| {
        let r = q.strip_prefix(k)?;
        (r.is_empty() || r.starts_with(' ')).then(|| r.trim())
    })?;
    Some(with(|v| {
        v.iter()
            .filter(|e| rest.is_empty() || score(&e.norm, rest) >= 400)
            .take(MAX)
            .enumerate()
            .map(|(n, e)| item(e, n))
            .collect()
    }))
}

/// Normal aramada adı uyan son dosyalar (puanıyla).
pub(crate) fn matches(q: &str) -> Vec<(i32, Item)> {
    if q.chars().count() < 3 {
        return Vec::new();
    }
    with(|v| {
        v.iter()
            .enumerate()
            .filter_map(|(n, e)| {
                let s = score(&e.norm, q);
                // Daha yeni olan biraz önde.
                (s >= 650).then(|| (s - 60 - (n as i32).min(40), item(e, n)))
            })
            .take(3)
            .collect()
    })
}
