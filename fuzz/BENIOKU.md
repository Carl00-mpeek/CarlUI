# Bulanıklaştırma (fuzz)

Carl Vault'un dışarıdan veri okuyan ayrıştırıcıları için iki katman var:

1. **`crates/carlui-vault/tests/fuzz_smoke.rs`**: Kararlı (stable) Rust'ta,
   normal `cargo test` ile çalışır. Örnek girdilerden binlerce bozuk girdi
   türetir. Sabit tohum, CI'da her seferinde aynı sonuç. Daha uzun tarama:

   ```powershell
   $env:CARLUI_FUZZ_ROUNDS = "500000"
   cargo test -p carlui-vault --release --test fuzz_smoke
   ```

2. **Bu klasör (`fuzz/`)**: libFuzzer ile kapsam güdümlü bulanıklaştırma.
   Daha derin arar ama nightly ister ve çalışma alanının dışında durur.

   ```powershell
   rustup toolchain install nightly
   cargo install cargo-fuzz
   cargo +nightly fuzz run import -- -max_total_time=600
   ```

   Hedefler: `import` (CSV/JSON/otpauth dosyaları), `otp` (otpauth ve
   Google Authenticator adresleri), `backup` (`.carlvault` yedeği), `site`
   (otomatik doldurma ve passkey adres eşleştirmesi; yabancı alana passkey
   izni verilmediği de denetlenir).

   Windows'ta cargo-fuzz, MSVC ve AddressSanitizer ister (Visual Studio
   "C++ AddressSanitizer" bileşeni). Sorun çıkarsa WSL içinde de çalışır.

Bir çökme bulunursa girdiyi küçültüp (`cargo +nightly fuzz tmin <hedef> <dosya>`)
ilgili modüle birim testi olarak ekle, sonra düzelt.

Bulunanlar:

- `Totp::parse`, 10. baytı çok baytlı bir karakterin ortasına düşen
  girdide panikliyordu (kasa çöküyordu). Düzeltildi, birim testi eklendi.
