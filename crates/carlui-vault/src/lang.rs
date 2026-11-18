//! Arayüz dili. Windows'ta CarlUI'nin ortak ayarı (`carlui_core::lang`);
//! başka sistemlerde süreç genelinde tek bir atomik değer. `CARLUI_LANG`
//! ("tr"/"en") her iki yerde de ayarın önüne geçer.

#[cfg(not(windows))]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(windows))]
static TR: AtomicBool = AtomicBool::new(true);

/// Sistem dili Türkçe mi. Linux'ta `LC_ALL`, `LC_MESSAGES` ve `LANG` sırayla
/// okunur (ilk dolu olan geçerlidir).
pub fn system_is_turkish() -> bool {
    #[cfg(windows)]
    {
        carlui_core::lang::system_is_turkish()
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("tr"))
    }
}

/// Ayar değerini uygular: "tr", "en" ya da başka her şey için "auto".
pub fn apply(setting: &str) {
    #[cfg(windows)]
    carlui_core::lang::apply(setting);
    #[cfg(not(windows))]
    {
        let env = std::env::var("CARLUI_LANG").unwrap_or_default();
        let setting = if env.is_empty() { setting } else { env.as_str() };
        let tr = match setting {
            "tr" => true,
            "en" => false,
            _ => system_is_turkish(),
        };
        TR.store(tr, Ordering::Relaxed);
    }
}

/// Arayüz şu an Türkçe mi.
#[inline]
pub fn is_tr() -> bool {
    #[cfg(windows)]
    {
        carlui_core::lang::is_tr()
    }
    #[cfg(not(windows))]
    {
        TR.load(Ordering::Relaxed)
    }
}
