//! Bu modül adadaki aramanın pencere tarafını ayrı tutuyor: aç/kapat, öne
//! gelme (odak), Ctrl+Space kısayolu, tuşlar, tekerlek, ikon/sonuç mesajları.
//! Arama mantığı `launcher.rs`, çizimi `island/spot.rs` içindedir.

use crate::*;

fn key_input(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    }
}

/// Pencereyi öne getirip klavyeyi ona verir (bkz. `carlui_core::win::force_foreground`).
pub(crate) unsafe fn force_foreground(hwnd: HWND) {
    carlui_core::win::force_foreground(hwnd);
}

/// Şu an öndeki pencere tam ekran bir oyun/uygulama mı? (Özel tam ekran D3D ya da
/// monitörü çerçevesiz kaplayan pencere.) Kısayol tetiklendiği anda bakılır.
pub(crate) fn game_in_front() -> bool {
    unsafe {
        use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
        if SHQueryUserNotificationState().is_ok_and(|q| q == QUNS_RUNNING_D3D_FULL_SCREEN) {
            return true;
        }
        let fg = GetForegroundWindow();
        let class = carlui_core::win::class_name(fg);
        if fg.is_invalid() || matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "CarlUI.Island" | "CarlUI.Host") {
            return false;
        }
        // Tarayıcıda / oynatıcıda tam ekran video oyun değildir.
        if carlui_core::win::is_media_window(fg) {
            return false;
        }
        if GetWindowLongW(fg, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0 {
            return false;
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(fg, &mut r);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi);
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

/// Öndeki pencere tarayıcı/oynatıcı mı (tam ekranı oyun sayılmaz)?
pub(crate) fn game_free_front() -> bool {
    carlui_core::win::is_media_window(unsafe { GetForegroundWindow() })
}

pub(crate) const TIMER_GAME: usize = 10;

/// Alt basıldı ve henüz başka tuşla birleşmedi.
static ALT_ALONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl App {
    /// Kısayol oyun sırasında tetiklendi: hiçbir şey açma, tuşu oyuna geri ver,
    /// kısayolları oyun bitene kadar kaldır (3 sn'de bir bakılır).
    pub(crate) fn hotkey_in_game(&mut self, lp: LPARAM) {
        info!("kısayol tam ekran oyunda tetiklendi; yok sayıldı, tuş oyuna geri verildi");
        self.game = true;
        self.set_hotkey();
        let vk = ((lp.0 >> 16) & 0xFFFF) as u16;
        // Değiştiriciler (Ctrl vb.) hâlâ basılı; yalnızca yutulan tuşun basılışını yeniden gönder.
        unsafe {
            SendInput(&[key_input(vk, false)], std::mem::size_of::<INPUT>() as i32);
            SetTimer(self.island.hwnd, TIMER_GAME, 3000, None);
        }
    }

    pub(crate) fn check_game_over(&mut self) {
        if !game_in_front() {
            unsafe {
                let _ = KillTimer(self.island.hwnd, TIMER_GAME);
            }
            self.game = false;
            self.set_hotkey();
        }
    }

    pub(crate) fn open_search(&mut self) {
        // Tam ekran video/sunum üstünde de açılır; yalnızca oyunda açılmaz.
        if self.game || !self.cfg.launcher.enabled || self.spot.open {
            return;
        }
        let hwnd = self.island.hwnd;
        unsafe {
            let prev = GetForegroundWindow();
            if let Some(n) = self.note.take() {
                self.pending.push_front(n);
                let _ = KillTimer(hwnd, TIMER_NOTE);
            }
            self.expanded = false;
            self.date = false;
            self.spot.open(&self.cfg, if prev == hwnd { HWND::default() } else { prev });
            let p = cursor();
            self.last_mouse = (p.x, p.y);
            self.island.reset_highlight();
            // Klavye alabilmek için pencere geçici olarak etkinleştirilebilir olur.
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
            self.refresh(true);
            force_foreground(hwnd);
        }
        info!("arama açıldı");
    }

    /// `restore`: odağı aramadan önceki pencereye geri ver (Esc ile kapatınca).
    pub(crate) fn close_search(&mut self, restore: bool) {
        if !self.spot.open {
            return;
        }
        let prev = self.spot.prev_fg;
        self.spot.close();
        let hwnd = self.island.hwnd;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
            if restore && !prev.is_invalid() && IsWindow(prev).as_bool() {
                let _ = SetForegroundWindow(prev);
            }
        }
        if self.note.is_none() && !self.pending.is_empty() {
            self.show_next_note();
        } else {
            self.refresh(false);
        }
    }

    pub(crate) fn toggle_search(&mut self) {
        if self.spot.open {
            self.close_search(true);
        } else {
            self.open_search();
        }
    }

    pub(crate) fn spot_outcome(&mut self, o: Outcome) {
        match o {
            Outcome::Nothing => {}
            Outcome::Redraw => self.refresh(true),
            Outcome::Close(restore) => self.close_search(restore),
            Outcome::Timer(cmd) => {
                self.close_search(true);
                self.timer_cmd(cmd);
            }
            Outcome::Download(url) => {
                self.close_search(true);
                match url {
                    Some(u) => self.dl_start(u),
                    None => self.dl_cancel(),
                }
            }
        }
    }

    /// Arama kısayolunu (Ctrl+Space) kaydeder; oyunda kaldırır ki çakışmasın.
    pub(crate) fn set_hotkey(&mut self) {
        self.set_clip_hotkey();
        let hwnd = self.island.hwnd;
        unsafe {
            if self.hotkey_on {
                let _ = UnregisterHotKey(hwnd, HOTKEY_SEARCH);
                self.hotkey_on = false;
            }
            if !self.cfg.launcher.enabled || self.game {
                return;
            }
            // Doğrulama (fare tuşu, tek tuş, sistem kısayolu reddi) ortak `keys` modülünde.
            let Some(c) = self.cfg.shortcuts.search_combo() else { return };
            let mut mods = MOD_NOREPEAT;
            for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
                if on {
                    mods |= m;
                }
            }
            match RegisterHotKey(hwnd, HOTKEY_SEARCH, mods, c.vk) {
                Ok(()) => self.hotkey_on = true,
                Err(e) => carlui_core::warn!("arama kısayolu {} kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}", self.cfg.shortcuts.search),
            }
        }
    }

    pub(crate) fn on_key(&mut self, vk: VIRTUAL_KEY) -> bool {
        if !self.spot.open {
            return false;
        }
        let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
        let (ctrl, shift) = (down(VK_CONTROL), down(VK_SHIFT));
        let k = match vk {
            VK_ESCAPE => Key::Esc,
            VK_RETURN => Key::Enter { ctrl, shift },
            VK_TAB => Key::Tab { shift },
            VK_UP => Key::Up,
            VK_DOWN => Key::Down,
            VK_LEFT => Key::Left,
            VK_RIGHT => Key::Right,
            VK_PRIOR => Key::PageUp,
            VK_NEXT => Key::PageDown,
            VK_HOME => Key::Home { ctrl },
            VK_END => Key::End { ctrl },
            VK_BACK => Key::Back { ctrl },
            VK_DELETE => Key::Delete,
            VK_A if ctrl => Key::SelectAll,
            VK_V if ctrl => Key::Paste,
            VK_C if ctrl => Key::Copy,
            VK_INSERT if shift => Key::Paste,
            _ => return false,
        };
        let o = self.spot.key(&self.cfg, k);
        self.spot_outcome(o);
        true
    }

    /// Test/otomasyon olayları: search-open {q}, search-key {k}, search-toggle.
    pub(crate) fn search_event(&mut self, name: &str, data: &serde_json::Value) {
        let g = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        match name {
            // Test/otomasyon: aramayı aç ve metni yaz; tuş gönder.
            "search-open" => {
                self.open_search();
                let q = g("q");
                for u in q.encode_utf16() {
                    let o = self.spot.char(&self.cfg, u);
                    self.spot_outcome(o);
                }
            }
            "search-key" => {
                let k = match g("k").as_str() {
                    "esc" => Some(Key::Esc),
                    "enter" => Some(Key::Enter { ctrl: false, shift: false }),
                    "tab" => Some(Key::Tab { shift: false }),
                    "alt" => Some(Key::Alt),
                    "up" => Some(Key::Up),
                    "down" => Some(Key::Down),
                    "left" => Some(Key::Left),
                    "right" => Some(Key::Right),
                    "back" => Some(Key::Back { ctrl: false }),
                    _ => None,
                };
                if let (Some(k), true) = (k, self.spot.open) {
                    let o = self.spot.key(&self.cfg, k);
                    self.spot_outcome(o);
                }
            }
            "search-toggle" => self.toggle_search(),
            _ => {}
        }
    }
}

/// Aramaya ait pencere mesajları; ilgisizse None.
pub(crate) fn window_message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    unsafe {
        Some(match msg {
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
            with_app(|a| {
                if a.spot.open && a.spot.wheel((delta as f32 / 40.0).round() as i32) {
                    a.refresh(true);
                }
            });
            LRESULT(0)
        }
        // Alt tek başına basılıp bırakılırsa eylemler açılır (arada başka tuş yoksa;
        // Alt+F4 gibi akorlar etkilenmez). Pencere menüsü moduna da girilmez.
        WM_SYSKEYDOWN if wp.0 as u16 == VK_MENU.0 => {
            if lp.0 & (1 << 30) == 0 {
                ALT_ALONE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if with_app(|a| a.spot.open).unwrap_or(false) { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_SYSKEYUP | WM_KEYUP if wp.0 as u16 == VK_MENU.0 => {
            let alone = ALT_ALONE.swap(false, std::sync::atomic::Ordering::Relaxed);
            let open = with_app(|a| {
                if a.spot.open && alone {
                    let o = a.spot.key(&a.cfg, Key::Alt);
                    a.spot_outcome(o);
                }
                a.spot.open
            })
            .unwrap_or(false);
            if open { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            ALT_ALONE.store(false, std::sync::atomic::Ordering::Relaxed);
            let handled = with_app(|a| a.on_key(VIRTUAL_KEY(wp.0 as u16))).unwrap_or(false);
            if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_CHAR => {
            with_app(|a| {
                if a.spot.open {
                    let o = a.spot.char(&a.cfg, wp.0 as u16);
                    a.spot_outcome(o);
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            // Başka bir yere tıklanınca / geçilince arama kapanır.
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                with_app(|a| a.close_search(false));
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // Alt+F4 adayı kapatmasın; yalnızca aramayı kapatır.
            with_app(|a| a.close_search(true));
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wp.0 as i32 == HOTKEY_SEARCH {
                with_app(|a| if game_in_front() { a.hotkey_in_game(lp) } else { a.toggle_search() });
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_GAME => {
            with_app(|a| a.check_game_over());
            LRESULT(0)
        }
        WM_CARLUI_SEARCH if wp.0 == 2 => {
            // Win menüsünde yazılmaya başlandı: aç ve ilk harfi kutuya koy.
            with_app(|a| {
                a.open_search();
                if a.spot.open && lp.0 > 0 {
                    let o = a.spot.char(&a.cfg, lp.0 as u16);
                    a.spot_outcome(o);
                }
            });
            LRESULT(0)
        }
        WM_CARLUI_SEARCH => {
            with_app(|a| a.toggle_search());
            LRESULT(0)
        }
        WM_APP_SPOT_WAKE => {
            with_app(|a| {
                if a.spot.open {
                    a.spot.rebuild(&a.cfg, false);
                    a.refresh(true);
                }
            });
            LRESULT(0)
        }
        WM_APP_ICONS => {
            with_app(|a| {
                let got: Vec<IconPx> = a.icons_in.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
                for (path, px, w, h, pixels) in got {
                    a.island.add_icon(&a.gfx, path, px, w, h, &pixels);
                }
                if a.spot.open {
                    a.refresh(true);
                }
            });
            LRESULT(0)
        }
            _ => return None,
        })
    }
}
