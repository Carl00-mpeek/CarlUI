//! Bu modül "bir modül çöktü, kullanıcı bize ne gönderebilir?" sorununu
//! çözüyor. Ana süreçteki bekçi, modül panikle (çıkış kodu 101) ya da bir
//! Windows istisnasıyla (0xC0000000 ve üstü; `panic = "abort"` derlemede panik
//! de 0xC0000409 olarak görünür) biterse buradan bir rapor yazar:
//! `%LOCALAPPDATA%\CarlUI\logs\crash-<modül>-<tarih>.txt`.
//!
//! Rapor yalnızca bu bilgisayarda kalır; hiçbir yere gönderilmez. İçinde sürüm,
//! Windows derlemesi, çıkış kodu ve o sürecin günlüğünden son satırlar vardır.
//! Kullanıcı klasörü yolu `%USERPROFILE%` ile değiştirilir ki rapor bir hata
//! kaydına yapıştırılınca kullanıcı adı görünmesin. Eski raporlar günlüklerle
//! birlikte 7 günde silinir (`log::init`).

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Hata bildirim şablonu; rapor dosyasının başında yazar.
pub const ISSUE_URL: &str = "https://github.com/Carl00-mpeek/CarlUI/issues/new?template=hata.yml";

/// Rapora eklenen en çok günlük satırı.
const TAIL: usize = 40;

/// Çıkış kodu bir çökme mi? Rust paniği 101; Windows istisnaları (erişim
/// ihlali 0xC0000005, `panic = "abort"`/fail-fast 0xC0000409…) 0xC0000000 ve
/// üstü. Görev Yöneticisi'nden sonlandırma (1) ya da normal çıkış çökme sayılmaz.
pub fn is_crash(code: Option<i32>) -> bool {
    code.is_some_and(|c| c == 101 || (c as u32) >= 0xC000_0000)
}

/// Çıkış kodunun okunur adı.
pub fn describe(code: Option<i32>) -> String {
    match code {
        None => "bilinmiyor".into(),
        Some(101) => "101 (Rust paniği)".into(),
        Some(c) => {
            let u = c as u32;
            let name = match u {
                0xC000_0005 => " (erişim ihlali)",
                0xC000_00FD => " (yığın taşması)",
                0xC000_0409 => " (panik / fail-fast)",
                0xC000_0374 => " (yığın bozulması)",
                0xC000_001D => " (geçersiz komut)",
                0xC000_0094 => " (sıfıra bölme)",
                _ => "",
            };
            if u >= 0xC000_0000 {
                format!("0x{u:08X}{name}")
            } else {
                c.to_string()
            }
        }
    }
}

/// Günlükte bu sürecin ("başladı (pid N)") başladığı satırdan sonraki son
/// `max` satır. Başlangıç satırı bulunamazsa günlüğün son `max` satırı.
pub fn tail_since_start(log: &str, pid: u32, max: usize) -> Vec<&str> {
    let marker = format!("başladı (pid {pid})");
    let lines: Vec<&str> = log.lines().map(|l| l.trim_start_matches('\u{feff}')).collect();
    let from = lines.iter().rposition(|l| l.contains(&marker)).unwrap_or(0);
    let run = &lines[from..];
    run[run.len().saturating_sub(max)..].to_vec()
}

/// Kullanıcı klasörü yolunu (büyük/küçük harf fark etmeden) `%USERPROFILE%` ile değiştirir.
pub fn redact(text: &str, profile: Option<&str>) -> String {
    let Some(p) = profile.map(|p| p.trim_end_matches(['\\', '/'])).filter(|p| p.len() >= 3) else {
        return text.to_string();
    };
    let lower = text.to_lowercase();
    let needle = p.to_lowercase();
    // Küçük harfe çevirmek bazı karakterlerde bayt uzunluğunu değiştirebilir;
    // o zaman güvenli yol: değiştirmeden bırakmak yerine düz eşleşme.
    if lower.len() != text.len() {
        return text.replace(p, "%USERPROFILE%");
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(at) = lower[i..].find(&needle) {
        out.push_str(&text[i..i + at]);
        out.push_str("%USERPROFILE%");
        i += at + needle.len();
    }
    out.push_str(&text[i..]);
    out
}

/// Rapor metni. Saf işlev: dosya ve sistem okumaz (test edilebilsin).
pub fn compose(module: &str, pid: u32, code: Option<i32>, version: &str, build: u32, when: &str, tail: &[&str]) -> String {
    let panic = tail.iter().rev().find(|l| l.contains("[ÇÖKME]")).map(|l| l.split("[ÇÖKME]").nth(1).unwrap_or("").trim());
    let mut s = String::new();
    s.push_str("CarlUI çökme raporu\n");
    s.push_str("===================\n\n");
    s.push_str("Bu dosya yalnızca bu bilgisayarda; hiçbir yere gönderilmedi.\n");
    s.push_str("Hata bildirmek istersen içeriğini şu sayfadaki \"Log\" kutusuna yapıştır:\n");
    s.push_str(ISSUE_URL);
    s.push_str("\n\n");
    s.push_str(&format!("Zaman:        {when}\n"));
    s.push_str(&format!("Modül:        {module} (pid {pid})\n"));
    s.push_str(&format!("CarlUI:       {version}\n"));
    s.push_str(&format!("Windows:      derleme {build}\n"));
    s.push_str(&format!("Çıkış kodu:   {}\n", describe(code)));
    if let Some(p) = panic.filter(|p| !p.is_empty()) {
        s.push_str(&format!("Panik:        {p}\n"));
    }
    s.push_str(&format!("\nGünlüğün son {} satırı\n", tail.len()));
    s.push_str("----------------------\n");
    for l in tail {
        s.push_str(l);
        s.push('\n');
    }
    s
}

fn local_stamp() -> (String, String) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
    )
}

fn read_log(dir: &Path, log_name: &str, pid: u32) -> Vec<String> {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    let marker = format!("başladı (pid {pid})");
    // Gece yarısını geçen süreç: başlangıç dünkü dosyada olabilir.
    let mut best: Option<String> = None;
    for d in [days, days.saturating_sub(1)] {
        if let Ok(text) = std::fs::read_to_string(dir.join(format!("{log_name}-{d}.log"))) {
            let found = text.contains(&marker);
            if best.is_none() || found {
                best = Some(text);
            }
            if found {
                break;
            }
        }
    }
    best.map(|t| tail_since_start(&t, pid, TAIL).into_iter().map(str::to_string).collect()).unwrap_or_default()
}

/// Raporu yazar ve yolunu döndürür. `log_name`, modülün `log::init`'e verdiği
/// ad (ör. "assist" modülü "convert" günlüğüne yazar).
pub fn write(module: &str, log_name: &str, pid: u32, code: Option<i32>) -> Option<PathBuf> {
    let dir = crate::paths::log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let tail = read_log(&dir, log_name, pid);
    let tail: Vec<&str> = tail.iter().map(String::as_str).collect();
    let (when, stamp) = local_stamp();
    let text = compose(module, pid, code, &crate::update::current(), crate::os::build(), &when, &tail);
    let profile = std::env::var("USERPROFILE").ok();
    let text = redact(&text, profile.as_deref());
    let path = dir.join(format!("crash-{module}-{stamp}.txt"));
    let mut f = std::fs::File::create(&path).ok()?;
    f.write_all(b"\xEF\xBB\xBF").ok()?;
    f.write_all(text.replace('\n', "\r\n").as_bytes()).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_codes() {
        assert!(is_crash(Some(101)));
        assert!(is_crash(Some(0xC000_0409u32 as i32)));
        assert!(is_crash(Some(0xC000_0005u32 as i32)));
        assert!(!is_crash(Some(0)));
        assert!(!is_crash(Some(1)));
        assert!(!is_crash(Some(crate::RELOAD_EXIT)));
        assert!(!is_crash(None));
    }

    #[test]
    fn describe_codes() {
        assert_eq!(describe(Some(0xC000_0005u32 as i32)), "0xC0000005 (erişim ihlali)");
        assert_eq!(describe(Some(101)), "101 (Rust paniği)");
        assert_eq!(describe(Some(3)), "3");
    }

    #[test]
    fn tail_starts_at_this_process() {
        let log = "\u{feff}a [dock] [BİLGİ] başladı (pid 10)\nb eski\nc [dock] [BİLGİ] başladı (pid 20)\nd yeni\ne [dock] [ÇÖKME] boom @ x.rs:1\n";
        assert_eq!(tail_since_start(log, 20, 40), vec!["c [dock] [BİLGİ] başladı (pid 20)", "d yeni", "e [dock] [ÇÖKME] boom @ x.rs:1"]);
        assert_eq!(tail_since_start(log, 20, 1), vec!["e [dock] [ÇÖKME] boom @ x.rs:1"]);
        // Başlangıç yoksa son satırlar.
        assert_eq!(tail_since_start(log, 99, 2), vec!["d yeni", "e [dock] [ÇÖKME] boom @ x.rs:1"]);
        assert!(tail_since_start("", 1, 5).is_empty());
    }

    #[test]
    fn redacts_profile_any_case() {
        let t = r"açılamadı: C:\Users\Ayşe\Desktop\a.png ve c:\users\ayşe\x";
        // Türkçe karakter içeren yol: küçük harf bayt uzunluğu aynı kalır.
        let r = redact(t, Some(r"C:\Users\Ayşe\"));
        assert!(!r.to_lowercase().contains("ayşe"), "{r}");
        assert_eq!(r.matches("%USERPROFILE%").count(), 2);
        assert_eq!(redact("değişmez", None), "değişmez");
        assert_eq!(redact("C:\\a", Some("C:")), "C:\\a");
    }

    #[test]
    fn redact_with_length_changing_lowercase() {
        // 'İ' küçük harfte uzar; düz eşleşmeye düşer ama yine de siler.
        let r = redact(r"C:\Users\İLKER\x İ", Some(r"C:\Users\İLKER"));
        assert_eq!(r, r"%USERPROFILE%\x İ");
    }

    #[test]
    fn report_has_summary_and_panic() {
        let tail = ["t [dock] [BİLGİ] başladı (pid 7)", "t [dock] [ÇÖKME] index out of bounds @ src/a.rs:3"];
        let r = compose("dock", 7, Some(0xC000_0409u32 as i32), "1.2.5", 26100, "2026-11-20 10:00:00", &tail);
        assert!(r.contains("Modül:        dock (pid 7)"));
        assert!(r.contains("CarlUI:       1.2.5"));
        assert!(r.contains("Panik:        index out of bounds @ src/a.rs:3"));
        assert!(r.contains(ISSUE_URL));
        assert!(r.ends_with("src/a.rs:3\n"));
    }
}
