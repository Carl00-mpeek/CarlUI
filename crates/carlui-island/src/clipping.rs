//! Bu modül pano geçmişinin pencere tarafını ayrı tutuyor:
//! - Panoyu dinleme: Windows her kopyada WM_CLIPBOARDUPDATE yollar. Okuma
//!   (resim çözme, PNG'ye çevirme) arayüzü dondurmasın diye bir işçi iş
//!   parçacığında yapılır. Kendi yazdığımız kopya pano sıra numarasıyla tanınır,
//!   geçmişe ikinci kez girmez.
//! - Aç/kapat: kısayol (varsayılan Win+V; Win'li kısayolları dock'un klavye
//!   kancası yakalar, diğerleri RegisterHotKey), tuşlar, fare, tekerlek.
//! - Seçileni kullanma: panoya yaz, önceki pencereye odağı geri ver ve
//!   (ayar açıksa) Ctrl+V gönder.
//! Mantık `clipper.rs`, çizim `island/clipview.rs`, düzenleyici `editor.rs`.

use crate::*;
use carlui_clip::{capture, store::Kind, write};
use clipper::{Act, ClipKey, ClipOut};
use std::sync::{atomic::AtomicU32, mpsc};
use windows::Win32::System::{
    Com::{CoInitializeEx, COINIT_MULTITHREADED},
    DataExchange::GetClipboardSequenceNumber,
};

pub(crate) const WM_APP_CLIP: u32 = WM_APP + 9;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_MOUSEHWHEEL: u32 = 0x020E;
pub(crate) const HOTKEY_CLIP: i32 = 2;
const TIMER_PASTE: usize = 9;
/// Kart genişleyince (pano penceresinde) düzenleyiciyi aç.
const TIMER_CARD: usize = 11;
/// Resim önizlemesinin üstünde bu kadar bekleyince resim büyür.
const TIMER_HOVER: usize = 12;
const HOVER_MS: u32 = 550;
/// Görüntüleyici kapandıktan sonra fare önizlemeden çıkana dek tekrar açma.
static HOVER_BLOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Pano penceresinin üstündeki boşluk (kart panelin üstüne taşabilsin).
pub(crate) const POP_MARGIN: f32 = 30.0;

/// Gizli kayıtların ömrü (sn; 0 = silinmez). Ayar değişince güncellenir.
pub(crate) static SECRET_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(60);
/// "Silindi · Geri al" şeridi bu kadar açık kalır.
const TIMER_UNDO: usize = 13;
const UNDO_MS: u32 = 6000;

/// Kendi yazdığımız son kopyanın pano sıra numarası.
static OWN_SEQ: AtomicU32 = AtomicU32::new(0);

/// Geçmişi arka planda yükler (ilk seferde eski Pano'dan içeri aktarır) ve
/// panodaki her yeni kopyayı okuyup ekler.
pub(crate) fn start_listener(hwnd: HWND, store: Arc<Mutex<carlui_clip::store::Store>>, max_items: usize) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    let raw = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("clip".into()).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let loaded = carlui_clip::store::Store::load(max_items);
        info!("pano geçmişi yüklendi ({} kayıt)", loaded.items.len());
        if let Ok(mut s) = store.lock() {
            *s = loaded;
        }
        let _ = PostMessageW(HWND(raw as _), WM_APP_CLIP, WPARAM(0), LPARAM(0));
        loop {
            // Kopya yokken de birkaç saniyede bir süresi dolan gizli kayıtlara bak.
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if store.lock().map(|mut s| s.expire_vault()).unwrap_or(false) {
                        info!("pano: süresi dolan kasa belirteci silindi");
                        let _ = PostMessageW(HWND(raw as _), WM_APP_CLIP, WPARAM(0), LPARAM(0));
                    }
                    let secs = SECRET_SECS.load(Ordering::Relaxed);
                    if secs > 0 && store.lock().map(|mut s| s.expire_secrets(secs as i64 * 1000)).unwrap_or(false) {
                        info!("pano: süresi dolan gizli kayıt silindi");
                        let _ = PostMessageW(HWND(raw as _), WM_APP_CLIP, WPARAM(0), LPARAM(0));
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            // Kopyalayan uygulama panoyu bıraksın; art arda gelen bildirimler tek okumaya insin.
            std::thread::sleep(Duration::from_millis(90));
            while rx.try_recv().is_ok() {}
            if GetClipboardSequenceNumber() == OWN_SEQ.load(Ordering::SeqCst) {
                continue;
            }
            let Some(c) = capture::read(HWND::default()) else { continue };
            let changed = store.lock().map(|mut s| s.add(c)).unwrap_or(false);
            if changed {
                let _ = PostMessageW(HWND(raw as _), WM_APP_CLIP, WPARAM(0), LPARAM(0));
            }
        }
    });
    tx
}

fn screen_to_client(hwnd: HWND, lp: LPARAM) -> (i32, i32) {
    let mut p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut p);
    }
    (p.x, p.y)
}

/// Pano paneli için ayrı, şeffaf, tıklaması yalnızca panelde olan pencere.
pub(crate) fn create_pop_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(pop_proc),
            hInstance: instance.into(),
            lpszClassName: w!("CarlUI.ClipPop"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            w!("CarlUI.ClipPop"),
            w!("CarlUI Clipboard"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )?;
        let on = BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, 4);
        SetWindowRgn(hwnd, CreateRectRgn(0, 0, 0, 0), false);
        Ok(hwnd)
    }
}

impl App {
    /// Pano panelini çizer (açıksa) ya da gizler.
    pub(crate) fn refresh_clip(&mut self) {
        let view = if self.clip.open { View::Clip } else { View::Hidden };
        let store = self.clip.store.clone();
        let guard = if self.clip.open { store.lock().ok() } else { None };
        let model = Model { media: None, note: None, note_open: false, app_volume: None, spot: None, clip: guard.as_deref().map(|st| self.clip.view(st)), hub: None, badge: 0, timer: None, offer: None };
        let animate = view != self.pop.view;
        if let Err(e) = self.pop.set_view(&self.gfx, view, &model, animate) {
            carlui_core::error!("pano paneli kurulamadı: {e}");
        }
        drop(guard);
        // "Geri al" şeridi açıksa süresini say.
        if self.clip.open && self.clip.undo_pending() {
            unsafe {
                SetTimer(self.pop.hwnd, TIMER_UNDO, 250, None);
            }
        }
    }

    fn undo_tick(&mut self) {
        if !self.clip.undo_pending() {
            unsafe {
                let _ = KillTimer(self.pop.hwnd, TIMER_UNDO);
            }
        } else if self.clip.undo_age_ms() >= UNDO_MS as u128 {
            unsafe {
                let _ = KillTimer(self.pop.hwnd, TIMER_UNDO);
            }
            self.clip.finish_undo();
            self.refresh_clip();
        }
    }

    /// Paneli farenin sağına yerleştirir (yer yoksa soluna; ekrandan taşmaz) ve
    /// açılış yuvarlağının farenin altında doğmasını ayarlar.
    fn place_pop(&mut self) {
        let p = cursor();
        let s = self.pop.s;
        let (pw, ph) = island::clip_panel_size(s);
        let (ww, wh) = self.pop.window_size();
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let _ = GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &mut mi);
        }
        let wa = mi.rcWork;
        let m = (8.0 * s) as i32;
        let gap = (18.0 * s) as i32;
        let (pw, ph) = (pw as i32, ph as i32);
        let mut left = p.x + gap;
        if left + pw > wa.right - m {
            left = p.x - gap - pw;
        }
        let left = left.clamp(wa.left + m, (wa.right - m - pw).max(wa.left + m));
        let top = (p.y - (24.0 * s) as i32).clamp(wa.top + m, (wa.bottom - m - ph).max(wa.top + m));
        let x = left - (ww - pw) / 2;
        let y = top - (POP_MARGIN * s).round() as i32;
        unsafe {
            let _ = SetWindowPos(self.pop.hwnd, HWND_TOPMOST, x, y, ww, wh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        let lx = ((p.x - x) as f32).clamp(0.0, ww as f32);
        let ly = ((p.y - y) as f32).clamp(0.0, wh as f32 - 40.0 * s);
        self.pop.pop_from = Some((lx, ly));
    }

    fn set_activatable(&self, on: bool) {
        let hwnd = self.pop.hwnd;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = if on { ex & !(WS_EX_NOACTIVATE.0 as isize) } else { ex | WS_EX_NOACTIVATE.0 as isize };
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        }
    }

    pub(crate) fn open_clip(&mut self) {
        if !self.cfg.clipboard.enabled || self.clip.open {
            return;
        }
        // Tam ekran oyun/uygulama öndeyse odak alınmaz (oyun küçülmesin): bkz. clipgame.
        let passive = (self.fullscreen || self.game || searching::game_in_front()) && clipgame::start(self.pop.hwnd);
        let hwnd = self.pop.hwnd;
        unsafe {
            let prev = GetForegroundWindow();
            if let Some(n) = self.note.take() {
                self.pending.push_front(n);
                let _ = KillTimer(self.island.hwnd, TIMER_NOTE);
                self.refresh(false);
            }
            let ours = prev == hwnd || prev == self.island.hwnd;
            self.clip.open(if ours { HWND::default() } else { prev });
            let p = cursor();
            self.last_mouse = (p.x, p.y);
            self.pop.reset_highlight();
            self.place_pop();
            if !passive {
                self.set_activatable(true);
            }
            self.refresh_clip();
            if !passive {
                searching::force_foreground(hwnd);
            }
        }
        info!("pano açıldı{}", if passive { " (oyun üstünde, odaksız)" } else { "" });
    }

    pub(crate) fn close_clip(&mut self, restore: bool) {
        if !self.clip.open {
            return;
        }
        let prev = self.clip.prev_fg;
        // Odaksız açıldıysa oyun zaten önde: odağa dokunma.
        let restore = restore && !clipgame::active();
        clipgame::stop();
        self.edit_id = None;
        // Düzenleyici bir anda kalkar; altındaki metin görüntüsü kartla birlikte solar.
        self.pop.card_ghost(true);
        editor::close();
        self.viewer.vanish();
        self.clip.close();
        self.set_activatable(false);
        unsafe {
            if restore && !prev.is_invalid() && IsWindow(prev).as_bool() {
                let _ = SetForegroundWindow(prev);
            }
        }
        self.refresh_clip();
        if self.note.is_none() && !self.pending.is_empty() {
            self.show_next_note();
        }
    }

    pub(crate) fn toggle_clip(&mut self) {
        if self.clip.open {
            self.close_clip(true);
        } else {
            if self.spot.open {
                self.close_search(true);
            }
            self.open_clip();
        }
    }

    /// Pano kısayolunu kaydeder. Win+V gibi Windows'un ayırdığı kısayollarda
    /// kayıt başarısız olur; onları dock'un klavye kancası yakalar.
    pub(crate) fn set_clip_hotkey(&mut self) {
        let hwnd = self.island.hwnd;
        unsafe {
            if self.clip_hotkey_on {
                let _ = UnregisterHotKey(hwnd, HOTKEY_CLIP);
                self.clip_hotkey_on = false;
            }
            if !self.cfg.clipboard.enabled || self.fullscreen || self.game {
                return;
            }
            let Some(c) = self.cfg.shortcuts.clipboard_combo() else { return };
            let mut mods = MOD_NOREPEAT;
            for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
                if on {
                    mods |= m;
                }
            }
            match RegisterHotKey(hwnd, HOTKEY_CLIP, mods, c.vk) {
                Ok(()) => self.clip_hotkey_on = true,
                Err(_) if c.mods.win => info!("pano kısayolu {} Windows'a ayrılmış; dock'un klavye kancası yakalayacak", self.cfg.shortcuts.clipboard),
                Err(e) => carlui_core::warn!("pano kısayolu {} kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}", self.cfg.shortcuts.clipboard),
            }
        }
    }

    pub(crate) fn clip_outcome(&mut self, o: ClipOut) {
        match o {
            ClipOut::Nothing => {}
            ClipOut::Redraw | ClipOut::Wrap => self.refresh_clip(),
            ClipOut::Close(restore) => self.close_clip(restore),
            ClipOut::Use { id, paste } => self.use_clip(&id, paste),
            // Düzenleyici klavye odağı ister; oyun üstünde açılmaz (oyun küçülürdü).
            ClipOut::Edit(_) if clipgame::active() => {}
            ClipOut::Edit(id) => self.open_edit_card(id),
            // Resimdeki QR: ekran görüntüsü aracıyla aynı yol (2FA → Carl Vault).
            ClipOut::Qr(act, data) => {
                self.close_clip(act != crate::shots::qr::Act::Vault);
                self.qr_act(act, data);
            }
        }
    }

    /// Metne tıklandı: önizleme kutusu panelden taşarak genişler, kısa süre
    /// sonra (animasyon bitince) metin kutusu kartın içine yerleşir.
    fn open_edit_card(&mut self, id: String) {
        if self.pop.card_open() || editor::is_open() {
            return;
        }
        self.viewer.vanish();
        match self.pop.open_card(&self.gfx) {
            Ok(true) => {
                self.edit_id = Some(id);
                unsafe {
                    SetTimer(self.pop.hwnd, TIMER_CARD, 190, None);
                }
            }
            Ok(false) => {}
            Err(e) => carlui_core::error!("düzenleme kartı açılamadı: {e}"),
        }
    }

    /// Kart genişledi: düzenleyiciyi kartın metin alanına koy.
    fn card_ready(&mut self) {
        unsafe {
            let _ = KillTimer(self.pop.hwnd, TIMER_CARD);
        }
        let Some(id) = self.edit_id.take() else { return };
        let Some(mut r) = self.pop.card_edit_rect() else { return };
        let text = self.clip.store.lock().ok().and_then(|s| s.get(&id).map(|c| c.text.clone()));
        let Some(text) = text else { return };
        unsafe {
            let mut p = POINT { x: r.left, y: r.top };
            let _ = ClientToScreen(self.pop.hwnd, &mut p);
            (r.right, r.bottom) = (p.x + r.right - r.left, p.y + r.bottom - r.top);
            (r.left, r.top) = (p.x, p.y);
        }
        if editor::open_at(self.pop.hwnd, self.island.hwnd, r, self.pop.s, id, &text) {
            self.pop.card_ghost(false);
        }
    }

    /// Göz düğmesi, resme tıklama ya da üstünde bekleme: resim önizlemeden
    /// kopup programın dışına taşarak büyür ve ekranın ortasına gelir.
    fn open_view_card(&mut self) {
        if self.pop.card_open() || self.viewer.is_open() {
            return;
        }
        let Some((surf, w, h, mut r)) = self.pop.preview_picture() else { return };
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let mut p = POINT { x: r.left, y: r.top };
            let _ = ClientToScreen(self.pop.hwnd, &mut p);
            (r.right, r.bottom) = (p.x + r.right - r.left, p.y + r.bottom - r.top);
            (r.left, r.top) = (p.x, p.y);
            let _ = GetMonitorInfoW(MonitorFromWindow(self.pop.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        }
        if let Err(e) = self.viewer.open(&self.gfx, &surf, w, h, r, mi.rcWork) {
            carlui_core::error!("resim görüntüleyici açılamadı: {e}");
        }
        HOVER_BLOCK.store(true, Ordering::Relaxed);
    }

    fn close_card(&mut self) {
        unsafe {
            let _ = KillTimer(self.pop.hwnd, TIMER_CARD);
        }
        self.edit_id = None;
        self.viewer.close(true);
        if self.pop.close_card(true) {
            self.refresh_clip();
        }
    }

    /// Kaydı panoya yazar; `paste` ise önceki pencereye yapıştırır.
    fn use_clip(&mut self, id: &str, paste: bool) {
        let hwnd = self.island.hwnd;
        // Kasa belirteci panoya yazılmaz: tek kullanımlık, kasa hedefe yazar.
        let vault = self.clip.store.lock().ok().and_then(|mut st| {
            let token = st.get(id).filter(|c| c.kind == Kind::Vault).map(|c| c.token.clone())?;
            st.remove(id);
            Some(token)
        });
        if let Some(token) = vault {
            let target = self.clip.prev_fg;
            self.close_clip(true);
            self.vault_type(token, target);
            return;
        }
        let ok = {
            let Ok(mut st) = self.clip.store.lock() else { return };
            let Some(c) = st.get(id).cloned() else { return };
            let seq = match c.kind {
                Kind::Text => write::set(hwnd, write::Data::Text(&c.text)),
                Kind::Files => {
                    let list: Vec<String> = c.text.lines().map(String::from).collect();
                    write::set(hwnd, write::Data::Files(&list))
                }
                Kind::Image => std::fs::read(st.image_path(&c)).ok().and_then(|png| write::set(hwnd, write::Data::Png(&png))),
                // Yukarıda ayrıldı; buraya gelmez.
                Kind::Vault => None,
            };
            if let Some(seq) = seq {
                OWN_SEQ.store(seq, Ordering::SeqCst);
                st.touch(id);
            }
            seq.is_some()
        };
        if !ok {
            carlui_core::warn!("pano: kayıt panoya yazılamadı");
            return;
        }
        self.close_clip(true);
        if paste && self.cfg.clipboard.paste_on_enter {
            unsafe {
                SetTimer(hwnd, TIMER_PASTE, 90, None);
            }
        }
    }

    /// Kasadan, panodan önceki pencereye yazmasını ister (Vault Aşama 2). Kasa
    /// belirteci, bu süreci ve hedefi doğrular; sonuç adada bildirim olur.
    fn vault_type(&mut self, token: String, target: HWND) {
        let raw = self.island.hwnd.0 as isize;
        let notes = self.notes_in.clone();
        let target = target.0 as isize;
        let _ = std::thread::Builder::new().name("vault-type".into()).spawn(move || {
            // Odak önceki pencereye dönsün (kasa da birkaç yüz ms bekler).
            std::thread::sleep(Duration::from_millis(120));
            let reply = carlui_core::vaultlink::ask(&carlui_core::vaultlink::Request::Type { token, hwnd: target });
            let (ok, msg) = match reply {
                Ok(r) => (r.ok, r.message),
                Err(m) => (false, m),
            };
            info!("pano: kasa yazma isteği {}: {msg}", if ok { "tamam" } else { "olmadı" });
            let (title, body) = if ok { (msg, String::new()) } else { (t!("Yazılamadı", "Couldn't type it").into(), msg) };
            if let Ok(mut q) = notes.lock() {
                q.push_back(Note { app: "Carl Vault".into(), title, body, ..Default::default() });
            }
            unsafe {
                let _ = PostMessageW(HWND(raw as _), WM_APP_NOTE, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// Düzenleyici kapandı (kaydedildi ya da vazgeçildi).
    fn edit_done(&mut self) {
        let done = editor::RESULT.lock().ok().and_then(|mut r| r.take());
        let mut paste = false;
        if let Some(d) = done {
            let hwnd = self.island.hwnd;
            if let Ok(mut st) = self.clip.store.lock() {
                st.set_text(&d.id, d.text.clone());
                st.touch(&d.id);
            }
            if let Some(seq) = write::set(hwnd, write::Data::Text(&d.text)) {
                OWN_SEQ.store(seq, Ordering::SeqCst);
                paste = d.paste;
            }
            info!("pano: metin düzenlendi ({} karakter)", d.text.chars().count());
        }
        if !self.clip.open {
            return;
        }
        if paste {
            // Önceki pencereye dön ve yapıştır.
            self.close_clip(true);
            unsafe {
                SetTimer(self.island.hwnd, TIMER_PASTE, 120, None);
            }
            return;
        }
        // Kart önizleme kutusuna geri döner; odak panelde değilse (başka yere
        // tıklandıysa) panel de kapanır.
        self.edit_id = None;
        // Metin görüntüsü geri gelsin: kart boş bir kutu olarak küçülmesin.
        self.pop.card_ghost(true);
        self.pop.close_card(true);
        if unsafe { GetForegroundWindow() } != self.pop.hwnd {
            self.close_clip(false);
        } else {
            self.clip.rebuild();
            self.refresh_clip();
        }
    }

    fn on_clip_key(&mut self, vk: VIRTUAL_KEY) -> bool {
        // Odaksız (oyun) modunda bu iş parçacığının tuş durumu güncellenmez.
        let down = |k: VIRTUAL_KEY| unsafe { if clipgame::active() { GetAsyncKeyState(k.0 as i32) } else { GetKeyState(k.0 as i32) } } < 0;
        let (ctrl, shift, alt) = (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU));
        let step = 60.0 * self.pop.s;
        if self.viewer.is_open() {
            // Resim açıkken: Esc kapatır, oklar gezdirir, +/- yakınlaştırır, Enter yapıştırır.
            let step = step * 2.0;
            match vk {
                VK_ESCAPE => self.close_card(),
                VK_LEFT | VK_RIGHT => self.viewer.scroll(if vk == VK_LEFT { -step } else { step }, 0.0),
                VK_UP | VK_DOWN => self.viewer.scroll(0.0, if vk == VK_UP { -step } else { step }),
                VK_OEM_PLUS | VK_ADD => self.viewer.zoom(1.25, None),
                VK_OEM_MINUS | VK_SUBTRACT => self.viewer.zoom(0.8, None),
                VK_0 | VK_NUMPAD0 => self.viewer.zoom(0.0, None),
                VK_RETURN => {
                    let o = self.clip.key(ClipKey::Enter { shift });
                    self.clip_outcome(o);
                }
                _ => {}
            }
            return true;
        }
        if self.pop.card_open() {
            if vk == VK_ESCAPE {
                self.close_card();
            }
            return true;
        }
        if vk == VK_G && ctrl {
            self.open_view_card();
            return true;
        }
        let k = match vk {
            VK_ESCAPE => ClipKey::Esc,
            VK_RETURN => ClipKey::Enter { shift },
            VK_TAB => ClipKey::Tab,
            VK_UP | VK_DOWN if shift => {
                self.pop.preview_scroll(0.0, if vk == VK_UP { -step } else { step });
                return true;
            }
            VK_LEFT | VK_RIGHT => {
                self.pop.preview_scroll(if vk == VK_LEFT { -step } else { step }, 0.0);
                return true;
            }
            VK_UP => ClipKey::Up,
            VK_DOWN => ClipKey::Down,
            VK_PRIOR => ClipKey::PageUp,
            VK_NEXT => ClipKey::PageDown,
            VK_HOME => ClipKey::Home,
            VK_END => ClipKey::End,
            VK_BACK => ClipKey::Back { ctrl },
            VK_DELETE => ClipKey::Delete,
            VK_F2 => ClipKey::Edit,
            VK_E if ctrl => ClipKey::Edit,
            VK_P if ctrl => ClipKey::Pin,
            VK_Z if alt => ClipKey::Wrap,
            VK_Z if ctrl => ClipKey::Undo,
            VK_OEM_PLUS | VK_ADD if ctrl => {
                self.pop.preview_zoom(1.25, None);
                return true;
            }
            VK_OEM_MINUS | VK_SUBTRACT if ctrl => {
                self.pop.preview_zoom(0.8, None);
                return true;
            }
            VK_0 | VK_NUMPAD0 if ctrl => {
                self.pop.preview_zoom(0.0, None);
                return true;
            }
            _ => return false,
        };
        let o = self.clip.key(k);
        self.clip_outcome(o);
        true
    }

    fn on_clipboard_update(&mut self) {
        if !self.cfg.clipboard.enabled || unsafe { GetClipboardSequenceNumber() } == OWN_SEQ.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.clip_tx.send(());
    }

    /// Test/otomasyon olayları: clip-open {q}, clip-key {k}, clip-toggle, clip-click {i},
    /// clip-scroll {dx, dy}, clip-zoom {f}, clip-view, clip-card-close,
    /// clip-edit-save {text, paste}.
    pub(crate) fn clip_event(&mut self, name: &str, data: &serde_json::Value) {
        let g = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let f = |k: &str| data.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        match name {
            "clip-open" => {
                self.open_clip();
                for u in g("q").encode_utf16() {
                    let o = self.clip.char(u);
                    self.clip_outcome(o);
                }
            }
            "clip-toggle" => self.toggle_clip(),
            "clip-key" if self.clip.open => {
                let k = match g("k").as_str() {
                    "esc" => ClipKey::Esc,
                    "enter" => ClipKey::Enter { shift: false },
                    "shift-enter" => ClipKey::Enter { shift: true },
                    "tab" => ClipKey::Tab,
                    "up" => ClipKey::Up,
                    "down" => ClipKey::Down,
                    "end" => ClipKey::End,
                    "back" => ClipKey::Back { ctrl: false },
                    "delete" => ClipKey::Delete,
                    "edit" => ClipKey::Edit,
                    "pin" => ClipKey::Pin,
                    "wrap" => ClipKey::Wrap,
                    "undo" => ClipKey::Undo,
                    _ => return,
                };
                let o = self.clip.key(k);
                self.clip_outcome(o);
            }
            "clip-click" if self.clip.open => {
                let i = data.get("i").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let o = self.clip.click(i, 0);
                self.clip_outcome(o);
            }
            "clip-act" if self.clip.open => {
                let a = match g("a").as_str() {
                    "pin" => Act::Pin,
                    "delete" => Act::Delete,
                    "copy" => Act::Copy,
                    _ => return,
                };
                let o = self.clip.run(a);
                self.clip_outcome(o);
            }
            "clip-scroll" => {
                self.pop.preview_scroll(f("dx"), f("dy"));
            }
            "clip-zoom" => self.pop.preview_zoom(f("f"), None),
            "clip-delete-cancel" if self.clip.open => {
                if self.clip.cancel_delete() {
                    self.clip_outcome(ClipOut::Redraw);
                }
            }
            "clip-view" if self.clip.open => self.open_view_card(),
            "clip-card-close" => self.close_card(),
            "clip-edit-save" => {
                let t = g("text");
                editor::test_save((!t.is_empty()).then_some(t.as_str()), data.get("paste").and_then(|v| v.as_bool()).unwrap_or(false));
            }
            _ => {}
        }
    }
}

/// Panoya ait pencere mesajları; ilgisizse None.
pub(crate) fn window_message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    unsafe {
        match msg {
            WM_CLIPBOARDUPDATE => {
                with_app(|a| a.on_clipboard_update());
                return Some(LRESULT(0));
            }
            WM_APP_CLIP => {
                with_app(|a| {
                    if a.clip.open {
                        a.clip.rebuild();
                        a.refresh_clip();
                    }
                });
                return Some(LRESULT(0));
            }
            WM_CARLUI_SEARCH if wp.0 == 1 => {
                with_app(|a| a.toggle_clip());
                return Some(LRESULT(0));
            }
            WM_HOTKEY if wp.0 as i32 == HOTKEY_CLIP => {
                with_app(|a| if searching::game_in_front() { a.hotkey_in_game(lp) } else { a.toggle_clip() });
                return Some(LRESULT(0));
            }
            editor::WM_APP_EDIT_DONE => {
                with_app(|a| a.edit_done());
                return Some(LRESULT(0));
            }
            WM_TIMER if wp.0 == TIMER_PASTE => {
                let _ = KillTimer(hwnd, TIMER_PASTE);
                write::paste();
                return Some(LRESULT(0));
            }
            _ => None,
        }
    }
}

/// Pano panelinin kendi penceresi: fare, tekerlek, tuşlar, odak.
extern "system" fn pop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_TIMER && wp.0 == island::TIMER_REGION {
            with_app(|a| a.pop.region_done());
            return LRESULT(0);
        }
        if msg == WM_TIMER && wp.0 == TIMER_CARD {
            with_app(|a| a.card_ready());
            return LRESULT(0);
        }
        if msg == WM_TIMER && wp.0 == TIMER_UNDO {
            with_app(|a| a.undo_tick());
            return LRESULT(0);
        }
        if msg == WM_TIMER && wp.0 == TIMER_HOVER {
            let _ = KillTimer(hwnd, TIMER_HOVER);
            with_app(|a| {
                // Fare hâlâ resmin üstündeyse büyüt.
                let mut p = cursor();
                let _ = ScreenToClient(hwnd, &mut p);
                if a.clip.open && a.pop.hit(p.x, p.y) == Hit::Preview && a.pop.preview_is_image() && GetForegroundWindow() == hwnd {
                    a.open_view_card();
                }
            });
            return LRESULT(0);
        }
        if !with_app(|a| a.clip.open).unwrap_or(false) {
            return match msg {
                WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            };
        }
        match msg {
            // Düzenleyici açıkken panele tıklamak odağı ondan almasın (Kaydet/X için);
            // oyun üstünde de odak oyunda kalır.
            WM_MOUSEACTIVATE => LRESULT(if editor::is_open() || clipgame::active() { MA_NOACTIVATE } else { MA_ACTIVATE } as isize),
            clipgame::WM_APP_CLIP_KEY => {
                with_app(|a| a.on_clip_key(VIRTUAL_KEY(wp.0 as u16)));
                LRESULT(0)
            }
            clipgame::WM_APP_CLIP_CHAR => {
                with_app(|a| {
                    if a.pop.card_open() || a.viewer.is_open() {
                        return;
                    }
                    let o = a.clip.char(wp.0 as u16);
                    a.clip_outcome(o);
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                let (x, y) = screen_to_client(hwnd, lp);
                with_app(|a| {
                    let keys = wp.0 & 0xFFFF;
                    let (ctrl, shift) = (keys & 0x8 != 0, keys & 0x4 != 0);
                    if a.viewer.is_open() {
                        let p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
                        a.viewer.zoom(if delta > 0.0 { 1.2 } else { 1.0 / 1.2 }, Some(p));
                        return;
                    }
                    let card = a.pop.card_open().then_some(());
                    if card.is_some() {
                        return;
                    }
                    if let Some(at) = a.pop.preview_point(x, y) {
                        let px = delta / 120.0 * 90.0 * a.pop.s;
                        if ctrl {
                            a.pop.preview_zoom(if delta > 0.0 { 1.15 } else { 1.0 / 1.15 }, Some(at));
                        } else if shift {
                            a.pop.preview_scroll(-px, 0.0);
                        } else {
                            a.pop.preview_scroll(0.0, -px);
                        }
                    } else if card.is_none() && a.clip.wheel((delta / 40.0).round() as i32) {
                        a.refresh_clip();
                    }
                });
                LRESULT(0)
            }
            WM_MOUSEHWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                with_app(|a| a.pop.preview_scroll(delta / 120.0 * 90.0 * a.pop.s, 0.0));
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let handled = with_app(|a| a.on_clip_key(VIRTUAL_KEY(wp.0 as u16))).unwrap_or(false);
                if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
            }
            WM_CHAR => {
                with_app(|a| {
                    if a.pop.card_open() || a.viewer.is_open() {
                        return;
                    }
                    let o = a.clip.char(wp.0 as u16);
                    a.clip_outcome(o);
                });
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Odak kartın düzenleyicisine geçtiyse panel açık kalır.
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && (!editor::is_open() || HWND(lp.0 as _) != editor::hwnd()) {
                    with_app(|a| a.close_clip(false));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                with_app(|a| a.close_clip(true));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    if a.dragging == Hit::Preview {
                        a.pop.preview_drag(x, y);
                        return;
                    }
                    let p = cursor();
                    if (p.x, p.y) == a.last_mouse {
                        return;
                    }
                    a.last_mouse = (p.x, p.y);
                    let h = a.pop.hit(x, y);
                    if let Hit::ClipRow(i) = h {
                        if a.clip.hover(i) {
                            a.refresh_clip();
                        }
                    }
                    // Resmin üstünde biraz bekleyince büyüsün.
                    if h == Hit::Preview && a.pop.preview_is_image() {
                        if !HOVER_BLOCK.load(Ordering::Relaxed) && !a.viewer.is_open() {
                            SetTimer(hwnd, TIMER_HOVER, HOVER_MS, None);
                        }
                    } else {
                        HOVER_BLOCK.store(false, Ordering::Relaxed);
                        let _ = KillTimer(hwnd, TIMER_HOVER);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    let h = a.pop.hit(x, y);
                    a.pressed = h;
                    // Metin önizlemesi ve resim kartı sürüklenir; resim önizlemesi tıklanır.
                    let pan = h == Hit::Preview && !a.pop.preview_is_image() && !a.viewer.is_open();
                    if pan {
                        a.dragging = Hit::Preview;
                        SetCapture(hwnd);
                        a.pop.preview_drag_start(x, y);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    let pressed = std::mem::replace(&mut a.pressed, Hit::None);
                    if a.dragging == Hit::Preview {
                        a.dragging = Hit::None;
                        let _ = ReleaseCapture();
                        // Metne kımıldamadan tıklandıysa: yerinde düzenle.
                        if a.pop.preview_drag_end(x, y) && pressed == Hit::Preview {
                            let o = a.clip.run(Act::Edit);
                            a.clip_outcome(o);
                        }
                        return;
                    }
                    let h = a.pop.hit(x, y);
                    if a.viewer.is_open() {
                        // Resim açıkken panele tıklamak resmi geri gönderir.
                        a.close_card();
                        return;
                    }
                    if a.pop.card_open() {
                        match (h, pressed) {
                            (Hit::CardBtn(0), Hit::CardBtn(0)) => editor::commit(true),
                            (Hit::CardBtn(1), Hit::CardBtn(1)) => editor::commit(false),
                            // Kartın dışına tıklama: kaydedip kapat.
                            (Hit::None, Hit::None) => {
                                if editor::is_open() {
                                    editor::commit(true);
                                } else {
                                    a.close_card();
                                }
                            }
                            _ => editor::focus(),
                        }
                        return;
                    }
                    if h != pressed {
                        return;
                    }
                    // Silme sorusu açıkken ✓/✗ dışındaki her tıklama soruyu kapatır;
                    // çöp kutusuna ikinci kez basmak da vazgeçmektir (çift tık kazayla silmesin).
                    if matches!(h, Hit::Card) {
                        return;
                    }
                    if !matches!(h, Hit::ClipBtn(4)) && a.clip.cancel_delete() {
                        if matches!(h, Hit::ClipBtn(2) | Hit::ClipBtn(5)) {
                            a.clip_outcome(ClipOut::Redraw);
                            return;
                        }
                        a.clip_outcome(ClipOut::Redraw);
                    }
                    let o = match h {
                        Hit::ClipRow(i) => a.clip.click(i, GetDoubleClickTime()),
                        Hit::ClipBtn(0) => a.clip.run(Act::Edit),
                        Hit::ClipBtn(1) => a.clip.run(Act::Pin),
                        Hit::ClipBtn(2) | Hit::ClipBtn(4) => a.clip.run(Act::Delete),
                        Hit::ClipBtn(6) => a.clip.undo(),
                        Hit::ClipBtn(3) | Hit::Preview => {
                            a.open_view_card();
                            ClipOut::Nothing
                        }
                        _ => ClipOut::Nothing,
                    };
                    a.clip_outcome(o);
                });
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    if let Hit::ClipRow(i) = a.pop.hit(x, y) {
                        let o = a.clip.context(i);
                        a.clip_outcome(o);
                    }
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
