//! Paketi diske yazma: güvenli klasör adı, `pet.json` ve PNG şeritleri.
//! Önce gizli bir geçici klasöre yazılır, sınırlar denetlenir, sonra tek
//! adımda yerine taşınır (yarım paket kalmaz). Var olan paket ancak
//! `overwrite` istenirse değiştirilir.

use super::sheet::Pet;
use std::path::{Path, PathBuf};

/// Dock'un sınırları (bkz. dock `live/pack.rs`).
pub const MAX_PNG: u64 = 512 * 1024;
pub const MAX_PACK: u64 = 2 * 1024 * 1024;
pub const MAX_MANIFEST: usize = 16 * 1024;

/// Windows'un ayrılmış aygıt adları: klasör adı olamaz (tek kaynak çekirdekte).
fn reserved(stem: &str) -> bool {
    carlui_core::live::is_reserved_name(stem)
}

/// Görünen addan güvenli klasör adı: Türkçe harfler sadeleşir, boşluklar
/// `_`, geri kalan her şey atılır; 1-32 karakter `[A-Za-z0-9_-]`. Yerleşik
/// bir hayvan adına ya da ayrılmış bir Windows adına denk gelirse sonuna
/// `_pet` eklenir.
pub fn folder_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        let c = match ch {
            'ç' => 'c',
            'Ç' => 'C',
            'ğ' => 'g',
            'Ğ' => 'G',
            'ı' => 'i',
            'İ' => 'I',
            'ö' => 'o',
            'Ö' => 'O',
            'ş' => 's',
            'Ş' => 'S',
            'ü' => 'u',
            'Ü' => 'U',
            'â' | 'à' | 'á' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'í' | 'ï' => 'i',
            'û' | 'ú' => 'u',
            'ô' | 'ó' => 'o',
            c if c.is_whitespace() => '_',
            c => c,
        };
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            // Art arda alt çizgiler tek olsun.
            if !(c == '_' && out.ends_with('_')) {
                out.push(c);
            }
        }
    }
    let mut out: String = out.trim_matches(|c| c == '_' || c == '-').chars().take(32).collect();
    if out.is_empty() {
        out = "pet".into();
    }
    if carlui_core::live::is_builtin(&out.to_ascii_lowercase()) || reserved(&out) {
        out.truncate(28);
        out.push_str("_pet");
    }
    out
}

/// `pet.json`'daki görünen ad: kontrol karakterleri atılır, 1-64 karakter.
pub fn display_name(name: &str, folder: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(64).collect();
    if s.is_empty() {
        folder.to_string()
    } else {
        s
    }
}

/// `pet.json` metni. `alert` (imleç yaklaşınca) zıplama şeridini paylaşır.
pub fn manifest_json(pet: &Pet) -> String {
    let mut states = serde_json::Map::new();
    for s in &pet.strips {
        states.insert(s.key.into(), serde_json::json!({ "file": s.file, "frames": s.frames, "ms": s.ms }));
    }
    if let Some(h) = pet.strip("hop") {
        states.insert("alert".into(), serde_json::json!({ "file": h.file, "frames": h.frames, "ms": h.ms }));
    }
    let v = serde_json::json!({
        "name": pet.name,
        "frame_width": pet.fw,
        "frame_height": pet.fh,
        "scale": 1,
        "filter": "nearest",
        "facing": "right",
        "baseline": pet.fh,
        "walk_speed": pet.walk_speed,
        "states": states,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Bağlantı değil, gerçek klasör mü?
fn real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_dir()).unwrap_or(false)
}

/// Paketi `root\<folder>` altına yazar ve klasörü döndürür.
pub fn write(pet: &Pet, root: &Path, folder: &str, overwrite: bool) -> Result<PathBuf, String> {
    if !carlui_core::live::valid_pack_name(folder) || reserved(folder) {
        return Err(format!("{}: {folder}", t!("Geçersiz paket adı", "Invalid pack name")));
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = root.join(folder);
    let exists = std::fs::symlink_metadata(&target).is_ok();
    if exists && !overwrite {
        return Err(t!(format!("\"{folder}\" adında bir hayvan zaten var"), format!("A pet named \"{folder}\" already exists")));
    }
    if exists && !real_dir(&target) {
        return Err(t!(format!("\"{folder}\" bir klasör değil"), format!("\"{folder}\" is not a folder")));
    }
    let pid = std::process::id();
    let tmp = root.join(format!(".{folder}.new{pid}"));
    if real_dir(&tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    std::fs::create_dir(&tmp).map_err(|e| e.to_string())?;
    let r = fill(pet, &tmp).and_then(|_| {
        if exists {
            let old = root.join(format!(".{folder}.old{pid}"));
            std::fs::rename(&target, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::rename(&tmp, &target) {
                // Eskisini geri koy.
                let _ = std::fs::rename(&old, &target);
                return Err(e.to_string());
            }
            let _ = std::fs::remove_dir_all(&old);
            Ok(())
        } else {
            std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
        }
    });
    if r.is_err() && real_dir(&tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    r.map(|_| target)
}

/// Şeritleri ve `pet.json`'u klasöre yazar, boyut sınırlarını denetler.
fn fill(pet: &Pet, dir: &Path) -> Result<(), String> {
    let mut total = 0u64;
    for s in &pet.strips {
        let p = dir.join(s.file);
        super::wic::save_png(&p, s.frames * pet.fw, pet.fh, &s.rgba)?;
        let len = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
        if len > MAX_PNG {
            return Err(format!("{} çok büyük ({len} bayt)", s.file));
        }
        total += len;
    }
    let json = manifest_json(pet);
    if json.len() > MAX_MANIFEST || total + json.len() as u64 > MAX_PACK {
        return Err(t!("Paket çok büyük", "The pack is too large").into());
    }
    std::fs::write(dir.join("pet.json"), json).map_err(|e| e.to_string())
}
