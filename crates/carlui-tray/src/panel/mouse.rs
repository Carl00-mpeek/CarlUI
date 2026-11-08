//! Panelde fare: isabet testi, ikon hover'ı, kutucuk/kaydırıcı/ikon tıklamaları
//! ve panel açıkken içeriğin yenilenmesi.

use super::*;

impl Panel {
    pub(super) fn hit(&self, x: i32, y: i32) -> Hit {
        if contains(&self.button, x, y) {
            return Hit::Button;
        }
        let lx = x - self.panel.left;
        let ly = y - self.panel.top;
        let speaker = |sl: &Slider| lx < sl.hit.left && ly >= sl.hit.top && ly < sl.hit.bottom;
        if let Some(t) = &self.wifi {
            if contains(&t.rect, lx, ly) {
                return Hit::Wifi;
            }
        }
        if let Some(t) = &self.bt {
            if contains(&t.rect, lx, ly) {
                return Hit::Bluetooth;
            }
        }
        if self.audio_btn.is_some_and(|r| contains(&r, lx, ly)) {
            return Hit::AudioBtn;
        }
        if let Some(i) = self.device_rows.iter().position(|r| contains(r, lx, ly)) {
            return Hit::Device(i);
        }
        if let Some(i) = self.app_icons.iter().position(|(r, _)| contains(r, lx, ly)) {
            return Hit::AppIcon(i);
        }
        if let Some(i) = self.app_sliders.iter().position(|sl| contains(&sl.hit, lx, ly)) {
            return Hit::AppVol(i);
        }
        if let Some(sl) = &self.vol {
            if contains(&sl.hit, lx, ly) {
                return Hit::Volume;
            }
            if speaker(sl) {
                return Hit::VolumeIcon;
            }
        }
        if let Some(sl) = &self.bright {
            if contains(&sl.hit, lx, ly) {
                return Hit::Bright;
            }
        }
        self.cells.iter().position(|c| contains(&c.rect, lx, ly)).map(Hit::Cell).unwrap_or(Hit::None)
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        if self.state != State::Open {
            return;
        }
        if matches!(self.dragging, Hit::Volume | Hit::Bright | Hit::AppVol(_)) {
            self.set_slider_from_x(self.dragging, x);
            return;
        }
        let idx = match self.hit(x, y) {
            Hit::Cell(i) => Some(i),
            _ => None,
        };
        if idx != self.hovered {
            if let Some(c) = self.hovered.and_then(|i| self.cells.get(i)) {
                let _ = self.motion.animate(&c.hl, h!("Opacity"), 0.0, 1.0, 35, 0);
            }
            if let Some(c) = idx.and_then(|i| self.cells.get(i)) {
                let _ = self.motion.animate(&c.hl, h!("Opacity"), 1.0, 1.0, 35, 0);
            }
            self.hovered = idx;
        }
    }

    pub fn mouse_leave(&mut self) {
        if self.dragging == Hit::None {
            self.mouse_move(-10000, -10000);
        }
    }

    /// Dönüş: true ise panel kapatılmalı (düğmeye tıklandı).
    pub fn mouse_down(&mut self, x: i32, y: i32) -> bool {
        let h = self.hit(x, y);
        self.pressed = h;
        match h {
            Hit::Button => return true,
            Hit::Volume | Hit::Bright | Hit::AppVol(_) => {
                self.dragging = h;
                unsafe {
                    SetCapture(self.hwnd);
                }
                self.set_slider_from_x(h, x);
            }
            Hit::Cell(i) => {
                if let Some(c) = self.cells.get(i) {
                    let _ = self.motion.scale(&c.hl, 0.94, 0.6, 35, 0);
                }
            }
            _ => {}
        }
        false
    }

    /// Dönüş: tıklanan tepsi ikonunun indeksi (sol tık).
    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<usize> {
        let pressed = std::mem::replace(&mut self.pressed, Hit::None);
        if self.dragging != Hit::None {
            unsafe {
                let _ = ReleaseCapture();
            }
            if self.dragging == Hit::Bright {
                self.apply_brightness_now();
            }
            self.dragging = Hit::None;
            return None;
        }
        let h = self.hit(x, y);
        if let Hit::Cell(i) = pressed {
            if let Some(c) = self.cells.get(i) {
                let _ = self.motion.scale(&c.hl, 1.0, 0.6, 35, 0);
            }
        }
        if h != pressed {
            return None;
        }
        match h {
            Hit::Wifi if self.radios.ethernet_shown() => {
                quick::to_wireless(self.radios, self.hwnd.0 as isize, WM_APP_RADIOS);
                // Açılan Windows ağ listesi panelin altında kalmasın: panel kapanır.
                unsafe {
                    let _ = PostMessageW(self.hwnd, WM_APP_OUTSIDE, WPARAM(0), LPARAM(0));
                }
            }
            Hit::Wifi => {
                let on = self.radios.wifi != Some(true);
                quick::set_radio(false, on, self.hwnd.0 as isize, WM_APP_RADIOS);
            }
            Hit::Bluetooth => {
                let on = self.radios.bluetooth != Some(true);
                quick::set_radio(true, on, self.hwnd.0 as isize, WM_APP_RADIOS);
            }
            Hit::VolumeIcon => {
                if let Some(v) = &self.volume {
                    v.toggle_mute();
                }
            }
            Hit::AudioBtn => self.toggle_mixer(),
            Hit::Device(i) => self.pick_device(i),
            Hit::AppIcon(i) => {
                if let Some(a) = self.apps.get_mut(i) {
                    a.toggle_mute();
                    let v = if a.muted { 0.0 } else { a.volume };
                    if let Some(sl) = self.app_sliders.get_mut(i) {
                        Self::apply_slider(sl, v);
                    }
                }
                self.set_app_muted_look(i);
            }
            Hit::Cell(i) => return self.cells.get(i).map(|c| c.icon),
            _ => {}
        }
        None
    }

    pub fn right_click(&self, x: i32, y: i32) -> Option<usize> {
        match self.hit(x, y) {
            Hit::Cell(i) => self.cells.get(i).map(|c| c.icon),
            _ => None,
        }
    }

    /// Panel açıkken ikon listesi ya da parlaklık desteği değişti: paneli
    /// (yüksekliği değişebileceği için pencere ve bölgesiyle birlikte) animasyonsuz yeniden kur.
    pub fn refresh(&mut self, gfx: &Gfx, host: &mut Host) {
        if self.state != State::Open {
            return;
        }
        // Açılış animasyonu sürerken yeniden kurmak animasyonu keser; bitince uygula.
        if self.animating {
            self.refresh_due = true;
            return;
        }
        self.refresh_due = false;
        if let Some((button, top)) = self.last_open {
            let _ = self.open_with(gfx, host, button, top, false);
        }
    }
}
