//! Bu modül "arayüz hem Türkçe hem İngilizce olsun" sorununu çözüyor. Metinler
//! kodun içinde iki dilde yan yana yazılır: `t!("Ayarlar", "Settings")`.
//! Seçili dil süreç genelinde tek bir atomik değerdir; `config::load` her
//! okumada `general.language` ayarını buraya uygular, böylece ayar değişince
//! tüm modüller bir sonraki çizimde yeni dile geçer.
//!
//! `general.language`: "auto" (Windows'un görüntü dili Türkçeyse Türkçe,
//! değilse İngilizce), "tr" ya da "en".

use std::sync::atomic::{AtomicBool, Ordering};

static TR: AtomicBool = AtomicBool::new(true);

#[link(name = "kernel32")]
extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

/// Windows'un görüntü dili Türkçe mi (LANG_TURKISH = 0x1F).
pub fn system_is_turkish() -> bool {
    unsafe { GetUserDefaultUILanguage() & 0x3FF == 0x1F }
}

/// Ayar değerini uygular: "tr", "en" ya da başka her şey için "auto".
/// `CARLUI_LANG` ortam değişkeni (deneme için) ayarın önüne geçer.
pub fn apply(setting: &str) {
    let env = std::env::var("CARLUI_LANG").unwrap_or_default();
    let setting = if env.is_empty() { setting } else { env.as_str() };
    let tr = match setting {
        "tr" => true,
        "en" => false,
        _ => system_is_turkish(),
    };
    TR.store(tr, Ordering::Relaxed);
}

/// Arayüz şu an Türkçe mi.
#[inline]
pub fn is_tr() -> bool {
    TR.load(Ordering::Relaxed)
}

/// Yuvarlanmış yüzde: Türkçede "%42", İngilizcede "42%".
pub fn pct(v: impl Into<f64>) -> String {
    let v = v.into();
    if is_tr() { format!("%{v:.0}") } else { format!("{v:.0}%") }
}

/// İki dildeki metinden seçili olanı verir: `t!("Kapat", "Close")`.
/// İki kol aynı türde olmalıdır (`&str`, `String`, `format!(…)`…).
#[macro_export]
macro_rules! t {
    ($tr:expr, $en:expr $(,)?) => {
        if $crate::lang::is_tr() { $tr } else { $en }
    };
}
