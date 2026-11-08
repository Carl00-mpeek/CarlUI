//! Bu modül "döviz kuru nereden gelir?" sorununu çözüyor: Avrupa Merkez
//! Bankası verisini yayınlayan ücretsiz Frankfurter servisinden (eski
//! CarlSpotlight'ın kullandığı adres) günlük kur alınır. Her kur çifti bir
//! saat önbellekte tutulur; istek arka planda yapılır, arayüz beklemez.

use carlui_core::warn;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{core::HSTRING, Foundation::Uri, Web::Http::HttpClient};

#[derive(Clone, Debug)]
pub enum Rate {
    Loading,
    Ready { rate: f64, date: String },
    Failed,
}

#[derive(Clone, Default)]
pub struct Rates {
    cache: Arc<Mutex<HashMap<(String, String), (Rate, Instant)>>>,
}

pub(crate) fn fetch(from: &str, to: &str) -> Option<(f64, String)> {
    let url = format!("https://api.frankfurter.dev/v2/rate/{from}/{to}");
    let client = HttpClient::new().ok()?;
    let text = client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(url)).ok()?).ok()?.get().ok()?.to_string();
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let rate = v.get("rate")?.as_f64()?;
    let date = v.get("date").and_then(|d| d.as_str()).unwrap_or_default();
    // 2026-11-01 → 01.11.2026
    let date = match date.split('-').collect::<Vec<_>>().as_slice() {
        [y, m, d] => format!("{d}.{m}.{y}"),
        _ => date.to_string(),
    };
    Some((rate, date))
}

impl Rates {
    /// Kuru döndürür; yoksa arka planda ister ve hazır olunca `done`'u çağırır.
    pub fn get(&self, from: &str, to: &str, done: impl FnOnce() + Send + 'static) -> Rate {
        let key = (from.to_string(), to.to_string());
        {
            let c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((r, at)) = c.get(&key) {
                let fresh = match r {
                    Rate::Ready { .. } => at.elapsed() < Duration::from_secs(3600),
                    Rate::Loading => true,
                    Rate::Failed => at.elapsed() < Duration::from_secs(30),
                };
                if fresh {
                    return r.clone();
                }
            }
        }
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone(), (Rate::Loading, Instant::now()));
        let cache = self.cache.clone();
        std::thread::Builder::new()
            .name("currency".into())
            .spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                }
                let r = match fetch(&key.0, &key.1) {
                    Some((rate, date)) => Rate::Ready { rate, date },
                    None => {
                        warn!("kur alınamadı: {}/{}", key.0, key.1);
                        Rate::Failed
                    }
                };
                cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (r, Instant::now()));
                done();
            })
            .ok();
        Rate::Loading
    }
}
