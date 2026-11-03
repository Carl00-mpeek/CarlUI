//! Bu modül "Epic indiriyor ama hangi oyunu?" sorununu çözüyor: Epic Games
//! Launcher kurmakta olduğu oyunun kaydını
//! `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\Pending\<id>.item`
//! dosyasına JSON olarak yazar (`DisplayName`, `bIsIncompleteInstall`); kurulum
//! bitince kayıt bir üst klasöre, tamamlanmış olarak geçer. Dosyalar yalnızca
//! okunur. Kayıtta indirilecek boyut olmadığı için yüzde bilinmez; inen miktar
//! ve hız ağ alımından gelir.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

fn manifests() -> PathBuf {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(pd).join(r"Epic\EpicGamesLauncher\Data\Manifests")
}

/// Klasördeki `.item` kayıtları (değişme zamanı, ad, yarım mı).
fn items(dir: &std::path::Path) -> Vec<(SystemTime, String, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .filter_map(|e| {
            let m = e.metadata().and_then(|m| m.modified()).ok()?;
            let (name, incomplete) = parse(&std::fs::read_to_string(e.path()).ok()?)?;
            Some((m, name, incomplete))
        })
        .collect()
}

/// Kayıttan (oyun adı, kurulum yarım mı).
fn parse(text: &str) -> Option<(String, bool)> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let name = v.get("DisplayName")?.as_str()?.trim().to_string();
    let incomplete = v.get("bIsIncompleteInstall").and_then(|b| b.as_bool()).unwrap_or(false);
    (!name.is_empty()).then_some((name, incomplete))
}

/// Şu an inen oyunun adı: son bir günde değişmiş, yarım kalmış en yeni kayıt.
pub fn downloading() -> Option<String> {
    let dir = manifests();
    let mut all = items(&dir.join("Pending"));
    all.extend(items(&dir).into_iter().filter(|i| i.2));
    all.into_iter()
        .filter(|(m, ..)| m.elapsed().unwrap_or_default() < Duration::from_secs(24 * 3600))
        .max_by_key(|(m, ..)| *m)
        .map(|(_, name, _)| name)
}

/// Epic bu oyunun kurulumunu bitirdi mi: tamamlanmış kaydı var, bekleyeni yok.
pub fn complete(game: &str) -> bool {
    let dir = manifests();
    let pending = items(&dir.join("Pending")).iter().any(|i| i.1 == game);
    !pending && items(&dir).iter().any(|i| i.1 == game && !i.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item() {
        let j = r#"{"FormatVersion":0,"bIsIncompleteInstall":true,"DisplayName":"Fortnite","InstallLocation":"C:\\Program Files\\Epic Games\\Fortnite","InstallSize":34359738368}"#;
        assert_eq!(parse(j), Some(("Fortnite".into(), true)));
        assert_eq!(parse(&j.replace("true", "false")), Some(("Fortnite".into(), false)));
        assert_eq!(parse("{}"), None);
    }
}
