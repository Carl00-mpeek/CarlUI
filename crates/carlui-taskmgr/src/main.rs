//! Bu modül CarlUI'nin kendi görev yöneticisi (`carlui-taskmgr.exe`). Sorun:
//! Windows Görev Yöneticisi imzasız programları öne çıkarmıyor, başlangıçta
//! çalışan şeylerin yalnızca bir kısmını gösteriyor, CPU sıcaklığını ve
//! çekirdeklere dağılımı göstermiyor ve CarlUI'nin cam görünümüne uymuyor.
//! Çözüm: tek pencere, solda sekmeler (İşlemler, Başlangıç, CPU, CPU
//! yönlendirme, GPU, RAM, Disk), her biri kendi grafiği, miktarı ve yüzdesiyle.
//! Veriler `carlui-sys`'ten gelir (yönetici gerekmez), arka plan Windows 11
//! akrilik camıdır. Dock'tan "Görev yöneticisi" ile `--pid N` verilirse o süreç
//! seçili açılır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod admin;
mod draw;
mod groups;
mod pages;
mod power;
mod procs;
mod sample;
mod sign;
mod startup;
mod util;

use carlui_core::{info, log};
use carlui_ui::glassmenu::{self, Icon, Row};
use draw::{Align, Fonts, Painter, R, TEXT};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dwm::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        System::{Com::*, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::{Controls::MARGINS, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const TIMER_TICK: usize = 1;
const TIMER_ANIM: usize = 2;
const WM_APP_SIGNED: u32 = WM_APP + 1;
const SIDEBAR: f32 = 212.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Processes,
    Startup,
    Cpu,
    Routing,
    Gpu,
    Ram,
    Disk,
}

/// Kenar çubuğu sırası; `--tab N` bu sıradaki sekmeyle açar.
const TABS: [Tab; 7] = [Tab::Processes, Tab::Startup, Tab::Cpu, Tab::Routing, Tab::Gpu, Tab::Ram, Tab::Disk];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortCol {
    Name,
    Pid,
    Cpu,
    Gpu,
    Mem,
    Io,
    Sign,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Tab(Tab),
    Sort(SortCol),
    Proc(u32),
    /// Birden çok süreçli uygulama satırı (anahtar: exe yolu).
    Group(String),
    /// Grubun solundaki ok: aç/kapa.
    Expand(String),
    Kill,
    Suspend,
    Resume,
    Reveal,
    UnsignedFirst,
    HideWindows,
    ClearFilter,
    StartRow(usize),
    /// Başlangıç satırının solundaki anahtar.
    StartSwitch(usize),
    StartHideWindows,
    StartToggle,
    StartReveal,
    StartRefresh,
    CpuDetail(bool),
    Mode(power::Mode),
    Route(power::Route),
    Eco,
    /// Kenar çubuğunun altındaki "Yönetici olarak çalıştır".
    RunAsAdmin,
    /// Tablonun boş/gövde alanı (tekerlek kaydırma hedefi).
    Scroll,
}

pub struct App {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Act)>,
    pub accent: u32,
    pub tab: Tab,
    pub sampler: sample::Sampler,
    pub procs: procs::Procs,
    pub signs: sign::Checker,
    pub selected: Option<u32>,
    pub selected_group: Option<String>,
    /// Açık/kapalı gruplar ve son değişim anı (animasyon).
    pub expanded: HashMap<String, (bool, Instant)>,
    pub sort: (SortCol, bool),
    pub unsigned_first: bool,
    pub hide_windows: bool,
    pub filter: String,
    pub scroll: HashMap<Tab, f32>,
    pub max_scroll: HashMap<Tab, f32>,
    pub startup: Option<Vec<startup::Item>>,
    startup_slot: Arc<Mutex<Option<Vec<startup::Item>>>>,
    pub start_selected: Option<usize>,
    pub start_hide_windows: bool,
    pub cpu_detail: bool,
    pub mode: power::Mode,
    pub routes: HashMap<u32, (power::Route, Option<bool>)>,
    message: Option<(String, Instant)>,
    /// Yönetici yetkisiyle mi çalışıyoruz (düğme yerine rozet gösterilir).
    admin: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

impl App {
    pub fn say(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), Instant::now()));
    }

    pub fn visible_startup(&self) -> Vec<(usize, &startup::Item)> {
        let f = self.filter.to_lowercase();
        let mut v: Vec<(usize, &startup::Item)> = self.startup.iter().flatten().enumerate().filter(|(_, i)| !(self.start_hide_windows && i.windows)).filter(|(_, i)| f.is_empty() || i.name.to_lowercase().contains(&f) || i.command.to_lowercase().contains(&f)).collect();
        // İmzasızlar önce; açıp kapatınca satır yer değiştirmesin diye durum sıralamaya girmez.
        v.sort_by_key(|(_, i)| (self.signs.get(&i.path).rank(), i.name.to_lowercase()));
        v
    }

    fn selected_row(&self) -> Option<&procs::Row> {
        self.selected.and_then(|pid| self.procs.rows.iter().find(|r| r.pid == pid))
    }

    /// Düğmelerin uygulanacağı süreçler: seçili grup (tümü) ya da tek süreç.
    pub fn targets(&self) -> (String, Vec<procs::Row>) {
        if let Some(k) = &self.selected_group {
            let rows: Vec<procs::Row> = self.procs.rows.iter().filter(|r| groups::key_of(r) == *k).cloned().collect();
            let name = rows.first().map(|r| r.name.clone()).unwrap_or_default();
            return (if rows.len() > 1 { t!(format!("{name} ({} süreç)", rows.len()), format!("{name} ({} processes)", rows.len())) } else { name }, rows);
        }
        match self.selected_row() {
            Some(r) => (r.name.clone(), vec![r.clone()]),
            None => (String::new(), Vec::new()),
        }
    }

    fn start_scan(&mut self) {
        self.startup = None;
        let slot = self.startup_slot.clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let items = startup::scan();
            info!("başlangıç taraması: {} öğe", items.len());
            if let Ok(mut s) = slot.lock() {
                *s = Some(items);
            }
            unsafe {
                let _ = PostMessageW(HWND(hwnd as _), WM_APP_SIGNED, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn refresh_routes(&mut self) {
        let mut top: Vec<&procs::Row> = self.procs.rows.iter().filter(|r| !r.windows).collect();
        top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        let pids: Vec<u32> = top.iter().take(40).map(|r| r.pid).chain(self.selected).collect();
        self.routes = pids.into_iter().map(|p| (p, (power::route(p), power::eco(p)))).collect();
    }

    fn tick(&mut self) {
        self.sampler.tick();
        self.procs.tick();
        if self.tab == Tab::Routing {
            self.refresh_routes();
            self.mode = power::mode();
        }
        self.invalidate();
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::Tab(t) => {
                self.tab = t;
                self.filter.clear();
                if t == Tab::Startup && self.startup.is_none() {
                    self.start_scan();
                }
                if t == Tab::Routing {
                    self.mode = power::mode();
                    self.refresh_routes();
                }
            }
            Act::Sort(c) => {
                self.sort = if self.sort.0 == c { (c, !self.sort.1) } else { (c, !matches!(c, SortCol::Name | SortCol::Pid)) };
            }
            Act::Proc(pid) => {
                self.selected = Some(pid);
                self.selected_group = None;
                if self.tab == Tab::Routing {
                    self.routes.insert(pid, (power::route(pid), power::eco(pid)));
                }
            }
            Act::Group(key) => {
                self.selected_group = Some(key);
                self.selected = None;
            }
            Act::Expand(key) => {
                groups::toggle(self, &key);
                unsafe {
                    SetTimer(self.hwnd, TIMER_ANIM, 15, None);
                }
            }
            Act::Kill => {
                let (name, rows) = self.targets();
                if !rows.is_empty() {
                    let ok = rows.iter().filter(|r| carlui_sys::process::terminate(r.pid)).count();
                    self.say(if ok == rows.len() {
            t!(format!("{name} kapatıldı"), format!("{name} ended"))
        } else if ok > 0 {
            t!(format!("{name}: {ok}/{} süreç kapatıldı (kalanlar için yönetici izni gerekebilir)", rows.len()), format!("{name}: {ok}/{} processes ended (the rest may need admin rights)", rows.len()))
        } else {
            t!(format!("{name} kapatılamadı (yönetici izni gerekebilir)"), format!("Couldn't end {name} (may need admin rights)"))
        });
                    self.procs.tick();
                }
            }
            Act::Suspend | Act::Resume => {
                let (name, rows) = self.targets();
                if !rows.is_empty() {
                    let ok = rows.iter().filter(|r| if a == Act::Suspend { carlui_sys::process::suspend(r.pid) } else { carlui_sys::process::resume(r.pid) }).count();
                    let verb = if a == Act::Suspend { t!("durduruldu", "suspended") } else { t!("sürdürüldü", "resumed") };
                    self.say(if ok > 0 { format!("{name} {verb}") } else { t!(format!("{name}: işlem yapılamadı (yönetici izni gerekebilir)"), format!("{name}: action failed (may need admin rights)")) });
                    self.procs.tick();
                }
            }
            Act::Reveal => {
                if let Some(r) = self.targets().1.into_iter().find(|r| !r.path.is_empty()) {
                    util::reveal(&r.path);
                }
            }
            Act::UnsignedFirst => self.unsigned_first = !self.unsigned_first,
            Act::HideWindows => self.hide_windows = !self.hide_windows,
            Act::ClearFilter => self.filter.clear(),
            Act::StartRow(i) => self.start_selected = Some(i),
            Act::StartHideWindows => self.start_hide_windows = !self.start_hide_windows,
            Act::StartSwitch(i) => {
                self.start_selected = Some(i);
                self.act(Act::StartToggle);
            }
            Act::StartToggle => {
                let item = self.start_selected.and_then(|i| self.startup.as_ref().and_then(|s| s.get(i)).cloned());
                if let (Some(i), Some(item)) = (self.start_selected, item) {
                    match startup::set_enabled(&item, !item.enabled) {
                        Ok(()) => {
                            if let Some(s) = self.startup.as_mut().and_then(|s| s.get_mut(i)) {
                                s.enabled = !item.enabled;
                            }
                            self.say(format!("{} {}", item.name, if item.enabled { t!("devre dışı bırakıldı", "disabled") } else { t!("etkinleştirildi", "enabled") }));
                        }
                        Err(e) => self.say(e),
                    }
                }
            }
            Act::StartReveal => {
                if let Some(item) = self.start_selected.and_then(|i| self.startup.as_ref().and_then(|s| s.get(i))) {
                    util::reveal(&item.path.clone());
                }
            }
            Act::StartRefresh => self.start_scan(),
            Act::CpuDetail(on) => self.cpu_detail = on,
            Act::Mode(m) => {
                if power::set_mode(m) {
                    self.mode = power::mode();
                } else {
                    self.say(t!("Güç modu değiştirilemedi", "Couldn't change the power mode"));
                }
            }
            Act::Route(r) => {
                let (name, rows) = self.targets();
                if !rows.is_empty() {
                    let ok = rows.iter().filter(|row| power::set_route(row.pid, r)).count();
                    self.say(if ok > 0 { format!("{name} → {}", r.label()) } else { t!(format!("{name}: çekirdek ataması yapılamadı (yönetici izni gerekebilir)"), format!("{name}: couldn't set core affinity (may need admin rights)")) });
                    for row in &rows {
                        self.routes.insert(row.pid, (power::route(row.pid), power::eco(row.pid)));
                    }
                }
            }
            Act::Eco => {
                let (name, rows) = self.targets();
                if let Some(first) = rows.first() {
                    let on = !power::eco(first.pid).unwrap_or(false);
                    let ok = rows.iter().filter(|row| power::set_eco(row.pid, on)).count();
                    self.say(if ok > 0 {
            t!(format!("{name}: verimlilik modu {}", if on { "açık" } else { "kapalı" }), format!("{name}: efficiency mode {}", if on { "on" } else { "off" }))
        } else {
            t!(format!("{name}: verimlilik modu değiştirilemedi"), format!("{name}: couldn't change efficiency mode"))
        });
                    for row in &rows {
                        self.routes.insert(row.pid, (power::route(row.pid), power::eco(row.pid)));
                    }
                }
            }
            Act::RunAsAdmin => {
                let tab = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
                if admin::relaunch(tab) {
                    info!("yönetici olarak yeniden açılıyor");
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                    return;
                }
                self.say(t!("Yönetici izni verilmedi", "Admin permission was not given"));
            }
            Act::Scroll => {}
        }
        self.invalidate();
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<Act> {
        // Sonra çizilen üstte: sondan ara, gövde (Scroll) en son seçenek.
        self.hits.iter().rev().filter(|(r, a)| r.contains(pt) && *a != Act::Scroll).map(|(_, a)| a.clone()).next()
    }

    fn paint(&mut self) -> Result<()> {
        let mut rc = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut rc)? };
        let (pw, ph) = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: pw, height: ph }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.rt = Some(rt);
        }
        let rt = self.rt.clone().unwrap();
        unsafe {
            let size = rt.GetPixelSize();
            if size.width != pw || size.height != ph {
                rt.Resize(&D2D_SIZE_U { width: pw, height: ph })?;
            }
        }
        let s = self.scale;
        let (w, h) = (pw as f32 / s, ph as f32 / s);
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = Painter { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            // Hafif koyu ton: akrilik çok açık arka planlarda da okunaklı kalsın.
            p.fill(R::new(0.0, 0.0, w, h), 0x101014, 0.30);
            p.fill(R::new(0.0, 0.0, SIDEBAR, h), 0x000000, 0.12);
            self.sidebar(&mut p, h);
            let area = R::new(SIDEBAR + 24.0, 18.0, w - SIDEBAR - 48.0, h - 36.0);
            let max = match self.tab {
                Tab::Processes => pages::processes::paint(self, &mut p, area),
                Tab::Startup => pages::startup::paint(self, &mut p, area),
                Tab::Cpu => pages::cpu::paint(self, &mut p, area),
                Tab::Routing => pages::routing::paint(self, &mut p, area),
                Tab::Gpu => pages::gpu::paint(self, &mut p, area),
                Tab::Ram => pages::ram::paint(self, &mut p, area),
                Tab::Disk => pages::disk::paint(self, &mut p, area),
            };
            self.max_scroll.insert(self.tab, max);
            if let Some((m, at)) = &self.message {
                if at.elapsed().as_secs() < 5 {
                    let tw = p.fonts.width(m, 13.0, false) + 28.0;
                    let r = R::new(SIDEBAR + (w - SIDEBAR - tw) / 2.0, h - 52.0, tw, 34.0);
                    p.round(r, 17.0, 0x202024, 0.96);
                    p.outline(r, 17.0, 0xFFFFFF, 0.12);
                    p.text(m, r, 13.0, false, TEXT, 0.95, Align::Center);
                }
            }
        }
        self.hits = hits;
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
        }
        Ok(())
    }

    fn sidebar(&self, p: &mut Painter, h: f32) {
        p.text(t!("Görev Yöneticisi", "Task Manager"), R::new(20.0, 14.0, SIDEBAR - 30.0, 30.0), 15.0, true, TEXT, 0.95, Align::Left);
        let sm = &self.sampler;
        let gpu = sm.gpu.as_ref().map(|g| format!("{} · {:.0}°", carlui_core::lang::pct(g.load), g.temp)).unwrap_or_default();
        let disk = sm.disks.iter().map(|d| d.active).fold(0.0f32, f32::max);
        let cpu_t = sm.temps.as_ref().map(|t| format!(" · {:.0}°", t.package)).unwrap_or_default();
        let tabs: [(Tab, char, &str, String); TABS.len()] = [
            (Tab::Processes, '\u{E9F5}', t!("İşlemler", "Processes"), format!("{}", self.procs.rows.len())),
            (Tab::Startup, '\u{E7E8}', t!("Başlangıç", "Startup"), String::new()),
            (Tab::Cpu, '\u{E950}', "CPU", format!("{}{cpu_t}", carlui_core::lang::pct(sm.now.usage))),
            (Tab::Routing, '\u{E8AB}', t!("CPU yönlendirme", "CPU routing"), String::new()),
            (Tab::Gpu, '\u{E7F4}', "GPU", gpu),
            (Tab::Ram, '\u{EEA0}', "RAM", carlui_core::lang::pct(sm.ram.percent())),
            (Tab::Disk, '\u{EDA2}', "Disk", carlui_core::lang::pct(disk)),
        ];
        for (i, (t, g, label, value)) in tabs.iter().enumerate() {
            let r = R::new(10.0, 56.0 + i as f32 * 42.0, SIDEBAR - 20.0, 38.0);
            if self.tab == *t {
                p.round(r, 7.0, 0xFFFFFF, 0.10);
                p.round(R::new(r.x, r.y + 10.0, 3.0, r.h - 20.0), 1.5, self.accent, 1.0);
            } else if p.hover(r) {
                p.round(r, 7.0, 0xFFFFFF, 0.06);
            }
            p.glyph(*g, R::new(r.x + 10.0, r.y, 24.0, r.h), 15.0, TEXT, 0.9);
            p.text(label, R::new(r.x + 42.0, r.y, r.w - 50.0, r.h), 13.5, self.tab == *t, TEXT, 0.92, Align::Left);
            p.text(value, R::new(r.x + 42.0, r.y, r.w - 52.0, r.h), 12.0, false, TEXT, 0.5, Align::Right);
            p.hit(r, Act::Tab(*t));
        }
        // En altta: yönetici değilsek düğme, yöneticiysek yalnızca rozet.
        let r = R::new(10.0, h - 52.0, SIDEBAR - 20.0, 38.0);
        if self.admin {
            p.glyph('\u{EA18}', R::new(r.x + 10.0, r.y, 24.0, r.h), 15.0, self.accent, 1.0);
            p.text(t!("Yönetici olarak çalışıyor", "Running as admin"), R::new(r.x + 42.0, r.y, r.w - 50.0, r.h), 12.5, false, TEXT, 0.6, Align::Left);
        } else {
            p.round(r, 7.0, 0xFFFFFF, if p.hover(r) { 0.12 } else { 0.06 });
            p.outline(r, 7.0, 0xFFFFFF, 0.10);
            p.glyph('\u{EA18}', R::new(r.x + 10.0, r.y, 24.0, r.h), 15.0, TEXT, 0.9);
            p.text(t!("Yönetici olarak çalıştır", "Run as administrator"), R::new(r.x + 42.0, r.y, r.w - 50.0, r.h), 13.0, false, TEXT, 0.92, Align::Left);
            p.hit(r, Act::RunAsAdmin);
        }
    }

    /// Sağ tık: tek süreç ya da bir uygulamanın tüm süreçleri için.
    fn context_menu(&mut self, sel: Act) {
        self.act(sel);
        let (name, targets) = self.targets();
        let Some(first) = targets.first() else { return };
        let pid = first.pid;
        let suspended = targets.iter().all(|r| r.suspended);
        self.paint().ok();
        let g = |c: char| Icon::Glyph(c, None);
        let item = |id: usize, t: &str, icon: Icon| Row::Item { id, text: t.into(), icon };
        let eco = power::eco(pid).unwrap_or(false);
        let rows = vec![
            Row::Label(name, Icon::None),
            item(1, t!("Dosya konumunu aç", "Open file location"), g('\u{E838}')),
            Row::Sep,
            Row::Header(t!("Çekirdekler", "Cores").into()),
            item(4, t!("Yalnızca P çekirdekleri", "P-cores only"), g('\u{E945}')),
            item(5, t!("Yalnızca E çekirdekleri", "E-cores only"), g('\u{EC0A}')),
            item(6, t!("Tüm çekirdekler", "All cores"), g('\u{E950}')),
            item(7, if eco { t!("Verimlilik modunu kapat", "Turn off efficiency mode") } else { t!("Verimlilik modu", "Efficiency mode") }, g('\u{EC0A}')),
            Row::Sep,
            if suspended { item(3, t!("Sürdür", "Resume"), g('\u{E768}')) } else { item(2, t!("Durdur", "Suspend"), Icon::Glyph('\u{E733}', Some(draw::RED))) },
            item(8, t!("Görevi sonlandır", "End task"), Icon::Glyph('\u{E711}', Some(draw::RED))),
        ];
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        // Menü imlecin altında değil üstünde açılır; ekrana sığdırılır.
        let n = rows.len() as i32;
        let at = (pt.x + 120, pt.y + (n * 32 * self.scale as i32) / 2);
        let id = glassmenu::run(self.hwnd, rows, Some(self.accent), at);
        let a = match id {
            Some(1) => Act::Reveal,
            Some(2) => Act::Suspend,
            Some(3) => Act::Resume,
            Some(4) => Act::Route(power::Route::Performance),
            Some(5) => Act::Route(power::Route::Efficient),
            Some(6) => Act::Route(power::Route::All),
            Some(7) => Act::Eco,
            Some(8) => Act::Kill,
            _ => return,
        };
        self.act(a);
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with_app(|a| a.paint().ok());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            with_app(|a| a.invalidate());
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_TICK => {
            with_app(|a| {
                a.tick();
                if a.tab == Tab::Startup {
                    if let Some(s) = a.startup_slot.lock().ok().and_then(|mut s| s.take()) {
                        a.startup = Some(s);
                    }
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_ANIM => {
            with_app(|a| {
                if !groups::animating(a) {
                    let _ = KillTimer(hwnd, TIMER_ANIM);
                }
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_APP_SIGNED => {
            with_app(|a| {
                if let Some(s) = a.startup_slot.lock().ok().and_then(|mut s| s.take()) {
                    a.startup = Some(s);
                }
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|a| {
                let (x, y) = xy(lp);
                let pt = (x / a.scale, y / a.scale);
                if let Some(act) = a.hit_at(pt) {
                    a.act(act);
                }
            });
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let target = with_app(|a| {
                let (x, y) = xy(lp);
                match a.hit_at((x / a.scale, y / a.scale)) {
                    Some(Act::Proc(pid)) => Some(Act::Proc(pid)),
                    Some(Act::Group(k)) | Some(Act::Expand(k)) => Some(Act::Group(k)),
                    _ => None,
                }
            })
            .flatten();
            if let Some(sel) = target {
                with_app(|a| a.context_menu(sel));
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            with_app(|a| {
                let d = (wp.0 >> 16) as i16 as f32 / 120.0;
                let max = a.max_scroll.get(&a.tab).copied().unwrap_or(0.0);
                let s = a.scroll.entry(a.tab).or_insert(0.0);
                *s = (*s - d * 90.0).clamp(0.0, max);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            with_app(|a| {
                if !matches!(a.tab, Tab::Processes | Tab::Startup) {
                    return;
                }
                let c = char::from_u32(wp.0 as u32).unwrap_or('\0');
                match c {
                    '\u{8}' => {
                        a.filter.pop();
                    }
                    '\u{1b}' => a.filter.clear(),
                    c if !c.is_control() => a.filter.push(c),
                    _ => return,
                }
                a.scroll.insert(a.tab, 0.0);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if VIRTUAL_KEY(wp.0 as u16) == VK_DELETE {
                with_app(|a| {
                    if a.tab == Tab::Processes {
                        a.act(Act::Kill);
                    }
                });
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DPICHANGED => {
            with_app(|a| {
                a.scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                a.rt = None;
            });
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lp.0 as *mut MINMAXINFO);
            mm.ptMinTrackSize = POINT { x: 900, y: 560 };
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() -> Result<()> {
    log::init("taskmgr");
    // Ayarı okumak arayüz dilini de uygular.
    let _ = carlui_core::config::load();
    // Yönetici izni gereken başlangıç değişikliği için kendini UAC ile açtığında.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--startup-set") {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        std::process::exit(startup::run_helper(&argv[i + 1..]));
    }
    let pid_arg = std::env::args().skip_while(|a| a != "--pid").nth(1).and_then(|s| s.parse::<u32>().ok());
    let tab_arg = std::env::args().skip_while(|a| a != "--tab").nth(1).and_then(|s| s.parse::<usize>().ok()).and_then(|i| TABS.get(i).copied());
    let replace = argv.iter().any(|a| a == "--replace");
    let admin = admin::is_elevated();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let m = CreateMutexW(None, true, w!("Local\\CarlUI.TaskMgr"));
        // Yönetici olarak yeniden açılırken eski kopya kapanana kadar bekle.
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        let taken = replace && exists && m.as_ref().is_ok_and(|m| {
            let r = windows::Win32::System::Threading::WaitForSingleObject(*m, 5000);
            r == WAIT_OBJECT_0 || r == WAIT_ABANDONED
        });
        if exists && !taken {
            if let Ok(h) = FindWindowW(w!("CarlUI.TaskMgr"), None) {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
            return Ok(());
        }
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION))?,
            lpszClassName: w!("CarlUI.TaskMgr"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem().max(96) as f32 / 96.0;
        let (ww, wh) = ((1180.0 * dpi) as i32, (760.0 * dpi) as i32);
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("CarlUI.TaskMgr"), &HSTRING::from(if admin { t!("Görev Yöneticisi (Yönetici)", "Task Manager (Administrator)") } else { t!("Görev Yöneticisi", "Task Manager") }), WS_OVERLAPPEDWINDOW, (sw - ww) / 2, (sh - wh) / 2, ww, wh, None, None, inst, None)?;
        // Ayarlar'da dil değişince (ayar dosyası yeniden okunur, dil uygulanır)
        // başlık ve sayfa hemen yeni dile geçer.
        {
            let h = hwnd.0 as isize;
            carlui_core::config::watch(move |_| {
                let hwnd = HWND(h as _);
                let title = if admin { t!("Görev Yöneticisi (Yönetici)", "Task Manager (Administrator)") } else { t!("Görev Yöneticisi", "Task Manager") };
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, &HSTRING::from(title));
                let _ = InvalidateRect(hwnd, None, false);
            });
        }
        let on: i32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });

        let signs = sign::Checker::new({
            let h = hwnd.0 as isize;
            move || {
                let _ = PostMessageW(HWND(h as _), WM_APP_SIGNED, WPARAM(0), LPARAM(0));
            }
        });
        let mut app = App {
            hwnd,
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            tab: tab_arg.unwrap_or(Tab::Processes),
            sampler: sample::Sampler::new(),
            procs: procs::Procs::new(),
            signs,
            selected: pid_arg,
            selected_group: None,
            expanded: HashMap::new(),
            sort: (SortCol::Cpu, true),
            unsigned_first: false,
            hide_windows: false,
            filter: String::new(),
            scroll: HashMap::new(),
            max_scroll: HashMap::new(),
            startup: None,
            startup_slot: Arc::default(),
            start_selected: None,
            start_hide_windows: true,
            cpu_detail: false,
            mode: power::mode(),
            routes: HashMap::new(),
            message: None,
            admin,
        };
        if app.tab == Tab::Startup {
            app.start_scan();
        }
        // --pid ile gelindiyse süreci listede bulunur yap (ada göre sırala, filtrele).
        if let Some(pid) = pid_arg {
            if let Some(r) = app.procs.rows.iter().find(|r| r.pid == pid) {
                app.filter = r.name.clone();
            }
        }
        APP.with(|a| *a.borrow_mut() = Some(app));
        SetTimer(hwnd, TIMER_TICK, 1000, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
