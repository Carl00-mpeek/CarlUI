//! Bu modül "tek dosyalık kurucu, kurulacak her şeyi yanında taşısın" sorununu
//! derleme anında gömülen baytlarla çözüyor: build.rs, target\release'teki exe
//! dosyalarını ve tarayıcı eklentisini `include_bytes!` ile buraya koyar.
//! Kurucu yalnızca bu dizileri diske yazar; ağdan hiçbir şey indirmez.
//! Seçeneklerin diskte ne kadar yer tutacağı da buradan hesaplanır.

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

use crate::{install::Options, office};

/// Carl Office seçilirse eklenen bayt.
pub fn office_bytes() -> u64 {
    FILES.iter().find(|(n, _)| *n == office::EXE).map_or(0, |(_, b)| b.len() as u64)
}

/// Duvar kâğıdı seçilirse eklenen bayt.
pub fn wallpaper_bytes() -> u64 {
    WALLPAPER.len() as u64
}

/// Kaldırıcı kurucunun kopyasıdır; boyutu kurucunun kendisi kadar.
fn uninstaller_bytes() -> u64 {
    std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len())
}

/// Seçimlere göre kurulumun diskte kaplayacağı yer (bayt).
pub fn installed_bytes(o: &Options) -> u64 {
    let base: u64 = FILES.iter().chain(EXTENSION.iter()).map(|(_, b)| b.len() as u64).sum::<u64>() + XPI.map_or(0, |x| x.len() as u64);
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
