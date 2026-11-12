//! Karşılama işareti: yalnızca gerçekten ilk kurulumda (dosya yokken yazılan
//! `Config::fresh`) karşılama beklenir; eski ayar dosyaları "karşılandı" sayılır.

use super::Config;

#[test]
fn varsayilan_karsilanmis_sayilir() {
    assert!(Config::default().general.welcomed);
    assert!(!Config::default().needs_welcome());
}

/// Yükselten kullanıcı: dosyada alan yok → karşılama gösterilmez.
#[test]
fn eski_ayar_dosyasi_karsilama_istemez() {
    let empty: Config = toml::from_str("").unwrap();
    assert!(!empty.needs_welcome());
    let old: Config = toml::from_str("[general]\nlanguage = \"tr\"\nhide_taskbar = false\n").unwrap();
    assert!(!old.needs_welcome());
    assert_eq!(old.general.language, "tr");
}

/// İlk kurulum: `fresh` karşılama bekler, kaydedilip okununca da bekler.
#[test]
fn ilk_kurulum_karsilama_bekler_ve_korunur() {
    let fresh = Config::fresh();
    assert!(fresh.needs_welcome());
    let text = toml::to_string_pretty(&fresh).unwrap();
    let back: Config = toml::from_str(&text).unwrap();
    assert!(back.needs_welcome());
    // İşaret dışında her şey varsayılanla aynı.
    let mut d = back.clone();
    d.general.welcomed = true;
    assert_eq!(d, Config::default());
}

/// İşaret yazılınca bir daha istenmez (geri dönüş de korunur).
#[test]
fn isaret_yazilinca_kalici() {
    let mut c = Config::fresh();
    c.general.welcomed = true;
    let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
    assert!(!back.needs_welcome());
}
