//! Bu modül "indirme sürüyor mu, bitti mi?" sorununu tarayıcıya dokunmadan
//! şu yöntemle çözüyor: İndirilenler klasörü Windows'un klasör değişiklik
//! bildirimiyle izlenir (boştayken hiç yoklanmaz). Tarayıcıların yarım
//! dosyaları (.crdownload, .part, .partial, .download) görülünce saniyede bir
//! boyutlarına bakılır ve adanın göstergelerinde "İndirme 45 MB" görünür;
//! yarım dosya kaybolup asıl dosya oluşunca "İndirme tamamlandı" bildirimi
//! gelir. Toplam boyut tarayıcıdan okunamadığı için yüzde gösterilmez.

use crate::notify::Note;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Storage::FileSystem::*,
    System::Threading::{WaitForSingleObject, INFINITE},
    UI::WindowsAndMessaging::PostMessageW,
};

const PARTIAL: &[&str] = &[".crdownload", ".part", ".partial", ".download"];

/// Süren indirmeler: (dosya adı, şu anki bayt).
static ACTIVE: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

pub fn active() -> Vec<(String, u64)> {
    ACTIVE.lock().map(|v| v.clone()).unwrap_or_default()
}

/// "rapor.pdf.crdownload" → "rapor.pdf"; Firefox'un "abc123.part" gibi adsız yarım dosyası → None.
fn final_name(partial: &str) -> Option<String> {
    let lower = partial.to_lowercase();
    let ext = PARTIAL.iter().find(|e| lower.ends_with(*e))?;
    let base = &partial[..partial.len() - ext.len()];
    (base.contains('.')).then(|| base.to_string())
}

fn scan(dir: &Path) -> HashMap<String, u64> {
    let mut m = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if PARTIAL.iter().any(|x| lower.ends_with(x)) {
                m.insert(name, e.metadata().map(|md| md.len()).unwrap_or(0));
            }
        }
    }
    m
}

/// İzleyiciyi başlatır; biten indirmeler `notes`'a eklenip `msg` gönderilir.
pub fn start(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("downloads".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            let Some(dir) = carlui_core::win::downloads_dir() else { return };
            let wdir: Vec<u16> = dir.as_os_str().encode_wide_z();
            let h = unsafe {
                FindFirstChangeNotificationW(windows::core::PCWSTR(wdir.as_ptr()), false, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE)
            };
            let Ok(h) = h else {
                carlui_core::warn!("İndirilenler izlenemiyor: {}", dir.display());
                return;
            };
            let mut prev = scan(&dir);
            loop {
                // İndirme sürerken saniyede bir boyut; yokken yalnızca değişiklikte uyan.
                let wait = if prev.is_empty() { INFINITE } else { 1000 };
                unsafe {
                    WaitForSingleObject(h, wait);
                    let _ = FindNextChangeNotification(h);
                }
                let now = scan(&dir);
                for name in prev.keys().filter(|n| !now.contains_key(*n)) {
                    let Some(done) = final_name(name) else { continue };
                    if dir.join(&done).exists() {
                        carlui_core::info!("indirme bitti: {done}");
                        let note = Note { app: "İndirmeler".into(), title: "İndirme tamamlandı".into(), file: Some(dir.join(&done).display().to_string()), body: done, ..Default::default() };
                        if let Ok(mut q) = notes.lock() {
                            q.push_back(note);
                        }
                        unsafe {
                            let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(0), LPARAM(0));
                        }
                    }
                }
                if let Ok(mut a) = ACTIVE.lock() {
                    *a = now.iter().map(|(n, s)| (final_name(n).unwrap_or_else(|| n.clone()), *s)).collect();
                }
                prev = now;
            }
        })
        .ok();
}

trait WideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl WideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(final_name("rapor.pdf.crdownload"), Some("rapor.pdf".into()));
        assert_eq!(final_name("setup.exe.part"), Some("setup.exe".into()));
        assert_eq!(final_name("Kgx3c1.part"), None);
        assert_eq!(final_name("notes.txt"), None);
    }
}
