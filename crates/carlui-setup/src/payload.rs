//! Bu modül "tek dosyalık kurucu, kurulacak her şeyi yanında taşısın ama
//! indirmesi küçük olsun" sorununu çözüyor: build.rs exe'leri, eklentiyi,
//! duvar kâğıdını ve küçük kaldırıcıyı tek bir zstd akışı olarak sıkıştırıp
//! buraya gömer; burada yalnızca adlar ve akış içindeki yerleri durur. Akış
//! ilk gerektiğinde bir kez belleğe açılır. Kurucu ağdan hiçbir şey indirmez.
//! Seçeneklerin diskte ne kadar yer tutacağı da (sıkıştırılmamış boyut)
//! buradan hesaplanır.

use crate::{install::Options, office};
use std::{io::Read, sync::OnceLock};

/// Akış içindeki bir dosya.
pub struct Entry {
    pub name: &'static str,
    off: usize,
    pub len: usize,
}

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

/// Kurucuda dosya gömülü mü (kaldırıcı derlemesinde değil).
pub fn embedded() -> bool {
    !PACKED.is_empty()
}

static RAW: OnceLock<Vec<u8>> = OnceLock::new();

/// Sıkıştırılmış akışı açar (ilk çağrıda; ~0,5 sn). Bozuksa boş döner.
pub fn unpack() -> &'static [u8] {
    RAW.get_or_init(|| {
        let mut out = Vec::with_capacity(RAW_LEN);
        if PACKED.is_empty() {
            return out;
        }
        let ok = ruzstd::decoding::StreamingDecoder::new(PACKED).is_ok_and(|mut d| d.read_to_end(&mut out).is_ok());
        if !ok || out.len() != RAW_LEN {
            out.clear();
        }
        out
    })
}

impl Entry {
    /// Dosyanın içeriği; akış açılamadıysa boş.
    pub fn bytes(&self) -> &'static [u8] {
        unpack().get(self.off..self.off + self.len).unwrap_or(&[])
    }
}

/// Carl Office seçilirse eklenen bayt.
pub fn office_bytes() -> u64 {
    FILES.iter().find(|e| e.name == office::EXE).map_or(0, |e| e.len as u64)
}

/// Duvar kâğıdı seçilirse eklenen bayt.
pub fn wallpaper_bytes() -> u64 {
    WALLPAPER.as_ref().map_or(0, |e| e.len as u64)
}

/// Kaldırıcının boyutu: gömülü küçük kaldırıcı, yoksa (geliştirme derlemesi)
/// kurucunun kendisi kopyalanır.
fn uninstaller_bytes() -> u64 {
    match &UNINSTALLER {
        Some(u) => u.len as u64,
        None => std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len()),
    }
}

/// Seçimlere göre kurulumun diskte kaplayacağı yer (bayt).
pub fn installed_bytes(o: &Options) -> u64 {
    let base: u64 = FILES.iter().chain(EXTENSION.iter()).chain(XPI.iter()).map(|e| e.len as u64).sum();
    let office = if o.office { 0 } else { office_bytes() };
    let wall = if o.wallpaper { wallpaper_bytes() } else { 0 };
    base - office + wall + uninstaller_bytes()
}

/// Programlar listesinde görünen boyut (KB).
pub fn size_kb(o: &Options) -> u32 {
    (installed_bytes(o) / 1024) as u32
}

/// "3,7 MB" (Türkçe virgüllü) ya da "3.7 MB".
pub fn mb(bytes: u64) -> String {
    let s = format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    if carlui_core::lang::is_tr() {
        s.replace('.', ",")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_matches_entries() {
        let raw = unpack();
        assert_eq!(raw.len(), RAW_LEN);
        for e in FILES.iter().chain(EXTENSION.iter()) {
            assert_eq!(e.bytes().len(), e.len, "{}", e.name);
        }
    }
}
