//! Pano (clipboard) mantığı: panodan okuma, geçmişi saklama, geri yazma ve
//! metin dönüşümleri. Arayüz adada (`carlui-island`) çizilir; bu kütüphane
//! pencereden bağımsızdır ve tek başına test edilebilir.

pub mod capture;
pub mod image;
pub mod secret;
pub mod store;
pub mod transform;
pub mod write;
