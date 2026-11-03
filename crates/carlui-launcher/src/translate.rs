//! Bu modül "tarayıcı açmadan kısa bir metni çevir" sorununu şu yöntemle
//! çözüyor: "tr: hello", "en: merhaba", "de: teşekkürler" ya da "çevir …"
//! yazınca metin Google Çeviri'nin herkese açık uç noktasına gönderilir
//! (yalnızca bu önekle yazılan metin, başka hiçbir şey). Her tuşta istek
//! atılmaz: yazma durduktan ~350 ms sonra yalnızca son metin çevrilir, sonuç
//! önbelleğe alınır ve hazır olunca arama yenilenir. Enter çeviriyi kopyalar.

use crate::search::{enc, norm, Act, Icon, Item};
use carlui_core::warn;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use windows::{core::HSTRING, Foundation::Uri, Web::Http::HttpClient};

const LANGS: &[(&str, &str, &str)] = &[
    ("tr", "Türkçe", "Turkish"),
    ("en", "İngilizce", "English"),
    ("de", "Almanca", "German"),
    ("fr", "Fransızca", "French"),
    ("es", "İspanyolca", "Spanish"),
    ("it", "İtalyanca", "Italian"),
    ("ru", "Rusça", "Russian"),
    ("ar", "Arapça", "Arabic"),
    ("ja", "Japonca", "Japanese"),
    ("ko", "Korece", "Korean"),
    ("zh", "Çince", "Chinese"),
    ("nl", "Felemenkçe", "Dutch"),
    ("pt", "Portekizce", "Portuguese"),
];

fn lang_name(code: &str) -> &str {
    let base = code.split('-').next().unwrap_or(code);
    LANGS.iter().find(|(c, ..)| *c == base).map(|(_, tr, en)| t!(*tr, *en)).unwrap_or(code)
}

#[derive(Clone)]
enum State {
    Loading,
    Ready { text: String, from: String },
    Failed,
}

type Key = (String, String);

#[derive(Default)]
struct Inner {
    cache: HashMap<Key, State>,
    wanted: Option<Key>,
    worker: bool,
}

static INNER: Mutex<Option<Inner>> = Mutex::new(None);

fn fetch(to: &str, text: &str) -> Option<(String, String)> {
    let url = format!("https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={to}&dt=t&q={}", enc(text));
    let client = HttpClient::new().ok()?;
    let body = client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(url)).ok()?).ok()?.get().ok()?.to_string();
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let out: String = v.get(0)?.as_array()?.iter().filter_map(|s| s.get(0).and_then(|t| t.as_str())).collect();
    let from = v.get(2).and_then(|l| l.as_str()).unwrap_or("").to_string();
    (!out.is_empty()).then_some((out, from))
}

/// Çeviriyi döndürür; yoksa kısa bir beklemeden sonra arka planda ister ve
/// hazır olunca `wake`'i çağırır.
fn get(to: &str, text: &str, wake: Arc<dyn Fn() + Send + Sync>) -> State {
    let key = (to.to_string(), text.to_string());
    let mut g = INNER.lock().unwrap_or_else(|e| e.into_inner());
    let inner = g.get_or_insert_with(Inner::default);
    if let Some(s) = inner.cache.get(&key) {
        return s.clone();
    }
    inner.wanted = Some(key);
    if !inner.worker {
        inner.worker = true;
        std::thread::Builder::new()
            .name("translate".into())
            .spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                }
                loop {
                    std::thread::sleep(Duration::from_millis(350));
                    let key = {
                        let mut g = INNER.lock().unwrap_or_else(|e| e.into_inner());
                        let inner = g.get_or_insert_with(Inner::default);
                        match inner.wanted.take() {
                            Some(k) if !inner.cache.contains_key(&k) => k,
                            Some(_) => continue,
                            None => {
                                inner.worker = false;
                                return;
                            }
                        }
                    };
                    let state = match fetch(&key.0, &key.1) {
                        Some((text, from)) => State::Ready { text, from },
                        None => {
                            warn!("çeviri alınamadı ({})", key.0);
                            State::Failed
                        }
                    };
                    {
                        let mut g = INNER.lock().unwrap_or_else(|e| e.into_inner());
                        let inner = g.get_or_insert_with(Inner::default);
                        if inner.cache.len() > 200 {
                            inner.cache.clear();
                        }
                        inner.cache.insert(key, state);
                    }
                    wake();
                }
            })
            .ok();
    }
    State::Loading
}

/// "tr: metin" / "çevir metin" → (hedef dil, metin).
fn parse(raw: &str) -> Option<(String, String)> {
    let t = raw.trim_start();
    if let Some((code, rest)) = t.split_once(':') {
        let code = code.trim().to_lowercase();
        if code.len() == 2 && LANGS.iter().any(|(c, ..)| *c == code) && !rest.trim().is_empty() {
            return Some((code, rest.trim().to_string()));
        }
    }
    let (word, rest) = t.split_once(' ')?;
    if matches!(norm(word).as_str(), "cevir" | "translate") && !rest.trim().is_empty() {
        // Türkçe görünüyorsa İngilizceye, değilse Türkçeye.
        let turkish = rest.chars().any(|c| "çğışöüÇĞİŞÖÜ".contains(c));
        return Some((if turkish { "en" } else { "tr" }.into(), rest.trim().to_string()));
    }
    None
}

pub(crate) fn items(raw: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Item> {
    let (to, text) = parse(raw)?;
    let g = Icon::Glyph('\u{F2B7}');
    let it = match get(&to, &text, wake) {
        State::Ready { text: out, from } => {
            let sub = format!("{} → {} · {}", lang_name(&from), lang_name(&to), t!("Enter ile kopyala", "Enter to copy"));
            Item::new(out.clone(), sub, g, Act::Copy(out), "Çeviri")
        }
        State::Loading => Item::new(format!("{text} → {}", lang_name(&to)), t!("Çevriliyor…", "Translating…"), g, Act::Pending, "Çeviri"),
        State::Failed => Item::new(format!("{text} → {}", lang_name(&to)), t!("Çeviri alınamadı, bağlantıyı kontrol et", "Translation failed, check your connection"), g, Act::Pending, "Çeviri"),
    };
    Some(it)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse("tr: hello world"), Some(("tr".into(), "hello world".into())));
        assert_eq!(parse("EN:merhaba"), Some(("en".into(), "merhaba".into())));
        assert_eq!(parse("çevir teşekkürler"), Some(("en".into(), "teşekkürler".into())));
        assert_eq!(parse("cevir good morning"), Some(("tr".into(), "good morning".into())));
        assert_eq!(parse("c:\\windows"), None);
        assert_eq!(parse("xx: deneme"), None);
        assert_eq!(parse("tr:"), None);
    }
}
