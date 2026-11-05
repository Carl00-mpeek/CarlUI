//! Bu crate "ücretli bulut hizmetlerinin yaptığı işleri bilgisayarda, ücretsiz
//! nasıl yaparız?" sorusunu çözüyor. Modeller kurucuda gelmez; kullanıcı
//! Ayarlar → Yapay zekâ'dan küçük/orta/büyük seçip resmi kaynaktan indirir
//! (`catalog`, `store`). Modelleri ONNX Runtime ile çalıştırırız (`runtime`);
//! resmi ağırlıklar ONNX değilse grafiği kendimiz kurarız (`onnx`, `weights`).

#[macro_use]
extern crate carlui_core;

pub mod audio;
pub mod catalog;
pub mod command;
pub mod esrgan;
pub mod g2p_en;
pub mod g2p_tr;
pub mod hw;
pub mod listen;
pub mod marian;
pub mod mic;
pub mod net;
mod nn;
pub mod onnx;
pub mod rmbg;
pub mod runtime;
pub mod spm;
pub mod store;
pub mod tts;
pub mod vosk;
pub mod weights;
pub mod whisper;

/// Düz (premultiplied olmayan) BGRA resim, satırlar yukarıdan aşağı.
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}
