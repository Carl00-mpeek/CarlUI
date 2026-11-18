//! İşletim sistemi sürümü ve yazı tipi algılama.
//!
//! CarlUI Windows 11 için tasarlandı; Windows 10'da bazı API'ler (Mica/Acrylic
//! backdrop, köşe yuvarlama, host backdrop fırçası) ve yazı tipleri
//! ("Segoe Fluent Icons", "Segoe UI Variable") yok. Buradaki yardımcılar bir kez
//! ölçüp önbelleğe alır, çağıranlar uygun yedeğe geçer.

use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, DWRITE_FACTORY_TYPE_SHARED};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

/// Windows derleme numarası (ör. 19045 = Win10 22H2, 22631 = Win11 23H2).
/// Kayıt defterinden okunur; uyumluluk katmanları bunu yalan söyletmez.
pub fn build() -> u32 {
    static B: OnceLock<u32> = OnceLock::new();
    *B.get_or_init(|| unsafe {
        let mut buf = [0u16; 32];
        let mut len = (buf.len() * 2) as u32;
        let ok = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
        .is_ok();
        if !ok {
            return 22000;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n]).trim().parse().unwrap_or(22000)
    })
}

/// Windows 11 (derleme 22000+) mi?
pub fn is_win11() -> bool {
    build() >= 22000
}

fn has_family(name: PCWSTR) -> bool {
    unsafe {
        let Ok(f) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) else { return false };
        let mut col: Option<IDWriteFontCollection> = None;
        if f.GetSystemFontCollection(&mut col, BOOL(0)).is_err() {
            return false;
        }
        let Some(col) = col else { return false };
        let mut idx = 0u32;
        let mut exists = BOOL(0);
        col.FindFamilyName(name, &mut idx, &mut exists).is_ok() && exists.as_bool()
    }
}

/// "Segoe Fluent Icons" kurulu mu? (Win11'de var; Win10'da genelde yok.)
pub fn has_fluent_icons() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| has_family(w!("Segoe Fluent Icons")))
}

/// "Segoe UI Variable" kurulu mu?
pub fn has_ui_variable() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| has_family(w!("Segoe UI Variable Text")))
}

/// Simge yazı tipi adı: Fluent Icons yoksa Win10'daki Segoe MDL2 Assets.
pub fn icon_font() -> &'static str {
    if has_fluent_icons() { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" }
}

/// Arayüz yazı tipi adı: Segoe UI Variable yoksa klasik Segoe UI.
pub fn ui_font() -> &'static str {
    if has_ui_variable() { "Segoe UI Variable Text" } else { "Segoe UI" }
}

/// `icon_font()` için PCWSTR (statik, null sonlu).
pub fn icon_font_w() -> PCWSTR {
    if has_fluent_icons() { w!("Segoe Fluent Icons") } else { w!("Segoe MDL2 Assets") }
}

/// `ui_font()` için PCWSTR (statik, null sonlu).
pub fn ui_font_w() -> PCWSTR {
    if has_ui_variable() { w!("Segoe UI Variable Text") } else { w!("Segoe UI") }
}

/// Yalnızca Fluent Icons'ta bulunan birkaç glifi MDL2'deki karşılığına çevirir.
/// Fluent Icons varsa glif aynen döner. CarlUI'nin kullandığı E7xx–EDxx glifleri
/// iki yazı tipinde de aynı kod noktasındadır.
pub fn glyph(c: char) -> char {
    if has_fluent_icons() {
        return c;
    }
    match c as u32 {
        0xF3B1 => '\u{E748}', // Oturumu kapat → SwitchUser
        0xEEA0 => '\u{E950}', // RAM → Component
        0xF2B7 => '\u{E8C1}', // Çeviri → Character
        0xF012 => '\u{E7B8}', // Arşiv → Package
        _ => c,
    }
}

/// Bir dizgideki tüm simge karakterlerine `glyph` uygular.
pub fn glyphs(s: &str) -> std::borrow::Cow<'_, str> {
    if has_fluent_icons() || !s.chars().any(|c| glyph(c) != c) {
        std::borrow::Cow::Borrowed(s)
    } else {
        std::borrow::Cow::Owned(s.chars().map(glyph).collect())
    }
}
