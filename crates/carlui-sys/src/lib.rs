//! Bu modül "ada ve görev yöneticisi aynı donanım bilgisini ayrı ayrı, farklı
//! yollarla okumasın" sorununu tek bir kütüphaneyle çözüyor. Her kaynak kendi
//! dosyasında, hepsi yönetici gerektirmeden çalışır (gerektiren tek şey CPU
//! sıcaklığıdır; bkz. `temp`):
//! - `cpu`: çekirdek başına kullanım (çekirdekten doğrudan sayaçlar) ve anlık GHz.
//! - `gpu`: NVIDIA sürücüsünün kendi kütüphanesi (nvml.dll): sıcaklık, yük, VRAM.
//! - `ram`: fiziksel bellek ve kayıt (commit) kullanımı.
//! - `disk`: disk başına etkinlik, okuma/yazma hızı, NVMe sağlık kaydı ve sıcaklık.
//! - `net`: fiziksel ağ bağdaştırıcılarından alınan toplam bayt.
//! - `process`: süreç listesi, durdur/sürdür/sonlandır.
//! - `temp`: CPU paket ve çekirdek sıcaklıkları (PawnIO sürücüsü kuruluysa).

#[macro_use]
extern crate carlui_core;

pub mod bt;
pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod gpu_procs;
pub mod net;
pub mod privacy;
pub mod process;
pub mod ram;
pub mod temp;

/// Sayıyı seçili dilin ondalık işaretiyle yazar ("12,3" / "12.3").
pub fn tr(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if carlui_core::lang::is_tr() { s.replace('.', ",") } else { s }
}

/// Bayt → "1,2 GB" / "340 MB".
pub fn bytes(b: f64) -> String {
    const K: f64 = 1024.0;
    if b >= K * K * K * K {
        format!("{} TB", tr(b / (K * K * K * K), 1))
    } else if b >= K * K * K {
        format!("{} GB", tr(b / (K * K * K), 1))
    } else if b >= K * K {
        format!("{} MB", tr(b / (K * K), 0))
    } else {
        format!("{} KB", tr(b / K, 0))
    }
}
