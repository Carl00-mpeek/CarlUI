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

pub fn installed(m: &Model) -> bool {
    dir(m).join(".ok").is_file()
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
    let total = m.size().max(1) as f64;
    let mut done = 0u64;
    for f in m.files {
        if !crate::net::plain_name(f.name) {
            return Err(t!("Model dosya adı geçersiz.", "Invalid model file name.").into());
        }
        let dst = d.join(f.name);
        let base = done;
        crate::net::fetch(f.url, &dst, f.size, f.sha256, cancel, &|n| progress(((base + n.min(f.size)) as f64 / total).min(0.99) as f32))?;
        if f.unzip {
            crate::net::unzip(&dst, &d, None)?;
        }
        done += f.size;
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
