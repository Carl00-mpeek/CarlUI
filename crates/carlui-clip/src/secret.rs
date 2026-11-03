//! Bu modül "kopyalanan şifre/anahtar geçmişte açıkça durmasın" sorununu çözüyor.
//! Şifre yöneticileri çoğu zaman panoya "geçmişe alma" işareti koyar (bkz.
//! `capture`), koymayanlar ya da elle kopyalanan şifreler için metne bakılır:
//! bilinen anahtar biçimleri (sk-…, ghp_…, AKIA…, JWT, özel anahtar bloğu) ya da
//! boşluksuz, kısa, harf/rakam/simge karışık "şifreye benzeyen" metin.
//! Böyle kayıtlar listede maskelenir ve bir süre sonra kendiliğinden silinir.

const PREFIXES: &[&str] = &[
    "sk-", "sk_live_", "sk_test_", "rk_live_", "pk_live_", "ghp_", "gho_", "ghu_", "ghs_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "xapp-", "AKIA", "ASIA", "AIza", "ya29.", "npm_", "pypi-", "hf_",
];

/// Metin şifre ya da gizli anahtar gibi mi görünüyor?
pub fn looks_secret(text: &str) -> bool {
    let t = text.trim();
    if t.starts_with("-----BEGIN") && t.contains("PRIVATE KEY") {
        return true;
    }
    if t.is_empty() || t.len() > 200 || t.chars().any(char::is_whitespace) {
        return false;
    }
    if PREFIXES.iter().any(|p| t.starts_with(p)) && t.len() >= 16 {
        return true;
    }
    // JWT: üç base64url parça, ilki "eyJ" ile başlar.
    if t.starts_with("eyJ") && t.split('.').count() == 3 && t.len() >= 40 {
        return true;
    }
    if !(8..=64).contains(&t.chars().count()) {
        return false;
    }
    // Link, yol, e-posta, kod parçası değil.
    if t.contains("://") || t.contains('\\') || t.starts_with("www.") || t.contains("..") {
        return false;
    }
    if t.chars().any(|c| "(){}[]<>,;\"'`=".contains(c)) {
        return false;
    }
    let (mut lo, mut up, mut dg, mut sy) = (false, false, false, false);
    for c in t.chars() {
        if c.is_lowercase() {
            lo = true;
        } else if c.is_uppercase() {
            up = true;
        } else if c.is_ascii_digit() {
            dg = true;
        } else {
            sy = true;
        }
    }
    // Rakam şart, dört türden en az üçü: "Merhaba123!" evet, "carlui-island" hayır.
    // En az üç harf: "v0.58.10" gibi sürüm numaraları şifre sayılmasın.
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    dg && letters >= 3 && (lo as u8 + up as u8 + dg as u8 + sy as u8) >= 3
}

#[cfg(test)]
mod tests {
    use super::looks_secret as s;

    #[test]
    fn detects() {
        assert!(s("Merhaba123!"));
        assert!(s("Xk9#pL2qZ"));
        assert!(s("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(s("sk-proj-AbCdEf0123456789abcdef"));
        assert!(s("-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----"));
    }

    #[test]
    fn ignores() {
        for t in ["merhaba dünya", "carlui-island", "https://example.com/A1b", "C:\\Users\\A1b", "Rgba(232,72,72,0xE0)", "0532-123-45-67", "ali.veli@posta.com", "2026-11-01", "v0.58.10", "Hello", "abc123", "a1b2c3d4e5"] {
            assert!(!s(t), "{t}");
        }
    }
}
