#![no_main]
//! İçe aktarma dosyası (CSV, JSON, otpauth satırları): panik olmamalı.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = carlui_vault::import::parse(data);
});
