//! Pencere: durum, arka plan işleri ve girdi. Çizim `list` (uygulama listesi ve
//! ayrıntı) ile `flow` (kaldırma adımları) dosyalarındadır. Kayıt, disk ve
//! kaldırma işlerinin hepsi arka planda yürür; sonuçlar kuyruğa düşer ve
//! `WM_APP_TICK` ile pencereye bildirilir.

mod flow;
mod icons;
mod list;

use crate::{
    apps::{self, App},
    clean::{self, Report},
    field::Field,
    leftovers::{self, Left},
    reg, run,
};
use carlui_ui::{
    paint::{Painter, R},
    view::{self, Ctx, Opts, View, WM_APP_TICK},
};
use icons::Icons;
use std::{
    cmp::Reverse,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL},
        },
    },
};

const PAD: f32 = 16.0;

#[derive(Clone, Debug)]
pub enum A {
    Nothing,
    Search,
    Tab(usize),
    Sort(usize),
    Refresh,
    Row(usize),
    Uninstall,
    Yes,
    Back,
    ScanOnly,
    OpenPlace,
    /// Kaldırıcı bitti, artıklara geç.
    Finished,
    Toggle(usize),
    All,
    Reveal(usize),
    Delete,
    /// Listeye dön ve listeyi yenile.
    Close,
}

enum Msg {
    Apps(Vec<App>),
    /// Kaldırıcı kapandı ama kayıt duruyor; varsa başlatma hatası.
    Exited(Option<String>),
    /// Kaldırma kaydı silindi.
    Gone,
    Package(std::result::Result<(), String>),
    Scan(Vec<Left>),
    Clean(Report),
}

#[derive(Clone)]
struct Mail {
    queue: Arc<Mutex<Vec<Msg>>>,
    hwnd: isize,
}

impl Mail {
    fn send(&self, m: Msg) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push(m);
        unsafe {
            let _ = PostMessageW(HWND(self.hwnd as _), WM_APP_TICK, WPARAM(0), LPARAM(0));
        }
    }
}

enum Flow {
    Detail,
    Confirm,
    Running { exited: bool, error: Option<String> },
    Busy,
    Review { items: Vec<Left>, gone: bool, scroll: usize },
    Done(Report),
}

pub struct Ui {
    apps: Vec<App>,
    /// `apps` içindeki sıralar: süzülmüş ve sıralanmış liste.
    view: Vec<usize>,
    sel: Option<usize>,
    search: Field,
    tab: usize,
    sort: usize,
    scroll: usize,
    rows: usize,
    loading: bool,
    flow: Flow,
    /// Süren arka plan işinin başlığı.
    busy: String,
    started: Instant,
    gone: bool,
    stop: Arc<AtomicBool>,
    mail: Mail,
    icons: Icons,
    target: usize,
    /// Simgelerin bağlı olduğu çizim hedefi.
    rt: usize,
}

fn work(com: COINIT, mail: &Mail, f: impl FnOnce(&Mail) + Send + 'static) {
    let mail = mail.clone();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, com);
        }
        f(&mail);
    });
}

pub fn human(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if i == 0 { format!("{b} B") } else if v >= 100.0 { format!("{v:.0} {}", U[i]) } else { format!("{v:.1} {}", U[i]) };
    if carlui_core::lang::is_tr() {
        s.replace('.', ",")
    } else {
        s
    }
}

fn open(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn reveal(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), w!("explorer.exe"), &HSTRING::from(format!("/select,\"{path}\"")), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

impl Ui {
    fn selected(&self) -> Option<&App> {
        self.sel.and_then(|i| self.apps.get(i))
    }

    fn load(&mut self) {
        self.loading = true;
        work(COINIT_MULTITHREADED, &self.mail, |m| m.send(Msg::Apps(apps::list())));
    }

    /// Süzgeç ve sıralama burada, çizimin dışında uygulanır.
    fn refilter(&mut self) {
        let q = self.search.text.to_lowercase();
        let mut v: Vec<usize> = (0..self.apps.len())
            .filter(|&i| {
                let a = &self.apps[i];
                let kind = match self.tab {
                    1 => !a.is_store(),
                    2 => a.is_store(),
                    _ => true,
                };
                kind && (q.is_empty() || a.name.to_lowercase().contains(&q) || a.publisher.to_lowercase().contains(&q))
            })
            .collect();
        let apps = &self.apps;
        match self.sort {
            1 => v.sort_by_key(|&i| Reverse(apps[i].size)),
            2 => v.sort_by_cached_key(|&i| (Reverse(apps[i].date.clone()), apps[i].name.to_lowercase())),
            _ => v.sort_by_cached_key(|&i| apps[i].name.to_lowercase()),
        }
        if !self.sel.is_some_and(|s| v.contains(&s)) {
            self.sel = v.first().copied();
        }
        self.view = v;
    }

    fn scan(&mut self, gone: bool) {
        let i = self.target;
        let Some(app) = self.apps.get(i).cloned() else { return };
        let others: Vec<App> = self.apps.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, a)| a.clone()).collect();
        self.gone = gone;
        self.begin(t!("Artıklar taranıyor…", "Scanning for leftovers…").to_string());
        work(COINIT_APARTMENTTHREADED, &self.mail, move |m| m.send(Msg::Scan(leftovers::scan(&app, &others, gone))));
    }

    fn begin(&mut self, title: String) {
        self.busy = title;
        self.started = Instant::now();
        self.flow = Flow::Busy;
    }

    /// Onaydan sonra: Mağaza paketi doğrudan, program kendi kaldırıcısıyla gider.
    fn uninstall(&mut self) {
        let Some(app) = self.selected().cloned() else { return };
        self.target = self.sel.unwrap_or(0);
        carlui_core::info!("kaldırıcı: {} kaldırılıyor", app.name);
        if app.is_store() {
            self.begin(t!(format!("{} kaldırılıyor…", app.name), format!("Removing {}…", app.name)));
            work(COINIT_MULTITHREADED, &self.mail, move |m| m.send(Msg::Package(apps::remove_package(&app.package))));
            return;
        }
        self.flow = Flow::Running { exited: false, error: None };
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = self.stop.clone();
        work(COINIT_APARTMENTTHREADED, &self.mail, move |m| {
            if let Err(e) = run::wait(&app.uninstall) {
                m.send(Msg::Exited(Some(e)));
                return;
            }
            // Birçok kaldırıcı işi başka bir sürece devredip hemen kapanır;
            // gerçek bitiş kaydın silinmesidir.
            if app.still_there() {
                m.send(Msg::Exited(None));
            }
            while !stop.load(Ordering::Relaxed) {
                if !reg::exists(&app.key) {
                    m.send(Msg::Gone);
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }

    fn receive(&mut self, msg: Msg) {
        match msg {
            Msg::Apps(v) => {
                let name = self.selected().map(|a| a.name.clone());
                self.apps = v;
                self.sel = name.and_then(|n| self.apps.iter().position(|a| a.name == n));
                self.loading = false;
                self.refilter();
            }
            Msg::Exited(error) => {
                if let Flow::Running { .. } = self.flow {
                    self.flow = Flow::Running { exited: true, error };
                }
            }
            Msg::Gone => {
                if let Flow::Running { .. } = self.flow {
                    self.scan(true);
                }
            }
            Msg::Package(r) => {
                let mut rep = Report::default();
                match r {
                    Ok(()) => rep.removed = 1,
                    Err(e) => rep.failed.push(e),
                }
                self.flow = Flow::Done(rep);
            }
            Msg::Scan(items) => self.flow = Flow::Review { items, gone: self.gone, scroll: 0 },
            Msg::Clean(rep) => self.flow = Flow::Done(rep),
        }
    }

    fn step(&mut self, by: isize) {
        let at = self.sel.and_then(|s| self.view.iter().position(|v| *v == s)).unwrap_or(0) as isize;
        let at = (at + by).clamp(0, self.view.len() as isize - 1).max(0) as usize;
        self.sel = self.view.get(at).copied();
        if at < self.scroll {
            self.scroll = at;
        } else if at >= self.scroll + self.rows {
            self.scroll = at + 1 - self.rows;
        }
    }

    fn browsing(&self) -> bool {
        matches!(self.flow, Flow::Detail)
    }
}

impl View for Ui {
    type A = A;

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        self.icons.frame(4);
        match self.flow {
            Flow::Detail | Flow::Confirm => self.paint_browse(p, w, h),
            _ => self.paint_flow(p, R::new(PAD, PAD, w - PAD * 2.0, h - PAD * 2.0)),
        }
    }

    fn click(&mut self, a: A, _ctx: &mut Ctx) {
        match a {
            A::Nothing | A::Search => {}
            A::Tab(i) => {
                self.tab = i;
                self.scroll = 0;
                self.refilter();
            }
            A::Sort(i) => {
                self.sort = i;
                self.scroll = 0;
                self.refilter();
            }
            A::Refresh => self.load(),
            A::Row(i) => {
                self.sel = Some(i);
                self.flow = Flow::Detail;
            }
            A::Uninstall => self.flow = Flow::Confirm,
            A::Yes => self.uninstall(),
            A::Back => {
                self.stop.store(true, Ordering::Relaxed);
                self.flow = Flow::Detail;
            }
            A::ScanOnly => {
                self.target = self.sel.unwrap_or(0);
                let stale = self.selected().is_some_and(|a| a.stale);
                self.scan(stale);
            }
            A::OpenPlace => {
                if let Some(a) = self.selected() {
                    open(&a.location);
                }
            }
            A::Finished => {
                self.stop.store(true, Ordering::Relaxed);
                let there = self.apps.get(self.target).is_some_and(|a| a.still_there());
                self.scan(!there);
            }
            A::Toggle(i) => {
                if let Flow::Review { items, .. } = &mut self.flow {
                    if let Some(it) = items.get_mut(i) {
                        it.on = !it.on;
                    }
                }
            }
            A::All => {
                if let Flow::Review { items, .. } = &mut self.flow {
                    let on = !items.iter().all(|i| i.on);
                    items.iter_mut().for_each(|i| i.on = on);
                }
            }
            A::Reveal(i) => {
                if let Flow::Review { items, .. } = &self.flow {
                    if let Some(it) = items.get(i) {
                        reveal(&it.path);
                    }
                }
            }
            A::Delete => {
                let Flow::Review { items, .. } = &self.flow else { return };
                let chosen: Vec<Left> = items.iter().filter(|i| i.on).cloned().collect();
                self.begin(t!("Artıklar siliniyor…", "Removing leftovers…").to_string());
                work(COINIT_APARTMENTTHREADED, &self.mail, move |m| m.send(Msg::Clean(clean::clean(&chosen))));
            }
            A::Close => {
                self.flow = Flow::Detail;
                self.load();
            }
        }
    }

    fn key(&mut self, vk: u16, _ctx: &mut Ctx) -> bool {
        if matches!(self.flow, Flow::Confirm) && vk == 0x1B {
            self.flow = Flow::Detail;
            return true;
        }
        if !self.browsing() {
            return false;
        }
        match vk {
            0x26 => self.step(-1),
            0x28 => self.step(1),
            0x21 => self.step(-(self.rows as isize)),
            0x22 => self.step(self.rows as isize),
            0x74 => self.load(),
            0x1B if !self.search.text.is_empty() => {
                self.search.text.clear();
                self.refilter();
            }
            _ => {
                if !self.search.key(vk, view::ctrl()) {
                    return false;
                }
                self.scroll = 0;
                self.refilter();
            }
        }
        true
    }

    fn char(&mut self, c: char, _ctx: &mut Ctx) {
        if self.browsing() && !view::ctrl() && !c.is_control() {
            self.search.char(c);
            self.scroll = 0;
            self.refilter();
        }
    }

    fn wheel(&mut self, delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {
        let n = (delta.abs().ceil() as usize).max(1) * 3;
        let move_by = |s: usize| if delta > 0.0 { s.saturating_sub(n) } else { s + n };
        match &mut self.flow {
            Flow::Review { scroll, .. } => *scroll = move_by(*scroll),
            Flow::Detail | Flow::Confirm => self.scroll = move_by(self.scroll),
            _ => {}
        }
    }

    fn tick(&mut self, _ctx: &mut Ctx) {
        let msgs = std::mem::take(&mut *self.mail.queue.lock().unwrap_or_else(|e| e.into_inner()));
        for m in msgs {
            self.receive(m);
        }
    }

    fn animating(&self) -> bool {
        self.icons.missed || matches!(self.flow, Flow::Busy)
    }

    fn opened(&mut self, hwnd: HWND) {
        self.mail.hwnd = hwnd.0 as isize;
        self.load();
    }
}

pub fn run() -> Result<()> {
    let ui = Ui {
        apps: Vec::new(),
        view: Vec::new(),
        sel: None,
        search: Field::default(),
        tab: 0,
        // Yeni kurulanlar üstte.
        sort: 2,
        scroll: 0,
        rows: 1,
        loading: true,
        flow: Flow::Detail,
        busy: String::new(),
        started: Instant::now(),
        gone: false,
        stop: Arc::new(AtomicBool::new(false)),
        mail: Mail { queue: Arc::new(Mutex::new(Vec::new())), hwnd: 0 },
        icons: Icons::new()?,
        target: 0,
        rt: 0,
    };
    let ui = view::run(ui, Opts { title: t!("CarlUI Tam Kaldırıcı", "CarlUI Uninstaller").into(), w: 1060.0, h: 720.0, at: None, resizable: true, popup: false, ..Default::default() })?;
    ui.stop.store(true, Ordering::Relaxed);
    Ok(())
}
