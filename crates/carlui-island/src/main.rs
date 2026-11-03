//! Ada modülü (`carlui-island.exe`): ekranın üst ortasındaki sabit cam hap.
//!
//! - Normalde saati gösterir; tıklayınca birkaç saniye tarih de görünür.
//! - Bildirim gelince aşağı genişleyip bildirimi gösterir, süre dolunca kendi
//!   küçülür. Tıklanırsa bildirimin geldiği uygulamayı açar.
//! - Bir şey çalarken kapak + zıplayan çubuklar görünür; tıklayınca medya
//!   paneli açılır: ilerleme çubuğu (sarılabilir), önceki/oynat/sonraki ve
//!   yalnızca o uygulamanın sesi. Komutlar şarkıyı çalan uygulamaya gider.
//! - Tam ekran uygulama/oyun açıkken tamamen gizlenir, bildirim göstermez.
//!
//! Boştayken hiçbir şey yoklanmaz: saat dakikada bir, medya ve bildirimler
//! olayla güncellenir; animasyonlar compositor'de çalışır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod activate;
mod audio;
mod banners;
mod clipper;
mod clipgame;
mod bridge;
mod clipping;
mod dlman;
mod offering;
mod downloads;
mod editor;
mod hubbing;
mod island;
mod launcher;
mod media;
mod netwatch;
mod notify;
mod searching;
mod shots;
mod steam;
mod timing;
mod update;
mod viewer;

use carlui_core::{
    bus::{BusClient, Event},
    config::{self, ClockMode, Config},
    info, log, paths,
    theme::Theme,
};
use carlui_ui::gfx::Gfx;
use carlui_core::win::WM_CARLUI_SEARCH;
use island::{Hit, Island, Model, View};
use launcher::{IconPx, Key, Launcher, Outcome};
use media::{Cmd, Snap};
use notify::Note;
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        atomic::{AtomicI32, AtomicIsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        Media::Audio::ISimpleAudioVolume,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::CreateMutexW,
            WinRT::*,
        },
        UI::{
            Accessibility::*,
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_APP_BUS: u32 = WM_APP + 1;
const WM_APP_MEDIA: u32 = WM_APP + 2;
const WM_APP_NOTE: u32 = WM_APP + 3;
const WM_APP_CONFIG: u32 = WM_APP + 4;
const WM_APP_FG: u32 = WM_APP + 5;
const WM_APP_PEEK: u32 = WM_APP + 6;
const WM_APP_SPOT_WAKE: u32 = WM_APP + 7;
const WM_APP_ICONS: u32 = WM_APP + 8;

const HOTKEY_SEARCH: i32 = 1;

const TIMER_CLOCK: usize = 1;
const TIMER_NOTE: usize = 2;
const TIMER_COLLAPSE: usize = 3;
const TIMER_DATE: usize = 4;
const TIMER_TICK: usize = 5;
const TIMER_FG: usize = 6;
const TIMER_PAUSED: usize = 7;
const TIMER_PEEK: usize = 8;
/// Oyundan çıkınca ada yerine oturduktan sonra kaçan bildirimleri göster.
const TIMER_MISSED: usize = 14;
/// Oyunda en fazla bu kadar bildirim saklanır (en yeniler).
const MISSED_MAX: usize = 20;

/// Duraklatılan şarkı bu kadar süre adada kalır.
const PAUSED_GRACE: Duration = Duration::from_secs(60);

static HWND_ISLAND: AtomicIsize = AtomicIsize::new(0);
static PEEK_TOP: AtomicI32 = AtomicI32::new(0);
static PEEK_L: AtomicI32 = AtomicI32::new(0);
static PEEK_R: AtomicI32 = AtomicI32::new(0);

struct App {
    gfx: Gfx,
    island: Island,
    bus: BusClient,
    inbox: Arc<Mutex<VecDeque<Event>>>,
    cfg: Config,
    cfg_slot: Arc<Mutex<Option<Config>>>,
    media_slot: Arc<Mutex<Option<Snap>>>,
    media: Option<Snap>,
    paused_since: Option<Instant>,
    notes_in: Arc<Mutex<VecDeque<Note>>>,
    pending: VecDeque<Note>,
    note: Option<Note>,
    /// Son gösterilen bildirimler (tray balonu + aynı bildirimin Windows kopyası tekrar görünmesin).
    recent: VecDeque<(String, Instant)>,
    expanded: bool,
    date: bool,
    fullscreen: bool,
    desktop_fg: bool,
    covered: bool,
    peek: bool,
    vols: Vec<ISimpleAudioVolume>,
    vols_app: String,
    dragging: Hit,
    pressed: Hit,
    mon: RECT,
    fg_hooks: Vec<HWINEVENTHOOK>,
    loc_hook: HWINEVENTHOOK,
    loc_pid: u32,
    peek_hook: HHOOK,
    spot: Launcher,
    icons_in: Arc<Mutex<Vec<IconPx>>>,
    hotkey_on: bool,
    last_mouse: (i32, i32),
    clip: clipper::Clipper,
    /// Pano paneli: farenin yanında açılan ayrı cam pencere (adayla aynı çizici).
    pop: Island,
    clip_tx: std::sync::mpsc::Sender<()>,
    clip_hotkey_on: bool,
    /// Düzenleme kartı genişlerken: düzenleyicinin açılacağı kayıt.
    edit_id: Option<String>,
    /// Panodaki resmi ekranda büyüten şeffaf katman.
    viewer: viewer::Viewer,
    /// Tam ekran oyun var: kısayollar kayıtsız (ada gizleme ayarından bağımsız).
    game: bool,
    /// Özel (exclusive) tam ekran D3D: üstüne pencere çizilemez.
    exclusive: bool,
    /// Oyundayken gelen bildirimler; oyundan çıkınca gösterilir.
    missed: VecDeque<Note>,
    /// Üstüne gelince açılan hâl ve saklanan bildirimler.
    hub: hubbing::Hub,
    shots: shots::Shots,
    /// Aramadan kurulan zamanlayıcı/kronometre (hapta saat yerine görünür).
    timer: Option<timing::Timer>,
    /// Tarayıcı eklentisinden gelen, karar bekleyen indirmeler (bkz. `offering`).
    offers: VecDeque<bridge::Offer>,
    /// Kartta gösterilen kayıt yeri (İndirilenler).
    offer_folder: String,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

fn pick_monitor(cfg: &Config) -> RECT {
    let all = carlui_core::win::monitors();
    let n = cfg.general.top_monitor as usize;
    let m = if n == 0 { all.iter().find(|m| m.primary) } else { all.get(n) }.or(all.first());
    m.map(|m| m.rect).unwrap_or(RECT { left: 0, top: 0, right: 1920, bottom: 1080 })
}

unsafe extern "system" fn peek_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp.0 as u32 == WM_MOUSEMOVE {
        let pt = (*(lp.0 as *const MSLLHOOKSTRUCT)).pt;
        if pt.y <= PEEK_TOP.load(Ordering::Relaxed) + 1 && pt.x >= PEEK_L.load(Ordering::Relaxed) && pt.x <= PEEK_R.load(Ordering::Relaxed) {
            let _ = PostMessageW(HWND(HWND_ISLAND.load(Ordering::Relaxed) as _), WM_APP_PEEK, WPARAM(0), LPARAM(0));
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe extern "system" fn win_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if event == EVENT_OBJECT_LOCATIONCHANGE && (id_object != OBJID_WINDOW.0 || hwnd != GetForegroundWindow()) {
        return;
    }
    let _ = PostMessageW(HWND(HWND_ISLAND.load(Ordering::Relaxed) as _), WM_APP_FG, WPARAM(0), LPARAM(0));
}

impl App {
    fn media_active(&self) -> bool {
        self.cfg.notify.media_bar
            && self.media.as_ref().is_some_and(|m| m.playing || self.paused_since.map(|t| t.elapsed() < PAUSED_GRACE).unwrap_or(true))
    }

    fn desired(&self) -> View {
        if self.fullscreen && !self.spot.open {
            return View::Hidden;
        }
        if self.spot.open {
            return View::Search;
        }
        if !self.offers.is_empty() {
            return View::Offer;
        }
        if self.note.is_some() {
            return View::Note;
        }
        if self.hub.level > 0 {
            return View::Hub;
        }
        let media = self.media_active();
        if self.expanded && media {
            return View::MediaFull;
        }
        if self.date {
            return View::Date;
        }
        if media {
            return View::Media;
        }
        match self.cfg.clock.mode {
            _ if self.timer.is_some() || dlman::pill(&self.cfg.download).is_some() => View::Clock,
            ClockMode::Always => View::Clock,
            // Masaüstünde gizliyken fare üst kenara gidince saat görünür.
            _ if self.peek => View::Clock,
            ClockMode::WhenAppActive if self.desktop_fg => View::Hidden,
            ClockMode::WhenAppActive => View::Clock,
            ClockMode::Never => View::Hidden,
        }
    }

    fn refresh_vols(&mut self) {
        let app = self.media.as_ref().map(|m| m.app.clone()).unwrap_or_default();
        if app != self.vols_app || self.vols.is_empty() {
            self.vols = audio::app_volumes(&app);
            self.vols_app = app;
        }
    }

    /// Görünümü duruma göre ayarlar. `rebuild`: görünüm aynı kalsa da içeriği yenile.
    fn refresh(&mut self, rebuild: bool) {
        let view = self.desired();
        let media_on = self.media_active();
        let media_panel = view == View::MediaFull || (view == View::Hub && media_on);
        if media_panel {
            self.refresh_vols();
        }
        // Şarkı/video çalarken ada kapağın rengini alır (ayarla kapatılabilir).
        let art = (self.cfg.island.media_tint && self.media_active() && matches!(view, View::Media | View::MediaFull | View::Search | View::Clip))
            .then(|| self.media.as_ref().and_then(|m| m.art.as_ref()))
            .flatten();
        let rebuild = self.island.set_mood(&self.gfx, art) || rebuild;
        let model = Model {
            media: self.media.as_ref(),
            note: self.note.as_ref(),
            app_volume: if media_panel { audio::get(&self.vols) } else { None },
            spot: (view == View::Search).then(|| self.spot.view()),
            clip: None,
            hub: if view == View::Hub { Some(self.hub.view(media_on)) } else { None },
            badge: self.hub.notes.len(),
            timer: self.timer.as_ref().map(|t| t.text()).or_else(|| dlman::pill(&self.cfg.download)),
            offer: self.offers.front().filter(|_| view == View::Offer).map(|o| island::OfferView {
                name: &o.name,
                size: o.size,
                host: &o.host,
                folder: &self.offer_folder,
                more: self.offers.len() - 1,
            }),
        };
        if view != self.island.view {
            if let Err(e) = self.island.set_view(&self.gfx, view, &model, true) {
                carlui_core::error!("ada görünümü kurulamadı ({view:?}): {e}");
            }
        } else if rebuild && view != View::Hidden && self.dragging == Hit::None {
            if let Err(e) = self.island.set_view(&self.gfx, view, &model, false) {
                carlui_core::error!("ada görünümü yenilenemedi ({view:?}): {e}");
            }
        }
        let wanted = std::mem::take(&mut self.island.icon_requests);
        if !wanted.is_empty() {
            self.spot.request_icons(wanted);
        }
        let hwnd = self.island.hwnd;
        unsafe {
            if media_panel && self.media.as_ref().is_some_and(|m| m.playing && m.end > 0.0) {
                SetTimer(hwnd, TIMER_TICK, 1000, None);
            } else {
                let _ = KillTimer(hwnd, TIMER_TICK);
            }
        }
        // Maksimize pencere üstündeyse yukarı saklan (ayar açıksa); bildirim ve açık panel her zaman iner.
        let up = self.cfg.island.hide_over_maximized && self.covered && !self.peek && self.note.is_none() && !self.expanded && !matches!(view, View::Hidden | View::Search | View::Clip | View::Hub);
        self.island.slide(up);
        // Ayara göre gizlenen ada (masaüstünde "Uygulamada" saati) de fareyle çağrılabilir.
        let hidden = !self.fullscreen && (view == View::Hidden || (self.peek && view == View::Clock && self.cfg.clock.mode != ClockMode::Always));
        self.set_peek_hook(up || hidden);
    }

    fn set_peek_hook(&mut self, on: bool) {
        unsafe {
            if on && self.peek_hook.is_invalid() {
                let r = self.island.pill_rect();
                let left = self.mon.left + ((self.mon.right - self.mon.left) - self.island.win_w as i32) / 2;
                PEEK_TOP.store(self.mon.top, Ordering::SeqCst);
                // Gizliyken hap küçüktür: en az ortadaki 300 px'lik şerit tetikler.
                let mid = left + self.island.win_w as i32 / 2;
                PEEK_L.store((left + r.left - 40).min(mid - 150), Ordering::SeqCst);
                PEEK_R.store((left + r.right + 40).max(mid + 150), Ordering::SeqCst);
                self.peek_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(peek_proc), GetModuleHandleW(None).unwrap_or_default(), 0).unwrap_or_default();
            } else if !on && !self.peek_hook.is_invalid() {
                let _ = UnhookWindowsHookEx(self.peek_hook);
                self.peek_hook = HHOOK::default();
            }
        }
    }

    fn show_next_note(&mut self) {
        unsafe {
            let _ = KillTimer(self.island.hwnd, TIMER_NOTE);
        }
        self.note = self.pending.pop_front();
        if self.note.is_some() {
            self.expanded = false;
            self.date = false;
            unsafe {
                SetTimer(self.island.hwnd, TIMER_NOTE, self.cfg.notify.duration_ms.clamp(1500, 15000), None);
            }
        }
        // Bildirimden bildirime geçerken de içerik yenilensin.
        self.refresh(true);
    }

    /// Oyun bitti: oyundayken gelen bildirimleri göster. Çoksa önce bir özet,
    /// ardından son üçü.
    fn show_missed(&mut self) {
        if self.fullscreen || self.missed.is_empty() {
            return;
        }
        let all: Vec<Note> = self.missed.drain(..).collect();
        if all.len() > 3 {
            let mut apps: Vec<&str> = Vec::new();
            for n in &all {
                if !n.app.is_empty() && !apps.contains(&n.app.as_str()) {
                    apps.push(&n.app);
                }
            }
            let body = apps.iter().take(4).copied().collect::<Vec<_>>().join(", ");
            let summary = Note { app: "CarlUI".into(), aumid: String::new(), title: t!(format!("Oyundayken {} bildirim geldi", all.len()), format!("{} notifications arrived while gaming", all.len())), body, ..Default::default() };
            self.pending.push_back(summary);
        }
        let skip = all.len().saturating_sub(3);
        self.pending.extend(all.into_iter().skip(skip));
        info!("oyun bitti: {} bildirim gösteriliyor", self.pending.len());
        if self.note.is_none() && !self.spot.open && !self.clip.open {
            self.show_next_note();
        }
    }

    fn push_note(&mut self, n: Note) {
        if !self.cfg.notify.enabled {
            return;
        }
        let key = format!("{}|{}", n.title, n.body);
        self.recent.retain(|(_, t)| t.elapsed() < Duration::from_secs(8));
        if self.recent.iter().any(|(k, _)| *k == key) {
            return;
        }
        self.recent.push_back((key, Instant::now()));
        if self.fullscreen {
            // Oyun bölünmesin: sakla, çıkınca göster.
            if self.cfg.game.hold_notes {
                info!("bildirim oyun bitince gösterilecek: {} — {}", n.app, n.title);
                self.missed.push_back(n);
                while self.missed.len() > MISSED_MAX {
                    self.missed.pop_front();
                }
            }
            return;
        }
        info!("bildirim: {} — {}", n.app, n.title);
        self.pending.push_back(n);
        // Arama açıkken bildirimler sıraya girer, kapanınca gösterilir.
        if self.note.is_none() && !self.spot.open && !self.clip.open {
            self.show_next_note();
        }
    }

    fn on_media(&mut self) {
        let new = self.media_slot.lock().ok().and_then(|g| g.clone());
        let was_playing = self.media.as_ref().is_some_and(|m| m.playing);
        match &new {
            Some(m) if m.playing => self.paused_since = None,
            Some(_) if was_playing || self.paused_since.is_none() => {
                self.paused_since = Some(Instant::now());
                unsafe {
                    SetTimer(self.island.hwnd, TIMER_PAUSED, PAUSED_GRACE.as_millis() as u32 + 200, None);
                }
            }
            _ => {}
        }
        if new.is_none() {
            self.expanded = false;
        }
        self.media = new;
        self.refresh(true);
    }

    fn check_foreground(&mut self) {
        unsafe {
            let fg = GetForegroundWindow();
            let class = carlui_core::win::class_name(fg);
            let desktop = fg.is_invalid() || matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd");
            let mut covered = false;
            if !desktop && !fg.is_invalid() && IsZoomed(fg).as_bool() {
                let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                covered = GetMonitorInfoW(mon, &mut info).as_bool() && info.rcMonitor == self.mon;
            }
            // Maksimize takibi için yalnızca öndeki uygulamanın konum olaylarını dinle.
            if self.cfg.island.hide_over_maximized {
                let mut pid = 0;
                GetWindowThreadProcessId(fg, Some(&mut pid));
                if pid != self.loc_pid {
                    if !self.loc_hook.is_invalid() {
                        let _ = UnhookWinEvent(self.loc_hook);
                    }
                    self.loc_hook = SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(win_event), pid, 0, WINEVENT_OUTOFCONTEXT);
                    self.loc_pid = pid;
                }
            } else if !self.loc_hook.is_invalid() {
                let _ = UnhookWinEvent(self.loc_hook);
                self.loc_hook = HWINEVENTHOOK::default();
                self.loc_pid = 0;
            }
            if desktop != self.desktop_fg || covered != self.covered {
                self.desktop_fg = desktop;
                self.covered = covered;
                self.refresh(false);
            }
        }
    }

    fn place_window(&mut self) {
        self.mon = pick_monitor(&self.cfg);
        let (w, h) = self.island.window_size();
        let x = self.mon.left + ((self.mon.right - self.mon.left) - w) / 2;
        unsafe {
            let _ = SetWindowPos(self.island.hwnd, HWND_TOPMOST, x, self.mon.top, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    fn apply_config(&mut self, cfg: Config) {
        let theme = Theme::from_config(&cfg.theme);
        self.island.apply_theme(&theme);
        self.pop.apply_theme(&theme);
        self.island.set_top(cfg.island.top_margin);
        let monitor_changed = cfg.general.top_monitor != self.cfg.general.top_monitor;
        let banners = cfg.notify.enabled && cfg.notify.hide_windows_banners;
        if banners != (self.cfg.notify.enabled && self.cfg.notify.hide_windows_banners) {
            notify::set_hide_banners(banners);
        }
        self.cfg = cfg;
        if monitor_changed {
            self.place_window();
        }
        if !self.cfg.launcher.enabled {
            self.close_search(false);
        }
        if !self.cfg.clipboard.enabled {
            self.close_clip(false);
        }
        if let Ok(mut st) = self.clip.store.lock() {
            st.max_items = self.cfg.clipboard.max_items.max(10);
        }
        clipping::SECRET_SECS.store(self.cfg.clipboard.secret_seconds, Ordering::Relaxed);
        bridge::ENABLED.store(self.cfg.launcher.download_manager, Ordering::Relaxed);
        self.shots.set_combo(&self.cfg);
        self.set_hotkey();
        self.check_foreground();
        self.refresh(true);
    }

    fn dump(&self) {
        let left = self.mon.left + ((self.mon.right - self.mon.left) - self.island.win_w as i32) / 2;
        let r = self.island.pill_rect();
        let pop_pill = {
            let r = self.pop.pill_rect();
            let mut w = RECT::default();
            let _ = unsafe { GetWindowRect(self.pop.hwnd, &mut w) };
            serde_json::json!({"l": w.left + r.left, "t": w.top + r.top, "r": w.left + r.right, "b": w.top + r.bottom})
        };
        let state = serde_json::json!({
            "view": format!("{:?}", self.island.view),
            "pill": { "l": left + r.left, "t": self.mon.top + r.top, "r": left + r.right, "b": self.mon.top + r.bottom },
            "slid_up": self.island.slid_up,
            "covered": self.covered,
            "desktop": self.desktop_fg,
            "clock_mode": format!("{:?}", self.cfg.clock.mode),
            "timer": self.timer.as_ref().map(|t| t.text()),
            "download": dlman::pill(&self.cfg.download),
            "peek": self.peek,
            "fullscreen": self.fullscreen,
            "game": self.game,
            "memory_mb": (carlui_core::power::working_set_mb() * 10.0).round() / 10.0,
            "missed": self.missed.len(),
            "hub": self.hub.level,
            "kept": self.hub.notes.iter().map(|n| serde_json::json!({"title": n.title, "app": n.app, "shot": n.shot, "at": n.at})).collect::<Vec<_>>(),
            "shots": self.shots.debug(),
            "expanded": self.expanded,
            "note": self.note.as_ref().map(|n| format!("{} — {}", n.title, n.body)),
            "pending": self.pending.len(),
            "media": self.media.as_ref().map(|m| serde_json::json!({
                "app": m.app, "title": m.title, "artist": m.artist, "playing": m.playing,
                "pos": m.position(), "end": m.end, "art": m.art.as_ref().map(|a| a.len()),
                "can_seek": m.can_seek,
            })),
            "app_volume": audio::get(&self.vols),
            "vol_sessions": self.vols.len(),
            "search": self.spot.debug(),
            "foreground_is_island": unsafe { GetForegroundWindow() } == self.island.hwnd,
            "hotkey": self.hotkey_on,
            "clip": self.clip.debug(),
            "clip_items": self.clip.store.lock().map(|s| s.items.len()).unwrap_or(0),
            "clip_top": self.clip.store.lock().ok().and_then(|s| s.items.first().map(|c| serde_json::json!({"kind": format!("{:?}", c.kind), "text": c.text.chars().take(80).collect::<String>(), "w": c.w, "h": c.h, "pinned": c.pinned}))),
            "preview": self.pop.preview_debug(),
            "viewer": self.viewer.debug(),
            "pop_view": format!("{:?}", self.pop.view),
            "pop_pill": pop_pill,
            "foreground_is_pop": unsafe { GetForegroundWindow() } == self.pop.hwnd,
            "clip_hotkey": self.clip_hotkey_on,
            "game": self.game,
            "editor_open": editor::is_open(),
        });
        let path = paths::log_dir().join("..").join("island-state.json");
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap_or_default());
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Shutdown => unsafe {
                let _ = DestroyWindow(self.island.hwnd);
            },
            Event::Fullscreen { active, exclusive, monitor } => {
                self.exclusive = active && exclusive;
                // Başka monitördeki tam ekran video adayı gizlemesin (Windows görev
                // çubuğu gibi); özel tam ekran oyun ise her yerde oyun modudur.
                let active = active && (exclusive || carlui_core::win::fullscreen_here(self.island.hwnd, &monitor));
                // Kısayollar oyunda her zaman kalkar (gizleme ayarından bağımsız):
                // Ctrl+Space gibi bir kısayol kayıtlıyken Windows tuşu oyuna vermez.
                // Tarayıcı/oynatıcıdaki tam ekran video oyun sayılmaz: arama açık kalır.
                let game = active && (exclusive || !searching::game_free_front());
                if self.game != game {
                    self.game = game;
                    self.set_hotkey();
                }
                if self.cfg.game.hide_on_fullscreen || !active {
                    if game {
                        self.close_search(false);
                        self.close_clip(false);
                    }
                    let was = std::mem::replace(&mut self.fullscreen, active);
                    if active {
                        // Sırada bekleyen bildirimler de oyun sonrasına kalsın.
                        if self.cfg.game.hold_notes {
                            self.missed.extend(self.pending.drain(..));
                        }
                        self.pending.clear();
                        self.note = None;
                        self.expanded = false;
                        self.date = false;
                    } else if was && !self.missed.is_empty() {
                        unsafe {
                            SetTimer(self.island.hwnd, TIMER_MISSED, 1200, None);
                        }
                    }
                    self.set_hotkey();
                    self.refresh(false);
                }
            }
            Event::ShowDesktop | Event::ExplorerRestarted => unsafe {
                let _ = SetWindowPos(self.island.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            },
            Event::Custom { name, data } => {
                let g = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                match name.as_str() {
                    // Eski tip tepsi balonları (Windows'un kendi bildirimi yoksa).
                    "tray-balloon" | "island-note" => {
                        let body = if name == "island-note" { g("body") } else { g("text") };
                        self.push_note(Note { app: g("app"), aumid: g("aumid"), title: g("title"), body, ..Default::default() });
                    }
                    "island-dump" => self.dump(),
                    // {"secs": 300, "label": "çay"} · secs 0 = kronometre · {"stop": true}
                    "timer" => {
                        let secs = data.get("secs").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let label = data.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let cmd = if data.get("stop").is_some() {
                            timing::Cmd::Stop
                        } else if secs == 0 {
                            timing::Cmd::Stopwatch
                        } else {
                            timing::Cmd::Start(secs, label)
                        };
                        self.timer_cmd(cmd);
                    }
                    // {"url": "https://…"} · {"cancel": true}
                    "download" => {
                        if data.get("cancel").is_some() {
                            self.dl_cancel();
                        } else if !g("url").is_empty() {
                            self.dl_start(g("url"));
                        }
                    }
                    n if n.starts_with("search-") => self.search_event(n, &data),
                    n if n.starts_with("clip-") => self.clip_event(n, &data),
                    n if n.starts_with("hub-") => self.hub_event(n, &data),
                    n if n.starts_with("shot-") => self.shot_event(n, &data),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn command(&mut self, cmd: Cmd) {
        if let Some(m) = &self.media {
            media::command(m.app.clone(), cmd);
        }
    }

    fn mouse_up(&mut self, x: i32, y: i32) {
        let pressed = std::mem::replace(&mut self.pressed, Hit::None);
        if pressed == Hit::Pill {
            self.island.press(false);
        }
        if self.dragging != Hit::None {
            let which = std::mem::replace(&mut self.dragging, Hit::None);
            unsafe {
                let _ = ReleaseCapture();
            }
            if which == Hit::Seek {
                if let (Some(v), Some(m)) = (self.island.drag_bar(Hit::Seek, x), self.media.as_mut()) {
                    let target = v as f64 * m.end;
                    m.pos = target;
                    m.at = Some(Instant::now());
                    media::command(m.app.clone(), Cmd::Seek(target));
                    let snap = m.clone();
                    self.island.run_progress(&snap);
                    self.island.tick_elapsed(&self.gfx, &snap);
                }
            }
            return;
        }
        let h = self.island.hit(x, y);
        if h != pressed || h == Hit::None {
            return;
        }
        let view = self.island.view;
        match view {
            View::Note => {
                if let Some(n) = self.note.take() {
                    if let Some(f) = &n.file {
                        dlman::open(f);
                    } else if !activate::open_app(&n.aumid) && !n.app.is_empty() {
                        activate::open_app(&n.app);
                    }
                }
                self.show_next_note();
            }
            View::Offer => {
                if let Hit::OfferBtn(b) = h {
                    self.offer_choice(b);
                }
            }
            View::Clock if self.hub_click() => {}
            View::Media if self.hub_click() => {}
            View::Hub => match h {
                Hit::HubClear | Hit::NoteRow(_) | Hit::NoteDel(_) | Hit::NoteAct(..) | Hit::GroupToggle(_) | Hit::GroupClear(_) => self.hub_hit(h),
                Hit::Prev => self.command(Cmd::Prev),
                Hit::Next => self.command(Cmd::Next),
                Hit::Play => self.command(Cmd::PlayPause),
                Hit::Source => {
                    if let Some(m) = &self.media {
                        activate::open_app(&m.app);
                    }
                }
                _ => self.hub_close(),
            },
            View::Clock => {
                self.date = true;
                unsafe {
                    SetTimer(self.island.hwnd, TIMER_DATE, 4000, None);
                }
                self.refresh(false);
            }
            View::Date => {
                self.date = false;
                self.refresh(false);
            }
            View::Media => {
                self.expanded = true;
                self.vols.clear();
                self.refresh(false);
            }
            View::MediaFull => match h {
                Hit::Prev => self.command(Cmd::Prev),
                Hit::Next => self.command(Cmd::Next),
                Hit::Play => self.command(Cmd::PlayPause),
                Hit::Source => {
                    if let Some(m) = &self.media {
                        activate::open_app(&m.app);
                    }
                }
                _ => {
                    self.expanded = false;
                    self.refresh(false);
                }
            },
            View::Search => {
                if let Hit::Row(i) | Hit::Tile(i) | Hit::Chip(i) = h {
                    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                    let o = self.spot.activate(&self.cfg, i, ctrl, shift);
                    self.spot_outcome(o);
                }
            }
            View::Hidden | View::Clip => {}
        }
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = clipping::window_message(hwnd, msg, wp, lp) {
        return r;
    }
    if let Some(r) = searching::window_message(hwnd, msg, wp, lp) {
        return r;
    }
    if let Some(r) = hubbing::window_message(hwnd, msg, wp, lp) {
        return r;
    }
    if let Some(r) = shots::window_message(hwnd, msg, wp, lp) {
        return r;
    }
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => {
                let open = with_app(|a| a.spot.open).unwrap_or(false);
                LRESULT(if open { MA_ACTIVATE } else { MA_NOACTIVATE } as isize)
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let (x, y) = xy(lp);
                with_app(|a| {
                    let _ = KillTimer(hwnd, TIMER_COLLAPSE);
                    a.hub_hover();
                    if a.island.view == View::Search {
                        // İçerik değişince Windows imleç kımıldamadan da WM_MOUSEMOVE
                        // yollar; yalnızca gerçek hareket seçimi değiştirsin.
                        let p = cursor();
                        if (p.x, p.y) == a.last_mouse {
                            return;
                        }
                        a.last_mouse = (p.x, p.y);
                        if let Hit::Row(i) | Hit::Tile(i) | Hit::Chip(i) = a.island.hit(x, y) {
                            if a.spot.hover(i) {
                                a.refresh(true);
                            }
                        }
                        return;
                    }
                    match a.dragging {
                        Hit::Volume => {
                            if let Some(v) = a.island.drag_bar(Hit::Volume, x) {
                                audio::set(&a.vols, v);
                            }
                        }
                        Hit::Seek => {
                            a.island.drag_bar(Hit::Seek, x);
                        }
                        _ => {}
                    }
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| {
                    a.hub_leave();
                    if (a.expanded || a.hub.level > 0) && a.dragging == Hit::None {
                        SetTimer(hwnd, TIMER_COLLAPSE, if a.hub.level > 0 { 650 } else { 900 }, None);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    let h = a.island.hit(x, y);
                    a.pressed = h;
                    match h {
                        Hit::Seek | Hit::Volume => {
                            a.dragging = h;
                            SetCapture(hwnd);
                            if let Some(v) = a.island.drag_bar(h, x) {
                                if h == Hit::Volume {
                                    audio::set(&a.vols, v);
                                }
                            }
                        }
                        Hit::Pill if a.island.view != View::Search => a.island.press(true),
                        _ => {}
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| a.mouse_up(x, y));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| match a.island.view {
                    View::Search => {
                        if let Hit::Row(i) | Hit::Tile(i) | Hit::Chip(i) = a.island.hit(x, y) {
                            let o = a.spot.open_actions(i);
                            a.spot_outcome(o);
                        }
                    }
                    View::Note => {
                        if let Some(n) = a.note.take() {
                            a.archive(n);
                        }
                        a.show_next_note();
                    }
                    View::Hub => a.hub_close(),
                    View::MediaFull | View::Date => {
                        a.expanded = false;
                        a.date = false;
                        a.refresh(false);
                    }
                    _ => {}
                });
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| match wp.0 {
                    island::TIMER_REGION => a.island.region_done(),
                    TIMER_CLOCK => {
                        let (_, sec, _, _) = island::now_hm();
                        SetTimer(hwnd, TIMER_CLOCK, (60 - sec as u32) * 1000 + 50, None);
                        if matches!(a.island.view, View::Clock | View::Date | View::Media | View::Search) {
                            a.refresh(true);
                        }
                    }
                    TIMER_NOTE => {
                        // Fare bildirimin üstündeyse okunuyor demektir: bekle.
                        let p = cursor();
                        let mut wr = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut wr);
                        if a.island.hit(p.x - wr.left, p.y - wr.top) != Hit::None {
                            SetTimer(hwnd, TIMER_NOTE, 800, None);
                        } else {
                            if let Some(n) = a.note.take() {
                                a.archive(n);
                            }
                            a.show_next_note();
                        }
                    }
                    TIMER_MISSED => {
                        let _ = KillTimer(hwnd, TIMER_MISSED);
                        a.show_missed();
                    }
                    TIMER_COLLAPSE => {
                        let _ = KillTimer(hwnd, TIMER_COLLAPSE);
                        a.expanded = false;
                        if a.hub.level > 0 {
                            a.hub_close();
                        } else {
                            a.refresh(false);
                        }
                    }
                    TIMER_DATE => {
                        let _ = KillTimer(hwnd, TIMER_DATE);
                        a.date = false;
                        a.refresh(false);
                    }
                    TIMER_TICK => {
                        if let Some(m) = a.media.clone() {
                            a.island.tick_elapsed(&a.gfx, &m);
                        }
                    }
                    TIMER_FG => {
                        let _ = KillTimer(hwnd, TIMER_FG);
                        a.check_foreground();
                    }
                    timing::TIMER_COUNT => a.timer_tick(),
                    dlman::TIMER_DL => a.dl_tick(),
                    TIMER_PAUSED => {
                        let _ = KillTimer(hwnd, TIMER_PAUSED);
                        a.refresh(false);
                    }
                    TIMER_PEEK => {
                        let p = cursor();
                        let mut wr = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut wr);
                        // Fare hâlâ üst kenardaki tetik şeridindeyse açık kalsın.
                        let edge = p.y <= PEEK_TOP.load(Ordering::Relaxed) + 1 && p.x >= PEEK_L.load(Ordering::Relaxed) && p.x <= PEEK_R.load(Ordering::Relaxed);
                        if !edge && a.island.hit(p.x - wr.left, p.y - wr.top) == Hit::None {
                            let _ = KillTimer(hwnd, TIMER_PEEK);
                            a.peek = false;
                            a.refresh(false);
                        }
                    }
                    _ => {}
                });
                LRESULT(0)
            }
            WM_APP_PEEK => {
                with_app(|a| {
                    if !a.peek {
                        a.peek = true;
                        a.refresh(false);
                        SetTimer(hwnd, TIMER_PEEK, 1500, None);
                    }
                });
                LRESULT(0)
            }
            WM_APP_FG => {
                SetTimer(hwnd, TIMER_FG, 150, None);
                LRESULT(0)
            }
            WM_APP_MEDIA => {
                with_app(|a| a.on_media());
                LRESULT(0)
            }
            bridge::WM_APP_OFFER => {
                with_app(|a| a.offers_arrived());
                LRESULT(0)
            }
            WM_APP_NOTE => {
                with_app(|a| {
                    let notes: Vec<Note> = a.notes_in.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
                    for n in notes {
                        a.push_note(n);
                    }
                });
                LRESULT(0)
            }
            WM_APP_CONFIG => {
                with_app(|a| {
                    if let Some(cfg) = a.cfg_slot.lock().ok().and_then(|mut c| c.take()) {
                        a.apply_config(cfg);
                    }
                });
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
            WM_DISPLAYCHANGE => {
                with_app(|a| a.place_window());
                LRESULT(0)
            }
            WM_DESTROY => {
                // Ada yokken bildirimler görünmez kalmasın: balonları geri aç.
                notify::set_hide_banners(false);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn create_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: w!("CarlUI.Island"),
            // Normal ok: ada tıklamayı yine alır, el imleci "bağlantı" gibi durmasın.
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            w!("CarlUI.Island"),
            w!("CarlUI Island"),
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
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const _ as *const _, size);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, size);
        // Başlangıçta hiçbir yer tıklanmasın.
        SetWindowRgn(hwnd, CreateRectRgn(0, 0, 0, 0), false);
        Ok(hwnd)
    }
}

fn main() -> Result<()> {
    log::init("island");
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\CarlUI.Island"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            info!("ada zaten çalışıyor");
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        let _queue = CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?;

        let cfg = config::load();
        let theme = Theme::from_config(&cfg.theme);
        let gfx = Gfx::new()?;
        let hwnd = create_window()?;
        HWND_ISLAND.store(hwnd.0 as isize, Ordering::SeqCst);
        let mon = pick_monitor(&cfg);
        let mut dpi = (96u32, 96u32);
        let hmon = MonitorFromPoint(POINT { x: mon.left + 10, y: mon.top + 10 }, MONITOR_DEFAULTTOPRIMARY);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi.0, &mut dpi.1);
        let s = dpi.0 as f32 / 96.0;
        let island = Island::new(&gfx, hwnd, &theme, s, cfg.island.top_margin)?;

        let hwnd_raw = hwnd.0 as isize;
        let inbox: Arc<Mutex<VecDeque<Event>>> = Arc::default();
        let inbox_bus = inbox.clone();
        let bus = BusClient::connect("island", move |ev| {
            if let Ok(mut q) = inbox_bus.lock() {
                q.push_back(ev);
            }
            let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_BUS, WPARAM(0), LPARAM(0));
        });
        let cfg_slot: Arc<Mutex<Option<Config>>> = Arc::default();
        let slot = cfg_slot.clone();
        config::watch(move |c| {
            if let Ok(mut g) = slot.lock() {
                *g = Some(c);
            }
            let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_CONFIG, WPARAM(0), LPARAM(0));
        });
        let media_slot: Arc<Mutex<Option<Snap>>> = Arc::default();
        media::start(hwnd_raw, WM_APP_MEDIA, media_slot.clone());
        let notes_in: Arc<Mutex<VecDeque<Note>>> = Arc::default();
        notify::set_hide_banners(cfg.notify.enabled && cfg.notify.hide_windows_banners);
        notify::start(hwnd_raw, WM_APP_NOTE, notes_in.clone());
        downloads::start(hwnd_raw, WM_APP_NOTE, notes_in.clone());
        update::start(hwnd_raw, WM_APP_NOTE, notes_in.clone());
        netwatch::start(hwnd_raw);

        let icons_in: Arc<Mutex<Vec<IconPx>>> = Arc::default();
        let spot = Launcher::new(
            move || {
                let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_SPOT_WAKE, WPARAM(0), LPARAM(0));
            },
            icons_in.clone(),
            move || {
                let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_ICONS, WPARAM(0), LPARAM(0));
            },
        );

        // Pano geçmişi: arka planda yüklenir, her kopya işçi iş parçacığında okunur.
        let store = Arc::new(Mutex::new(carlui_clip::store::Store::placeholder(cfg.clipboard.max_items)));
        clipping::SECRET_SECS.store(cfg.clipboard.secret_seconds, Ordering::Relaxed);
        let clip_tx = clipping::start_listener(hwnd, store.clone(), cfg.clipboard.max_items);
        if let Err(e) = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd) {
            carlui_core::warn!("pano dinlenemiyor: {e}");
        }
        // Aramada "pano …" sorgusu: gizli olmayan metin kayıtları.
        let src = store.clone();
        carlui_launcher::clipsearch::set_source(move || {
            src.lock()
                .map(|s| s.items.iter().filter(|c| c.kind == carlui_clip::store::Kind::Text && !c.secret).take(300).map(|c| c.text.clone()).collect())
                .unwrap_or_default()
        });
        let clip = clipper::Clipper::new(store);
        let pop = Island::new(&gfx, clipping::create_pop_window()?, &theme, s, clipping::POP_MARGIN)?;
        let viewer = viewer::Viewer::new(&gfx, pop.hwnd, &theme, s)?;

        let fg_hooks = vec![
            SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(win_event), 0, 0, WINEVENT_OUTOFCONTEXT),
            SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND, None, Some(win_event), 0, 0, WINEVENT_OUTOFCONTEXT),
        ];

        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                gfx,
                island,
                bus,
                inbox,
                cfg,
                cfg_slot,
                media_slot,
                media: None,
                paused_since: None,
                notes_in,
                pending: VecDeque::new(),
                note: None,
                recent: VecDeque::new(),
                expanded: false,
                date: false,
                fullscreen: false,
                desktop_fg: false,
                covered: false,
                peek: false,
                vols: Vec::new(),
                vols_app: String::new(),
                dragging: Hit::None,
                pressed: Hit::None,
                mon,
                fg_hooks,
                loc_hook: HWINEVENTHOOK::default(),
                loc_pid: 0,
                peek_hook: HHOOK::default(),
                spot,
                icons_in,
                hotkey_on: false,
                last_mouse: (i32::MIN, i32::MIN),
                clip,
                pop,
                clip_tx,
                clip_hotkey_on: false,
                edit_id: None,
                viewer,
                game: false,
                exclusive: false,
                missed: VecDeque::new(),
                hub: Default::default(),
                shots: shots::Shots::new(hwnd),
                timer: None,
                offers: VecDeque::new(),
                offer_folder: carlui_core::win::downloads_dir().map(|d| d.display().to_string()).unwrap_or_else(|| t!("İndirilenler", "Downloads").into()),
            })
        });
        bridge::start(hwnd.0 as isize);
        with_app(|a| {
            bridge::ENABLED.store(a.cfg.launcher.download_manager, Ordering::Relaxed);
            a.shots.set_combo(&a.cfg);
            a.set_hotkey();
            a.place_window();
            a.check_foreground();
            a.refresh(false);
            let _ = &a.bus;
        });
        let (_, sec, _, _) = island::now_hm();
        SetTimer(hwnd, TIMER_CLOCK, (60 - sec as u32) * 1000 + 50, None);
        info!("ada başladı");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if editor::pre_translate(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some(a) = APP.with(|a| a.borrow_mut().take()) {
            for h in &a.fg_hooks {
                let _ = UnhookWinEvent(*h);
            }
            if !a.loc_hook.is_invalid() {
                let _ = UnhookWinEvent(a.loc_hook);
            }
            if !a.peek_hook.is_invalid() {
                let _ = UnhookWindowsHookEx(a.peek_hook);
            }
        }
        info!("ada kapandı");
    }
    Ok(())
}
