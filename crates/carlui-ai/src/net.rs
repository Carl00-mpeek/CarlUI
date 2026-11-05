//! Bu modül "dışarıdan indirilen dosyaya nasıl güveniriz?" sorusunu çözüyor.
//! Adresler programın içine gömülüdür ve değiştirilemez; burada ayrıca:
//! - yalnızca HTTPS ve izinli sunucular (yönlendirmeler de yalnızca HTTPS),
//! - Windows'un kendi curl.exe/tar.exe'si tam yoluyla (PATH'e konan sahte bir
//!   curl çalışmaz) ve kullanıcının .curlrc'si okunmadan,
//! - beklenen boyuttan büyük indirme kesilir,
//! - SHA-256 ve tam boyut tutmadan dosya yerine konmaz,
//! - arşivde `..`, mutlak yol, sürücü harfi veya kısayol (symlink) varsa açılmaz.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const NO_WINDOW: u32 = 0x0800_0000;

/// İndirmeye izin verilen sunucular (ilk adres; yönlendirme sonrası dosya
/// zaten SHA-256 ile doğrulanır).
pub const HOSTS: &[&str] = &["huggingface.co", "github.com", "raw.githubusercontent.com", "alphacephei.com", "api.nuget.org"];

/// Adres HTTPS mi ve sunucusu izinli mi? (kullanıcı adı, port, ters eğik çizgi kabul edilmez)
pub fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\' || c == '@') {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    HOSTS.contains(&host)
}

/// SHA-256 metni geçerli mi (64 küçük harf onaltılık).
pub fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Klasöre yazılacak dosya adı düz bir ad mı (klasör ayırıcı, `..`, sürücü yok).
pub fn plain_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', ':', '\0']) && !n.starts_with('.')
}

/// `%SystemRoot%\System32\<ad>`; PATH'teki başka bir kopya kullanılmaz.
pub fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").filter(|s| !s.is_empty()).unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

/// `url`'yi indirir; boyut ve SHA-256 tutarsa `dst`'ye koyar. `tick(bayt)` sık çağrılır.
pub fn fetch(url: &str, dst: &Path, size: u64, sha256: &str, cancel: &AtomicBool, tick: &dyn Fn(u64)) -> Result<(), String> {
    if !allowed(url) || !valid_sha(sha256) || size == 0 {
        carlui_core::warn!("izinsiz indirme reddedildi: {url}");
        return Err(t!("Bu indirme adresine izin verilmiyor.", "This download address isn't allowed.").into());
    }
    let part = dst.with_extension("part");
    let _ = std::fs::remove_file(&part);
    let fail = |part: &Path, msg: String| {
        let _ = std::fs::remove_file(part);
        Err(msg)
    };
    let mut child = Command::new(system32("curl.exe"))
        // -q en başta olmalı: kullanıcının .curlrc'si (vekil, --insecure vb.) okunmaz.
        .args(["-q", "--location", "--fail", "--silent", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5", "--tlsv1.2", "--retry", "3", "--connect-timeout", "20", "--max-filesize"])
        .arg(size.to_string())
        .arg("--output")
        .arg(&part)
        .arg("--url")
        .arg(url)
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|e| format!("{}: {e}", t!("curl başlatılamadı", "couldn't start curl")))?;
    loop {
        let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if cancel.load(Ordering::Relaxed) || got > size {
            let _ = child.kill();
            let _ = child.wait();
            if got > size {
                carlui_core::warn!("indirme beklenenden büyük, kesildi: {url}");
                return fail(&part, t!("İndirilen dosya beklenenden büyük; durduruldu.", "The download was larger than expected; stopped.").into());
            }
            return fail(&part, t!("İndirme durduruldu", "Download stopped").into());
        }
        tick(got);
        match child.try_wait() {
            Ok(Some(s)) if s.success() => break,
            Ok(Some(_)) | Err(_) => return fail(&part, t!(format!("İndirilemedi: {url}"), format!("Couldn't download: {url}"))),
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let sum = crate::store::sha256_file(&part)?;
    if len != size || sum != sha256 {
        carlui_core::warn!("doğrulama tutmadı: {url} ({len} bayt, {sum})");
        return fail(&part, t!("İndirilen dosya doğrulanamadı; tekrar deneyin.", "The downloaded file failed verification; try again.").into());
    }
    let _ = std::fs::remove_file(dst);
    std::fs::rename(&part, dst).map_err(|e| e.to_string())
}

/// Arşivdeki bir kaydın adı güvenli mi?
fn safe_entry(n: &str) -> bool {
    !n.is_empty() && !n.starts_with(['/', '\\']) && !n.contains([':', '\0']) && !n.split(['/', '\\']).any(|p| p == "..")
}

fn tar(args: &[&std::ffi::OsStr]) -> Option<String> {
    let out = Command::new(system32("tar.exe")).args(args).creation_flags(NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Arşivi önce listeler, tehlikeli kayıt yoksa `into` içine açar. `only`:
/// yalnızca bu kaydı çıkar. Arşiv dosyası her durumda silinir.
pub fn unzip(zip: &Path, into: &Path, only: Option<&str>) -> Result<(), String> {
    let r = (|| {
        let bad = || -> String { t!("Arşiv güvenli değil; açılmadı.", "The archive isn't safe; not extracted.").into() };
        let names = tar(&["-tf".as_ref(), zip.as_os_str()]).ok_or_else(|| t!("Arşiv okunamadı", "Couldn't read the archive").to_string())?;
        if let Some(n) = names.lines().find(|n| !safe_entry(n.trim_end_matches('\r'))) {
            carlui_core::warn!("arşivde güvensiz kayıt: {n}");
            return Err(bad());
        }
        // Ayrıntılı listede ilk harf türdür: kısayol (l) ve sabit bağ (h) kabul edilmez.
        let long = tar(&["-tvf".as_ref(), zip.as_os_str()]).unwrap_or_default();
        if long.lines().any(|l| l.starts_with(['l', 'h'])) {
            carlui_core::warn!("arşivde kısayol var: {}", zip.display());
            return Err(bad());
        }
        let mut a: Vec<&std::ffi::OsStr> = vec!["-xf".as_ref(), zip.as_os_str(), "-C".as_ref(), into.as_os_str()];
        if let Some(o) = only {
            a.push(o.as_ref());
        }
        tar(&a).map(|_| ()).ok_or_else(|| t!("Arşiv açılamadı", "Couldn't extract the archive").into())
    })();
    let _ = std::fs::remove_file(zip);
    r
}

/// Dosyayı indirmeden sunucuya sorar: son yanıttaki boyut ve (Hugging Face
/// büyük dosyalarında) bildirilen SHA-256. Yalnızca bağlantı denetimi için.
pub fn probe(url: &str) -> (Option<u64>, Option<String>) {
    let Ok(out) = Command::new(system32("curl.exe")).args(["-q", "--silent", "--head", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5", "--url", url]).creation_flags(NO_WINDOW).output() else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    let mut size = None;
    let mut sha = None;
    for l in text.lines() {
        if let Some(v) = l.strip_prefix("content-length:") {
            size = v.trim().parse().ok();
        } else if let Some(v) = l.strip_prefix("x-linked-etag:") {
            // Büyük (LFS) dosyalarda SHA-256; küçüklerde git SHA-1 olur, o karşılaştırılmaz.
            sha = Some(v.trim().trim_matches('"').to_string()).filter(|s| s.len() == 64);
        }
    }
    (size, sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Elle çalıştırılır (ağ gerekir): `cargo test -p carlui-ai links_unchanged -- --ignored`.
    /// Her adres hâlâ aynı boyutu (ve biliniyorsa aynı SHA'yı) veriyor mu?
    #[test]
    #[ignore]
    fn links_unchanged() {
        let mut all: Vec<(&str, u64, &str)> = crate::catalog::MODELS.iter().flat_map(|m| m.files.iter().map(|f| (f.url, f.size, f.sha256))).collect();
        all.push((crate::runtime::ORT_URL, crate::runtime::ORT_SIZE, crate::runtime::ORT_SHA256));
        let mut bad = Vec::new();
        for (url, size, sha) in all {
            let (got, remote_sha) = probe(url);
            let ok = got == Some(size) && remote_sha.as_deref().is_none_or(|s| s == sha);
            println!("{} {url} ({got:?})", if ok { "tamam" } else { "DEĞİŞMİŞ" });
            if !ok {
                bad.push(url);
            }
        }
        assert!(bad.is_empty(), "değişen bağlantılar: {bad:#?}");
    }

    #[test]
    fn urls() {
        assert!(allowed("https://huggingface.co/a/b/resolve/0123/x.onnx"));
        assert!(allowed("https://github.com/x/y/releases/download/v1/z.zip"));
        assert!(!allowed("http://huggingface.co/a"));
        assert!(!allowed("https://huggingface.co.evil.com/a"));
        assert!(!allowed("https://evil.com/huggingface.co/a"));
        assert!(!allowed("https://huggingface.co@evil.com/a"));
        assert!(!allowed("https://huggingface.co:8443/a"));
        assert!(!allowed("https://huggingface.co\\@evil.com/a"));
        assert!(!allowed("https://github.com/a b"));
        assert!(!allowed("file:///C:/x"));
    }

    #[test]
    fn names() {
        assert!(plain_name("model.onnx"));
        for n in ["", ".", "..", "../x", "a/b", "a\\b", "C:x", ".ok"] {
            assert!(!plain_name(n), "{n}");
        }
        assert!(safe_entry("vosk-model/am/final.mdl"));
        assert!(safe_entry("runtimes/win-x64/native/onnxruntime.dll"));
        for n in ["../x", "a/../../x", "/etc/x", "\\x", "C:/x", "a\\..\\x", ""] {
            assert!(!safe_entry(n), "{n}");
        }
        assert!(valid_sha("8f680bba319e01a653d2e8a5dbc17a9157179e0576e6ce74ce0c06356c6e24f9"));
        assert!(!valid_sha("8F680BBA319E01A653D2E8A5DBC17A9157179E0576E6CE74CE0C06356C6E24F9"));
        assert!(!valid_sha("abc"));
    }
}
