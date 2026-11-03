//! Çizim: ortak `carlui_ui::paint` parçaları, görev yöneticisinin eylemleriyle
//! (`Act`) bağlanır; ayrıca bayt/hız biçimleri.

pub use carlui_ui::paint::{Align, Fonts, R, GREEN, ORANGE, RED, TEXT};

pub type Painter<'a> = carlui_ui::paint::Painter<'a, crate::Act>;

/// Bayt → "1,2 GB".
pub fn bytes(b: f64) -> String {
    carlui_sys::bytes(b)
}

/// Bayt/sn → "3,4 MB/sn" (0 ise "0").
pub fn speed(b: f64) -> String {
    if b < 1024.0 { "0".into() } else { format!("{}/sn", carlui_sys::bytes(b)) }
}