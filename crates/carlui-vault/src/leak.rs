//! Sızıntı denetimi (Have I Been Pwned "Pwned Passwords"). Sorun: kasadaki
//! bir parola daha önce bir veri sızıntısında yayınlanmışsa saldırganların
//! deneme listelerindedir; ama parolayı denetlemek için bir sunucuya yollamak
//! kabul edilemez. Çözüm (k-anonimlik): parolanın SHA-1 özeti çıkarılır,
//! sunucuya yalnızca özetin ilk 5 onaltılık karakteri gider. Sunucu o önekle
//! başlayan yüzlerce özetin sonunu döner; eşleşme bu bilgisayarda aranır.
//! Parola ve tam özet hiçbir yere gönderilmez. `Add-Padding` ile yanıt boyu da
//! sabitlenir. İstek yalnızca kullanıcı "denetle" deyince yapılır.

use sha1::{Digest, Sha1};
use std::{collections::HashMap, os::windows::process::CommandExt, path::PathBuf, process::Command};
use zeroize::Zeroizing;

const NO_WINDOW: u32 = 0x0800_0000;
const MAX_BODY: usize = 2 * 1024 * 1024;
const HOST: &str = "https://api.pwnedpasswords.com/range/";

/// SHA-1 özetinin büyük harfli onaltılık hali: (5 karakterlik önek, 35 karakterlik son).
pub fn split(password: &str) -> (String, String) {
    let d = Sha1::digest(password.as_bytes());
    let hex: Zeroizing<String> = Zeroizing::new(d.iter().map(|b| format!("{b:02X}")).collect());
    (hex[..5].to_string(), hex[5..].to_string())
}

/// Aralık yanıtında ("SON:SAYI" satırları) bu son kaç kez görülmüş; yoksa 0.
pub fn count_in(body: &str, suffix: &str) -> u64 {
    body.lines()
        .filter_map(|l| l.trim().split_once(':'))
        .find(|(s, _)| s.eq_ignore_ascii_case(suffix))
        .and_then(|(_, n)| n.trim().parse().ok())
        .unwrap_or(0)
}

pub fn range_url(prefix: &str) -> Option<String> {
    (prefix.len() == 5 && prefix.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("{HOST}{}", prefix.to_ascii_uppercase()))
}

fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").filter(|s| !s.is_empty()).unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

/// Öneke düşen özet sonlarını indirir (Windows'un curl.exe'si, sertleştirilmiş
/// bayraklarla). Arka plan iş parçacığından çağrılmalı.
pub fn fetch_range(prefix: &str) -> Option<String> {
    let url = range_url(prefix)?;
    let out = Command::new(system32("curl.exe"))
        // -q en başta: kullanıcının .curlrc'si okunmaz.
        .args(["-q", "--silent", "--fail", "--proto", "=https", "--max-redirs", "0", "--max-time", "15", "-H", "Add-Padding: true", "--user-agent", "CarlUI-Vault", "--url", &url])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() || out.stdout.len() > MAX_BODY {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Denetlenecek bir parola: kaydın kimliği ve adı, alanın değeri.
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub password: Zeroizing<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub name: String,
    /// Sızıntılarda görülme sayısı.
    pub count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub checked: usize,
    /// Ağ hatası yüzünden denetlenemeyen parola sayısı.
    pub failed: usize,
    pub found: Vec<Finding>,
}

/// Parolaları denetler; aynı öneke düşenler için tek istek atılır. `get` ağı
/// soyutlar (testte sahte yanıt).
pub fn check_with(list: Vec<Candidate>, mut get: impl FnMut(&str) -> Option<String>) -> Report {
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut r = Report::default();
    for c in list {
        if c.password.is_empty() {
            continue;
        }
        let (prefix, suffix) = split(&c.password);
        let body = cache.entry(prefix.clone()).or_insert_with(|| get(&prefix));
        match body {
            Some(b) => {
                r.checked += 1;
                let n = count_in(b, &suffix);
                if n > 0 {
                    r.found.push(Finding { id: c.id, name: c.name, count: n });
                }
            }
            None => r.failed += 1,
        }
    }
    r.found.sort_by(|a, b| b.count.cmp(&a.count));
    r
}

pub fn check(list: Vec<Candidate>) -> Report {
    check_with(list, fetch_range)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, pw: &str) -> Candidate {
        Candidate { id: id.into(), name: id.to_uppercase(), password: Zeroizing::new(pw.into()) }
    }

    #[test]
    fn ozet_bolunur() {
        // SHA1("password") = 5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8
        assert_eq!(split("password"), ("5BAA6".into(), "1E4C9B93F3F0682250B6CF8331B7EE68FD8".into()));
    }

    #[test]
    fn yalnizca_onek_gider() {
        assert_eq!(range_url("5baa6").unwrap(), "https://api.pwnedpasswords.com/range/5BAA6");
        assert!(range_url("5BAA61").is_none());
        assert!(range_url("ZZZZZ").is_none());
        let mut asked = Vec::new();
        let r = check_with(vec![cand("a", "password"), cand("b", "password"), cand("c", "Çok-uzun-ve-eşsiz-parola-91!"), cand("d", "")], |p| {
            asked.push(p.to_string());
            assert_eq!(p.len(), 5, "sunucuya yalnızca 5 karakter");
            Some("1E4C9B93F3F0682250B6CF8331B7EE68FD8:9545824\r\n0000000000000000000000000000000000A:0\r\n".into())
        });
        assert_eq!(asked.len(), 2, "aynı önek için tek istek");
        assert_eq!(r.checked, 3);
        assert_eq!(r.found.len(), 2);
        assert_eq!(r.found[0].count, 9_545_824);
    }

    #[test]
    fn ag_hatasi_sayilir() {
        let r = check_with(vec![cand("a", "x")], |_| None);
        assert_eq!((r.checked, r.failed, r.found.len()), (0, 1, 0));
        assert_eq!(count_in("ABC:0\nDEF:12", "def"), 12);
        assert_eq!(count_in("ABC:0", "ABC"), 0, "dolgu satırı sızıntı sayılmaz");
    }
}
