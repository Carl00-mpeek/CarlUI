//! Güçlü parola üretici. Rastgelelik işletim sisteminden (`getrandom`); her
//! karakter eşit olasılıkla seçilir (modulo sapması yok, reddederek örnekleme).
//! Seçilen her karakter türünden en az biri bulunur ki "en az bir rakam/sembol"
//! isteyen siteler reddetmesin. Karışabilen karakterler (0/O, 1/l/I) atılır.

use zeroize::Zeroizing;

const LOWER: &str = "abcdefghijkmnopqrstuvwxyz"; // l yok
const UPPER: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ"; // I, O yok
const DIGITS: &str = "23456789"; // 0, 1 yok
/// Sitelerin çoğunun kabul ettiği semboller (tırnak, boşluk, ters bölü yok).
const SYMBOLS: &str = "!@#$%&*-_+=?";

pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub len: usize,
    pub symbols: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { len: 20, symbols: true }
    }
}

/// 0..n arası düzgün dağılımlı rastgele sayı.
fn below(n: usize) -> usize {
    debug_assert!(n > 0 && n <= 256);
    let limit = 256 - 256 % n; // bunun üstü atılır (sapma olmasın)
    loop {
        let mut b = [0u8; 1];
        getrandom::getrandom(&mut b).expect("işletim sistemi rastgeleliği");
        if (b[0] as usize) < limit {
            return b[0] as usize % n;
        }
    }
}

fn pick(set: &str) -> char {
    let c: Vec<char> = set.chars().collect();
    c[below(c.len())]
}

pub fn generate(o: Options) -> Zeroizing<String> {
    let len = o.len.clamp(MIN_LEN, MAX_LEN);
    let mut sets = vec![LOWER, UPPER, DIGITS];
    if o.symbols {
        sets.push(SYMBOLS);
    }
    let all: String = sets.concat();
    // Her türden bir tane, kalanı tüm kümeden; sonra karıştır (Fisher–Yates).
    let mut out: Zeroizing<Vec<char>> = Zeroizing::new(sets.iter().map(|s| pick(s)).collect());
    while out.len() < len {
        out.push(pick(&all));
    }
    for i in (1..out.len()).rev() {
        out.swap(i, below(i + 1));
    }
    Zeroizing::new(out.iter().collect())
}

/// Kaba güç tahmini (bit): karakter türlerinden kurulan küme büyüklüğü ×
/// uzunluk; aynı karakterin tekrarı ve kısa kalıplar için kırpılır. Yalnızca
/// "zayıf / orta / güçlü" göstermek içindir.
pub fn bits(pw: &str) -> f64 {
    let has = |f: fn(&char) -> bool| pw.chars().any(|c| f(&c));
    let mut pool = 0.0;
    if has(char::is_ascii_lowercase) {
        pool += 26.0;
    }
    if has(char::is_ascii_uppercase) {
        pool += 26.0;
    }
    if has(char::is_ascii_digit) {
        pool += 10.0;
    }
    if has(|c| c.is_ascii_punctuation() || *c == ' ') {
        pool += 32.0;
    }
    if has(|c| !c.is_ascii()) {
        pool += 60.0;
    }
    let mut distinct: Vec<char> = pw.chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    let n = pw.chars().count().min(distinct.len() * 2) as f64;
    if pool == 0.0 {
        0.0
    } else {
        n * f64::log2(pool)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Weak,
    Fair,
    Strong,
}

pub fn strength(pw: &str) -> Strength {
    match bits(pw) {
        b if b < 45.0 => Strength::Weak,
        b if b < 75.0 => Strength::Fair,
        _ => Strength::Strong,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords() {
        for len in [8, 12, 20, 64, 3, 500] {
            let p = generate(Options { len, symbols: true });
            assert_eq!(p.chars().count(), len.clamp(MIN_LEN, MAX_LEN));
            assert!(p.chars().any(|c| c.is_ascii_lowercase()));
            assert!(p.chars().any(|c| c.is_ascii_uppercase()));
            assert!(p.chars().any(|c| c.is_ascii_digit()));
            assert!(p.chars().any(|c| SYMBOLS.contains(c)));
            assert!(!p.chars().any(|c| "0O1lI \"'\\".contains(c)));
        }
        let p = generate(Options { len: 16, symbols: false });
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
        // Tekrar etmez (2^100 üstü olasılıkta aynısı çıkmaz).
        assert_ne!(generate(Options::default()), generate(Options::default()));
    }

    #[test]
    fn distribution_is_flat() {
        // 8 rakamlı kümeden 16000 seçim: her biri ~2000 civarında olmalı.
        let mut n = [0usize; 8];
        for _ in 0..16000 {
            n[DIGITS.find(pick(DIGITS)).unwrap()] += 1;
        }
        assert!(n.iter().all(|&k| (1700..2300).contains(&k)), "{n:?}");
    }

    #[test]
    fn strengths() {
        assert_eq!(strength("123456"), Strength::Weak);
        assert_eq!(strength("aaaaaaaaaaaaaaaaaaaa"), Strength::Weak, "tekrar güçlü sayılmaz");
        assert_eq!(strength("Parola2024"), Strength::Fair);
        assert_eq!(strength(&generate(Options::default())), Strength::Strong);
    }
}
