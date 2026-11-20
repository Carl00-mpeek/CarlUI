#![no_main]
//! Otomatik doldurma ve passkey adres eşleştirmesi. Panik dışında bir
//! güvenlik değişmezi de denetlenir: rp_allowed yabancı alana izin vermemeli.
use carlui_vault::{autofill, import, passkey};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    let (a, b) = s.split_once('\0').unwrap_or((s, ""));
    let _ = autofill::url_matches(a, b);
    let _ = passkey::unb64(a);
    let _ = import::host(b);
    if passkey::rp_allowed(a, b) {
        let (_, host) = passkey::origin_of(b).expect("izin verilen adresin kökeni olmalı");
        let rp = a.trim().to_ascii_lowercase();
        let host = host.to_ascii_lowercase();
        assert!(host == rp || host.ends_with(&format!(".{rp}")));
    }
});
