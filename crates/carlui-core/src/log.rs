//! Bu modül "çökünce ne oldu?" sorusunu, her sürecin kendi günlük log
//! dosyasına zaman damgalı satırlar yazmasıyla ve panik kancasıyla çözüyor.
//! Çökme anındaki mesaj ve konum da dosyaya düşer.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

static SINK: Mutex<Option<(String, File)>> = Mutex::new(None);

fn timestamp() -> String {
    let st = windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { st() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

/// Süreç başında bir kez çağrılır. `module` log satırlarında ve dosya adında görünür.
pub fn init(module: &str) {
    let dir = crate::paths::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    // Eski logları temizle: 7 günden eskiler silinir.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 7 * 86_400);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let path = dir.join(format!("{module}-{days}.log"));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        // Yeni dosyaya UTF-8 BOM: Not Defteri ve PowerShell Türkçe karakterleri doğru göstersin.
        if f.metadata().map(|m| m.len() == 0).unwrap_or(false) {
            let _ = f.write_all(b"\xEF\xBB\xBF");
        }
        *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some((module.to_string(), f));
    }

    std::panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "bilinmeyen panik".into());
        write("ÇÖKME", &format!("{msg} @ {loc}"));
    }));
    write("BİLGİ", &format!("başladı (pid {})", std::process::id()));
}

pub fn write(level: &str, msg: &str) {
    let line = format!("{} [{level}] {msg}", timestamp());
    if cfg!(debug_assertions) {
        eprintln!("{line}");
    }
    if let Ok(mut guard) = SINK.lock() {
        if let Some((module, f)) = guard.as_mut() {
            let _ = writeln!(f, "{} [{module}] [{level}] {msg}", timestamp());
        }
    }
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::log::write("BİLGİ", &format!($($t)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::log::write("UYARI", &format!($($t)*)) };
}

#[macro_export]
macro_rules! error {
    ($($t:tt)*) => { $crate::log::write("HATA", &format!($($t)*)) };
}
