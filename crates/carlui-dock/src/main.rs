//! Dock modülü (`carlui-dock.exe`). Normalde `carlui.exe` tarafından başlatılır;
//! geliştirme sırasında tek başına da çalışabilir.
//!
//! Bu dosya parçaları birbirine bağlar: pencere takipçisi → dock modeli,
//! fare → dock/etiket/önizleme, girdi iş parçacığı → otomatik gizleme ve
//! Win+1…9, olay yolu → tam ekran/Win+D/kapanış, ayar dosyası → canlı yenileme.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod apps;
mod dock;
mod iconcache;
mod input;
mod jumplist;
mod live;
mod media;
mod menu;
mod fly;
mod popup;
mod procs;
mod radial;
mod resolver;
mod sandbox;
mod screens;
mod tracker;

use carlui_core::{
    bus::{BusClient, Event},
    config::{self, Config, DockBehavior, WinKey},
    info, log, paths,
};
use carlui_ui::gfx::Gfx;
use dock::{Dock, Kind};
use input::Input;
use popup::{Mode, Popup};
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tracker::Tracker;
use windows::{
    core::*,
    UI::Composition::Desktop::DesktopWindowTarget,
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::CreateMutexW,
            WinRT::{Composition::ICompositorDesktopInterop, *},
        },
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_APP_BUS: u32 = WM_APP + 1;
const WM_APP_CONFIG: u32 = WM_APP + 2;
const TIMER_PREVIEW: usize = 31;
const TIMER_GRACE: usize = 32;
/// Meşgulken kaçan pencere listesi yenilemesinin yeniden denemesi.
const TIMER_REFRESH: usize = 33;

struct App {
    gfx: Gfx,
    target: DesktopWindowTarget,
    hwnd: HWND,
    dock: Dock,
    popup: Popup,
    /// Çark dock (yalnızca ana dock; ek ekran kopyalarında yok).
    radial: Option<radial::Radial>,
    tracker: Tracker,
    input: Option<Input>,
    /// Olay yolu (kum havuzunda yok).
    bus: Option<BusClient>,
    inbox: Arc<Mutex<VecDeque<Event>>>,
    pending_cfg: Arc<Mutex<Option<Config>>>,
    fullscreen: bool,
    /// Sürükleme sırasında gelen pencere değişiklikleri bırakınca uygulanır.
    sync_pending: bool,
    /// Tray paneli açıkken dock otomatik gizlenmez.
    tray_open: bool,
    /// Win menüsü açık: dock görünür ve yerinde kalır.
    start_open: bool,
    /// Canlı dock (kapalıyken `None`: hiçbir kaynak yok).
    live: Option<live::Live>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Uygulama meşgulse (modal menü içinden gelen iç içe mesaj) `false` döner.
fn with_app(f: impl FnOnce(&mut App)) -> bool {
    APP.with(|a| {
        if let Ok(mut a) = a.try_borrow_mut() {
            if let Some(a) = a.as_mut() {
                f(a);
            }
            true
        } else {
            false
        }
    })
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn track_leave(hwnd: HWND) {
    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
}

impl App {
    fn start_input(&mut self) {
        self.input = None;
        let edge = self.dock.behavior() != DockBehavior::AlwaysVisible;
        let cfg = &self.dock.cfg;
        let k = &cfg.shortcuts;
        let start = match k.win_key {
            WinKey::Menu => 2,
            WinKey::Search if cfg.launcher.enabled => 1,
            _ => 0,
        };
        // Ek ekran dock'u yalnızca kenar algılar; kısayollar ana dock'ta.
        // Kum havuzu hiç kanca kurmaz (gerçek dock'un girdisi etkilenmesin).
        self.input = if sandbox::on() {
            None
        } else if screens::child().is_some() {
            Input::start(self.hwnd, edge, None, 0, None, None)
        } else {
            Input::start(self.hwnd, edge, k.app_number_mods(), start, cfg.clipboard.enabled.then(|| k.clipboard_combo()).flatten(), k.converter_mods())
        };
        input::set_fullscreen(self.fullscreen);
        self.watch_locations();
        self.update_span();
        self.register_radial();
    }

    /// Çark dock kısayolunu ayara göre kaydeder ya da kaldırır.
    fn register_radial(&mut self) {
        let Some(r) = self.radial.as_mut() else { return };
        carlui_core::hotkey::unregister(self.hwnd, radial::HOTKEY_ID);
        r.vk = 0;
        if !self.dock.cfg.dock.radial {
            return;
        }
        if let Some(c) = self.dock.cfg.shortcuts.radial_combo() {
            match carlui_core::hotkey::register(self.hwnd, radial::HOTKEY_ID, c) {
                Ok(()) => {
                    r.vk = c.vk;
                    info!("çark dock kısayolu: {}", self.dock.cfg.shortcuts.radial);
                }
                Err(e) => carlui_core::warn!("çark dock kısayolu alınamadı: {e}"),
            }
        }
    }

    /// Kısayol: kapalıysa imleçte açar, açıksa kapatır.
    fn toggle_radial(&mut self, sticky: bool, at: Option<(i32, i32)>) {
        let hwnd = self.hwnd;
        let Some(r) = self.radial.as_mut() else { return };
        if r.open {
            r.hide(hwnd);
            self.popup.hide();
            return;
        }
        if self.fullscreen {
            return;
        }
        self.popup.hide();
        if let Err(e) = r.show(&self.gfx, &self.dock, hwnd, at) {
            carlui_core::warn!("çark dock açılamadı: {e}");
        }
        if sticky {
            r.set_sticky();
        }
    }

    fn radial_tick(&mut self) {
        let hwnd = self.hwnd;
        let Some(r) = self.radial.as_mut() else { return };
        match r.tick() {
            radial::Tick::Idle => {}
            radial::Tick::Hover => match r.hovered() {
                Some((k, key)) => {
                    let anchor = r.label_anchor(k);
                    let Some(e) = self.dock.entries.iter().find(|e| e.key == key) else {
                        self.popup.hide();
                        return;
                    };
                    let (key, name) = (e.key.clone(), e.name.clone());
                    let side = self.popup.side;
                    self.popup.side = config::DockPosition::Bottom;
                    let _ = self.popup.show_label(&self.gfx, &key, &name, anchor);
                    self.popup.side = side;
                }
                None => self.popup.hide(),
            },
            radial::Tick::Activate(key) => {
                r.hide(hwnd);
                self.popup.hide();
                // Halka açıkken liste kaymış olabilir: öğe anahtarıyla bulunur.
                if let Some(i) = self.dock.index_of(&key) {
                    let fg = unsafe { GetForegroundWindow() };
                    self.dock.activate(i, fg);
                }
            }
            radial::Tick::Close => {
                r.hide(hwnd);
                self.popup.hide();
            }
        }
    }

    /// Tam ekranda dock gizli: oyunun imleç/pencere kıpırtılarını dinlemeye gerek yok.
    fn watch_locations(&mut self) {
        self.tracker.watch_locations(!self.fullscreen && self.dock.behavior() == DockBehavior::Intellihide);
    }

    fn update_span(&mut self) {
        let (l, r, edge, side) = self.dock.edge_span();
        input::set_span(l, r, edge, side);
        self.popup.side = side;
    }

    fn refresh_windows(&mut self) {
        self.tracker.refresh();
        // Bırakma mesajı kaybolduysa (yakalama başka pencereye geçti) dock
        // "basılı" sanıp listeyi sonsuza dek ertelemesin.
        if self.dock.is_interacting() && unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
            info!("dock: takılı kalan basma iptal edildi");
            self.dock.cancel_press();
        }
        if self.dock.is_interacting() {
            self.sync_pending = true;
            return;
        }
        self.sync_pending = false;
        let _ = self.dock.sync(&self.gfx, &self.tracker, true);
        self.update_span();
        // Açık önizleme, pencere listesi değiştiyse yenilensin ya da kapansın.
        if self.popup.mode == Mode::Preview {
            let key = self.popup.key.clone();
            match self.dock.entries.iter().position(|e| e.key == key && !e.windows.is_empty()) {
                Some(i) if self.dock.entries[i].windows != self.popup.windows => self.show_preview(i),
                Some(i) => {
                    let _ = self.popup.retitle(&self.gfx, &self.preview_items(i));
                    // Başlık değişince (sekme geçişi) düğmeler başka karta geçebilir.
                    self.media_changed();
                    media::refresh(self.hwnd);
                }
                None => self.popup.hide(),
            }
        }
        self.intellihide_check();
    }

    /// Küçülen/büyüyen pencerenin animasyon hedefi: dock'taki ikonu.
    fn min_rect(&self, w: HWND) -> Option<RECT> {
        if self.dock.suppressed() {
            return None;
        }
        let root = unsafe { GetAncestor(w, GA_ROOTOWNER) };
        let i = self.dock.entries.iter().position(|e| e.windows.iter().any(|&h| h == w || h == root))?;
        Some(self.dock.tray_button(i).0)
    }

    /// Küçülen pencere dock'taki simgesine uçar, açılan pencere simgeden çıkar.
    /// Dock gizliyken (otomatik gizleme, tam ekran) uçacak yer olmadığından
    /// Windows'un kendi animasyonu kalır.
    fn fly(&self, w: HWND, minimizing: bool) {
        if self.dock.slid_away || !fly::enabled() {
            return;
        }
        if let Some(icon) = self.min_rect(w) {
            fly::start(w, icon, minimizing);
        }
    }

    fn intellihide_check(&mut self) {
        // Tray paneli açıkken dock yerinde kalır (panel dock'a bağlı açılır).
        if self.dock.behavior() != DockBehavior::Intellihide || self.dock.mouse_inside || self.dock.menu_open || self.tray_open || self.start_open {
            return;
        }
        let covered = self.dock.overlapped(&self.tracker);
        if covered && !self.popup.mouse_inside {
            self.popup.hide();
        }
        self.dock.slide(covered);
    }

    fn preview_items(&self, i: usize) -> Vec<(HWND, String)> {
        self.dock.entries[i].windows.iter().map(|&h| (h, tracker::title(h))).collect()
    }

    fn show_preview(&mut self, i: usize) {
        let items = self.preview_items(i);
        let e = &self.dock.entries[i];
        let key = e.key.clone();
        let session = media::find(&e.app);
        let anchor = self.dock.anchor(i);
        let _ = self.popup.show_preview(&self.gfx, &key, &items, session, anchor);
        media::refresh(self.hwnd);
    }

    /// Medya oturumları değişti: açık önizleme güncellensin.
    fn media_changed(&mut self) {
        if self.popup.mode != Mode::Preview {
            return;
        }
        let Some(i) = self.dock.entries.iter().position(|e| e.key == self.popup.key) else { return };
        if !self.popup.update_media(media::find(&self.dock.entries[i].app)) {
            self.show_preview(i);
        }
    }

    /// Fare dock üzerinde dolaştıkça etiket/önizleme durumunu günceller.
    fn update_hover(&mut self) {
        let hwnd = self.hwnd;
        match self.dock.hovered() {
            Some(i) => unsafe {
                let _ = KillTimer(hwnd, TIMER_GRACE);
                let e = &self.dock.entries[i];
                let key = e.key.clone();
                if self.popup.key == key && self.popup.mode != Mode::Hidden {
                    return;
                }
                let has_windows = e.kind == Kind::App && !e.windows.is_empty();
                // Önizleme açıkken başka bir uygulamaya geçince bekletmeden onunkini göster.
                if self.popup.mode == Mode::Preview && has_windows {
                    self.show_preview(i);
                    return;
                }
                let _ = KillTimer(hwnd, TIMER_PREVIEW);
                if self.dock.cfg.dock.show_labels {
                    let name = e.name.clone();
                    let anchor = self.dock.anchor(i);
                    let _ = self.popup.show_label(&self.gfx, &key, &name, anchor);
                } else {
                    self.popup.hide();
                    self.popup.key = key;
                }
                if has_windows && self.dock.cfg.dock.show_previews {
                    SetTimer(hwnd, TIMER_PREVIEW, 450, None);
                    // Önizleme açılana dek oturum listesi hazır olsun.
                    media::refresh(hwnd);
                }
            },
            None => unsafe {
                let _ = KillTimer(hwnd, TIMER_PREVIEW);
                if self.popup.mode != Mode::Hidden {
                    SetTimer(hwnd, TIMER_GRACE, 200, None);
                }
            },
        }
    }

    fn arm_autohide(&self, ms: u32) {
        if self.dock.behavior() != DockBehavior::AlwaysVisible {
            unsafe {
                SetTimer(self.hwnd, dock::TIMER_AUTOHIDE, ms, None);
            }
        }
    }

    fn autohide_tick(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, dock::TIMER_AUTOHIDE);
        }
        if self.dock.mouse_inside || self.popup.mouse_inside || self.dock.menu_open || self.tray_open || self.start_open {
            return;
        }
        if cfg!(debug_assertions) || std::env::var_os("CARLUI_TRACE").is_some() {
            info!("otomatik gizleme zamanlayıcısı (dock_in={}, pop_in={})", self.dock.mouse_inside, self.popup.mouse_inside);
        }
        match self.dock.behavior() {
            DockBehavior::AutoHide => {
                self.popup.hide();
                self.dock.slide(true);
            }
            DockBehavior::Intellihide => self.intellihide_check(),
            DockBehavior::AlwaysVisible => {}
        }
    }

    fn context_menu(&mut self, idx: Option<usize>) {
        self.popup.hide();
        // Yerlerin kendi menüleri var (bkz. dock/places.rs).
        match idx.map(|i| (i, self.dock.entries[i].kind)) {
            Some((i, Kind::Trash)) => {
                self.dock.trash_menu(i);
                self.arm_autohide(900);
                return;
            }
            Some((i, Kind::Folder)) => {
                if self.dock.entries[i].key == dock::quick::KEY {
                    self.dock.quick_menu(i);
                    // Eklenen öğe yığının simgesinde hemen görünsün.
                    let _ = self.dock.sync(&self.gfx, &self.tracker, true);
                } else {
                    self.dock.stack_menu(i);
                }
                self.arm_autohide(900);
                return;
            }
            _ => {}
        }
        let (at, side) = match idx {
            Some(i) => self.dock.menu_at(i),
            None => {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                }
                // Boş yere sağ tık: imlecin hizasında, dock'un iç kenarında.
                let ((x, y), side) = self.dock.menu_at(0);
                (if side == 0 { (pt.x, y) } else { (x, pt.y) }, side)
            }
        };
        self.dock.menu_open = true;
        let pids = idx.map(|i| procs::pids(&self.dock.entries[i])).unwrap_or_default();
        let jump = idx.filter(|&i| self.dock.entries[i].kind == Kind::App).map(|i| jumplist::load(&self.dock.entries[i])).unwrap_or_default().into_iter().filter(|g| g.title.is_empty() || self.dock.cfg.dock.menu_lists).collect::<Vec<_>>();
        let cmd = menu::show(self.hwnd, &self.dock.cfg.dock, idx.map(|i| &self.dock.entries[i]), procs::suspended(&pids), &jump, at, side);
        self.dock.menu_open = false;
        if let menu::Cmd::Jump(n) = cmd {
            if let Some(it) = jump.iter().flat_map(|g| &g.items).nth(n) {
                info!("uygulama görevi: {}", it.title);
                it.run();
            }
        }
        self.run_cmd(cmd, idx, &pids);
        self.arm_autohide(900);
    }

    fn run_cmd(&mut self, cmd: menu::Cmd, idx: Option<usize>, pids: &[u32]) {
        let name = idx.map(|i| self.dock.entries[i].name.clone()).unwrap_or_default();
        match (cmd, idx) {
            (menu::Cmd::TaskManager, _) => procs::task_manager(pids.first().copied()),
            (menu::Cmd::Suspend, Some(_)) => procs::suspend(pids, &name),
            (menu::Cmd::Resume, Some(_)) => procs::resume(pids, &name),
            (menu::Cmd::Kill, Some(_)) => procs::kill(pids, &name),
            (menu::Cmd::NewWindow, Some(i)) => self.dock.launch(i),
            (menu::Cmd::TogglePin, Some(i)) => {
                self.dock.toggle_pin(&self.gfx, &self.tracker, i);
                self.update_span();
            }
            (menu::Cmd::CloseWindow | menu::Cmd::CloseAll, Some(i)) => {
                for &w in &self.dock.entries[i].windows {
                    tracker::close(w);
                }
            }
            (menu::Cmd::AddQuick, Some(i)) => {
                let e = &self.dock.entries[i];
                // Sabitlenmişte kendi yolu (.lnk/exe/AppsFolder), değilse AUMID ya da exe.
                let exe = if e.exe.is_empty() { e.app.exe.clone() } else { e.exe.clone() };
                let mut targets = Vec::new();
                if let Some(p) = &e.pin {
                    targets.push(p.path.clone());
                }
                if !e.app.aumid.is_empty() {
                    targets.push(format!("shell:AppsFolder\\{}", e.app.aumid));
                }
                targets.push(exe);
                let target = targets.join(" | ");
                if targets.iter().any(|t| dock::quick::add(t)) {
                    let _ = self.dock.sync(&self.gfx, &self.tracker, true);
                } else {
                    carlui_core::warn!("hızlı erişime eklenemedi: {target}");
                }
            }
            (menu::Cmd::Settings, _) => procs::settings(),
            (menu::Cmd::RestoreTaskbar, _) => {
                config::update(|c| c.general.hide_taskbar = false);
                self.quit();
            }
            (menu::Cmd::Quit, _) => self.quit(),
            _ => {}
        }
    }

    /// Test: `dock-cmd {name, cmd}` — menüyü açmadan bir uygulamaya komut uygular.
    fn cmd_event(&mut self, data: &serde_json::Value) {
        let s = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
        let (name, cmd) = (s("name"), s("cmd"));
        let Some(i) = self.dock.entries.iter().position(|e| e.name.to_lowercase().contains(&name)) else { return };
        let cmd = match cmd.as_str() {
            "suspend" => menu::Cmd::Suspend,
            "resume" => menu::Cmd::Resume,
            "kill" => menu::Cmd::Kill,
            "taskmgr" => menu::Cmd::TaskManager,
            _ => return,
        };
        let pids = procs::pids(&self.dock.entries[i]);
        self.run_cmd(cmd, Some(i), &pids);
    }

    /// Olay yoluna gönderir (kum havuzunda yol yok: sessizce atlanır).
    fn send_bus(&self, ev: &Event) {
        if let Some(b) = self.bus.as_ref() {
            b.send(ev);
        }
    }

    fn quit(&self) {
        if let Some(bus) = self.bus.as_ref().filter(|b| b.is_connected()) {
            bus.send(&Event::Custom { name: "quit".into(), data: Default::default() });
        } else {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    fn win_key(&mut self, n: usize) {
        let apps: Vec<usize> = (0..self.dock.entries.len()).filter(|&i| self.dock.entries[i].kind == Kind::App).collect();
        if let Some(&i) = apps.get(n.saturating_sub(1)) {
            self.dock.activate(i, unsafe { GetForegroundWindow() });
        }
    }

    /// Test/tanı: dock'un o anki durumunu JSON olarak yazar.
    fn dump_state(&self) {
        let entries: Vec<serde_json::Value> = self
            .dock
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (x, y) = self.dock.icon_center(i);
                let jump: Vec<String> = if e.kind == Kind::App { jumplist::load(e).iter().flat_map(|g| g.items.iter().map(move |it| format!("{} › {}", g.title, it.title))).collect() } else { Vec::new() };
                serde_json::json!({
                    "key": e.key, "name": e.name, "group": e.group, "kind": format!("{:?}", e.kind),
                    "windows": e.windows.len(), "pinned": e.pin.is_some(), "x": x, "y": y,
                    "jump": jump, "jump_ids": if e.kind == Kind::App { jumplist::debug_ids(e) } else { Vec::new() },
                })
            })
            .collect();
        let state = serde_json::json!({
            "entries": entries,
            "tracked": self.tracker.windows.iter().map(|w| format!("{} {} {}", carlui_core::win::class_name(w.hwnd), tracker::title(w.hwnd), w.app.key())).collect::<Vec<_>>(),
            "slid_away": self.dock.slid_away,
            "behavior": format!("{:?}", self.dock.behavior()),
            "suppressed": self.dock.is_suppressed(),
            "popup": format!("{:?}", self.popup.mode),
            "popup_key": self.popup.key,
            "popup_windows": self.popup.windows.len(),
            "popup_inside": self.popup.mouse_inside,
            "dock_inside": self.dock.mouse_inside,
            "dock_hovered": self.dock.hovered(),
            "rehooks": input::REHOOKS.load(std::sync::atomic::Ordering::SeqCst),
            "input": self.input.is_some(),
            "menu_accent": format!("{:?} {:06x?}", self.dock.cfg.dock.menu_accent, self.dock.cfg.dock.menu_accent_color(carlui_ui::glassmenu::accent)),
        });
        let file = match screens::child() {
            Some(n) => format!("dock-state-screen{n}.json"),
            None => "dock-state.json".into(),
        };
        let path = paths::log_dir().join("..").join(file);
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap_or_default());
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Shutdown => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            Event::Fullscreen { active, exclusive, monitor } => {
                // Win tuşu yalnızca özel tam ekran oyunda Windows'a bırakılır.
                input::set_exclusive(active && exclusive);
                // Windows görev çubuğu gibi: yalnızca kendi monitöründeki tam ekranda gizlen.
                let active = active && carlui_core::win::fullscreen_here(self.hwnd, &monitor);
                if active == self.fullscreen {
                    return;
                }
                self.fullscreen = active;
                input::set_fullscreen(active);
                self.watch_locations();
                if active {
                    self.popup.hide();
                } else if self.dock.behavior() == DockBehavior::Intellihide && !self.dock.mouse_inside && self.dock.overlapped(&self.tracker) {
                    // Tam ekrandan büyütülmüş pencereye dönüş: dock bir an belirip kaybolmasın.
                    self.dock.park();
                }
                self.dock.set_suppressed(active);
                if !active {
                    self.intellihide_check();
                }
            }
            Event::ShowDesktop => self.dock.reassert(),
            Event::ExplorerRestarted => self.dock.reassert(),
            Event::Custom { name, data } if name == "tray-state" => {
                let open = data.get("open").and_then(|v| v.as_bool()).unwrap_or(false);
                self.tray_open = open;
                // Eski tray "chevron" göndermez: açık olması yeter.
                let chevron = data.get("chevron").and_then(|v| v.as_bool()).unwrap_or(open);
                self.dock.set_tray_open(chevron);
                if !open {
                    self.arm_autohide(900);
                }
            }
            Event::Custom { name, .. } if name == "dock-dump" => self.dump_state(),
            // Adadan hayvan tepkisi sinyali (bildirim/müzik; içerik taşımaz).
            Event::Custom { name, data } if name.starts_with("pet-") => {
                if let Some(l) = self.live.as_mut() {
                    l.pet_signal(&name, &data);
                }
            }
            // Test: çark dock imleçte, tıklama bekleyen kipte açılır.
            Event::Custom { name, data } if name == "dock-radial" => {
                let at = data.get("x").and_then(|x| x.as_i64()).zip(data.get("y").and_then(|y| y.as_i64())).map(|(x, y)| (x as i32, y as i32));
                self.toggle_radial(true, at)
            }
            Event::Custom { name, data } if name == "dock-cmd" => self.cmd_event(&data),
            // Test: klasör yığınını (anahtarıyla, ör. "downloads") açar.
            Event::Custom { name, data } if name == "dock-stack" => {
                let key = data.get("key").and_then(|v| v.as_str()).unwrap_or("downloads");
                if let Some(i) = self.dock.entries.iter().position(|e| e.kind == Kind::Folder && (e.key == key || e.key.contains(key))) {
                    self.dock.stack_menu(i);
                }
            }
            // Test: enjekte edilen Win tuşu kancada yok sayıldığı için akor bus ile de tetiklenebilir.
            Event::Custom { name, .. } if name == "dock-convert" => procs::converter(),
            Event::Custom { name, .. } if name == "dock-stall-input" => {
                if let Some(i) = &self.input {
                    i.stall_once();
                }
            }
            _ => {}
        }
    }

    fn apply_config(&mut self, cfg: Config) {
        // Dock'un kendi yazdığı değişiklik (sıralama/sabitleme) zaten uygulanmış durumda.
        if cfg == self.dock.cfg {
            return;
        }
        // Yalnızca dock'u ilgilendirmeyen bölümler (imleç, ada, bildirim, saat,
        // oyun, istatistik) değiştiyse baştan kurma; dock titremesin.
        let old = &self.dock.cfg;
        let mut mine = cfg.clone();
        mine.cursor = old.cursor.clone();
        mine.island = old.island.clone();
        mine.notify = old.notify.clone();
        mine.clock = old.clock.clone();
        mine.game = old.game.clone();
        mine.stats = old.stats.clone();
        if mine == *old {
            self.dock.cfg = cfg;
            return;
        }
        // Yalnızca canlı dock değiştiyse dock'u baştan kurma; yalnızca onu yenile.
        mine.dock.live = old.dock.live.clone();
        if mine == *old {
            self.dock.cfg = cfg;
            self.rebuild_live();
            return;
        }
        // Ek ekran dock'u: seçenek kapandıysa ya da monitörü yoksa kapan.
        if screens::child().is_some() && (!cfg.dock.other_screens || screens::monitor().is_none()) {
            info!("ek ekran dock'u kapanıyor");
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            return;
        }
        if cfg.dock.other_screens && !self.dock.cfg.dock.other_screens {
            screens::spawn(true);
        }
        self.tracker.set_monitor(screens::filter(cfg.dock.other_screens));
        self.popup.hide();
        if let Some(r) = self.radial.as_mut() {
            r.hide(self.hwnd);
        }
        // Canlı dock eski camın görsellerine bağlı: dock'tan önce bırakılır.
        self.live = None;
        match Dock::new(&self.gfx, self.hwnd, &self.target, &cfg, &self.tracker, screens::rect()) {
            Ok(mut d) => {
                d.set_suppressed(self.fullscreen);
                self.dock = d;
                self.rebuild_live();
                if let Ok(p) = Popup::new(&self.gfx, self.popup.hwnd, &self.dock.theme, self.dock.scale()) {
                    self.popup = p;
                }
                self.start_input();
                self.arm_autohide(1500);
                info!("dock yeni ayarlarla yeniden kuruldu");
            }
            Err(e) => carlui_core::error!("dock yeniden kurulamadı: {e}"),
        }
    }

    /// Canlı dock'u ayara göre (yeniden) kurar; kapalıysa tüm kaynaklarını bırakır.
    fn rebuild_live(&mut self) {
        let cfg = &self.dock.cfg.dock.live;
        if self.live.as_ref().is_some_and(|l| l.config() == cfg) {
            return;
        }
        self.live = None;
        self.live = live::Live::new(cfg, &self.gfx, &self.dock, self.fullscreen, self.tracker.windows.len());
    }
}

/// Dock penceresi: önce asıl işleyici, sonra canlı dock (yalnızca açıksa)
/// aynı mesaja bakar. Kapalıyken maliyet tek bir `Option` kontrolüdür.
extern "system" fn dock_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let r = dock_proc_inner(hwnd, msg, wp, lp);
    with_app(|a| {
        if let Some(l) = a.live.as_mut() {
            l.on_message(&a.gfx, &a.dock, a.fullscreen, a.tracker.windows.len(), msg, wp, lp);
        }
    });
    r
}

fn dock_proc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // Dock'a tıklamak odaktaki pencereden odağı çalmasın.
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                track_leave(hwnd);
                let (x, y) = xy(lp);
                with_app(|a| {
                    a.dock.mouse_move(x, y);
                    let _ = KillTimer(hwnd, dock::TIMER_AUTOHIDE);
                    a.update_hover();
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                if std::env::var_os("CARLUI_TRACE").is_some() {
                    info!("dock: fare çıktı");
                }
                with_app(|a| {
                    a.dock.mouse_leave();
                    a.update_hover();
                    a.arm_autohide(700);
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                let (x, y) = xy(lp);
                with_app(|a| {
                    a.dock.mouse_down(x, y);
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let (x, y) = xy(lp);
                with_app(|a| {
                    let clicked = a.dock.mouse_up(&a.gfx, &a.tracker, x, y);
                    a.update_span();
                    if clicked.is_none() {
                        // Görev çubuğu görünümünde sağ uçtaki durum alanı.
                        // Her simge kendi panelini açar: ağ → ağ listesi, ses → ses ve uygulama sesleri.
                        match a.dock.status_hit(x, y) {
                            Some(p @ (dock::Part::Net | dock::Part::Volume)) if screens::child().is_none() => {
                                a.popup.hide();
                                let (r, top) = a.dock.status_rect(p);
                                let mode = if p == dock::Part::Net { "net" } else { "sound" };
                                a.send_bus(&Event::Custom {
                                    name: "tray-toggle".into(),
                                    data: serde_json::json!({ "l": r.left, "t": r.top, "r": r.right, "b": r.bottom, "dock_top": top, "side": a.dock.side() as u8, "mode": mode }),
                                });
                            }
                            Some(dock::Part::Battery) => dock::open_uri("ms-settings:batterysaver"),
                            Some(dock::Part::Net | dock::Part::Volume) => dock::open_uri("ms-actioncenter:controlcenter/&showFooter=true"),
                            Some(dock::Part::Clock) => dock::open_uri("ms-actioncenter:"),
                            None => {}
                        }
                    }
                    if let Some(i) = clicked {
                        a.popup.hide();
                        if a.dock.entries[i].kind == Kind::Tray {
                            let (r, top) = a.dock.tray_button(i);
                            // Görev çubuğunda ses/ağ kendi simgelerinde: ok yalnızca uygulamalar ve parlaklık.
                            let mode = if a.dock.taskbar() { "tray" } else { "full" };
                            a.send_bus(&Event::Custom {
                                name: "tray-toggle".into(),
                                data: serde_json::json!({ "l": r.left, "t": r.top, "r": r.right, "b": r.bottom, "dock_top": top, "side": a.dock.side() as u8, "mode": mode }),
                            });
                        } else if a.dock.entries[i].kind == Kind::Folder {
                            a.dock.stack_menu(i);
                        } else {
                            // Birden çok pencere varken hiçbiri önde değilse hangisini
                            // istediğini tahmin etme: önizlemeden seçtir. Öndeyse sırayla dön.
                            let e = &a.dock.entries[i];
                            let fg = GetForegroundWindow();
                            if e.kind == Kind::App && e.windows.len() > 1 && !e.windows.contains(&fg) {
                                a.show_preview(i);
                            } else {
                                a.dock.activate(i, fg);
                            }
                        }
                    }
                    if a.sync_pending {
                        a.refresh_windows();
                    }
                });
                LRESULT(0)
            }
            WM_MBUTTONUP => {
                // Orta tık: yeni pencere.
                let (x, y) = xy(lp);
                with_app(|a| {
                    if let Some(i) = a.dock.hit(x, y).filter(|&i| a.dock.entries[i].kind == Kind::App) {
                        a.dock.launch(i);
                    }
                });
                LRESULT(0)
            }
            WM_HOTKEY if wp.0 as i32 == radial::HOTKEY_ID => {
                with_app(|a| a.toggle_radial(false, None));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    let idx = a.dock.hit(x, y);
                    // Ağ simgesine sağ tık: kablo bağlıysa Ethernet, değilse Wi-Fi ayarları.
                    if idx.is_none() && a.dock.status_hit(x, y) == Some(dock::Part::Net) {
                        a.popup.hide();
                        dock::open_uri(if carlui_core::wired::wired_up() { "ms-settings:network-ethernet" } else { "ms-settings:network-wifi" });
                        return;
                    }
                    a.context_menu(idx);
                });
                LRESULT(0)
            }
            WM_TIMER => {
                let id = wp.0;
                with_app(|a| match id {
                    radial::TIMER => a.radial_tick(),
                    dock::TIMER_REGION => a.dock.fit_region(),
                    dock::TIMER_CLEANUP => {
                        let _ = KillTimer(hwnd, id);
                        a.dock.cleanup();
                    }
                    dock::TIMER_AUTOHIDE => a.autohide_tick(),
                    // Kum havuzu: gizli dock'u geri getir, sonra yine gizlensin.
                    sandbox::TIMER_CYCLE => {
                        if a.dock.live_hidden().1 {
                            a.dock.slide(false);
                            a.arm_autohide(2500);
                        }
                    }
                    dock::TIMER_STATUS => {
                        if a.dock.taskbar() {
                            a.dock.status_tick(&a.gfx);
                        } else {
                            let _ = KillTimer(hwnd, id);
                        }
                    }
                    dock::TIMER_TRASH => {
                        if a.dock.cfg.dock.recycle_bin {
                            SetTimer(hwnd, dock::TIMER_TRASH, 3000, None);
                            a.dock.trash_tick(&a.gfx, false);
                        } else {
                            let _ = KillTimer(hwnd, id);
                        }
                    }
                    TIMER_PREVIEW => {
                        let _ = KillTimer(hwnd, id);
                        if let Some(i) = a.dock.hovered() {
                            if a.dock.entries[i].key == a.popup.key && !a.dock.entries[i].windows.is_empty() {
                                a.show_preview(i);
                            }
                        }
                    }
                    TIMER_REFRESH => {
                        let _ = KillTimer(hwnd, id);
                        a.refresh_windows();
                    }
                    TIMER_GRACE => {
                        let _ = KillTimer(hwnd, id);
                        let over_popup = a.popup.mouse_inside && a.popup.mode == Mode::Preview;
                        if a.dock.hovered().is_none() && !over_popup {
                            a.popup.hide();
                        }
                    }
                    _ => {}
                });
                LRESULT(0)
            }
            carlui_core::win::WM_CARLUI_DOCK_HOLD => {
                with_app(|a| {
                    a.start_open = wp.0 != 0;
                    info!("Win menüsü {}", if a.start_open { "açık: dock tutuluyor" } else { "kapandı" });
                    if a.start_open {
                        a.dock.slide(false);
                    } else {
                        a.arm_autohide(900);
                    }
                });
                LRESULT(0)
            }
            tracker::WM_APP_WINDOWS => {
                // Menü açıkken gelen yenileme yutulursa tracker "bekliyor" bayrağı
                // açık kalır ve dock yeni pencereleri bir daha görmez: kısa süre sonra yeniden dene.
                if !with_app(|a| a.refresh_windows()) {
                    SetTimer(hwnd, TIMER_REFRESH, 150, None);
                }
                LRESULT(0)
            }
            tracker::WM_APP_MINMAX => {
                with_app(|a| a.fly(HWND(wp.0 as _), lp.0 != 0));
                LRESULT(0)
            }
            media::WM_APP_MEDIA => {
                with_app(|a| a.media_changed());
                LRESULT(0)
            }
            tracker::WM_APP_LOCATION => {
                tracker::location_handled();
                with_app(|a| a.intellihide_check());
                LRESULT(0)
            }
            input::WM_APP_EDGE => {
                if wp.0 == 1 {
                    with_app(|a| {
                        a.dock.slide(false);
                        // İmleç dock'a hiç girmezse bir süre sonra yine gizlensin.
                        a.arm_autohide(1200);
                    });
                }
                LRESULT(0)
            }
            input::WM_APP_WINKEY => {
                with_app(|a| a.win_key(wp.0));
                LRESULT(0)
            }
            input::WM_APP_CONVERT => {
                procs::converter();
                LRESULT(0)
            }
            WM_APP_BUS => {
                with_app(|a| {
                    let events: Vec<Event> = a.inbox.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
                    for ev in events {
                        a.handle_event(ev);
                    }
                });
                LRESULT(0)
            }
            WM_APP_CONFIG => {
                with_app(|a| {
                    if let Some(cfg) = a.pending_cfg.lock().ok().and_then(|mut p| p.take()) {
                        a.apply_config(cfg);
                    }
                });
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                // Çözünürlük/ölçek değişince dock aynı ayarlarla yeniden kurulsun.
                with_app(|a| {
                    // Monitör takıldıysa onun dock'u da gelsin (çalışanlar kendiliğinden çıkar).
                    screens::spawn(a.dock.cfg.dock.other_screens);
                    let mut cfg = a.dock.cfg.clone();
                    cfg.dock.icon_size += 0.0001; // eşitlik kontrolünü atlat
                    a.apply_config(cfg);
                    let mut c = a.dock.cfg.clone();
                    c.dock.icon_size = config::load().dock.icon_size;
                    a.dock.cfg = c;
                });
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                track_leave(hwnd);
                let (x, y) = xy(lp);
                with_app(|a| {
                    a.popup.mouse_move(x, y);
                    let _ = KillTimer(a.hwnd, TIMER_GRACE);
                    let _ = KillTimer(a.hwnd, dock::TIMER_AUTOHIDE);
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| {
                    a.popup.mouse_leave();
                    SetTimer(a.hwnd, TIMER_GRACE, 200, None);
                    a.arm_autohide(700);
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| match a.popup.click(x, y) {
                    Some(popup::Action::Activate(w)) => {
                        a.popup.hide();
                        tracker::activate(w);
                    }
                    Some(popup::Action::Close(w)) => tracker::close(w),
                    Some(popup::Action::Media(app, cmd)) => {
                        if cmd == media::Cmd::PlayPause {
                            a.popup.toggle_play();
                        }
                        media::send(app, cmd, a.hwnd);
                    }
                    None => {}
                });
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| match wp.0 {
                    popup::TIMER_THUMBS => a.popup.show_thumbs(),
                    popup::TIMER_HIDE => a.popup.finish_hide(),
                    _ => {}
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn create_window(class: PCWSTR, proc: WNDPROC, title: PCWSTR) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            title,
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
        let size = std::mem::size_of::<BOOL>() as u32;
        // Host backdrop brush'ın masaüstünü örnekleyebilmesi için gerekli.
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const _ as *const _, size);
        // Aero Peek / Win+Tab önizlemelerinde görünmesin.
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, size);
        Ok(hwnd)
    }
}

/// İlk açılışta Windows görev çubuğundaki sabitlenmişleri aynı sırayla içeri aktar.
fn first_run_import(cfg: &mut Config) {
    if cfg.dock.imported_taskbar_pins {
        return;
    }
    let pins = apps::import_taskbar_pins();
    info!("görev çubuğundan {} sabitlenmiş uygulama içeri aktarıldı", pins.len());
    if cfg.dock.pinned.is_empty() {
        cfg.dock.pinned = pins.into_iter().filter(|p| !p.path.to_lowercase().ends_with("explorer.exe")).collect();
    }
    cfg.dock.imported_taskbar_pins = true;
    let pinned = cfg.dock.pinned.clone();
    config::update(|c| {
        c.dock.pinned = pinned;
        c.dock.imported_taskbar_pins = true;
    });
}

fn main() -> Result<()> {
    let sandboxed = sandbox::on();
    log::init(if sandboxed { "dock-sandbox" } else { "dock" });
    unsafe {
        let child = if sandboxed { None } else { screens::child() };
        let _mutex = CreateMutexW(None, true, &screens::mutex_name())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            info!("dock zaten çalışıyor");
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        // Composition, bu thread'de bir DispatcherQueue ister.
        let _queue = CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?;
        menu::enable_dark_menus();

        let mut cfg = config::load();
        sandbox::tweak(&mut cfg);
        carlui_ui::tone::follow(&cfg.theme);
        if sandboxed {
            if screens::monitor().is_none() {
                info!("kum havuzu: ek monitör yok, açılmıyor");
                return Ok(());
            }
            info!("kum havuzu dock'u açılıyor (canlı: {:?} / {:?})", cfg.dock.live.pet, cfg.dock.live.scene);
        } else if let Some(n) = child {
            if !cfg.dock.other_screens || screens::monitor().is_none() {
                info!("ek ekran dock'u ({}) gerekmiyor", n + 1);
                return Ok(());
            }
            info!("ek ekran dock'u ({}) açılıyor", n + 1);
        } else {
            first_run_import(&mut cfg);
        }

        // Ek ekran dock'u ayrı sınıf adı kullanır: Win menüsü ve diğer
        // modüller ana dock'u sınıf adıyla bulur.
        let (class, popup_class) = if sandboxed {
            (w!("CarlUI.DockSandbox"), w!("CarlUI.DockSandboxPopup"))
        } else if child.is_some() { (w!("CarlUI.Dock2"), w!("CarlUI.Dock2Popup")) } else { (w!("CarlUI.Dock"), w!("CarlUI.DockPopup")) };
        let hwnd = create_window(class, Some(dock_proc), w!("CarlUI Dock"))?;
        let popup_hwnd = create_window(popup_class, Some(popup_proc), w!("CarlUI Dock Popup"))?;
        let hwnd_raw = hwnd.0 as isize;
        let gfx = Gfx::new()?;
        let target = gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)?;
        let mut tracker = Tracker::new(hwnd);
        tracker.set_monitor(screens::filter(cfg.dock.other_screens));
        let dock = Dock::new(&gfx, hwnd, &target, &cfg, &tracker, screens::rect())?;
        let popup = Popup::new(&gfx, popup_hwnd, &dock.theme, dock.scale())?;
        let radial = if child.is_none() && !sandboxed { radial::Radial::new(&gfx, &dock.theme).map_err(|e| carlui_core::warn!("çark dock kurulamadı: {e}")).ok() } else { None };

        let inbox: Arc<Mutex<VecDeque<Event>>> = Arc::default();
        let inbox_bus = inbox.clone();
        let bus_name = child.map(|n| format!("dock2-{n}")).unwrap_or_else(|| "dock".into());
        let bus = (!sandboxed).then(|| {
            BusClient::connect(&bus_name, move |ev| {
                if let Ok(mut q) = inbox_bus.lock() {
                    q.push_back(ev);
                }
                let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_BUS, WPARAM(0), LPARAM(0));
            })
        });

        let pending_cfg: Arc<Mutex<Option<Config>>> = Arc::default();
        let pending = pending_cfg.clone();
        config::watch(move |mut c| {
            sandbox::tweak(&mut c);
            carlui_ui::tone::restart_on_change(&c.theme);
            if let Ok(mut p) = pending.lock() {
                *p = Some(c);
            }
            let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_CONFIG, WPARAM(0), LPARAM(0));
        });

        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                gfx,
                target,
                hwnd,
                dock,
                popup,
                radial,
                tracker,
                input: None,
                bus,
                inbox,
                pending_cfg,
                fullscreen: false,
                sync_pending: false,
                tray_open: false,
                start_open: false,
                live: None,
            })
        });
        with_app(|a| {
            a.start_input();
            a.arm_autohide(1500);
            a.rebuild_live();
        });
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        screens::spawn(cfg.dock.other_screens);
        if sandbox::cycle() {
            SetTimer(hwnd, sandbox::TIMER_CYCLE, sandbox::CYCLE_MS, None);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| a.borrow_mut().take());
        info!("dock kapandı");
    }
    Ok(())
}
