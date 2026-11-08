//! Bu modül "modeller nereye iner, nasıl doğrulanır, nasıl silinir?"
//! sorusunu çözüyor. Her model `%LOCALAPPDATA%\CarlUI\models\<id>` altında
//! durur; indirme `net` modülüyle yapılır (yalnızca HTTPS ve izinli sunucular).
//! İlerleme `.part` dosyasının boyutundan okunur. Boyut ve SHA-256 tutmadan
//! dosya yerine konmaz, zip'ler güvenlik denetiminden sonra açılır ve klasöre `.ok`
//! yazılır; `.ok` yoksa model kurulu sayılmaz.

use crate::catalog::Model;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub fn root() -> PathBuf {
    carlui_core::paths::data_dir().join("models")
}

/// Bazı girdiler başka bir modelin dosyalarını kullanır (komut için Whisper).
fn real_id(id: &str) -> &str {
    match id {
        "cmd-whisper-base" => "whisper-base",
        "cmd-whisper-small" => "whisper-small",
        "cmd-whisper-turbo" => "whisper-turbo",
        x => x,
    }
}

pub fn dir(m: &Model) -> PathBuf {
    root().join(real_id(m.id))
}

/// `.ok` yazılmış ve katalogdaki her dosya yerinde: katalog bir dosyayı
/// değiştirdiyse (ör. yeni motor paketi) model yeniden indirilecek sayılır.
pub fn installed(m: &Model) -> bool {
    let d = dir(m);
    let files = crate::catalog::get(real_id(m.id)).map_or(m.files, |b| b.files);
    d.join(".ok").is_file() && files.iter().all(|f| present(&d, f))
}

/// Açılan arşiv silinir; yerine SHA'sını taşıyan damga kalır. Katalog başka
/// bir paket gösterirse damga tutmaz ve paket yeniden indirilir.
fn stamp(d: &Path, f: &crate::catalog::File) -> PathBuf {
    d.join(format!(".{}.sha256", f.name))
}

fn unpacked(d: &Path, f: &crate::catalog::File) -> bool {
    std::fs::read_to_string(stamp(d, f)).is_ok_and(|s| s.trim().eq_ignore_ascii_case(f.sha256))
}

fn present(d: &Path, f: &crate::catalog::File) -> bool {
    if f.unzip {
        unpacked(d, f)
    } else {
        d.join(f.name).is_file()
    }
}

/// İndirilecek bayt: yerinde olmayan dosyalar (kurulu olsa da yeni dosya eklenmiş olabilir).
pub fn missing(m: &Model) -> u64 {
    if installed(m) {
        return 0;
    }
    let d = dir(m);
    let files = crate::catalog::get(real_id(m.id)).map_or(m.files, |b| b.files);
    files
        .iter()
        .filter(|f| if f.unzip { !unpacked(&d, f) } else { std::fs::metadata(d.join(f.name)).map_or(true, |x| x.len() != f.size) })
        .map(|f| f.size)
        .sum()
}

/// Diskte kapladığı yer (kurulu değilse 0).
pub fn used(m: &Model) -> u64 {
    fn walk(p: &Path) -> u64 {
        std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.metadata() {
            Ok(md) if md.is_dir() => walk(&e.path()),
            Ok(md) => md.len(),
            Err(_) => 0,
        }).sum()
    }
    if installed(m) {
        walk(&dir(m))
    } else {
        0
    }
}

fn consent_file(m: &Model) -> PathBuf {
    root().join(".licenses").join(m.id)
}

/// Kısıtlı lisanslı modelde kullanıcı lisansı onayladı mı (kısıtsızsa her zaman evet).
/// Onay model silinse de kalır; aynı soru ikinci kez sorulmaz.
pub fn accepted(m: &Model) -> bool {
    !m.restricted || consent_file(m).is_file()
}

/// Lisans onayını kaydeder (tarih ve lisans adıyla).
pub fn accept(m: &Model) -> Result<(), String> {
    let f = consent_file(m);
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    std::fs::write(&f, format!("{}\n{}\n{}\n{secs}\n", m.id, m.license, m.source)).map_err(|e| e.to_string())
}

/// Komut girdisi dikteyle aynı dosyaları kullanır; silmek ikisinden de kaldırır.
pub fn remove(m: &Model) -> Result<(), String> {
    let d = dir(m);
    let _ = std::fs::remove_file(d.join(".ok"));
    std::fs::remove_dir_all(&d).or_else(|e| if d.exists() { Err(e.to_string()) } else { Ok(()) })
}

/// Dosyanın SHA-256'sı (küçük harf onaltılık), parça parça okunarak.
pub fn sha256_file(p: &Path) -> Result<String, String> {
    use windows::Win32::Security::Cryptography::{BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE};
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = BCRYPT_HASH_HANDLE::default();
    unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut h, None, None, 0) }.ok().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    let res = (|| {
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            unsafe { BCryptHashData(h, &buf[..n], 0) }.ok().map_err(|e| e.to_string())?;
        }
        let mut out = [0u8; 32];
        unsafe { BCryptFinishHash(h, &mut out, 0) }.ok().map_err(|e| e.to_string())?;
        Ok(out.iter().map(|b| format!("{b:02x}")).collect())
    })();
    unsafe {
        let _ = BCryptDestroyHash(h);
    }
    res
}

/// Modeli indirir, doğrular ve kurar. `progress(0..1)`.
pub fn install(m: &Model, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<(), String> {
    if real_id(m.id) != m.id {
        let base = crate::catalog::get(real_id(m.id)).ok_or("?")?;
        return install(base, cancel, progress);
    }
    if !accepted(m) {
        return Err(t!("Bu modelin lisansı onaylanmadı.", "This model's license hasn't been accepted.").into());
    }
    let d = dir(m);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(d.join(".ok"));
    if m.files.iter().any(|f| !crate::net::plain_name(f.name)) {
        return Err(t!("Model dosya adı geçersiz.", "Invalid model file name.").into());
    }
    // Önceki kurulumdan doğrulanmış dosya kalmışsa yeniden indirilmez; ilerleme
    // yalnızca gerçekten inecek baytlara göre gösterilir.
    let kept: Vec<bool> = m
        .files
        .iter()
        .map(|f| {
            let dst = d.join(f.name);
            (f.unzip && unpacked(&d, f)) || std::fs::metadata(&dst).is_ok_and(|x| x.len() == f.size) && sha256_file(&dst).is_ok_and(|h| h.eq_ignore_ascii_case(f.sha256))
        })
        .collect();
    let total = m.files.iter().zip(&kept).filter(|(_, k)| !**k).map(|(f, _)| f.size).sum::<u64>().max(1) as f64;
    let mut done = 0u64;
    for (f, kept) in m.files.iter().zip(kept) {
        let dst = d.join(f.name);
        if !kept {
            let base = done;
            crate::net::fetch(f.url, &dst, f.size, f.sha256, cancel, &|n| progress(((base + n.min(f.size)) as f64 / total).min(0.99) as f32))?;
            done += f.size;
        }
        if f.unzip && !(kept && unpacked(&d, f)) {
            let _ = std::fs::remove_file(stamp(&d, f));
            crate::net::unzip(&dst, &d, None)?;
            std::fs::write(stamp(&d, f), f.sha256).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(d.join(".ok"), m.id).map_err(|e| e.to_string())?;
    progress(1.0);
    carlui_core::info!("model kuruldu: {}", m.id);
    Ok(())
}

/// Klasörde (alt klasörler dahil) adı verilen dosyayı bulur.
pub fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::File;

    const ZIP: File = File { url: "", name: "engine.zip", size: 1, sha256: "AB12", unzip: true };
    const PLAIN: File = File { url: "", name: "model.bin", size: 1, sha256: "cd34", unzip: false };

    #[test]
    fn unzipped_file_counts_by_stamp() {
        let d = std::env::temp_dir().join(format!("carlui-store-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Açılan zip silinir: damga yokken eksik sayılır.
        assert!(!present(&d, &ZIP));
        std::fs::write(stamp(&d, &ZIP), "ab12").unwrap();
        assert!(present(&d, &ZIP));
        // Katalog başka paket gösterirse damga tutmaz.
        std::fs::write(stamp(&d, &ZIP), "ffff").unwrap();
        assert!(!present(&d, &ZIP));
        assert!(!present(&d, &PLAIN));
        std::fs::write(d.join(PLAIN.name), "x").unwrap();
        assert!(present(&d, &PLAIN));
        let _ = std::fs::remove_dir_all(&d);
    }
}
