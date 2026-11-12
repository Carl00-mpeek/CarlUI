//! Turun yeni seçimlerinin testleri: vurgu rengi, hayvan, sahne ve özet
//! kartları ayarda yalnızca kendi alanlarını değiştirir; "Atla" hiçbirine
//! dokunmaz.

use super::{choices::*, model::*};
use carlui_core::{
    config::{AccentSource, Config},
    live::{self, LiveScene},
    theme::Palette,
};

#[test]
fn vurgu_yalnizca_paleti_yazar() {
    let base = Config::default();
    let mut c = base.clone();
    c.theme.accent_source = AccentSource::Wallpaper;
    assert_eq!(accent_selected(&c), None);
    set_accent(&mut c, Palette::Emerald);
    assert_eq!(accent_selected(&c), Some(Palette::Emerald));
    let mut back = c.clone();
    back.theme.palette = base.theme.palette;
    back.theme.accent_source = base.theme.accent_source;
    assert_eq!(back, base);
}

#[test]
fn hayvan_secimi_ve_renk_korunur() {
    let mut c = Config::default();
    assert!(!c.dock.live.has_pet());
    choose_pet(&mut c, "cat");
    assert_eq!(c.dock.live.pet, "cat");
    choose_variant(&mut c, "cat_black");
    assert_eq!(c.dock.live.pet, "cat_black");
    // Aynı ana karta yeniden basmak rengi bozmaz.
    choose_pet(&mut c, "cat");
    assert_eq!(c.dock.live.pet, "cat_black");
    assert_eq!(pet_main(&c.dock.live.pet), "cat");
    choose_pet(&mut c, "mouse");
    assert_eq!(c.dock.live.pet, "mouse");
    assert_eq!(pet_main("mouse"), "mouse");
    assert!(variants("mouse").is_none());
    // Bilinmeyen varyant yazılmaz.
    choose_variant(&mut c, "../kotu");
    assert_eq!(c.dock.live.pet, "mouse");
    choose_pet(&mut c, "");
    assert!(!c.dock.live.has_pet());
}

/// Kartlardaki her hayvan ve her renk varyantı gerçek bir yerleşik hayvandır.
#[test]
fn kartlar_yerlesik_hayvanlar() {
    for id in PETS {
        assert!(live::is_builtin(id), "{id}");
        assert_ne!(pet_label(id), pet_label("bilinmeyen-paket"), "{id}");
        assert_eq!(pet_main(id), id, "{id}");
        if let Some(v) = variants(id) {
            assert!(v.iter().all(|(vid, _)| live::is_builtin(vid) && pet_main(vid) == id), "{id}");
        }
    }
}

#[test]
fn hayvan_ve_sahne_baska_alana_dokunmaz() {
    let base = Config::default();
    let mut c = base.clone();
    choose_pet(&mut c, "dolphin");
    set_scene(&mut c, LiveScene::Aquarium);
    assert_eq!(c.dock.live.scene, LiveScene::Aquarium);
    let mut back = c.clone();
    back.dock.live.pet = base.dock.live.pet.clone();
    back.dock.live.scene = base.dock.live.scene;
    assert_eq!(back, base);
    assert_eq!(scenes().len(), 7);
    assert_eq!(scenes()[0].0, LiveScene::None);
}

/// Atla: tur boyunca hiçbir seçim yapılmadıysa ayar varsayılan kalır,
/// yalnızca karşılama işareti yazılır.
#[test]
fn atla_varsayilanlari_korur() {
    let mut c = Config::fresh();
    for s in Step::ALL {
        assert_eq!(navigate(s, Nav::Skip), Outcome::Close);
    }
    mark(&mut c);
    assert_eq!(c, Config::default());
}

/// Özet kartları: dört kart; tuşlar ayardan, kapalı arama tuş göstermez.
#[test]
fn ozet_kartlari_ayardan_gelir() {
    let mut c = Config::default();
    c.shortcuts.search = "Alt+Space".into();
    let h = highlights(&c);
    assert_eq!(h.len(), 4);
    assert_eq!(h[2].keys.first().map(|k| k.1.as_str()), Some("Alt+Space"));
    assert!(h[3].keys.len() <= 2);
    assert!(h[3].keys.iter().all(|(_, k)| k != "Alt+Space" && !k.trim().is_empty()));
    c.launcher.enabled = false;
    assert!(highlights(&c)[2].keys.is_empty());
    c.dock.taskbar = true;
    assert_ne!(highlights(&c)[0].title, "Dock");
}
