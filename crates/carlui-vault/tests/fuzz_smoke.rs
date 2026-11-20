//! Dışarıdan gelen veriyi okuyan Carl Vault ayrıştırıcıları için kararlı
//! (stable) Rust'ta çalışan bozma testi: içe aktarma dosyaları, otpauth ve
//! Google Authenticator adresleri, `.carlvault` yedek başlığı, passkey ve
//! otomatik doldurma adres eşleştirmesi.
//!
//! Her ayrıştırıcıya geçerli örnekler ve onlardan türetilen binlerce bozuk
//! girdi (bayt çevirme, kesme, ekleme, silme, çoğaltma, örnekleri birleştirme)
//! verilir. Beklenen tek şey: panik yok, sonsuz döngü yok. Sonuç doğru mu,
//! birim testleri ölçer.
//!
//! Tohum sabit, sonuç her çalıştırmada aynı. Daha uzun bir tarama için:
//! `set CARLUI_FUZZ_ROUNDS=200000` + `cargo test -p carlui-vault --test fuzz_smoke --release`.
//! Asıl bulanıklaştırma (libFuzzer, kapsam güdümlü) `fuzz/` klasöründe.

use carlui_vault::{autofill, export, import, passkey, totp::Totp};

/// Bağımlılıksız, belirlenimci rastgele sayı üreteci (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Ayrıştırıcıların özel anlam verdiği baytlar: bunlar rastgele bayttan daha
/// çok dal açar.
const INTERESTING: &[u8] = b",\"\n\r{}[]:=&?%/#@\\\x00\xff\xef\xbb\xbf0123456789-+.eE ";

fn mutate(rng: &mut Rng, seeds: &[&[u8]], out: &mut Vec<u8>) {
    out.clear();
    out.extend_from_slice(seeds[rng.below(seeds.len())]);
    for _ in 0..1 + rng.below(8) {
        let len = out.len();
        match rng.below(8) {
            0 if len > 0 => {
                let i = rng.below(len);
                out[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                out[i] = INTERESTING[rng.below(INTERESTING.len())];
            }
            2 => {
                let i = rng.below(len + 1);
                out.insert(i, INTERESTING[rng.below(INTERESTING.len())]);
            }
            3 if len > 0 => {
                let a = rng.below(len);
                let b = (a + 1 + rng.below(16)).min(len);
                out.drain(a..b);
            }
            4 => out.truncate(rng.below(len + 1)),
            5 if len > 0 => {
                // Bir parçayı çoğalt (iç içe yapılar, uzun satırlar).
                let a = rng.below(len);
                let b = (a + 1 + rng.below(64)).min(len);
                let chunk = out[a..b].to_vec();
                let at = rng.below(out.len() + 1);
                for _ in 0..1 + rng.below(4) {
                    out.splice(at..at, chunk.iter().copied());
                }
            }
            6 => {
                // Başka bir örnekten parça ekle.
                let other = seeds[rng.below(seeds.len())];
                if !other.is_empty() {
                    let a = rng.below(other.len());
                    let b = (a + 1 + rng.below(64)).min(other.len());
                    let at = rng.below(out.len() + 1);
                    out.splice(at..at, other[a..b].iter().copied());
                }
            }
            _ => out.push(rng.next() as u8),
        }
    }
}

fn rounds(default: usize) -> usize {
    std::env::var("CARLUI_FUZZ_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// `f`'yi tohumlarla ve bozulmuş hallerle çağırır; panik olursa girdiyi
/// onaltılık yazıp testi düşürür (yeniden üretilebilsin).
fn run(name: &str, seed: u64, seeds: &[&[u8]], n: usize, f: impl Fn(&[u8])) {
    for s in seeds {
        f(s);
    }
    let mut rng = Rng(seed);
    let mut buf = Vec::new();
    for i in 0..n {
        mutate(&mut rng, seeds, &mut buf);
        let input = buf.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&input))).is_err() {
            let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
            panic!("{name}: {i}. girdide panik; girdi (hex): {hex}");
        }
    }
}

const BITWARDEN_CSV: &[u8] = b"folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n,,login,GitHub,,,0,https://github.com,carl,\"p,a\"\"ss\",otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP\n";
const CHROME_CSV: &[u8] = b"name,url,username,password,note\ngithub.com,https://github.com/login,carl,\"p,a\"\"ss\",\"iki\nsatir\"\n,https://www.gitlab.com/,carl2,x,\n";
const CARD_CSV: &[u8] = b"name,cardholder name,card number,expiry,cvv\nVisa,Carl,4111111111111111,12/29,123\n";
const BITWARDEN_JSON: &[u8] = br#"{"encrypted":false,"items":[{"type":1,"name":"GitHub","notes":null,"login":{"username":"carl","password":"s3cret","totp":"otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP","uris":[{"uri":"https://github.com"}]},"fields":[{"name":"PIN","value":"42"}]},{"type":3,"name":"Visa","card":{"cardholderName":"Carl","number":"4111111111111111","expMonth":"12","expYear":"2029","code":"123"}},{"type":2,"name":"Not","notes":"metin"}]}"#;
const AEGIS_JSON: &[u8] = br#"{"version":1,"db":{"version":2,"entries":[{"type":"totp","name":"carl","issuer":"GitHub","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"period":30}}]}}"#;
const TWOFAS_JSON: &[u8] = br#"{"services":[{"name":"Discord","secret":"JBSWY3DPEHPK3PXP","otp":{"account":"carl","digits":8,"period":30,"algorithm":"SHA256","tokenType":"TOTP"}}],"schemaVersion":4}"#;
const ANDOTP_JSON: &[u8] = br#"[{"secret":"JBSWY3DPEHPK3PXP","label":"Google:carl@gmail.com","digits":6,"type":"TOTP","algorithm":"SHA1","period":30}]"#;
const URI_LINES: &[u8] = b"otpauth://totp/Example:alice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example&digits=8&period=60&algorithm=SHA512\notpauth://hotp/X:y?secret=GEZDGNBV&counter=3\n";
const MIGRATION: &[u8] = b"otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZSABKAEwAhABGAEgACjr4JKK%2Bf%2F%2F%2F%2F8B";

#[test]
fn import_files() {
    let seeds: &[&[u8]] = &[BITWARDEN_CSV, CHROME_CSV, CARD_CSV, BITWARDEN_JSON, AEGIS_JSON, TWOFAS_JSON, ANDOTP_JSON, URI_LINES, MIGRATION, b"", b"\xef\xbb\xbf", b"[", b"{"];
    run("import::parse", 0x1A2B_3C4D, seeds, rounds(20_000), |d| {
        let _ = import::parse(d);
    });
}

#[test]
fn deep_json_does_not_overflow_stack() {
    // serde_json'un iç içelik sınırı (128) yığını korur; bu bir gerileme denetimi.
    for open in [b'[', b'{'] {
        let mut d = vec![open; 100_000];
        if open == b'{' {
            d = b"{\"a\":".repeat(50_000);
        }
        let _ = import::parse(&d);
    }
}

#[test]
fn otp_uris() {
    let seeds: &[&[u8]] = &[URI_LINES, MIGRATION, b"otpauth://totp/A?secret=JBSWY3DPEHPK3PXP", b"JBSWY3DPEHPK3PXP", b"otpauth://totp/%E2%82%AC:%?secret=&digits=99&period=0"];
    run("otp", 0x5EED_0001, seeds, rounds(20_000), |d| {
        let s = String::from_utf8_lossy(d);
        let _ = import::gauth::parse(&s);
        if let Ok(t) = Totp::parse(&s) {
            // Ayrıştırılan her TOTP, uç zamanlarda da kod üretebilmeli.
            for now in [0, 1, 59, u64::MAX / 2, u64::MAX] {
                let _ = t.code(now);
                let _ = t.remaining(now);
            }
        }
        let _ = carlui_vault::totp::names(&s);
    });
}

#[test]
fn backup_header() {
    // Geçerli başlıkta Argon2 çalışır ve pahalıdır; burada çoğu girdi
    // başlıkta ya da yük boyunda reddedilir. Tur sayısı bu yüzden düşük.
    let header = br#"CARLVAULT/1
{"kdf":{"m_kib":8,"t":1,"p":1,"salt":"00112233445566778899aabbccddeeff"},"created":0,"count":1}
"#;
    let mut sealed = header.to_vec();
    sealed.extend_from_slice(&[0u8; 64]);
    let seeds: &[&[u8]] = &[header, &sealed, b"", b"CARLVAULT/1\n", b"\n\n"];
    run("export::open", 0xBAC0_0001, seeds, rounds(2_000), |d| {
        let _ = export::is_backup(d);
        let _ = export::open(d, "parola");
    });
}

#[test]
fn site_matching() {
    let seeds: &[&[u8]] = &[
        b"github.com\x00https://github.com/login",
        b"login.example.co.uk\x00https://evil.co.uk/",
        b"https://a.b.c\x00https://c/",
        b"app://steam\x00http://[::1]:8080/x?y#z",
        b"xn--80ak6aa92e.com\x00https://\xd0\xb0\xd1\x80\xd1\x80\xd0\xb5.com",
    ];
    run("site", 0x0517_E001, seeds, rounds(20_000), |d| {
        let s = String::from_utf8_lossy(d);
        let (a, b) = s.split_once('\0').unwrap_or((&s, ""));
        let _ = autofill::url_matches(a, b);
        let _ = passkey::origin_of(b);
        let _ = passkey::unb64(a);
        let _ = import::host(b);
        // Güvenlik değişmezi: rp_allowed true diyorsa sayfa https olmalı
        // (yerel geliştirme adresleri hariç) ve sayfa alan adı rp'nin kendisi
        // ya da alt alan adı olmalı.
        if passkey::rp_allowed(a, b) {
            if let Some((_, host)) = passkey::origin_of(b) {
                let rp = a.trim().to_ascii_lowercase();
                let host = host.to_ascii_lowercase();
                assert!(host == rp || host.ends_with(&format!(".{rp}")), "rp_allowed({a:?}, {b:?}) yabancı alana izin verdi");
            }
        }
    });
}
