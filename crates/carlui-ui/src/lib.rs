//! Yerel (native) yüzeylerin ortak görsel parçaları: GPU altyapısı, cam
//! yüzey, cam menü, spring animasyonları ve ikonlar. Dock, tray paneli,
//! bildirimler, görev yöneticisi ve ayarlar aynı parçaları kullanır; böylece görünüm ve
//! his her yerde aynı kalır.

pub mod gfx;
pub mod glass;
pub mod glassgrid;
pub mod glassmenu;
pub mod icons;
pub mod motion;
pub mod paint;
pub mod text;
pub mod textarea;
pub mod tone;
pub mod tween;
pub mod view;
