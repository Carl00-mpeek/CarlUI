//! Bu modül "ekrandaki yabancı dildeki yazıyı hızlıca nasıl anlarız?"
//! sorununu çözüyor (ekran çevirisi). Ekran görüntüsü düzenleyicisinde
//! Ctrl+Shift+T ile seçilen alan OCR'dan geçer, metin geçici bir dosyaya
//! yazılır ve bu program `--translate <dosya>` ile başlar. Dil kendiliğinden
//! bulunur (Türkçe harfler/sözcükler → tr→en, yoksa en→tr), yerel Opus-MT
//! modeliyle çevrilir; sonuç panoya konur, kart tıklanınca özgün metin ile
//! çeviri yan yana bir metin dosyasında açılır. Hiçbir şey buluta gitmez.

use crate::{ai, progress::Shared};
use carlui_ai::{catalog, marian::Marian, store};
use std::sync::Arc;
use windows::{
    core::*,
    Win32::{Foundation::*, System::Com::*},
};

/// Metin Türkçe mi? Türkçeye özgü harfler ve sık sözcükler sayılır.
pub fn is_turkish(text: &str) -> bool {
    let letters = text.chars().filter(|c| "ğüşıöçĞÜŞİÖÇ".contains(*c)).count();
    let words: Vec<String> = text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return false;
    }
    const TR: &[&str] = &["ve", "bir", "bu", "için", "ile", "de", "da", "çok", "ne", "değil", "olarak", "gibi", "daha", "ama", "ya", "mi", "mı"];
    const EN: &[&str] = &["the", "and", "of", "to", "is", "in", "for", "that", "with", "you", "this", "are", "it", "on", "be"];
    let tr = words.iter().filter(|w| TR.contains(&w.as_str())).count() + letters;
    let en = words.iter().filter(|w| EN.contains(&w.as_str())).count();
    tr > en
}

/// Çeviriyi başlatır. `file`: OCR metni (UTF-8).
pub fn run(owner: HWND, accent: Option<u32>, file: &str) -> Result<()> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    // Geçici dosya yalnızca taşıma içindi.
    if file.contains("carlui-ocr-") {
        let _ = std::fs::remove_file(file);
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let tr = is_turkish(&text);
    let m = catalog::get(if tr { "opus-tr-en" } else { "opus-en-tr" }).expect("katalogda çeviri modeli yok");
    let need = ai::missing_bytes(m);
    if need > 0 && !crate::ask_model(owner, accent, m, need) {
        return Ok(());
    }
    let shared = Shared::new();
    shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
    let worker = shared.clone();
    crate::progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            work(&worker, m, &text, tr);
        });
    })
}

fn work(w: &Arc<Shared>, m: &'static catalog::Model, text: &str, tr: bool) {
    if !ai::ensure(w, m) {
        return;
    }
    w.update(|s| {
        s.title = t!("Model yükleniyor", "Loading the model").into();
        s.file = m.name.into();
        s.frac = 0.0;
    });
    let mut mt = match Marian::open(&store::dir(m)) {
        Ok(x) => x,
        Err(e) => {
            carlui_core::warn!("çeviri modeli açılamadı ({}): {e}", m.id);
            return ai::fail(w, t!("Model açılamadı", "Couldn't load the model").into());
        }
    };
    w.update(|s| {
        s.title = if tr { t!("Türkçe → İngilizce çevriliyor", "Translating Turkish → English") } else { t!("İngilizce → Türkçe çevriliyor", "Translating English → Turkish") }.into();
        s.file = preview(text);
    });
    let t0 = std::time::Instant::now();
    let out = match mt.translate(text, &w.cancel, &|f| w.update(|s| s.frac = f)) {
        Ok(o) => o,
        Err(e) => return ai::fail(w, e),
    };
    carlui_core::info!("çeviri: {} → {} karakter, {} ms", text.chars().count(), out.chars().count(), t0.elapsed().as_millis());
    let copied = carlui_clip::write::set(HWND::default(), carlui_clip::write::Data::Text(&out)).is_some();
    // Özgün ve çeviri yan yana; kart tıklanınca açılır.
    let path = std::env::temp_dir().join(format!("CarlUI {}.txt", t!("çeviri", "translation")));
    let body = format!("{out}\n\n――――――――――――――――――――\n\n{text}\n");
    let saved = std::fs::write(&path, body.as_bytes()).is_ok();
    w.update(|s| {
        s.title = if copied { t!("Çeviri panoya kopyalandı", "Translation copied") } else { t!("Çeviri hazır", "Translation ready") }.into();
        s.file = preview(&out);
        s.frac = 1.0;
        s.done = true;
        s.open = true;
        s.reveal = saved.then(|| path.to_string_lossy().into_owned());
    });
}

fn preview(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 60 {
        format!("{}…", one.chars().take(60).collect::<String>())
    } else {
        one
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language() {
        assert!(is_turkish("Bugün hava çok güzel"));
        assert!(is_turkish("Dosya bulunamadı"));
        assert!(!is_turkish("The file could not be found."));
        assert!(!is_turkish("Settings and privacy"));
        assert!(!is_turkish(""));
    }
}
