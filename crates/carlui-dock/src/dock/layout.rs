//! Dock dizilimi: slot konumları ve ayırıcılar (spring), tıklanabilir bölge,
//! pencere noktaları, açılış animasyonu, oyun/tam ekran gizlemesi ve otomatik
//! gizleme (intellihide).

use super::*;

impl Dock {
    /// Aktif (çıkmayan) öğelerin, verilen sırayla dizilmiş konumlarını hesaplar ve uygular.
    pub(super) fn relayout(&mut self, animate: bool) -> Result<()> {
        let order: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].leaving.is_none()).collect();
        self.relayout_order(&order, animate)
    }

    pub(super) fn relayout_order(&mut self, order: &[usize], animate: bool) -> Result<()> {
        let m = self.m;
        let mut x = m.pad;
        let mut last = None;
        let mut sep_xs = Vec::new();
        let mut centers = Vec::with_capacity(order.len());
        let separators = self.cfg.dock.separators;
        let taskbar = self.taskbar();
        // Görev çubuğunda tray düğmesi ortadaki dizilime girmez, sağ uçta durur.
        let mut tray = Vec::new();
        for &i in order {
            let g = self.entries[i].group;
            if taskbar && g == 3 {
                tray.push(i);
                continue;
            }
            if separators && last.is_some_and(|l| l != g) {
                sep_xs.push(x + m.sep / 2.0);
                x += m.sep;
            }
            centers.push(x + m.slot / 2.0);
            x += m.slot;
            last = Some(g);
        }
        let mut order = order.to_vec();
        let mut status_x = None;
        if taskbar && !self.taskbar_full() {
            // Compact: tray düğmesi ve durum alanı dizilimin sonuna eklenir,
            // bütün şerit ortada kalır.
            if separators && last.is_some() {
                sep_xs.push(x + m.sep / 2.0);
                x += m.sep;
            }
            for &i in &tray {
                centers.push(x + m.slot / 2.0);
                order.retain(|&o| o != i);
                order.push(i);
                x += m.slot;
            }
            x += (4.0 * m.s).round();
            status_x = Some(x);
            x += self.status_w();
        }
        let mut width = x + m.pad;
        let half = width / 2.0;
        if let (Some(sx), Some(st)) = (status_x, self.status.as_mut()) {
            st.at = sx - half;
        }
        if taskbar && self.taskbar_full() {
            // Ortadaki öğeler kendi genişliklerine göre ortalı kalır; cam tüm kenarı kaplar.
            // Sağdan sola: durum alanı, tray düğmesi, (varsa) ayırıcı.
            let mut right = self.status_left() + half;
            for &i in tray.iter().rev() {
                centers.push(right - m.slot / 2.0);
                order.retain(|&o| o != i);
                order.push(i);
                right -= m.slot;
            }
            if separators && !tray.is_empty() {
                sep_xs.push(right - m.sep / 2.0);
            }
            width = self.len;
        }
        let order = &order[..];
        let dragging = self.drag.as_ref().map(|d| d.index);
        let (vert, along) = (self.vert(), self.along_prop());

        for (&i, &c) in order.iter().zip(&centers) {
            let rel = c - half;
            let e = &mut self.entries[i];
            e.x = rel;
            if Some(i) == dragging {
                continue; // sürüklenen öğe imleci izler
            }
            if animate && !e.fresh {
                self.motion.animate(&e.slot, &along, rel - m.slot / 2.0, 0.78, 50, 0)?;
            } else {
                e.slot.SetOffset(if vert { v3(0.0, rel - m.slot / 2.0) } else { v3(rel - m.slot / 2.0, 0.0) })?;
            }
            if e.fresh {
                e.fresh = false;
                if animate {
                    e.slot.SetOpacity(0.0)?;
                    e.slot.SetScale(Vector3 { X: 0.3, Y: 0.3, Z: 1.0 })?;
                    self.motion.fade(&e.slot, h!("Opacity"), 1.0, 180, 60)?;
                    self.motion.scale(&e.slot, 1.0, 0.6, 50, 60)?;
                } else {
                    e.slot.SetOpacity(1.0)?;
                    e.slot.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
                }
            }
        }

        let sep_brush = self.comp.CreateColorBrushWithColor(carlui_ui::glass::exact(self.theme.separator))?;
        let sep_h = (24.0 * m.s).round();
        let sep_w = m.s.round().max(1.0);
        let sep_c = ((m.h - sep_h) / 2.0).round();
        while self.seps.len() < sep_xs.len() {
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&sep_brush)?;
            v.SetSize(self.ps(sep_w, sep_h))?;
            v.SetOffset(self.pv(sep_xs[self.seps.len()] - half, sep_c))?;
            self.items.Children()?.InsertAtBottom(&v)?;
            self.seps.push(v);
        }
        for (n, v) in self.seps.iter().enumerate() {
            match sep_xs.get(n) {
                Some(sx) => {
                    let to = (sx - half).round();
                    if animate {
                        self.motion.animate(v, &along, to, 0.78, 50, 0)?;
                        self.motion.fade(v, h!("Opacity"), 1.0, 150, 0)?;
                    } else {
                        v.SetOffset(self.pv(to, sep_c))?;
                        v.SetOpacity(1.0)?;
                    }
                }
                None => self.motion.fade(v, h!("Opacity"), 0.0, 120, 0)?,
            }
        }

        if let Some(st) = &self.status {
            let to = self.status_left();
            if animate && !st.full {
                self.motion.animate(&st.visual, &along, to, 0.78, 50, 0)?;
            } else {
                st.visual.SetOffset(v3(to, 0.0))?;
            }
        }
        if self.taskbar_full() {
            self.glass.root.SetSize(self.ps(width, m.h))?;
        } else if animate {
            self.motion.animate(&self.glass.root, &self.size_prop(), width, 0.78, 50, 0)?;
        } else {
            self.glass.root.SetSize(self.ps(width, m.h))?;
        }
        self.width = width;
        // Büyürken bölge hemen genişler; küçülürken animasyon bitince daralır.
        let grow = width.max(self.region_w);
        self.set_region(grow);
        unsafe {
            SetTimer(self.hwnd, TIMER_REGION, 650, None);
        }
        Ok(())
    }

    pub(super) fn set_region(&mut self, w: f32) {
        self.region_w = w;
        unsafe {
            let rgn = if self.slid_away {
                CreateRectRgn(0, 0, 0, 0)
            } else {
                let r = self.pill_rect(w);
                let (wx, wy) = self.wpos;
                CreateRectRgn(r.left - wx, r.top - wy, r.right - wx + 1, r.bottom - wy + 1)
            };
            SetWindowRgn(self.hwnd, rgn, true);
        }
    }

    pub fn fit_region(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_REGION);
        }
        self.set_region(self.width);
    }

    /// Pencere sayısına göre noktaları günceller; öndeki uygulamanın ilk noktası vurgulu haptır.
    pub fn update_dots(&mut self, foreground: HWND) -> Result<()> {
        let m = self.m;
        let (dw, aw, gap) = ((4.0 * m.s).round(), (14.0 * m.s).round(), (3.0 * m.s).round());
        let dh = (4.0 * m.s).round();
        let lines = !self.line_brushes.is_empty();
        for e in &mut self.entries {
            let n = e.windows.len().min(if lines { self.line_brushes.len() / 2 } else { 4 });
            let active = n > 0 && e.windows.contains(&foreground);
            if e.dots_sig == (n, active) {
                continue;
            }
            e.dots_sig = (n, active);
            let kids = e.dots.Children()?;
            kids.RemoveAll()?;
            let y = if self.side == DockPosition::Left { (7.0 * m.s - dh).round() } else { (m.h - 7.0 * m.s).round() };
            if lines {
                // Çizgi: boy sabit, pencere sayısınca bölünür; kapalı uygulamada yok.
                if n > 0 {
                    let brush = &self.line_brushes[(n - 1) * 2 + active as usize];
                    let (len, thick) = super::line_size(m.s, active);
                    let full = super::line_size(m.s, true).1;
                    // İnce çizgi tam kalınlıktakinin ortasına hizalanır.
                    let mid = ((full - thick) / 2.0).round();
                    let x = ((m.slot - len) / 2.0).round();
                    let y = if self.side == DockPosition::Left { (7.0 * m.s - full).round() + mid } else { (m.h - 6.0 * m.s).round() + mid };
                    let d = self.comp.CreateSpriteVisual()?;
                    d.SetSize(if self.side.vertical() { v2(thick, len) } else { v2(len, thick) })?;
                    d.SetBrush(brush)?;
                    d.SetOffset(if self.side.vertical() { v3(y, x) } else { v3(x, y) })?;
                    kids.InsertAtTop(&d)?;
                }
                e.dots.SetOpacity(0.0)?;
                self.motion.fade(&e.dots, h!("Opacity"), 1.0, 160, 0)?;
                continue;
            }
            let widths: Vec<f32> = (0..n).map(|i| if active && i == 0 { aw } else { dw }).collect();
            let total: f32 = widths.iter().sum::<f32>() + gap * n.saturating_sub(1) as f32;
            let mut x = ((m.slot - total) / 2.0).round();
            // Noktalar ekran kenarı tarafında (y yukarıda).
            for (i, w) in widths.iter().enumerate() {
                let d = self.comp.CreateSpriteVisual()?;
                d.SetSize(if self.side.vertical() { v2(dh, *w) } else { v2(*w, dh) })?;
                d.SetBrush(if active && i == 0 { &self.active_brush } else { &self.dot_brush })?;
                d.SetOffset(if self.side.vertical() { v3(y, x) } else { v3(x, y) })?;
                kids.InsertAtTop(&d)?;
                x += w + gap;
            }
            e.dots.SetOpacity(0.0)?;
            self.motion.fade(&e.dots, h!("Opacity"), 1.0, 160, 0)?;
        }
        Ok(())
    }

    /// Açılış: pill bir daireden genişler, ikonlar sırayla belirir.
    pub fn intro(&self) -> Result<()> {
        let mo = &self.motion;
        self.glass.root.SetSize(v2(self.m.h, self.m.h))?;
        mo.animate(&self.glass.root, &self.size_prop(), self.width, 0.72, 55, 80)?;
        self.glass.root.SetOpacity(0.0)?;
        mo.fade(&self.glass.root, h!("Opacity"), 1.0, 180, 0)?;
        for (i, e) in self.entries.iter().filter(|e| e.leaving.is_none()).enumerate() {
            let delay = 160 + i as i64 * 30;
            e.icon.SetOpacity(0.0)?;
            e.icon.SetScale(Vector3 { X: 0.4, Y: 0.4, Z: 1.0 })?;
            mo.fade(&e.icon, h!("Opacity"), 1.0, 220, delay)?;
            mo.scale(&e.icon, 1.0, 0.55, 50, delay)?;
        }
        Ok(())
    }

    pub fn suppressed(&self) -> bool {
        self.suppressed
    }

    /// Tam ekran/oyun sırasında dock'u tamamen kapatır; bitince açılış animasyonuyla döner.
    pub fn set_suppressed(&mut self, on: bool) {
        if on == self.suppressed {
            return;
        }
        self.suppressed = on;
        unsafe {
            if on {
                let _ = self.set_hover(None);
                self.press = None;
                self.drag = None;
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            } else {
                let _ = self.intro();
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.reassert();
            }
        }
    }

    /// İmleç şu an dock penceresinin üstünde mi (tıklama/menü sürüyor olabilir).
    pub fn cursor_over(&self) -> bool {
        if self.suppressed {
            return false;
        }
        unsafe {
            let mut pt = POINT::default();
            let mut r = RECT::default();
            GetCursorPos(&mut pt).is_ok() && GetWindowRect(self.hwnd, &mut r).is_ok() && pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
        }
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    /// Win+D ve "tümünü küçült" sonrasında dock'u yeniden en üste ve görünür yap.
    pub fn reassert(&self) {
        if self.suppressed {
            return;
        }
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    // ---- otomatik gizleme -------------------------------------------------

    pub fn behavior(&self) -> DockBehavior {
        self.cfg.dock.behavior
    }

    /// Otomatik gizlemede dock'u aşağı kaydırır (true) ya da geri getirir (false).
    pub fn slide(&mut self, away: bool) {
        if away == self.slid_away || self.suppressed {
            return;
        }
        carlui_core::info!("dock {}", if away { "gizlendi" } else { "gösterildi" });
        self.slid_away = away;
        let y = if away { self.c_hidden() } else { self.c0() };
        let _ = self.motion.animate(&self.glass.root, &self.across_prop(), y, if away { 1.0 } else { 0.75 }, if away { 70 } else { 45 }, 0);
        if away {
            let _ = self.set_hover(None);
            // Bölge hemen sıfırlanırsa dock kaymayı beklemeden "pat" diye kaybolur:
            // aşağı kayma bitince kırp (TIMER_REGION → fit_region, gizliyken boş bölge).
            unsafe {
                SetTimer(self.hwnd, TIMER_REGION, 380, None);
            }
        } else {
            self.set_region(self.width);
        }
    }

    /// Win menüsü açıldı: dock nerede olursa olsun görünsün. Zaten "açık"
    /// sayılıyorsa da bölgesini ve en üstteliğini tazeler (yeniden kuruluş ya da
    /// başka bir en üst pencere onu gizlemiş olabilir).
    pub fn show_now(&mut self) {
        if self.slid_away {
            self.slide(false);
        } else {
            self.set_region(self.width);
        }
        self.reassert();
    }

    /// Animasyonsuz gizli konuma al (tam ekrandan dönüşte pencere dock'u örtüyorsa
    /// açılış animasyonu görünmeden gizli kalsın).
    pub fn park(&mut self) {
        if self.slid_away {
            return;
        }
        self.slid_away = true;
        if let Ok(mut o) = self.glass.root.Offset() {
            if self.vert() {
                o.X = self.c_hidden();
            } else {
                o.Y = self.c_hidden();
            }
            let _ = self.glass.root.SetOffset(o);
        }
        self.set_region(self.width);
    }

    /// Intellihide: dock'un alanıyla çakışan (küçültülmemiş) bir pencere var mı?
    pub fn overlapped(&self, tracker: &Tracker) -> bool {
        let RECT { left: l, top: t, right: r, bottom: b } = self.pill_rect(self.width);
        tracker.windows.iter().any(|w| {
            !tracker::is_minimized(w.hwnd)
                && window_bounds(w.hwnd).is_some_and(|wr| wr.left < r && wr.right > l && wr.top < b && wr.bottom > t)
        })
    }

    /// Dock'un yaslandığı ekran kenarında boyuna aralığı ve kenarın konumu
    /// (girdi iş parçacığı için): (baş, son, kenar, taraf).
    pub fn edge_span(&self) -> (i32, i32, i32, DockPosition) {
        let r = self.pill_rect(self.width);
        match self.side {
            DockPosition::Bottom => (r.left, r.right, self.mon.bottom - 1, self.side),
            DockPosition::Left => (r.top, r.bottom, self.mon.left, self.side),
            DockPosition::Right => (r.top, r.bottom, self.mon.right - 1, self.side),
        }
    }
}
