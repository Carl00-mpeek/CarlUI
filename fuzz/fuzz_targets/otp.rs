#![no_main]
//! otpauth:// ve Google Authenticator otpauth-migration:// adresleri.
use carlui_vault::{import::gauth, totp};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|s: &str| {
    let _ = gauth::parse(s);
    let _ = totp::names(s);
    if let Ok(t) = totp::Totp::parse(s) {
        for now in [0, 59, u64::MAX] {
            let _ = t.code(now);
            let _ = t.remaining(now);
        }
    }
});
