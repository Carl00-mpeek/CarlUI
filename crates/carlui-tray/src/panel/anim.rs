//! Panelin açılışı (balon yükselir ve genişler, içerik sırayla belirir),
//! kapanışı (tersine) ve pencerenin tıklanabilir bölgesi.

use super::*;

impl Panel {
    /// Paneli dock'taki düğmenin (`button`, ekran koordinatı) üstünde açar.
    pub fn open(&mut self, gfx: &Gfx, host: &mut Host, button: RECT, dock_top: i32) -> Result<()> {
        // Ses bölümü her açılışta kapalı başlar (panel kısa kalsın).
        self.audio_open = false;
        self.grow_from = None;
        if self.mode == Mode::Net {
            // Önce bilinen liste görünür; tarama bitince liste yenilenir.
            self.scan_later(3500, true);
        }
        self.open_with(gfx, host, button, dock_top, true)
    }

    pub(super) fn open_with(&mut self, gfx: &Gfx, host: &mut Host, button: RECT, dock_top: i32, animate: bool) -> Result<()> {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_HIDE);
        }
        self.last_open = Some((button, dock_top));
        self.root.Children()?.RemoveAll()?;
        self.state = State::Open;
        self.hovered = None;
        host.prune();
        host.anchor = button;
        let s = self.s;
        let w = self.width;
        let margin = (10.0 * s).round();
        let bubble = ((button.right - button.left) as f32 * 0.8).round();
        self.bubble = bubble;

        // İçeriği önce kur: panelin yüksekliğini o belirler.
        let content = self.comp.CreateContainerVisual()?;
        let h = self.build_content(gfx, host, &content)?;
        self.height = h;
        let grow_from = self.grow_from.take();
        // Sıralı belirme listesine kaydırıcı satırlarını da ekle.
        if let Ok(k) = content.Children() {
            let mut all = Vec::new();
            if let Ok(it) = k.First() {
                for v in it {
                    all.push(v);
                }
            }
            self.stagger = all;
        }

        // Düğmenin bulunduğu monitör (dock başka ekranda da olabilir).
        let (bcx, bcy) = ((button.left + button.right) / 2, (button.top + button.bottom) / 2);
        let mons = carlui_core::win::monitors();
        let mon = mons
            .iter()
            .find(|m| bcx >= m.rect.left && bcx < m.rect.right && bcy >= m.rect.top && bcy < m.rect.bottom)
            .or_else(|| mons.iter().find(|m| m.primary))
            .map(|m| m.rect)
            .unwrap_or_default();
        let gap = (10.0 * s) as i32;
        let edge = 12.0 * s;
        let clamp_x = |x: f32| x.clamp(mon.left as f32 + edge, mon.right as f32 - edge - w).round() as i32;
        let clamp_y = |y: f32| y.clamp(mon.top as f32 + edge, (mon.bottom as f32 - edge - h).max(mon.top as f32 + edge)).round() as i32;
        // `dock_top`: dock camının iç kenarı (alt dock'ta üst kenarın y'si,
        // yan dock'ta iç kenarın x'i).
        let (px, ptop) = match self.side {
            1 => (dock_top + gap, clamp_y(bcy as f32 - h / 2.0)),
            2 => (dock_top - gap - w as i32, clamp_y(bcy as f32 - h / 2.0)),
            _ => (clamp_x(bcx as f32 - w / 2.0), dock_top - gap - h as i32),
        };
        let pbottom = ptop + h as i32;
        let m = margin as i32;
        let (wl, wr, wt, wb) = if self.side == 0 {
            (px.min(button.left) - m, (px + w as i32).max(button.right) + m, ptop - m, button.bottom)
        } else {
            (px.min(button.left) - m, (px + w as i32).max(button.right) + m, ptop.min(button.top) - m, pbottom.max(button.bottom) + m)
        };
        self.panel = RECT { left: px - wl, top: ptop - wt, right: px - wl + w as i32, bottom: pbottom - wt };
        self.button = RECT { left: button.left - wl, top: button.top - wt, right: button.right - wl, bottom: button.bottom - wt };
        OUT_L.store(wl, Ordering::SeqCst);
        OUT_T.store(wt, Ordering::SeqCst);
        OUT_R.store(wr, Ordering::SeqCst);
        OUT_B.store(wb, Ordering::SeqCst);

        self.win_size = (wr - wl, wb - wt);
        unsafe {
            // Animasyon sürerken bölge tüm pencere: balon şekil değiştirirken kırpılmasın.
            // Panel yerine oturunca yalnızca panel + düğme şeridine daralır.
            if animate {
                self.set_region(true);
                self.animating = true;
                SetTimer(self.hwnd, TIMER_REGION, 650, None);
            } else {
                self.set_region(false);
            }
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, wl, wt, wr - wl, wb - wt, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }

        // Cam balon.
        let glass = Glass::new(&self.comp, &self.theme, bubble / 2.0)?;
        glass.root.SetAnchorPoint(v2(0.5, 1.0))?;
        let bcx_l = (self.button.left + self.button.right) as f32 / 2.0;
        let bcy_l = (self.button.top + self.button.bottom) as f32 / 2.0;
        glass.root.SetSize(v2(bubble, bubble))?;
        glass.root.SetOffset(v3(bcx_l, bcy_l + bubble / 2.0))?;
        glass.root.SetCenterPoint(Vector3 { X: bubble / 2.0, Y: bubble, Z: 0.0 })?;
        let gk = glass.content.Children()?;

        let accent = self.comp.CreateSpriteVisual()?;
        accent.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        accent.SetBrush(&self.accent_brush(false)?)?;
        gk.InsertAtTop(&accent)?;

        // İçerik son panel konumuna bağlı: balon büyüdükçe kaymadan açığa çıkar.
        let follow = self.comp.CreateExpressionAnimationWithExpression(h!("Vector3(r.Size.X / 2 - W / 2, r.Size.Y - H, 0)"))?;
        follow.SetReferenceParameter(h!("r"), &glass.root)?;
        follow.SetScalarParameter(h!("W"), w)?;
        follow.SetScalarParameter(h!("H"), h)?;
        content.StartAnimation(h!("Offset"), &follow)?;
        gk.InsertAtTop(&content)?;

        // Balonun ortasında tıklanan simge: ok, ya da ses/ağ simgesi.
        let glyph = match self.mode {
            Mode::Sound => Some('\u{E767}'),
            Mode::Net => Some(if carlui_core::wired::wired_up() { '\u{E839}' } else { '\u{E701}' }),
            _ => None,
        };
        let (chevron, cw, ch) = match glyph {
            Some(c) => {
                let g = carlui_ui::tone::literal_if(true, || self.text.glyph(gfx, c, (bubble * 0.4).round(), 0.95))?;
                (self.sprite(&g.surface, g.width, g.height, 0.0, 0.0)?, g.width, g.height)
            }
            None => {
                let cpx = (bubble * 0.62).round();
                let chev_s = gfx.surface(cpx, cpx, |dc| icons::draw_chevron(dc, cpx, &gfx.round_stroke))?;
                (self.sprite(&chev_s, cpx, cpx, 0.0, 0.0)?, cpx, cpx)
            }
        };
        let center = self.comp.CreateExpressionAnimationWithExpression(h!("Vector3(r.Size.X / 2 - CW / 2, r.Size.Y / 2 - CH / 2, 0)"))?;
        center.SetReferenceParameter(h!("r"), &glass.root)?;
        center.SetScalarParameter(h!("CW"), cw)?;
        center.SetScalarParameter(h!("CH"), ch)?;
        chevron.StartAnimation(h!("Offset"), &center)?;
        gk.InsertAtTop(&chevron)?;
        self.root.Children()?.InsertAtTop(&glass.root)?;

        let pcx = (self.panel.left + self.panel.right) as f32 / 2.0;
        let radius = (22.0 * s * self.theme.radius_scale).round();
        if !animate {
            glass.root.SetOffset(v3(pcx, self.panel.bottom as f32))?;
            glass.root.SetSize(v2(w, h))?;
            glass.set_radius(radius)?;
            // Ses bölümü açılınca cam eski boyundan yukarı doğru yaylanarak uzar
            // (kapanırken pencere zaten yeni boya küçüldüğü için hemen oturur).
            if let Some(from) = grow_from.filter(|&f| f > 0.0 && f < h) {
                glass.root.SetSize(v2(w, from))?;
                self.motion.animate(&glass.root, h!("Size.Y"), h, 0.82, 55, 0)?;
            }
            accent.SetOpacity(0.0)?;
            chevron.SetOpacity(0.0)?;
            self.glass = Some(glass);
            self.content = Some(content);
            self.accent = Some(accent);
            self.chevron = Some(chevron);
            return Ok(());
        }

        // --- Animasyon ---
        let mo = &self.motion;
        // 2) Balon belirir
        glass.root.SetOpacity(0.0)?;
        glass.root.SetScale(Vector3 { X: 0.55, Y: 0.55, Z: 1.0 })?;
        mo.fade(&glass.root, h!("Opacity"), 1.0, 110, 0)?;
        mo.scale(&glass.root, 1.0, 0.6, 45, 0)?;
        // 3) Yükselir ve genişler
        mo.animate(&glass.root, h!("Offset.X"), pcx, 0.82, 55, 140)?;
        mo.animate(&glass.root, h!("Offset.Y"), self.panel.bottom as f32, 0.8, 50, 120)?;
        mo.animate(&glass.root, h!("Size.X"), w, 0.8, 55, 200)?;
        mo.animate(&glass.root, h!("Size.Y"), h, 0.82, 60, 200)?;
        glass.animate_radius(&self.comp, radius, 220, 200)?;
        mo.fade(&accent, h!("Opacity"), 0.0, 260, 220)?;
        mo.fade(&chevron, h!("Opacity"), 0.0, 120, 160)?;
        // 4) İçerik sırayla belirir
        for (i, v) in self.stagger.iter().enumerate() {
            v.SetOpacity(0.0)?;
            v.SetScale(Vector3 { X: 0.85, Y: 0.85, Z: 1.0 })?;
            let delay = 300 + i as i64 * 28;
            mo.fade(v, h!("Opacity"), 1.0, 180, delay)?;
            mo.scale(v, 1.0, 0.7, 45, delay)?;
        }

        self.glass = Some(glass);
        self.content = Some(content);
        self.accent = Some(accent);
        self.chevron = Some(chevron);

        unsafe {
            if self.hook.is_invalid() {
                self.hook = SetWindowsHookExW(WH_MOUSE_LL, Some(outside_proc), GetModuleHandleW(None).unwrap_or_default(), 0).unwrap_or_default();
            }
        }
        // Durum olaylarla güncel tutulur; izleyici kurulamadıysa bir kez sorulur.
        if !crate::radio_watch::running() {
            quick::query_radios(self.hwnd.0 as isize, WM_APP_RADIOS);
        }
        quick::query_brightness(self.hwnd.0 as isize, WM_APP_BRIGHT);
        Ok(())
    }

    /// Açılış animasyonu bitti: bölgeyi daralt ve bekleyen yenilemeyi uygula.
    pub fn opening_done(&mut self, gfx: &Gfx, host: &mut Host) {
        self.animating = false;
        if self.state == State::Open {
            self.set_region(false);
            if self.refresh_due {
                self.refresh(gfx, host);
            }
        }
    }

    /// `full`: tüm pencere (animasyon sırasında); değilse panel + panelden düğmeye inen şerit.
    pub fn set_region(&self, full: bool) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_REGION);
            let rgn = if full {
                CreateRectRgn(0, 0, self.win_size.0, self.win_size.1)
            } else {
                let r1 = CreateRectRgn(self.panel.left, self.panel.top, self.panel.right + 1, self.panel.bottom + 1);
                // Alt dock'ta panelden düğmeye inen şerit; yan dock'ta yalnızca düğme.
                let r2 = if self.side == 0 {
                    CreateRectRgn(self.button.left, self.panel.bottom - 2, self.button.right, self.button.bottom)
                } else {
                    CreateRectRgn(self.button.left, self.button.top, self.button.right, self.button.bottom)
                };
                let _ = CombineRgn(r1, r1, r2, RGN_OR);
                let _ = DeleteObject(r2);
                r1
            };
            SetWindowRgn(self.hwnd, rgn, true);
        }
    }

    /// Açılışın tersi: içerik söner, panel balona küçülür ve düğmeye iner.
    pub fn close(&mut self, animate: bool) {
        if self.state != State::Open {
            return;
        }
        self.state = State::Closing;
        self.unhook();
        if self.dragging == Hit::Bright {
            self.apply_brightness_now();
        }
        self.dragging = Hit::None;
        let (Some(glass), Some(content), Some(accent), Some(chevron)) = (&self.glass, &self.content, &self.accent, &self.chevron) else {
            self.finish_close();
            return;
        };
        if !animate {
            self.finish_close();
            return;
        }
        self.set_region(true);
        let mo = &self.motion;
        let b = self.bubble;
        let _ = mo.fade(content, h!("Opacity"), 0.0, 90, 0);
        // Vurgu rengi panel balon boyutuna yaklaşınca gelsin (büyük mor blok görünmesin).
        let _ = mo.fade(accent, h!("Opacity"), 1.0, 140, 150);
        let _ = mo.fade(chevron, h!("Opacity"), 1.0, 120, 120);
        let _ = mo.animate(&glass.root, h!("Size.X"), b, 0.9, 45, 40);
        let _ = mo.animate(&glass.root, h!("Size.Y"), b, 0.9, 45, 40);
        let _ = glass.animate_radius(&self.comp, b / 2.0, 160, 40);
        let bcx = (self.button.left + self.button.right) as f32 / 2.0;
        let bcy = (self.button.top + self.button.bottom) as f32 / 2.0;
        // Önce balon boyutuna küçül, sonra düğmeye in.
        let _ = mo.animate(&glass.root, h!("Offset.X"), bcx, 0.9, 45, 150);
        let _ = mo.animate(&glass.root, h!("Offset.Y"), bcy + b / 2.0, 0.85, 50, 170);
        let _ = mo.scale(&glass.root, 0.55, 1.0, 40, 360);
        let _ = mo.fade(&glass.root, h!("Opacity"), 0.0, 120, 360);
        unsafe {
            SetTimer(self.hwnd, TIMER_HIDE, 520, None);
        }
    }

    pub fn finish_close(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_HIDE);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if let Ok(k) = self.root.Children() {
            let _ = k.RemoveAll();
        }
        self.glass = None;
        self.content = None;
        self.accent = None;
        self.chevron = None;
        self.cells.clear();
        self.state = State::Closed;
    }

    pub(super) fn unhook(&mut self) {
        if !self.hook.is_invalid() {
            unsafe {
                let _ = UnhookWindowsHookEx(self.hook);
            }
            self.hook = HHOOK::default();
        }
    }
}
