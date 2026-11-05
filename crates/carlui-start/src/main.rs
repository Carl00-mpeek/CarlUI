//! Bu modül Win menüsü (`carlui-start.exe`). Sorun: Windows tuşu ya aramayı ya
//! da Windows'un kalabalık Başlat menüsünü açıyordu; sık kullanılan uygulamaları
//! ve CarlUI'nin kendi ayarlarını tek dokunuşla açacak sade bir yer yoktu.
//! Çözüm: dock'un üstünde açılan, akrilik camlı kompakt bir menü:
//! - Kullanıcının seçtiği uygulamalar ızgarada; sürükleyerek sıralanır, bir
//!   uygulamayı diğerinin üstüne bırakmak klasör yapar (telefonlardaki gibi).
//! - "Ekle" tüm yüklü uygulamaları (Windows'un "Tüm uygulamalar" listesi)
//!   süzgeçli bir listede gösterir; tıklamak sabitler/kaldırır.
//! - Üstteki çubuk adanın aramasını açar; menüdeyken yazmaya başlamak da öyle.
//! - Altta CarlUI Ayarları, Görev yöneticisi ve güç menüsü.
//! Süreç arka planda gizli bekler (anında açılsın diye); dock, Windows tuşu ya
//! da Başlat düğmesiyle `WM_CARLUI_START` gönderir. Odak gidince ya da dışarı tıklanınca (`outside.rs`) menü kapanır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod icons;
mod outside;
mod store;
mod system;
mod view;

use carlui_core::{
    config, info, log,
    win::{START_CLASS, WM_CARLUI_START},
};
use carlui_launcher::index::{self, Index};
use carlui_ui::{
    glassmenu::{self, Icon, Row},
    paint::{Fonts, R},
};
use std::{cell::RefCell, time::Instant};
use store::{Item, Store, Tile};
use system::Power;
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
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_APP_APPS: u32 = WM_APP + 1;
const WM_APP_FRAME: u32 = WM_APP + 2;
const OPEN_MS: f32 = 420.0;
const CLOSE_MS: f32 = 200.0;
const TRANS_MS: f32 = 160.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Home,
    /// Ana listedeki klasör.
    Folder(usize),
    /// Uygulama ekleme listesi; hedef klasör (yoksa ana liste).
    Add(Option<usize>),
}

#[derive(Clone, Debug)]
pub enum Act {
    Search,
    /// Izgaradaki öğe: (klasör, sıra).
    Tile(Option<usize>, usize),
    Back,
    Add,
    NewFolder,
    Rename,
    Dissolve,
    /// Uygulama listesindeki satır.
    Pick(usize),
    Settings,
    TaskMgr,
    Power,
    /// 0: İndirilenler, 1: Geri Dönüşüm Kutusu.
    Place(u8),
    Scroll,
}

pub struct App {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    pub icons: RefCell<icons::Icons>,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Act)>,
    accent: u32,
    pub store: Store,
    pub view: View,
    pub scroll: f32,
    max_scroll: f32,
    pub filter: String,
    pub rename: Option<String>,
    /// Basılan öğe ve basma noktası; imleç biraz kayınca sürükleme başlar.
    pub press: Option<(Option<usize>, usize, (f32, f32))>,
    drag: bool,
    index: Option<Index>,
    pub list: Vec<index::App>,
    visible: bool,
    /// Açılış/kapanış animasyonu.
    anim: Option<Anim>,
    /// Sayfa geçişi: (başlangıç, yön: 1 sağdan, -1 soldan).
    trans: Option<(Instant, f32)>,
    /// Menünün tam açıkken üst kenarı (fiziksel piksel).
    home_y: i32,
    home_x: i32,
    frame_queued: bool,
    /// Dışarı tıklamayla son kapanış: aynı tıklama dock'un Başlat düğmesine
    /// denk geldiyse menü hemen yeniden açılmasın.
    outside_at: Option<Instant>,
}

#[derive(Clone, Copy)]
struct Anim {
    t0: Instant,
    closing: bool,
    /// Kapanış hangi görünürlükten başladı.
    from: f32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

impl App {
    pub fn dragging(&self) -> bool {
        self.drag && self.press.is_some()
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn save(&self) {
        self.store.save();
    }

    fn go(&mut self, v: View) {
        // İçeri girerken (klasör, ekleme listesi) sağdan, geri dönerken soldan gelir.
        let deeper = match (self.view, v) {
            (View::Home, _) => true,
            (View::Folder(_), View::Add(_)) => true,
            _ => false,
        };
        if self.view != v {
            self.trans = Some((Instant::now(), if deeper { 1.0 } else { -1.0 }));
            self.kick();
        }
        self.view = v;
        self.scroll = 0.0;
        self.rename = None;
        if let View::Add(_) = v {
            self.filter.clear();
            if self.index.is_none() {
                let h = self.hwnd.0 as isize;
                self.index = Some(Index::start(move || unsafe {
                    let _ = PostMessageW(HWND(h as _), WM_APP_APPS, WPARAM(0), LPARAM(0));
                }));
            }
        }
        self.invalidate();
    }

    // ---- açma / kapama ------------------------------------------------------

    fn show(&mut self) {
        self.store = Store::load();
        self.view = View::Home;
        self.scroll = 0.0;
        self.rename = None;
        self.press = None;
        self.drag = false;
        self.scale = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0;
        let s = self.scale;
        // Dock'un camının hemen üstü (dock'un ölçüleri ayarlardan hesaplanır).
        let cfg = config::load();
        carlui_ui::tone::follow_window(&cfg.theme, self.hwnd);
        let k = cfg.dock.icon_size.clamp(20.0, 64.0) / 30.0;
        let mon = carlui_core::win::primary_monitor().map(|m| m.rect).unwrap_or_default();
        let (w, h) = ((view::W * s) as i32, (view::H * s) as i32);
        let dock_top = mon.bottom - ((62.0 * k + cfg.dock.bottom_margin.clamp(0.0, 200.0)) * s) as i32;
        let x = mon.left + (mon.right - mon.left - w) / 2;
        let y = (dock_top - (12.0 * s) as i32 - h).max(mon.top + 8);
        self.home_y = y;
        self.home_x = x;
        // Kapanırken yeniden açılırsa o anki yerden devam eder.
        let from = if self.anim.is_some_and(|a| a.closing) { self.reveal() } else { 0.0 };
        self.anim = Some(Anim { t0: Instant::now(), closing: false, from });
        self.trans = None;
        self.visible = true;
        self.mouse = (-1.0, -1.0);
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, x, y + self.slide_px(from), w, h, SWP_NOACTIVATE);
            self.place(from);
            // Gizliyken ilk kareyi (tamamen saydam içerik) çiz: önceki açılışın
            // son karesi bir an görünmesin.
            let _ = self.paint();
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        // Klavye menüye gelsin: yazmaya başlayınca harf aramaya aktarılır.
        carlui_core::win::force_foreground(self.hwnd);
        if unsafe { GetForegroundWindow() } != self.hwnd {
            carlui_core::warn!("Win menüsü klavye odağını alamadı");
        }
        // İlk çizim (ikonlar, ayarlar) zaman alabilir: süzülme ekrana geldiği
        // andan başlasın, ilk kısmı kaçırılıp menü "pat" diye belirmesin.
        if let Some(a) = &mut self.anim {
            a.t0 = Instant::now();
        }
        outside::arm(self.hwnd);
        // Dock gizliyse menüyle birlikte alttan çıksın, menü kapanana dek kalsın.
        carlui_core::win::dock_hold(true);
        self.kick();
    }

    /// Kayma mesafesi (fiziksel piksel): görünürlük 0'da 36 DIP aşağıda.
    fn slide_px(&self, reveal: f32) -> i32 {
        ((1.0 - reveal) * 36.0 * self.scale) as i32
    }

    /// Menünün tam boyu (fiziksel piksel).
    fn full_px(&self) -> (i32, i32) {
        ((view::W * self.scale) as i32, (view::H * self.scale) as i32)
    }

    /// Pencereyi gerçekten büyütür: alt kenar dock'un üstünde sabit, üst ve
    /// yanlar açılır. Bölgeyle kırpmak yetmiyordu: DWM'nin çerçevesi (kenarlık,
    /// gölge, köşeler) bölgeyi dinlemeyip tam boy beliriyordu. İçerik çizilirken
    /// aynı miktar kaydırılır; yani perde gibi açığa çıkar.
    fn place(&self, reveal: f32) {
        let (fw, fh) = self.full_px();
        let gh = 0.3 + 0.7 * reveal;
        let gw = 0.86 + 0.14 * reveal;
        let (cw, ch) = (((fw as f32) * gw) as i32, ((fh as f32) * gh) as i32);
        let x = self.home_x + (fw - cw) / 2;
        let y = self.home_y + self.slide_px(reveal) + (fh - ch);
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, x, y, cw, ch, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS);
        }
    }

    /// Menünün görünürlüğü (0 = kapalı, 1 = açık), yumuşatılmış.
    pub fn reveal(&self) -> f32 {
        match self.anim {
            None => 1.0,
            Some(a) => {
                let ms = if a.closing { CLOSE_MS } else { OPEN_MS };
                let t = (a.t0.elapsed().as_secs_f32() * 1000.0 / ms).min(1.0);
                if a.closing {
                    // Kapanış hızlanarak gider.
                    a.from * (1.0 - t * t)
                } else {
                    // Uzun, yavaşlayarak oturan süzülme.
                    a.from + (1.0 - a.from) * (1.0 - (1.0 - t).powi(4))
                }
            }
        }
    }

    /// Sayfa geçişi (klasöre gir/çık): içeriğin saydamlığı ve yatay kayması.
    pub fn trans(&self) -> (f32, f32) {
        match self.trans {
            None => (1.0, 0.0),
            Some((t0, dir)) => {
                let t = (t0.elapsed().as_secs_f32() * 1000.0 / TRANS_MS).min(1.0);
                let e = 1.0 - (1.0 - t).powi(3);
                (e, (1.0 - e) * 18.0 * dir)
            }
        }
    }

    fn animating(&self) -> bool {
        self.anim.is_some() || self.trans.is_some()
    }

    /// Animasyon karelerini başlatır: her kare ekranın yenilenmesiyle (DwmFlush)
    /// eşlenir; zamanlayıcının ~16 ms'lik kaba adımları yüzünden takılmaz.
    fn kick(&mut self) {
        if !self.frame_queued {
            self.frame_queued = true;
            unsafe {
                let _ = PostMessageW(self.hwnd, WM_APP_FRAME, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn frame(&mut self) {
        self.frame_queued = false;
        if let Some(a) = self.anim {
            let ms = if a.closing { CLOSE_MS } else { OPEN_MS };
            let done = a.t0.elapsed().as_secs_f32() * 1000.0 >= ms;
            let r = if done && !a.closing { 1.0 } else { self.reveal() };
            if done {
                self.anim = None;
                if a.closing {
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                    }
                    return;
                }
            }
            self.place(r);
        }
        if self.trans.is_some_and(|(t0, _)| t0.elapsed().as_secs_f32() * 1000.0 >= TRANS_MS) {
            self.trans = None;
        }
        let _ = self.paint();
        unsafe {
            let _ = ValidateRect(self.hwnd, None);
        }
        if self.animating() {
            unsafe {
                let _ = DwmFlush();
            }
            self.kick();
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        outside::disarm();
        carlui_core::win::dock_hold(false);
        self.finish_rename();
        unsafe {
            let _ = ReleaseCapture();
        }
        self.press = None;
        self.drag = false;
        // Açılırken kapatılırsa o anki yerden geri döner.
        let from = self.reveal();
        self.anim = Some(Anim { t0: Instant::now(), closing: true, from });
        self.kick();
    }

    fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else if !self.outside_at.is_some_and(|t| t.elapsed().as_millis() < 500) {
            self.show();
        }
    }

    // ---- eylemler -----------------------------------------------------------

    fn finish_rename(&mut self) {
        if let (Some(s), View::Folder(f)) = (self.rename.take(), self.view) {
            let s = s.trim().to_string();
            if let (false, Some(Item::Folder { name, .. })) = (s.is_empty(), self.store.items.get_mut(f)) {
                *name = s;
                self.save();
            }
        }
    }

    fn launch(&mut self, t: &Tile) {
        self.hide();
        system::launch(t);
    }

    fn open_tile(&mut self, folder: Option<usize>, i: usize) {
        match (folder, self.store.items.get(i)) {
            (None, Some(Item::Folder { .. })) => self.go(View::Folder(i)),
            _ => {
                if let Some(t) = view::tile_of(self, folder, i) {
                    self.launch(&t);
                }
            }
        }
    }

    fn act(&mut self, a: Act) {
        if !matches!(a, Act::Rename) {
            self.finish_rename();
        }
        match a {
            Act::Search => {
                self.hide();
                carlui_core::win::toggle_search();
            }
            Act::Tile(f, i) => self.open_tile(f, i),
            Act::Back => match self.view {
                View::Add(Some(f)) => self.go(View::Folder(f)),
                _ => self.go(View::Home),
            },
            Act::Add => {
                let target = if let View::Folder(f) = self.view { Some(f) } else { None };
                self.go(View::Add(target));
            }
            Act::NewFolder => {
                let f = self.store.new_folder();
                self.save();
                self.go(View::Folder(f));
                self.rename = self.store.items.get(f).map(|i| i.name().to_string());
            }
            Act::Rename => {
                if let View::Folder(f) = self.view {
                    self.rename = self.store.items.get(f).map(|i| i.name().to_string());
                }
            }
            Act::Dissolve => {
                if let View::Folder(f) = self.view {
                    self.store.dissolve(f);
                    self.save();
                    self.go(View::Home);
                }
            }
            Act::Pick(i) => {
                if let (View::Add(target), Some(a)) = (self.view, self.list.get(i)) {
                    self.store.toggle(target, Tile { name: a.name.clone(), path: a.launch_path(), args: String::new() });
                    self.save();
                }
            }
            Act::Settings => {
                self.hide();
                system::sibling("carlui-settings.exe");
            }
            Act::TaskMgr => {
                self.hide();
                system::sibling("carlui-taskmgr.exe");
            }
            Act::Power => self.power_menu(),
            Act::Place(n) => {
                self.hide();
                system::place(n);
            }
            Act::Scroll => {}
        }
        self.invalidate();
    }

    fn drop_drag(&mut self) {
        let Some((folder, i, _)) = self.press else { return };
        match folder {
            None => {
                let (slot, onto) = view::drop_at(self.mouse, self.scroll, self.store.items.len());
                let is_app = matches!(self.store.items.get(i), Some(Item::App(_)));
                match onto {
                    Some(o) if is_app && o != i => {
                        if let Some(f) = self.store.merge(i, o) {
                            info!("Win menüsü: yeni klasör ({f})");
                        }
                    }
                    Some(o) if o == i => {}
                    _ => self.store.reorder(None, i, if slot > i { slot - 1 } else { slot }),
                }
            }
            Some(f) => {
                if self.mouse.1 < view::GY - 8.0 {
                    self.store.move_out(f, i);
                } else {
                    let n = match self.store.items.get(f) {
                        Some(Item::Folder { apps, .. }) => apps.len(),
                        _ => 0,
                    };
                    let (slot, _) = view::drop_at(self.mouse, self.scroll, n);
                    self.store.reorder(Some(f), i, if slot > i { slot - 1 } else { slot });
                }
            }
        }
        self.save();
    }

    // ---- sağ tık menüleri ---------------------------------------------------

    fn menu(&mut self, rows: Vec<Row>) -> Option<usize> {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let r = glassmenu::run(self.hwnd, rows, Some(self.accent), (pt.x, pt.y));
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        r
    }

    fn item(id: usize, text: &str, g: char) -> Row {
        Row::Item { id, text: text.into(), icon: Icon::Glyph(g, None) }
    }

    fn context(&mut self, a: Option<Act>) {
        self.finish_rename();
        match a {
            Some(Act::Tile(None, i)) if matches!(self.store.items.get(i), Some(Item::Folder { .. })) => {
                let rows = vec![Self::item(0, t!("Aç", "Open"), '\u{E8DA}'), Self::item(1, t!("Yeniden adlandır", "Rename"), '\u{E8AC}'), Row::Sep, Self::item(2, t!("Klasörü dağıt", "Ungroup folder"), '\u{E7A7}')];
                match self.menu(rows) {
                    Some(0) => self.go(View::Folder(i)),
                    Some(1) => {
                        self.go(View::Folder(i));
                        self.rename = self.store.items.get(i).map(|x| x.name().to_string());
                    }
                    Some(2) => {
                        self.store.dissolve(i);
                        self.save();
                    }
                    _ => {}
                }
            }
            Some(Act::Tile(None, i)) => {
                let rows = vec![Self::item(0, t!("Aç", "Open"), '\u{E8A7}'), Self::item(1, t!("Yeni klasöre koy", "Put in new folder"), '\u{E8F4}'), Row::Sep, Self::item(2, t!("Sabitlemeyi kaldır", "Unpin"), '\u{E77A}')];
                match self.menu(rows) {
                    Some(0) => self.open_tile(None, i),
                    Some(1) => {
                        if let Some(Item::App(t)) = self.store.items.get(i).cloned() {
                            self.store.items[i] = Item::Folder { name: t!("Yeni klasör", "New folder").into(), apps: vec![t] };
                            self.save();
                            self.go(View::Folder(i));
                            self.rename = Some(t!("Yeni klasör", "New folder").into());
                        }
                    }
                    Some(2) => {
                        self.store.remove(None, i);
                        self.save();
                    }
                    _ => {}
                }
            }
            Some(Act::Tile(Some(f), i)) => {
                let rows = vec![Self::item(0, t!("Aç", "Open"), '\u{E8A7}'), Self::item(1, t!("Klasörden çıkar", "Remove from folder"), '\u{E8DE}'), Row::Sep, Self::item(2, t!("Sabitlemeyi kaldır", "Unpin"), '\u{E77A}')];
                match self.menu(rows) {
                    Some(0) => self.open_tile(Some(f), i),
                    Some(1) => {
                        self.store.move_out(f, i);
                        self.save();
                    }
                    Some(2) => {
                        self.store.remove(Some(f), i);
                        self.save();
                    }
                    _ => {}
                }
            }
            Some(Act::Scroll) if !matches!(self.view, View::Add(_)) => {
                let mut rows = vec![Self::item(0, t!("Uygulama ekle", "Add app"), '\u{E710}')];
                if self.view == View::Home {
                    rows.push(Self::item(1, t!("Yeni klasör", "New folder"), '\u{E8F4}'));
                }
                match self.menu(rows) {
                    Some(0) => self.act(Act::Add),
                    Some(1) => self.act(Act::NewFolder),
                    _ => {}
                }
            }
            _ => {}
        }
        self.invalidate();
    }

    fn power_menu(&mut self) {
        let rows = vec![
            Self::item(0, t!("Kilitle", "Lock"), '\u{E72E}'),
            Self::item(1, t!("Oturumu kapat", "Sign out"), '\u{F3B1}'),
            Row::Sep,
            Self::item(2, t!("Uyku", "Sleep"), '\u{E708}'),
            Self::item(3, t!("Yeniden başlat", "Restart"), '\u{E777}'),
            Self::item(4, t!("Kapat", "Shut down"), '\u{E7E8}'),
        ];
        let p = match self.menu(rows) {
            Some(0) => Power::Lock,
            Some(1) => Power::SignOut,
            Some(2) => Power::Sleep,
            Some(3) => Power::Restart,
            Some(4) => Power::Shutdown,
            _ => return,
        };
        self.hide();
        system::power(p);
    }

    // ---- klavye -------------------------------------------------------------

    fn char(&mut self, c: u16) {
        match c {
            0x1B => {
                if self.rename.take().is_some() {
                } else if self.view == View::Home {
                    self.hide();
                } else {
                    self.act(Act::Back);
                }
            }
            0x0D => self.finish_rename(),
            0x08 => {
                if let Some(s) = self.rename.as_mut() {
                    s.pop();
                } else if let View::Add(_) = self.view {
                    self.filter.pop();
                    self.scroll = 0.0;
                }
            }
            c if c >= 0x20 => {
                let Some(ch) = char::from_u32(c as u32) else { return };
                if let Some(s) = self.rename.as_mut() {
                    if s.chars().count() < 40 {
                        s.push(ch);
                    }
                } else if let View::Add(_) = self.view {
                    self.filter.push(ch);
                    self.scroll = 0.0;
                } else {
                    // Menüdeyken yazmaya başlamak: arama, ilk harfiyle birlikte.
                    self.hide();
                    carlui_core::win::search_with(c);
                }
            }
            _ => {}
        }
        self.invalidate();
    }

    // ---- çizim --------------------------------------------------------------

    fn refresh_list(&mut self) {
        let View::Add(_) = self.view else { return };
        let all = self.index.as_ref().map(|i| i.snapshot()).unwrap_or_default();
        let q = self.filter.to_lowercase();
        self.list = all.iter().filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q)).cloned().collect();
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
            self.icons.borrow_mut().clear();
        }
        let rt = self.rt.clone().unwrap();
        unsafe {
            let size = rt.GetPixelSize();
            if size.width != pw || size.height != ph {
                rt.Resize(&D2D_SIZE_U { width: pw, height: ph })?;
            }
        }
        self.refresh_list();
        let s = self.scale;
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        // Açılırken pencere tam boydan küçük: içerik tam boydaki yerinde kalsın
        // (alt kenara ve ortaya yaslı), pencere büyüdükçe açığa çıksın.
        let (fw, fh) = self.full_px();
        let (ox, oy) = (((fw - pw as i32) / 2).max(0) as f32, (fh - ph as i32).max(0) as f32);
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: -ox, M32: -oy });
            // Opak zemin (eski camın koyu tonu).
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.17, g: 0.17, b: 0.18, a: 1.0 }));
        }
        let content = {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = view::P { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            view::paint(self, &mut p)
        };
        self.max_scroll = (content - view::clip().h + 12.0).max(0.0);
        self.hits = hits;
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
        }
        Ok(())
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<Act> {
        self.hits.iter().rev().find(|(r, _)| r.contains(pt)).map(|(_, a)| a.clone())
    }

    /// Test: menünün durumunu `%LOCALAPPDATA%\CarlUI\start-state.json`'a yazar.
    fn dump(&self) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut rc);
        }
        let state = serde_json::json!({
            "visible": self.visible,
            "view": format!("{:?}", self.view),
            "rect": [rc.left, rc.top, rc.right, rc.bottom],
            "scale": self.scale,
            "items": self.store.items,
            "list": self.list.len(),
        });
        let _ = std::fs::write(carlui_core::paths::data_dir().join("start-state.json"), serde_json::to_vec_pretty(&state).unwrap_or_default());
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
        WM_CARLUI_START => {
            // wParam 1: test için durum dökümü.
            with_app(|a| if wp.0 == 1 { a.dump() } else { a.toggle() });
            LRESULT(0)
        }
        WM_APP_APPS => {
            with_app(|a| a.invalidate());
            LRESULT(0)
        }
        WM_APP_FRAME => {
            with_app(|a| a.frame());
            LRESULT(0)
        }
        outside::WM_APP_OUTSIDE => {
            with_app(|a| {
                if a.visible {
                    a.outside_at = Some(Instant::now());
                    a.hide();
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                if let Some((_, _, p0)) = a.press {
                    if !a.drag && ((a.mouse.0 - p0.0).powi(2) + (a.mouse.1 - p0.1).powi(2)) > 36.0 {
                        a.drag = true;
                    }
                }
                // Sürüklerken kenara yaklaşınca kaydır.
                if a.drag {
                    let c = view::clip();
                    if a.mouse.1 > c.bottom() - 20.0 {
                        a.scroll = (a.scroll + 8.0).min(a.max_scroll);
                    } else if a.mouse.1 < c.y + 20.0 && a.mouse.1 > c.y {
                        a.scroll = (a.scroll - 8.0).max(0.0);
                    }
                }
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                a.invalidate();
            });
            LRESULT(0)
        }
        0x02A3 /* WM_MOUSELEAVE */ => {
            with_app(|a| {
                if a.press.is_none() {
                    a.mouse = (-1.0, -1.0);
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                if !a.visible {
                    return;
                }
                match a.hit_at(a.mouse) {
                    Some(Act::Tile(f, i)) => {
                        a.finish_rename();
                        a.press = Some((f, i, a.mouse));
                        a.drag = false;
                        SetCapture(hwnd);
                    }
                    Some(act) => a.act(act),
                    None => {
                        a.finish_rename();
                        a.invalidate();
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                if let Some((f, i, _)) = a.press {
                    let _ = ReleaseCapture();
                    if a.drag {
                        a.drop_drag();
                    } else {
                        a.press = None;
                        a.open_tile(f, i);
                    }
                    a.press = None;
                    a.drag = false;
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                let act = a.hit_at(a.mouse);
                a.context(act);
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            with_app(|a| {
                let d = (wp.0 >> 16) as i16 as f32 / 120.0;
                a.scroll = (a.scroll - d * 70.0).clamp(0.0, a.max_scroll);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            with_app(|a| a.char(wp.0 as u16));
            LRESULT(0)
        }
        WM_ACTIVATE => {
            // Odak başka yere gitti: kapan. (Sağ tık menüsü açıkken `with_app`
            // meşgul olduğu için bu mesaj yok sayılır.)
            if wp.0 & 0xFFFF == 0 {
                with_app(|a| a.hide());
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DPICHANGED => {
            with_app(|a| {
                a.scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                a.rt = None;
                a.invalidate();
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

fn main() -> Result<()> {
    log::init("start");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Start"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // İkinci kez çalıştırmak: açık olanı aç/kapat.
            carlui_core::win::toggle_start();
            return Ok(());
        }
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: START_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, START_CLASS, w!("CarlUI Menu"), WS_POPUP, 0, 0, 1, 1, None, None, inst, None)?;
        let on = i32::from(!carlui_ui::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, 4);
        // Sistem camı (blur) yok: cam pencere bölgesi kırpmasını dinlemediği için
        // açılışta tam boy belirip içerikten önce geliyordu. Zemin D2D ile
        // opak çizilir; böylece zemin ve içerik tek parça büyür.
        let border: u32 = 0x00505050;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as _, 4);

        let app = App {
            hwnd,
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            icons: RefCell::new(icons::Icons::new()?),
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            store: Store::load(),
            view: View::Home,
            scroll: 0.0,
            max_scroll: 0.0,
            filter: String::new(),
            rename: None,
            press: None,
            drag: false,
            index: None,
            list: Vec::new(),
            visible: false,
            anim: None,
            trans: None,
            home_y: 0,
            home_x: 0,
            frame_queued: false,
            outside_at: None,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));
        info!("Win menüsü hazır");
        if std::env::args().any(|a| a == "--show") {
            with_app(|a| a.show());
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
