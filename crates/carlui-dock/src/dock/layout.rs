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
        for &i in order {
            let g = self.entries[i].group;
            if last.is_some_and(|l| l != g) {
                sep_xs.push(x + m.sep / 2.0);
                x += m.sep;
            }
            centers.push(x + m.slot / 2.0);
            x += m.slot;
            last = Some(g);
        }
        let width = x + m.pad;
        let half = width / 2.0;
        let dragging = self.drag.as_ref().map(|d| d.index);

        for (&i, &c) in order.iter().zip(&centers) {
            let rel = c - half;
            let e = &mut self.entries[i];
            e.x = rel;
            if Some(i) == dragging {
                continue; // sürüklenen öğe imleci izler
            }
            if animate && !e.fresh {
                self.motion.animate(&e.slot, h!("Offset.X"), rel - m.slot / 2.0, 0.78, 50, 0)?;
            } else {
                e.slot.SetOffset(v3(rel - m.slot / 2.0, 0.0))?;
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

        let sep_brush = self.comp.CreateColorBrushWithColor(color(self.theme.separator))?;
        let sep_h = (24.0 * m.s).round();
        let sep_w = m.s.round().max(1.0);
        while self.seps.len() < sep_xs.len() {
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&sep_brush)?;
            v.SetSize(v2(sep_w, sep_h))?;
            v.SetOffset(v3(sep_xs[self.seps.len()] - half, ((m.h - sep_h) / 2.0).round()))?;
            self.items.Children()?.InsertAtBottom(&v)?;
            self.seps.push(v);
        }
        for (n, v) in self.seps.iter().enumerate() {
            match sep_xs.get(n) {
                Some(sx) => {
                    let to = (sx - half).round();
                    if animate {
                        self.motion.animate(v, h!("Offset.X"), to, 0.78, 50, 0)?;
                        self.motion.fade(v, h!("Opacity"), 1.0, 150, 0)?;
                    } else {
                        v.SetOffset(v3(to, ((m.h - sep_h) / 2.0).round()))?;
                        v.SetOpacity(1.0)?;
                    }
                }
                None => self.motion.fade(v, h!("Opacity"), 0.0, 120, 0)?,
            }
        }

        if animate {
            self.motion.animate(&self.glass.root, h!("Size.X"), width, 0.78, 50, 0)?;
        } else {
            self.glass.root.SetSize(v2(width, m.h))?;
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
                let l = ((self.win_w - w) / 2.0).floor() as i32;
                let t = self.m.top as i32;
                CreateRectRgn(l, t, l + w.ceil() as i32 + 1, t + self.m.h as i32 + 1)
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
        for e in &mut self.entries {
            let n = e.windows.len().min(4);
            let active = n > 0 && e.windows.contains(&foreground);
            if e.dots_sig == (n, active) {
                continue;
            }
            e.dots_sig = (n, active);
            let kids = e.dots.Children()?;
            kids.RemoveAll()?;
            let widths: Vec<f32> = (0..n).map(|i| if active && i == 0 { aw } else { dw }).collect();
            let total: f32 = widths.iter().sum::<f32>() + gap * n.saturating_sub(1) as f32;
            let mut x = ((m.slot - total) / 2.0).round();
            let y = (m.h - 7.0 * m.s).round();
            for (i, w) in widths.iter().enumerate() {
                let d = self.comp.CreateSpriteVisual()?;
                d.SetSize(v2(*w, dh))?;
                d.SetBrush(if active && i == 0 { &self.active_brush } else { &self.dot_brush })?;
                d.SetOffset(v3(x, y))?;
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
        mo.animate(&self.glass.root, h!("Size.X"), self.width, 0.72, 55, 80)?;
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
        let y = if away { self.win_h + 8.0 * self.m.s } else { self.m.top };
        let _ = self.motion.animate(&self.glass.root, h!("Offset.Y"), y, if away { 1.0 } else { 0.75 }, if away { 70 } else { 45 }, 0);
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

    /// Animasyonsuz gizli konuma al (tam ekrandan dönüşte pencere dock'u örtüyorsa
    /// açılış animasyonu görünmeden gizli kalsın).
    pub fn park(&mut self) {
        if self.slid_away {
            return;
        }
        self.slid_away = true;
        if let Ok(mut o) = self.glass.root.Offset() {
            o.Y = self.win_h + 8.0 * self.m.s;
            let _ = self.glass.root.SetOffset(o);
        }
        self.set_region(self.width);
    }

    /// Intellihide: dock'un alanıyla çakışan (küçültülmemiş) bir pencere var mı?
    pub fn overlapped(&self, tracker: &Tracker) -> bool {
        let l = self.mon.left + ((self.win_w - self.width) / 2.0) as i32;
        let r = l + self.width as i32;
        let b = self.mon.bottom - self.m.bottom as i32;
        let t = b - self.m.h as i32;
        tracker.windows.iter().any(|w| {
            !tracker::is_minimized(w.hwnd)
                && window_bounds(w.hwnd).is_some_and(|wr| wr.left < r && wr.right > l && wr.top < b && wr.bottom > t)
        })
    }

    /// Ekranın alt kenarında dock'un yatay aralığı (girdi iş parçacığı için).
    pub fn edge_span(&self) -> (i32, i32, i32) {
        let l = self.mon.left + ((self.win_w - self.width) / 2.0) as i32;
        (l, l + self.width as i32, self.mon.bottom - 1)
    }
}
