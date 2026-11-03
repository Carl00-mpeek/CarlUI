//! Panelin içeriği: Wi-Fi/Bluetooth kutucukları, ses ve parlaklık kaydırıcıları,
//! tepsi ikonlarının ızgarası.

use super::*;

impl Panel {
    /// Panelin içeriğini (hızlı ayarlar + ikon ızgarası) kurar; yüksekliği döndürür.
    pub(super) fn build_content(&mut self, gfx: &Gfx, host: &mut Host, content: &ContainerVisual) -> Result<f32> {
        let s = self.s;
        let w = self.width;
        let pad = (16.0 * s).round();
        let gap = (8.0 * s).round();
        let rs = self.theme.radius_scale;
        let kids = content.Children()?;
        self.stagger.clear();
        self.cells.clear();
        let off_brush: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x14)))?.cast()?;
        let mut y = pad;

        // Hızlı ayar kutucukları: Wi-Fi, Bluetooth.
        let tile_w = ((w - pad * 2.0 - gap) / 2.0).floor();
        let tile_h = (48.0 * s).round();
        let mut tiles = Vec::new();
        for (n, (glyph, label)) in [('\u{E701}', "Wi-Fi"), ('\u{E702}', "Bluetooth")].iter().enumerate() {
            let x = pad + n as f32 * (tile_w + gap);
            let row = self.comp.CreateContainerVisual()?;
            row.SetOffset(v3(x, y))?;
            row.SetSize(v2(tile_w, tile_h))?;
            let (bg, _) = self.rrect(tile_w, tile_h, 14.0 * s * rs, &off_brush)?;
            let (on, _) = self.rrect(tile_w, tile_h, 14.0 * s * rs, &self.accent_brush(true)?)?;
            on.SetOpacity(0.0)?;
            let g = self.text.glyph(gfx, *glyph, 16.0 * s, 0.95)?;
            let l = self.text.render(gfx, label, 13.0 * s, true, tile_w - 50.0 * s, 0.95)?;
            let rk = row.Children()?;
            rk.InsertAtTop(&bg)?;
            rk.InsertAtTop(&on)?;
            rk.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, 14.0 * s, (tile_h - g.height) / 2.0)?)?;
            rk.InsertAtTop(&self.sprite(&l.surface, l.width, l.height, 42.0 * s, (tile_h - l.height) / 2.0)?)?;
            kids.InsertAtTop(&row)?;
            self.stagger.push(row.cast()?);
            tiles.push(Tile { rect: rect(x, y, tile_w, tile_h), on_layer: on });
        }
        self.bt = tiles.pop();
        self.wifi = tiles.pop();
        y += tile_h + (12.0 * s).round();

        // Kaydırıcılar: ses ve (destekleniyorsa) parlaklık.
        let (vol_value, muted) = self.volume.as_ref().map(|v| v.get()).unwrap_or((0.0, false));
        let vol_glyph = if muted || vol_value == 0.0 { '\u{E74F}' } else { '\u{E767}' };
        self.vol = Some(self.slider(gfx, content, vol_glyph, y, if muted { 0.0 } else { vol_value }, true)?);
        self.audio_button(gfx, content, y)?;
        y += (36.0 * s).round();
        self.bright = None;
        if let Some(b) = self.brightness {
            self.bright = Some(self.slider(gfx, content, '\u{E706}', y, b, false)?);
            y += (36.0 * s).round();
        }
        self.devices.clear();
        self.device_rows.clear();
        self.apps.clear();
        self.app_sliders.clear();
        self.app_icons.clear();
        if self.audio_open {
            y = self.build_mixer(gfx, content, y)?;
        }
        y += (6.0 * s).round();

        let sep = self.comp.CreateSpriteVisual()?;
        sep.SetBrush(&self.comp.CreateColorBrushWithColor(color(self.theme.separator))?)?;
        sep.SetSize(v2(w - pad * 2.0, 1.0))?;
        sep.SetOffset(v3(pad, y.round()))?;
        kids.InsertAtTop(&sep)?;
        self.stagger.push(sep.cast()?);
        y += (14.0 * s).round();

        // İkon ızgarası (4 sütun): ikon + kısa ad.
        let visible: Vec<usize> = (0..host.icons.len()).filter(|&i| !host.icons[i].hidden).collect();
        let cols = 4usize;
        let cell_w = ((w - pad * 2.0) / cols as f32).floor();
        let cell_h = (74.0 * s).round();
        let bg_px = (44.0 * s).round();
        let icon_px = (24.0 * s).round();
        let hover_brush: CompositionBrush = self.comp.CreateColorBrushWithColor(color(self.theme.hover))?.cast()?;
        let tile_brush: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x10)))?.cast()?;
        if visible.is_empty() {
            let t = self.text.render(gfx, t!("Arka planda çalışan uygulama yok", "No apps running in the background"), 12.5 * s, false, w - pad * 2.0, 0.6)?;
            let v = self.sprite(&t.surface, t.width, t.height, (w - t.width) / 2.0, y + 8.0 * s)?;
            kids.InsertAtTop(&v)?;
            self.stagger.push(v.cast()?);
            y += (40.0 * s).round();
        }
        for (n, &i) in visible.iter().enumerate() {
            let (c, r) = (n % cols, n / cols);
            let x = pad + c as f32 * cell_w;
            let cy = y + r as f32 * cell_h;
            let cell = self.comp.CreateContainerVisual()?;
            cell.SetOffset(v3(x, cy))?;
            cell.SetSize(v2(cell_w, cell_h))?;
            cell.SetCenterPoint(Vector3 { X: cell_w / 2.0, Y: bg_px / 2.0, Z: 0.0 })?;
            let ck = cell.Children()?;
            let (hl, _) = self.rrect(cell_w - 4.0 * s, cell_h - 2.0 * s, 14.0 * s * rs, &hover_brush)?;
            hl.SetOffset(v3(2.0 * s, 0.0))?;
            hl.SetOpacity(0.0)?;
            ck.InsertAtTop(&hl)?;
            let (tile, _) = self.rrect(bg_px, bg_px, 14.0 * s * rs, &tile_brush)?;
            tile.SetOffset(v3(((cell_w - bg_px) / 2.0).round(), (6.0 * s).round()))?;
            ck.InsertAtTop(&tile)?;
            if let Some(surf) = self.icon_surface(gfx, host, i, icon_px) {
                ck.InsertAtTop(&self.sprite(&surf, icon_px, icon_px, (cell_w - icon_px) / 2.0, 6.0 * s + (bg_px - icon_px) / 2.0)?)?;
            }
            let name = &host.icons[i].name;
            let t = self.text.render(gfx, name, 11.5 * s, false, cell_w - 6.0 * s, 0.85)?;
            ck.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, (cell_w - t.width) / 2.0, 6.0 * s + bg_px + 5.0 * s)?)?;
            kids.InsertAtTop(&cell)?;
            self.stagger.push(cell.cast()?);
            self.cells.push(Cell { rect: rect(x, cy, cell_w, cell_h), icon: i, hl });
        }
        y += cell_h * visible.len().div_ceil(cols) as f32;
        y += pad;
        Ok(y.round())
    }

    /// Simgeli kaydırıcı satırı. `button`: sağda ses bölümü düğmesine yer bırak.
    pub(super) fn slider(&self, gfx: &Gfx, content: &ContainerVisual, glyph: char, y: f32, value: f32, button: bool) -> Result<Slider> {
        let s = self.s;
        let w = self.width;
        let pad = (16.0 * s).round();
        let row_h = (36.0 * s).round();
        let row = self.comp.CreateContainerVisual()?;
        row.SetOffset(v3(0.0, y))?;
        let g = self.text.glyph(gfx, glyph, 16.0 * s, 0.9)?;
        row.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + 2.0 * s, (row_h - g.height) / 2.0)?)?;
        let x0 = pad + 36.0 * s;
        let tw = w - x0 - pad - 8.0 * s - if button { 34.0 * s } else { 0.0 };
        let th = (4.0 * s).round();
        let sl = self.track(&row, x0, tw, ((row_h - th) / 2.0).round(), y, row_h, value)?;
        content.Children()?.InsertAtTop(&row)?;
        Ok(sl)
    }

    /// Kaydırıcı rayı, dolgusu ve topuzu (`row` içinde, rayın üst kenarı `ty`).
    /// `y`/`row_h`: satırın panel içindeki yeri (isabet alanı için).
    pub(super) fn track(&self, row: &ContainerVisual, x0: f32, tw: f32, ty: f32, y: f32, row_h: f32, value: f32) -> Result<Slider> {
        let s = self.s;
        let rk = row.Children()?;
        let th = (4.0 * s).round();
        let track_brush: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x26)))?.cast()?;
        let (track, _) = self.rrect(tw, th, th / 2.0, &track_brush)?;
        track.SetOffset(v3(x0, ty))?;
        rk.InsertAtTop(&track)?;
        let (fill, fill_geom) = self.rrect(tw, th, th / 2.0, &self.accent_brush(true)?)?;
        fill.SetOffset(v3(x0, ty))?;
        rk.InsertAtTop(&fill)?;
        let knob_r = (8.0 * s).round();
        let knob_brush: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0xF2)))?.cast()?;
        let (knob, _) = self.rrect(knob_r * 2.0, knob_r * 2.0, knob_r, &knob_brush)?;
        rk.InsertAtTop(&knob)?;
        let mut sl = Slider { cy: ty + th / 2.0, hit: rect(x0 - knob_r, y, tw + knob_r * 2.0, row_h), x0, w: tw, fill, fill_geom, knob, knob_r, value };
        Self::apply_slider(&mut sl, value);
        Ok(sl)
    }

    pub(super) fn apply_slider(sl: &mut Slider, v: f32) {
        sl.value = v.clamp(0.0, 1.0);
        let fw = (sl.w * sl.value).max(0.01);
        let _ = sl.fill_geom.SetSize(v2(fw, sl.fill_geom.Size().map(|s| s.Y).unwrap_or(4.0)));
        let _ = sl.fill.SetSize(v2(fw, sl.fill_geom.Size().map(|s| s.Y).unwrap_or(4.0)));
        let _ = sl.knob.SetOffset(v3(sl.x0 + fw - sl.knob_r, sl.cy - sl.knob_r));
    }

    pub(super) fn set_slider_from_x(&mut self, which: Hit, x: i32) {
        let lx = (x - self.panel.left) as f32;
        let sl = match which {
            Hit::Volume => self.vol.as_mut(),
            Hit::Bright => self.bright.as_mut(),
            Hit::AppVol(i) => self.app_sliders.get_mut(i),
            _ => None,
        };
        let Some(sl) = sl else { return };
        let v = ((lx - sl.x0) / sl.w).clamp(0.0, 1.0);
        Self::apply_slider(sl, v);
        match which {
            Hit::Volume => {
                if let Some(vol) = &self.volume {
                    vol.set(v);
                }
            }
            Hit::AppVol(i) => {
                if let Some(a) = self.apps.get_mut(i) {
                    a.set_volume(v);
                }
                self.set_app_muted_look(i);
            }
            // DDC/CI yavaş: sürüklerken seyrek, bırakınca kesin uygula.
            Hit::Bright => unsafe {
                SetTimer(self.hwnd, TIMER_BRIGHT, 150, None);
            },
            _ => {}
        }
    }

    pub fn apply_brightness_now(&self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_BRIGHT);
        }
        if let Some(b) = &self.bright {
            let v = b.value;
            std::thread::spawn(move || quick::brightness_set(v));
        }
    }
}
