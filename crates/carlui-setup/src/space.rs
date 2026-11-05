//! Bu modül "kurulum yarıda disk dolduğu için bozulmasın, güncellemenin ne
//! kadar yer değiştirdiği görünsün" sorununu çözüyor: hedef sürücüdeki boş
//! yer (`GetDiskFreeSpaceExW`) ve mevcut kurulumun diskte tuttuğu yer
//! (kurucunun koyduğu dosyalar) ölçülür. Kurulacak boyutla karşılaştırma
//! penceredeki uyarıyı ve güncelleme farkını verir.

use crate::{install::Options, payload, uninstall};
use std::path::Path;
use windows::{core::HSTRING, Win32::Storage::FileSystem::GetDiskFreeSpaceExW};

/// Kurulumdan sonra en az bu kadar boş yer kalsın (geçici `.old` kopyaları,
/// ayarlar, günlükler için).
const MARGIN: u64 = 50 * 1024 * 1024;

/// Klasörün bulunduğu sürücüde kullanıcıya açık boş yer. Klasör henüz yoksa
/// var olan ilk üst klasöre bakılır.
pub fn free_bytes(dir: &Path) -> Option<u64> {
    let at = dir.ancestors().find(|p| p.exists())?;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(at.as_os_str()), Some(&mut free), None, None).ok()? };
    Some(free)
}

fn size_of(p: &Path) -> u64 {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// Mevcut kurulumun diskte tuttuğu yer (kurucunun koyduğu dosyalar; yoksa 0).
pub fn installed_now(dir: &Path) -> u64 {
    uninstall::installed_files(dir).iter().map(|p| size_of(p)).sum::<u64>() + size_of(&dir.join(r"wallpaper\carlui.png"))
}

/// Seçimlere göre kurulumun gerektirdiği ek yer (mevcut dosyaların yerine
/// yazılanlar düşülür) ve güvenlik payı.
pub fn needed(o: &Options, now: u64) -> u64 {
    payload::installed_bytes(o).saturating_sub(now) + MARGIN
}

/// "+1,2 MB" / "−0,4 MB" / "±0 MB".
pub fn diff(new: u64, old: u64) -> String {
    let d = new as i64 - old as i64;
    if d.unsigned_abs() < 50 * 1024 {
        return "±0 MB".into();
    }
    format!("{}{}", if d > 0 { "+" } else { "−" }, payload::mb(d.unsigned_abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_signs() {
        assert_eq!(diff(10, 10), "±0 MB");
        assert!(diff(3 * 1024 * 1024, 1024 * 1024).starts_with('+'));
        assert!(diff(1024 * 1024, 3 * 1024 * 1024).starts_with('−'));
    }

    #[test]
    fn free_space_of_temp() {
        let d = std::env::temp_dir().join("carlui-yok").join("alt");
        assert!(free_bytes(&d).is_some_and(|f| f > 0));
    }
}
