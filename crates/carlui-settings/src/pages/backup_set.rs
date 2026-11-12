//! Yedeğin dosya tarafı: `config.toml` dışında yedeğe girenler (`looks.json`,
//! `layouts.json`, `pets\`) ve geri yüklenmesi. Vault, pano geçmişi, modeller,
//! ffmpeg ve hava önbelleği bilerek dışarıda bırakılır. Yalnızca dosya işi
//! yapar (arayüz ve ayar uygulaması `backup.rs`'te); klasörler parametredir.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

/// Yedek klasörünün ad öneki; içe aktarırken "bu bir yedek klasörü" ayrımı buradan yapılır.
pub const FOLDER_PREFIX: &str = "CarlUI-yedek-";

/// Yedeğe giren tek tek dosyalar (klasör içindeki göreli yol).
const LOOKS: &str = "looks.json";
const LAYOUTS: &str = "layouts.json";
const PETS: &str = "pets";

/// Göreli yol yalnızca düz adlardan mı oluşuyor? (`..`, kök, sürücü yok.)
pub fn safe_rel(rel: &Path) -> bool {
    rel.components().count() > 0 && rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Göreli yolun gideceği gerçek konum; yedeğin tanıdığı üç yerin dışına çıkmaz.
/// `looks.json` → ayar klasörü, `layouts.json` ve `pets\...` → veri klasörü.
fn target(rel: &Path, cfg_dir: &Path, data_dir: &Path) -> Option<PathBuf> {
    if !safe_rel(rel) {
        return None;
    }
    let first = rel.components().next()?.as_os_str().to_str()?;
    match first {
        LOOKS if rel.components().count() == 1 => Some(cfg_dir.join(rel)),
        LAYOUTS if rel.components().count() == 1 => Some(data_dir.join(rel)),
        PETS if rel.components().count() > 1 => Some(data_dir.join(rel)),
        _ => None,
    }
}

fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let r = rel.join(e.file_name());
        // Bağlantılar (symlink/junction) izlenmez.
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(&e.path(), &r, out);
        } else if ft.is_file() {
            out.push(r);
        }
    }
}

/// Veri klasöründeki `pets\` altındaki dosyaların göreli yolları (sıralı).
fn pets_in(data_dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    walk(&data_dir.join(PETS), Path::new(PETS), &mut v);
    v.sort();
    v
}

/// Kaynak klasörlerden yedeklenecek (kaynak yol, göreli hedef) çiftleri.
pub fn files_to_copy(cfg_dir: &Path, data_dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut v = Vec::new();
    if cfg_dir.join(LOOKS).is_file() {
        v.push((cfg_dir.join(LOOKS), PathBuf::from(LOOKS)));
    }
    if data_dir.join(LAYOUTS).is_file() {
        v.push((data_dir.join(LAYOUTS), PathBuf::from(LAYOUTS)));
    }
    for r in pets_in(data_dir) {
        v.push((data_dir.join(&r), r));
    }
    v
}

fn copy_to(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(src, dst).map(|_| ())
}

/// `CarlUI-yedek-<tarih>`; varsa `-2`, `-3`… eklenerek henüz olmayan ad bulunur.
fn free_folder(base: &Path, date: &str) -> PathBuf {
    let first = base.join(format!("{FOLDER_PREFIX}{date}"));
    let mut p = first.clone();
    let mut n = 2;
    while p.exists() {
        p = base.join(format!("{FOLDER_PREFIX}{date}-{n}"));
        n += 1;
    }
    p
}

/// `base\CarlUI-yedek-<tarih>` klasörünü kurar: `config.toml` metni + ek dosyalar.
/// Klasör yolunu döndürür.
pub fn export(base: &Path, date: &str, config_text: &str, cfg_dir: &Path, data_dir: &Path) -> io::Result<PathBuf> {
    let folder = free_folder(base, date);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("config.toml"), config_text)?;
    for (src, rel) in files_to_copy(cfg_dir, data_dir) {
        copy_to(&src, &folder.join(rel))?;
    }
    Ok(folder)
}

/// Seçilen `config.toml` bir yedek klasörünün içinde mi?
pub fn is_backup_config(path: &Path) -> bool {
    let named = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case("config.toml"));
    named && path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(FOLDER_PREFIX))
}

/// Yedek klasöründeki ek parçaları geri koyar; olmayanlar sessizce atlanır.
/// Geri yüklenen dosya sayısını ve yüklenemeyen parçaların adlarını döndürür;
/// bir dosyadaki hata diğerlerini durdurmaz.
pub fn restore(folder: &Path, cfg_dir: &Path, data_dir: &Path) -> (usize, Vec<String>) {
    let mut rels = Vec::new();
    for top in [LOOKS, LAYOUTS] {
        if folder.join(top).is_file() {
            rels.push(PathBuf::from(top));
        }
    }
    walk(&folder.join(PETS), Path::new(PETS), &mut rels);
    let mut n = 0;
    let mut failed: Vec<String> = Vec::new();
    for rel in rels {
        let Some(dst) = target(&rel, cfg_dir, data_dir) else { continue };
        match copy_to(&folder.join(&rel), &dst) {
            Ok(()) => n += 1,
            // Hayvan paketleri tek parça sayılır: "pets" bir kez yazılır.
            Err(_) => {
                let part = if rel.starts_with(PETS) { PETS.to_string() } else { rel.to_string_lossy().into_owned() };
                if !failed.contains(&part) {
                    failed.push(part);
                }
            }
        }
    }
    (n, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("carlui-backup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn rel_safety() {
        assert!(safe_rel(Path::new("pets/a/b.png")));
        assert!(!safe_rel(Path::new("../x")));
        assert!(!safe_rel(Path::new("pets/../../x")));
        assert!(!safe_rel(Path::new(r"C:\x")));
        assert!(!safe_rel(Path::new(r"\x")));
        assert!(!safe_rel(Path::new("")));
    }

    #[test]
    fn targets_stay_in_three_places() {
        let (c, d) = (Path::new("C"), Path::new("D"));
        assert_eq!(target(Path::new("looks.json"), c, d), Some(c.join("looks.json")));
        assert_eq!(target(Path::new("layouts.json"), c, d), Some(d.join("layouts.json")));
        assert_eq!(target(Path::new("pets/x/y.png"), c, d), Some(d.join("pets").join("x").join("y.png")));
        assert_eq!(target(Path::new("vault/x"), c, d), None);
        assert_eq!(target(Path::new("pets"), c, d), None);
        assert_eq!(target(Path::new("pets/../a"), c, d), None);
    }

    #[test]
    fn export_then_restore_round_trip_and_exclusions() {
        let root = tmp("rt");
        let (cfg, data) = (root.join("cfg"), root.join("data"));
        put(&cfg.join("looks.json"), "L");
        put(&data.join("layouts.json"), "Y");
        put(&data.join("pets").join("kedi").join("a.png"), "P");
        put(&data.join("vault").join("v.bin"), "gizli");
        put(&data.join("weather.json"), "w");
        let list: Vec<String> = files_to_copy(&cfg, &data).into_iter().map(|(_, r)| r.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(list, vec!["looks.json", "layouts.json", "pets/kedi/a.png"]);

        let out = root.join("out");
        let folder = export(&out, "2026-11-12", "x=1", &cfg, &data).unwrap();
        assert!(folder.ends_with("CarlUI-yedek-2026-11-12"));
        assert_eq!(std::fs::read_to_string(folder.join("config.toml")).unwrap(), "x=1");
        assert!(!folder.join("vault").exists() && !folder.join("weather.json").exists());
        assert!(is_backup_config(&folder.join("config.toml")));
        assert!(!is_backup_config(&root.join("config.toml")));

        let (cfg2, data2) = (root.join("cfg2"), root.join("data2"));
        assert_eq!(restore(&folder, &cfg2, &data2), (3, vec![]));
        // Aynı gün ikinci yedek -2 adıyla açılır.
        let second = export(&out, "2026-11-12", "x=2", &cfg, &data).unwrap();
        assert!(second.ends_with("CarlUI-yedek-2026-11-12-2"));
        assert!(export(&out, "2026-11-12", "x=3", &cfg, &data).unwrap().ends_with("CarlUI-yedek-2026-11-12-3"));
        assert_eq!(std::fs::read_to_string(cfg2.join("looks.json")).unwrap(), "L");
        assert_eq!(std::fs::read_to_string(data2.join("layouts.json")).unwrap(), "Y");
        assert_eq!(std::fs::read_to_string(data2.join("pets").join("kedi").join("a.png")).unwrap(), "P");
        // Eksik parçalar sessizce atlanır.
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(restore(&empty, &cfg2, &data2), (0, vec![]));
        // Hedef klasör yerine dosya varsa parça başarısız sayılır, diğerleri yüklenir.
        let (cfg3, data3) = (root.join("cfg3"), root.join("data3"));
        put(&data3.join("pets"), "dosya");
        let (n, failed) = restore(&folder, &cfg3, &data3);
        assert_eq!((n, failed), (2, vec!["pets".to_string()]));
        let _ = std::fs::remove_dir_all(&root);
    }
}
