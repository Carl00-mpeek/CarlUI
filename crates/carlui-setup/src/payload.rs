//! Bu modül "tek dosyalık kurucu, kurulacak her şeyi yanında taşısın" sorununu
//! derleme anında gömülen baytlarla çözüyor: build.rs, target\release'teki exe
//! dosyalarını ve tarayıcı eklentisini `include_bytes!` ile buraya koyar.
//! Kurucu yalnızca bu dizileri diske yazar; ağdan hiçbir şey indirmez.

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

/// Kurulumun diskte kaplayacağı yer (KB, Programlar listesinde görünür).
pub fn size_kb() -> u32 {
    let n: usize = FILES.iter().chain(EXTENSION.iter()).map(|(_, b)| b.len()).sum::<usize>() + XPI.map_or(0, |x| x.len());
    (n / 1024) as u32
}
