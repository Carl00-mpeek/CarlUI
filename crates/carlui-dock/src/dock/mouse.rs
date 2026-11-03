//! Dock'ta fare: hover büyümesi, isabet testi, bas/bırak, sürükleyerek sıralama
//! ve sabitleme (sıra `config.toml`'a yazılır).

use super::*;

impl Dock {
    // ---- fare ---------------------------------------------------------------

    pub(super) fn animate_item(&self, i: usize, scale: f32, highlight: f32) -> Result<()> {
        let e = &self.entries[i];
        self.motion.scale(&e.icon, scale, 0.5, 42, 0)?;
        self.motion.animate(&e.hl, h!("Opacity"), highlight, 1.0, 35, 0)?;
        Ok(())
    }

    pub(super) fn set_hover(&mut self, idx: Option<usize>) -> Result<()> {
        if idx == self.hovered {
            return Ok(());
        }
        if let Some(old) = self.hovered.filter(|&o| o < self.entries.len()) {
            self.animate_item(old, 1.0, 0.0)?;
        }
        if let Some(new) = idx {
            self.animate_item(new, 1.14, 1.0)?;
        }
        self.hovered = idx;
        Ok(())
    }

    pub fn hovered(&self) -> Option<usize> {
        self.hovered.filter(|_| self.drag.is_none())
    }

    /// Basılı tutma/sürükleme sürerken öğe listesi yeniden kurulmamalı (indeksler kayar).
    pub fn is_interacting(&self) -> bool {
        self.press.is_some()
    }

    /// Basma/sürüklemeyi bırakmadan iptal et (bırakma mesajı hiç gelmediyse).
    pub fn cancel_press(&mut self) {
        let Some((i, _, _)) = self.press.take() else { return };
        if let Some(d) = self.drag.take() {
            if let Some(e) = self.entries.get(d.index) {
                let _ = self.motion.scale(&e.slot, 1.0, 0.7, 40, 0);
            }
        }
        if i < self.entries.len() {
            let _ = self.animate_item(i, 1.0, 0.0);
        }
    }

    pub(super) fn local_x(&self, x: i32) -> f32 {
        x as f32 - self.win_w / 2.0
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let lx = self.local_x(x);
        let ly = y as f32 - self.m.top;
        if !(0.0..=self.m.h).contains(&ly) || self.slid_away {
            return None;
        }
        self.entries.iter().position(|e| e.leaving.is_none() && (lx - e.x).abs() <= self.m.slot / 2.0)
    }

    /// Bir öğenin üstündeki ekran noktası (etiket/önizleme/menü konumu için).
    pub fn anchor(&self, i: usize) -> (i32, i32) {
        let x = self.mon.left + (self.win_w / 2.0 + self.entries[i].x) as i32;
        let y = self.mon.bottom - (self.m.bottom + self.m.h) as i32;
        (x, y)
    }

    /// Tray düğmesinin ekran dikdörtgeni ve dock'un üst kenarı (tray paneli balonu buradan çıkar).
    pub fn tray_button(&self, i: usize) -> (RECT, i32) {
        let (cx, cy) = self.icon_center(i);
        let half = (self.m.icon * 0.62) as i32;
        let r = RECT { left: cx - half, top: cy - half, right: cx + half, bottom: cy + half };
        (r, self.anchor(i).1)
    }

    /// Tray paneli açıkken düğmenin oku aşağı döner: yer boş kalmaz ve
    /// "buraya tıklayınca kapanır" der. Kapanınca yeniden yukarı bakar.
    pub fn set_tray_open(&self, open: bool) {
        if let Some(e) = self.entries.iter().find(|e| e.kind == Kind::Tray && e.leaving.is_none()) {
            let _ = self.motion.animate(&e.slot, h!("RotationAngleInDegrees"), if open { 180.0 } else { 0.0 }, 0.85, 70, 0);
        }
    }

    /// İkonun ekran üzerindeki merkezi (test ve tanı için).
    pub fn icon_center(&self, i: usize) -> (i32, i32) {
        let (x, top) = self.anchor(i);
        (x, top + (self.m.h / 2.0) as i32)
    }

    pub fn scale(&self) -> f32 {
        self.m.s
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_inside = true;
        if let Some((i, px, _)) = self.press {
            let draggable = self.entries[i].kind == Kind::App;
            if self.drag.is_none() && draggable && (x - px).abs() as f32 > 6.0 * self.m.s {
                let grab = self.local_x(px) - self.entries[i].x;
                self.drag = Some(Drag { index: i, grab, insert: None });
                let _ = self.motion.scale(&self.entries[i].slot, 1.12, 0.7, 40, 0);
                let _ = self.set_hover(None);
            }
            if self.drag.is_some() {
                let _ = self.drag_to(x);
            }
            return;
        }
        let _ = self.set_hover(self.hit(x, y));
    }

    pub fn mouse_leave(&mut self) {
        self.mouse_inside = false;
        if self.drag.is_none() {
            self.press = None;
            let _ = self.set_hover(None);
        }
    }

    pub fn mouse_down(&mut self, x: i32, y: i32) {
        if let Some(i) = self.hit(x, y) {
            self.press = Some((i, x, y));
            let _ = self.animate_item(i, 0.9, 1.0);
        }
    }

    /// Bırakınca tıklanan öğeyi döndürür (sürükleme değilse).
    pub fn mouse_up(&mut self, gfx: &Gfx, tracker: &Tracker, x: i32, y: i32) -> Option<usize> {
        let (p, _, _) = self.press.take()?;
        if let Some(drag) = self.drag.take() {
            let _ = self.motion.scale(&self.entries[drag.index].slot, 1.0, 0.7, 40, 0);
            self.finish_drag(gfx, tracker, drag);
            return None;
        }
        // Tıklamadan sonra vurgu söner (iz kalmasın); fare kımıldayınca hover yeniden başlar.
        let over = self.hit(x, y) == Some(p);
        let _ = self.animate_item(p, 1.0, 0.0);
        self.hovered = None;
        over.then_some(p)
    }

    // ---- sürükle-bırak ------------------------------------------------------

    /// Sabitlenmişler alanındaki öğe indeksleri, görsel sırayla.
    pub(super) fn pinned_indices(&self) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                e.leaving.is_none() && e.pin.is_some()
            })
            .collect()
    }

    /// Başlat/Ara düğmelerinden sonraki ilk konum (sabitleme alanının sol kenarı).
    fn last_glyph(&self, order: &[usize]) -> Option<usize> {
        order.iter().rposition(|&i| self.entries[i].group == 0)
    }

    pub(super) fn drag_to(&mut self, x: i32) -> Result<()> {
        let Some(drag) = self.drag.as_ref() else { return Ok(()) };
        let di = drag.index;
        let lx = self.local_x(x) - drag.grab;
        self.entries[di].slot.SetOffset(v3(lx - self.m.slot / 2.0, 0.0))?;

        // Hedef sıra: imlecin geçtiği sabitlenmiş öğe merkezlerine göre.
        let pinned: Vec<usize> = self.pinned_indices().into_iter().filter(|&i| i != di).collect();
        let first_pinned_left = self
            .entries
            .iter()
            .filter(|e| e.leaving.is_none() && e.group == 0)
            .map(|e| e.x + self.m.slot / 2.0)
            .reduce(f32::max)
            .unwrap_or(f32::MIN);
        let last_edge = pinned.last().map(|&i| self.entries[i].x).unwrap_or(first_pinned_left) + self.m.slot;
        let insert = if lx < first_pinned_left || lx > last_edge {
            None
        } else {
            Some(pinned.iter().filter(|&&i| self.entries[i].x < lx).count())
        };
        // Sabitlenmemiş bir öğe sabitleme alanına girmediyse kendi yerinde kalır.
        let from_running = self.entries[di].pin.is_none();
        if insert == self.drag.as_ref().and_then(|d| d.insert) && insert.is_some() {
            return Ok(());
        }
        if let Some(d) = self.drag.as_mut() {
            d.insert = insert;
        }

        let mut order: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].leaving.is_none() && i != di).collect();
        match insert {
            Some(k) => {
                let anchor = if k == 0 {
                    Some(self.last_glyph(&order).map_or(0, |p| p + 1))
                } else {
                    order.iter().position(|&i| i == pinned[k - 1]).map(|p| p + 1)
                };
                order.insert(anchor.unwrap_or(order.len()), di);
            }
            None if from_running => {
                let pos = (0..self.entries.len())
                    .filter(|&i| self.entries[i].leaving.is_none())
                    .position(|i| i == di)
                    .unwrap_or(order.len());
                order.insert(pos.min(order.len()), di);
            }
            None => {
                // Sabitlenmiş öğe alan dışına çekildi: kendi eski yerinde göster.
                let pos = (0..self.entries.len())
                    .filter(|&i| self.entries[i].leaving.is_none())
                    .position(|i| i == di)
                    .unwrap_or(order.len());
                order.insert(pos.min(order.len()), di);
            }
        }
        // Sürüklenen sabitlenmemiş öğe geçici olarak sabitlenmiş grubunda sayılsın.
        let saved_group = self.entries[di].group;
        if insert.is_some() {
            self.entries[di].group = 1;
        }
        self.relayout_order(&order, true)?;
        self.entries[di].group = saved_group;
        Ok(())
    }

    pub(super) fn finish_drag(&mut self, gfx: &Gfx, tracker: &Tracker, drag: Drag) {
        let di = drag.index;
        if let Some(k) = drag.insert {
            let mut pins: Vec<Pinned> = self.cfg.dock.pinned.clone();
            let pin = match &self.entries[di].pin {
                Some(p) => {
                    let key = pin_key(p);
                    pins.retain(|q| pin_key(q) != key);
                    p.clone()
                }
                None => self.pin_from_entry(di),
            };
            pins.insert(k.min(pins.len()), pin);
            self.cfg.dock.pinned = pins;
            carlui_core::config::save(&self.cfg);
            carlui_core::info!("dock sırası güncellendi");
        }
        let _ = self.sync(gfx, tracker, true);
    }

    pub(super) fn pin_from_entry(&self, i: usize) -> Pinned {
        let e = &self.entries[i];
        let path = if !e.app.aumid.is_empty() && e.exe.is_empty() {
            format!("shell:AppsFolder\\{}", e.app.aumid)
        } else {
            e.exe.clone()
        };
        Pinned { name: e.name.clone(), path, args: String::new(), icon: String::new(), aumid: e.app.aumid.clone(), exe: e.exe.clone() }
    }

    /// Sabitle / sabitlemeyi kaldır (menüden).
    pub fn toggle_pin(&mut self, gfx: &Gfx, tracker: &Tracker, i: usize) {
        let mut pins: Vec<Pinned> = self.cfg.dock.pinned.clone();
        match &self.entries[i].pin {
            Some(p) => {
                let key = pin_key(p);
                pins.retain(|q| pin_key(q) != key);
            }
            None => pins.push(self.pin_from_entry(i)),
        }
        self.cfg.dock.pinned = pins;
        carlui_core::config::save(&self.cfg);
        let _ = self.sync(gfx, tracker, true);
    }
}
