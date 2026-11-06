//! Panelin ses bölümü: ses kaydırıcısının yanındaki düğmeyle açılır. Üstte
//! çıkış aygıtları (tıklanan varsayılan olur), altta ses çalan uygulamalar
//! kendi kaydırıcılarıyla (ikona tıklamak o uygulamayı sessize alır).

use super::*;

impl Panel {
    /// Ses kaydırıcısı satırının sağındaki yuvarlak düğme (açıkken vurgulu, ok yukarı).
    pub(super) fn audio_button(&mut self, gfx: &Gfx, content: &ContainerVisual, y: f32) -> Result<()> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let bs = (28.0 * s).round();
        let x = self.width - pad - bs;
        let by = y + (((36.0 * s).round() - bs) / 2.0).round();
        let row = self.comp.CreateContainerVisual()?;
        row.SetOffset(v3(x, by))?;
        row.SetSize(v2(bs, bs))?;
        row.SetCenterPoint(Vector3 { X: bs / 2.0, Y: bs / 2.0, Z: 0.0 })?;
        let rk = row.Children()?;
        let brush: CompositionBrush = if self.audio_open {
            self.accent_brush(true)?
        } else {
            self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x18)))?.cast()?
        };
        let (bg, _) = self.rrect(bs, bs, bs / 2.0, &brush)?;
        rk.InsertAtTop(&bg)?;
        let g = carlui_ui::tone::literal_if(self.audio_open, || self.text.glyph(gfx, if self.audio_open { '\u{E70E}' } else { '\u{E70D}' }, 11.0 * s, 0.95))?;
        rk.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (bs - g.width) / 2.0, (bs - g.height) / 2.0)?)?;
        content.Children()?.InsertAtTop(&row)?;
        self.audio_btn = Some(rect(x, by, bs, bs));
        Ok(())
    }

    fn label(&self, gfx: &Gfx, kids: &VisualCollection, text: &str, y: f32) -> Result<f32> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let t = self.text.render(gfx, text, 11.5 * s, true, self.width - pad * 2.0, 0.55)?;
        kids.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, pad + 4.0 * s, y)?)?;
        Ok(t.height + (6.0 * s).round())
    }

    /// Bölümü `y`'den itibaren kurar; bittiği yeri döndürür.
    pub(super) fn build_mixer(&mut self, gfx: &Gfx, content: &ContainerVisual, mut y: f32) -> Result<f32> {
        let s = self.s;
        let w = self.width;
        let pad = (16.0 * s).round();
        let rs = self.theme.radius_scale;
        let kids = content.Children()?;
        y += (6.0 * s).round();

        // Çıkış aygıtları.
        y += self.label(gfx, &kids, t!("Çıkış aygıtı", "Output device"), y)?;
        self.devices = audio::devices();
        let row_w = w - pad * 2.0;
        let row_h = (34.0 * s).round();
        let off: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x10)))?.cast()?;
        for d in &self.devices {
            let row = self.comp.CreateContainerVisual()?;
            row.SetOffset(v3(pad, y))?;
            row.SetSize(v2(row_w, row_h))?;
            let rk = row.Children()?;
            let (bg, _) = self.rrect(row_w, row_h, 10.0 * s * rs, &off)?;
            rk.InsertAtTop(&bg)?;
            if d.default {
                let (on, _) = self.rrect(row_w, row_h, 10.0 * s * rs, &self.accent_brush(true)?)?;
                on.SetOpacity(if carlui_ui::tone::light() { 0.2 } else { 0.45 })?;
                rk.InsertAtTop(&on)?;
            }
            let g = self.text.glyph(gfx, '\u{E7F5}', 14.0 * s, if d.default { 0.95 } else { 0.7 })?;
            rk.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, 12.0 * s, (row_h - g.height) / 2.0)?)?;
            let t = self.text.render(gfx, &d.name, 12.5 * s, d.default, row_w - 72.0 * s, if d.default { 0.95 } else { 0.8 })?;
            rk.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, 38.0 * s, (row_h - t.height) / 2.0)?)?;
            if d.default {
                let c = self.text.glyph(gfx, '\u{E73E}', 12.0 * s, 0.95)?;
                rk.InsertAtTop(&self.sprite(&c.surface, c.width, c.height, row_w - 12.0 * s - c.width, (row_h - c.height) / 2.0)?)?;
            }
            kids.InsertAtTop(&row)?;
            self.device_rows.push(rect(pad, y, row_w, row_h));
            y += row_h + (4.0 * s).round();
        }
        if self.devices.is_empty() {
            y += self.label(gfx, &kids, t!("Etkin çıkış aygıtı bulunamadı", "No active output device"), y)?;
        }
        y += (8.0 * s).round();

        // Uygulamalar: simge, ad ve kendi kaydırıcısı.
        y += self.label(gfx, &kids, t!("Uygulamalar", "Apps"), y)?;
        self.apps = audio::apps(8);
        if self.apps.is_empty() {
            y += self.label(gfx, &kids, t!("Şu an ses çıkaran uygulama yok", "No apps are playing sound"), y)?;
        }
        let app_h = (44.0 * s).round();
        let px = (20.0 * s).round();
        let x0 = pad + 36.0 * s;
        let tw = w - x0 - pad - 8.0 * s;
        for i in 0..self.apps.len() {
            let row = self.comp.CreateContainerVisual()?;
            row.SetOffset(v3(0.0, y))?;
            let rk = row.Children()?;
            let icon_y = ((app_h - px) / 2.0).round();
            let icon = if self.apps[i].exe.is_empty() {
                let g = self.text.glyph(gfx, '\u{E770}', 16.0 * s, 0.9)?;
                Some(self.sprite(&g.surface, g.width, g.height, pad + 2.0 * s, (app_h - g.height) / 2.0)?)
            } else {
                icons::shell_icon(&gfx.wic, &self.apps[i].exe, px as i32)
                    .ok()
                    .and_then(|src| gfx.surface(px, px, |dc| icons::draw_bitmap(dc, &src, px)).ok())
                    .map(|surf| self.sprite(&surf, px, px, pad + 2.0 * s, icon_y))
                    .transpose()?
            };
            if let Some(v) = &icon {
                rk.InsertAtTop(v)?;
            }
            let t = self.text.render(gfx, &self.apps[i].name, 11.5 * s, false, tw, 0.8)?;
            rk.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, x0, (4.0 * s).round())?)?;
            let a = &self.apps[i];
            let value = if a.muted { 0.0 } else { a.volume };
            let sl = self.track(&row, x0, tw, (28.0 * s).round(), y, app_h, value)?;
            kids.InsertAtTop(&row)?;
            self.app_sliders.push(sl);
            self.app_icons.push((rect(pad - 4.0 * s, y, 36.0 * s, app_h), icon));
            self.set_app_muted_look(i);
            y += app_h;
        }
        Ok(y + (4.0 * s).round())
    }

    /// Sessizdeki uygulamanın simgesi soluk görünür.
    pub(super) fn set_app_muted_look(&self, i: usize) {
        let muted = self.apps.get(i).is_some_and(|a| a.muted || a.volume == 0.0);
        if let Some((_, Some(v))) = self.app_icons.get(i) {
            let _ = self.motion.fade(v, h!("Opacity"), if muted { 0.35 } else { 1.0 }, 140, 0);
        }
    }

    /// Bölümü aç/kapat: panel yeni yüksekliğine uzayarak yeniden kurulur.
    pub(super) fn toggle_mixer(&mut self) {
        self.audio_open = !self.audio_open;
        self.grow_from = Some(self.height);
        self.rebuild = true;
    }

    /// Aygıtı varsayılan yap; ana ses kaydırıcısı artık o aygıtı yönetir.
    pub(super) fn pick_device(&mut self, i: usize) {
        let Some(d) = self.devices.get(i) else { return };
        if d.default {
            return;
        }
        if audio::set_default(&d.id) {
            carlui_core::info!("çıkış aygıtı: {}", d.name);
        } else {
            carlui_core::warn!("çıkış aygıtı değiştirilemedi: {}", d.name);
        }
        self.volume = Volume::new();
        self.grow_from = Some(self.height);
        self.rebuild = true;
    }
}
