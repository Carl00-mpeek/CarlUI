#![no_main]
//! `.carlvault` yedeği: başlık ve yük. Geçerli başlıkta Argon2 çalışır;
//! libFuzzer'ın `-timeout` sınırı aşırı KDF ayarlarını da yakalar.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = carlui_vault::export::is_backup(data);
    let _ = carlui_vault::export::open(data, "parola");
});
