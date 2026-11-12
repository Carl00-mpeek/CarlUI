//! Canlı dock'un dock'tan okuduğu her şey tek yerde (salt okunur görünüm).
//! Dock'un yerleşimi, isabet testi ve boyutu bu modülden etkilenmez.

use super::Dock;
use carlui_core::{config::DockPosition, theme::Theme};
use windows::{
    UI::Composition::ContainerVisual,
    Win32::Foundation::{HWND, RECT},
};

pub struct LiveView {
    pub hwnd: HWND,
    /// Camın içerik katmanı (camın yuvarlak kırpması altında).
    pub content: ContainerVisual,
    /// Pill'in ekran dikdörtgeni (hedef genişlikle).
    pub pill: RECT,
    /// Camın piksel boyutu (yatayda genişlik x yükseklik).
    pub size: (f32, f32),
    pub scale: f32,
    pub side: DockPosition,
    pub theme: Theme,
    pub light: bool,
    /// Pencere tamamen gizli (tam ekran bastırması) ya da kenara kaymış.
    pub suppressed: bool,
    pub slid_away: bool,
    /// Dock'un monitörü (ekran koordinatı).
    pub mon: RECT,
    /// Ek ekran dock'u değil.
    pub primary: bool,
}

impl Dock {
    pub fn live_view(&self) -> LiveView {
        LiveView {
            hwnd: self.hwnd,
            content: self.glass.content.clone(),
            pill: self.pill_rect(self.width),
            size: self.live_size(),
            scale: self.m.s,
            side: self.side,
            theme: self.theme,
            light: self.cfg.theme.is_light(),
            suppressed: self.suppressed,
            slid_away: self.slid_away,
            mon: self.mon,
            primary: crate::screens::child().is_none(),
        }
    }

    /// Yalnızca duraklatma için gereken ucuz sinyaller (her mesajda okunur).
    pub fn live_hidden(&self) -> (bool, bool) {
        (self.suppressed, self.slid_away)
    }

    /// Camın şimdiki piksel boyutu (dikeyde eksenler yer değiştirir).
    pub fn live_size(&self) -> (f32, f32) {
        let (a, c) = (self.width, self.m.h);
        if self.vert() {
            (c, a)
        } else {
            (a, c)
        }
    }

    pub fn live_view_scale(&self) -> f32 {
        self.m.s
    }

    /// Pill'in şimdiki genişliği ve ekran dikdörtgeni (pet yürüyüş aralığı).
    pub fn live_pill(&self) -> RECT {
        self.pill_rect(self.width)
    }
}
