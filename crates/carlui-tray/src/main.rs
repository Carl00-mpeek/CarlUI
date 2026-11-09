//! Tray modülü (`carlui-tray.exe`): sistem tepsisi ikonları ve kontrol merkezi.
//!
//! İki penceresi vardır: uygulamaların tepsi mesajlarını alan gizli
//! `Shell_TrayWnd` (bkz. `host`) ve dock'taki yukarı oktan açılan panel
//! (bkz. `panel`). Dock ile olay yolu üzerinden konuşur:
//! dock → "tray-toggle" (düğmenin yeri), tray → "tray-state" (açık/kapalı).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod audio;
mod host;
mod panel;
mod quick;
mod radio_watch;

use carlui_core::{
    bus::{BusClient, Event},
    config, info, log, paths,
    theme::Theme,
};
use carlui_ui::gfx::Gfx;
use host::{Change, Host};
use panel::{Panel, State};
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::CreateMutexW,
            WinRT::*,
        },
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_APP_BUS: u32 = WM_APP + 1;
const WM_APP_ICONS: u32 = WM_APP + 2;
const TIMER_TOPMOST: usize = 1;
const TIMER_REQUEST: usize = 2;
/// Meşgulken gelen ikon mesajları kuyruğu işlensin.
const WM_APP_SAVED: u32 = WM_APP + 3;
const TIMER_SAVED: usize = 3;

struct App {
    gfx: Gfx,
    host: Host,
    panel: Panel,
    bus: BusClient,
    inbox: Arc<Mutex<VecDeque<Event>>>,
    icons_pending: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// Uygulama kilitliyken gelen ikon mesajlarının kopyaları (dwData, veri).
    static SAVED: RefCell<Vec<(usize, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl App {
    fn publish_state(&self) {
        let open = self.panel.state == State::Open;
        self.bus.send(&Event::Custom { name: "tray-state".into(), data: serde_json::json!({ "open": open }) });
    }

    fn toggle(&mut self, data: &serde_json::Value) {
        if self.panel.state == State::Open {
            self.panel.close(true);
        } else {
            let g = |k: &str| data.get(k).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let button = RECT { left: g("l"), top: g("t"), right: g("r"), bottom: g("b") };
            self.panel.side = g("side").clamp(0, 2) as u8;
            if let Err(e) = self.panel.open(&self.gfx, &mut self.host, button, g("dock_top")) {
                carlui_core::error!("panel açılamadı: {e}");
            }
        }
        self.publish_state();
    }

    /// İkon mesajının sonucunu yansıtır: balon bildirimi ya da panel yenilemesi.
    fn apply(&mut self, change: Change) {
        match change {
            Change::Balloon(b) => self.bus.send(&Event::Custom {
                name: "tray-balloon".into(),
                data: serde_json::json!({ "app": b.app, "title": b.title, "text": b.text }),
            }),
            Change::Icons if !self.icons_pending => {
                // Panel açıksa yenilemeyi biriktir (bazı uygulamalar ikonu sık değiştirir).
                self.icons_pending = true;
                unsafe {
                    let _ = PostMessageW(self.panel.hwnd, WM_APP_ICONS, WPARAM(0), LPARAM(0));
                }
            }
            _ => {}
        }
    }

    fn close(&mut self, animate: bool) {
        if self.panel.state == State::Open {
            self.panel.close(animate);
            self.publish_state();
        }
    }

    fn dump(&self) {
        self.host.log_summary();
        let icons: Vec<serde_json::Value> = self
            .host
            .icons
            .iter()
            .map(|i| serde_json::json!({ "name": i.name, "tip": i.tip, "hidden": i.hidden, "version": i.version, "pid": i.pid, "has_icon": !i.hicon.is_invalid() }))
            .collect();
        let state = serde_json::json!({ "icons": icons, "panel": format!("{:?}", self.panel.state) });
        let path = paths::log_dir().join("..").join("tray-state.json");
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap_or_default());
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Shutdown => unsafe {
                let _ = DestroyWindow(self.host.hwnd);
            },
            Event::ExplorerRestarted => self.host.keep_on_top(),
            Event::Fullscreen { active: true, .. } => self.close(false),
            Event::Custom { name, data } => match name.as_str() {
                "tray-toggle" => self.toggle(&data),
                "tray-close" => self.close(true),
                "tray-dump" => self.dump(),
                _ => {}
            },
            _ => {}
        }
    }
}

extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COPYDATA => {
                let r = with_app(|a| {
                    let (res, change, fwd) = a.host.copydata(lp);
                    a.apply(change);
                    (res, fwd)
                });
                let (own, fwd) = r.unwrap_or_else(|| {
                    // Tam biz meşgulken (iç içe mesaj) geldi: kopyasını sakla, hemen sonra işle.
                    SAVED.with(|s| s.borrow_mut().push(Host::copy_of(lp)));
                    let _ = PostMessageW(hwnd, WM_APP_SAVED, WPARAM(0), LPARAM(0));
                    (None, true)
                });
                // Explorer'a iletme kilit dışında: sürerken gelen mesajlar kaybolmasın.
                let forwarded = if fwd { Host::forward_copydata(hwnd, wp, lp) } else { LRESULT(0) };
                own.unwrap_or(forwarded)
            }
            WM_APP_SAVED => {
                let saved = SAVED.with(|s| std::mem::take(&mut *s.borrow_mut()));
                if !saved.is_empty() {
                    let done = with_app(|a| {
                        for d in &saved {
                            let change = a.host.copydata_saved(d);
                            a.apply(change);
                        }
                    });
                    if done.is_none() {
                        SAVED.with(|s| s.borrow_mut().extend(saved));
                        SetTimer(hwnd, TIMER_SAVED, 50, None);
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_SAVED => {
                let _ = KillTimer(hwnd, TIMER_SAVED);
                let _ = PostMessageW(hwnd, WM_APP_SAVED, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_TOPMOST => {
                with_app(|a| a.host.keep_on_top());
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_REQUEST => {
                // 2 sn, 15 sn, 60 sn: bazı uygulamalar (IDM, ISLC) açılıştan hemen
                // sonraki isteği yok sayıp ancak bir süre sonrakine cevap veriyor.
                static ROUND: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = ROUND.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = KillTimer(hwnd, TIMER_REQUEST);
                match n {
                    0 => {
                        SetTimer(hwnd, TIMER_REQUEST, 13_000, None);
                    }
                    1 => {
                        SetTimer(hwnd, TIMER_REQUEST, 45_000, None);
                    }
                    _ => {}
                }
                with_app(|a| a.host.request_icons());
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
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_DEVICECHANGE => {
                radio_watch::rescan();
                LRESULT(1)
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let (x, y) = xy(lp);
                with_app(|a| a.panel.mouse_move(x, y));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| a.panel.mouse_leave());
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    if a.panel.mouse_down(x, y) {
                        a.close(true);
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    if let Some(i) = a.panel.mouse_up(x, y) {
                        a.host.click(i, false, cursor());
                        a.close(true);
                    }
                    // Ses bölümü açıldı/kapandı ya da çıkış aygıtı değişti.
                    if std::mem::take(&mut a.panel.rebuild) {
                        let App { gfx, host, panel, .. } = a;
                        panel.refresh(gfx, host);
                    }
                });
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let (x, y) = xy(lp);
                with_app(|a| {
                    if let Some(i) = a.panel.right_click(x, y) {
                        // Uygulamanın menüsü (Discord'unki gibi Chromium menüleri) üstte
                        // duran panelin altında kalmasın ve menüye tıklayınca panelin
                        // "dışarı tıklandı" kapanışı menüyü odaksız bırakıp iptal etmesin:
                        // panel önce anında kapanır, menü sonra açılır.
                        a.close(false);
                        a.host.click(i, true, cursor());
                    }
                });
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| match wp.0 {
                    panel::TIMER_HIDE => a.panel.finish_close(),
                    panel::TIMER_BRIGHT => a.panel.apply_brightness_now(),
                    panel::TIMER_REGION => {
                        let App { gfx, host, panel, .. } = a;
                        panel.opening_done(gfx, host);
                    }
                    _ => {}
                });
                LRESULT(0)
            }
            panel::WM_APP_OUTSIDE => {
                with_app(|a| a.close(true));
                LRESULT(0)
            }
            panel::WM_APP_RADIOS => {
                with_app(|a| a.panel.set_radios(quick::decode(wp.0)));
                LRESULT(0)
            }
            panel::WM_APP_BRIGHT => {
                let v = (wp.0 != usize::MAX).then(|| wp.0 as f32 / 1000.0);
                with_app(|a| {
                    let known_before = a.panel.has_brightness();
                    a.panel.set_brightness_known(v);
                    // Parlaklık desteği ilk kez öğrenildiyse kaydırıcıyı eklemek için yenile.
                    if v.is_some() != known_before {
                        let App { gfx, host, panel, .. } = a;
                        panel.refresh(gfx, host);
                    }
                });
                LRESULT(0)
            }
            WM_APP_ICONS => {
                with_app(|a| {
                    a.icons_pending = false;
                    let App { gfx, host, panel, .. } = a;
                    panel.refresh(gfx, host);
                });
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn create_panel_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(panel_proc),
            hInstance: instance.into(),
            lpszClassName: w!("CarlUI.TrayPanel"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            w!("CarlUI.TrayPanel"),
            w!("CarlUI Tray"),
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
        Ok(hwnd)
    }
}

fn main() -> Result<()> {
    log::init("tray");
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\CarlUI.Tray"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            info!("tray zaten çalışıyor");
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        let _queue = CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?;

        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(host_proc), hInstance: instance.into(), lpszClassName: w!("Shell_TrayWnd"), ..Default::default() });
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("Shell_TrayWnd"), PCWSTR::null(), WS_POPUP, 0, 0, 0, 0, None, None, instance, None)?;
        let hwnd_raw = hwnd.0 as isize;
        let host = Host::new(hwnd);
        host.keep_on_top();
        SetTimer(hwnd, TIMER_TOPMOST, 3000, None);

        let cfg = config::load();
        // Ayar değişince yeniden okunur; bu, seçilen arayüz dilini de uygular
        // (panel her açılışta metinlerini yeni dille kurar).
        carlui_ui::tone::follow(&cfg.theme);
        config::watch(|c| carlui_ui::tone::restart_on_change(&c.theme));
        let theme = Theme::from_config(&cfg.theme);
        let gfx = Gfx::new()?;
        let panel_hwnd = create_panel_window()?;
        let s = GetDpiForWindow(panel_hwnd) as f32 / 96.0;
        let panel = Panel::new(&gfx, panel_hwnd, &theme, s)?;

        let inbox: Arc<Mutex<VecDeque<Event>>> = Arc::default();
        let inbox_bus = inbox.clone();
        let bus = BusClient::connect("tray", move |ev| {
            if let Ok(mut q) = inbox_bus.lock() {
                q.push_back(ev);
            }
            let _ = PostMessageW(HWND(hwnd_raw as _), WM_APP_BUS, WPARAM(0), LPARAM(0));
        });
        APP.with(|a| *a.borrow_mut() = Some(App { gfx, host, panel, bus, inbox, icons_pending: false }));
        radio_watch::start(panel_hwnd.0 as isize, panel::WM_APP_RADIOS);
        with_app(|a| a.host.request_icons());
        // Bazı ikonlar (ör. explorer'ın ses ikonu) ilk yayını kaçırıyor; kısa süre sonra bir kez daha iste.
        SetTimer(hwnd, TIMER_REQUEST, 2000, None);
        // Parlaklık desteğini baştan öğren: ilk açılışta panel yeniden kurulmasın.
        panel::query_brightness_at_start(panel_hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // Çıkarken uygulamalar ikonlarını yeniden explorer'a kaydetsin (zaten iletilmişti; güvence).
        let _ = DestroyWindow(hwnd);
        let _ = DestroyWindow(panel_hwnd);
        if let Some(a) = APP.with(|a| a.borrow_mut().take()) {
            a.host.request_icons();
        }
        info!("tray kapandı");
    }
    Ok(())
}
