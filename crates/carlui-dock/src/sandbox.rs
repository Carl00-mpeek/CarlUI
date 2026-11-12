//! Geliştirici kum havuzu (`CARLUI_DOCK_SANDBOX=1`): çalışan CarlUI'ye
//! dokunmadan ikinci bir dock açar. Yalnızca ortam değişkeni varken etkili;
//! yayın sürümünde hiçbir davranışı değiştirmez.
//!
//! Kum havuzundaki dock:
//! - ayrı sınıf adları ve ayrı tek kopya kilidi kullanır,
//! - olay yoluna bağlanmaz, ek ekran kopyası başlatmaz, çark kurmaz,
//! - klavye/fare kancası kurmaz (gerçek dock'un kısayolları etkilenmez),
//! - ana olmayan ilk monitöre oturur (yoksa hiç açılmaz),
//! - görev çubuğu içe aktarması yapmaz; ayar dosyasına hiç yazmaz.
//!
//! Canlı dock ayarları ortamdan alınır:
//! `CARLUI_SANDBOX_PET` (ad ya da `none`), `CARLUI_SANDBOX_SCENE`
//! (`none|rain|snow|night|clouds|clock|aquarium`), `CARLUI_SANDBOX_HIDE=1`
//! (otomatik gizleme + düzenli geri getirme: kayma ve duraklatma denemesi),
//! `CARLUI_SANDBOX_SNAKE=wrap|track` (yalnız `CARLUI_LIVE_DEBUG` ile: yılanı
//! sarılmaya zorlar ya da sahte imleçle başını kaldırtır).

use carlui_core::{
    config::{Config, DockBehavior},
    live::LiveScene,
};
use std::sync::OnceLock;

/// Kum havuzunda dock'u düzenli geri getiren zamanlayıcı.
pub const TIMER_CYCLE: usize = 0x5AB0;
/// Gizle/göster döngüsünün periyodu (ms).
pub const CYCLE_MS: u32 = 6000;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty())
}

/// Kum havuzu açık mı (süreç boyunca sabit).
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env("CARLUI_DOCK_SANDBOX").is_some_and(|v| v == "1"))
}

/// Gizle/göster döngüsü istendi mi.
pub fn cycle() -> bool {
    on() && env("CARLUI_SANDBOX_HIDE").is_some_and(|v| v == "1")
}

/// Yılan durum zorlaması (`CARLUI_SANDBOX_SNAKE=wrap|track`): yalnızca kum
/// havuzu VE `CARLUI_LIVE_DEBUG` birlikte açıkken okunur.
pub fn snake_force() -> Option<String> {
    (on() && std::env::var_os("CARLUI_LIVE_DEBUG").is_some()).then(|| env("CARLUI_SANDBOX_SNAKE")).flatten()
}

fn scene(v: &str) -> Option<LiveScene> {
    Some(match v {
        "none" => LiveScene::None,
        "rain" => LiveScene::Rain,
        "snow" => LiveScene::Snow,
        "night" => LiveScene::Night,
        "clouds" => LiveScene::Clouds,
        "clock" => LiveScene::Clock,
        "aquarium" => LiveScene::Aquarium,
        _ => return None,
    })
}

/// Yüklenen (ya da izlenen) ayarı kum havuzuna uyarlar.
pub fn tweak(cfg: &mut Config) {
    if !on() {
        return;
    }
    let d = &mut cfg.dock;
    d.behavior = if cycle() { DockBehavior::AutoHide } else { DockBehavior::AlwaysVisible };
    d.other_screens = false;
    d.radial = false;
    if let Some(p) = env("CARLUI_SANDBOX_PET") {
        d.live.pet = if p == "none" { String::new() } else { p };
    }
    if let Some(s) = env("CARLUI_SANDBOX_SCENE").as_deref().and_then(scene) {
        d.live.scene = s;
    }
    // Pil/tasarruf ölçümü bozmasın.
    d.live.pause_on_battery = false;
    // Hava denemesi (`CARLUI_LIVE_WEATHER`): gece/gündüz döngüsü de açık görünsün.
    if std::env::var_os("CARLUI_LIVE_WEATHER").is_some() {
        d.live.weather.day_night = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sahne_adlari() {
        assert_eq!(scene("aquarium"), Some(LiveScene::Aquarium));
        assert_eq!(scene("none"), Some(LiveScene::None));
        assert_eq!(scene("x"), None);
    }

    #[test]
    fn kapaliyken_ayar_degismez() {
        // Testler ortam değişkeni koymaz: kum havuzu kapalı, ayar aynı kalır.
        let mut c = Config::default();
        let before = c.dock.behavior;
        tweak(&mut c);
        assert_eq!(c.dock.behavior, before);
    }
}
