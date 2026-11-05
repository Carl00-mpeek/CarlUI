//! Bu modül "anlık tekrarı oyundan çıkmadan nasıl yönetirim?" sorununu
//! çözüyor: kısayolla (varsayılan Alt+F8) açılan küçük kaplama paneli. Ana
//! sayfada kaydı başlat/durdur, tek tıkla son N dakikayı kaydet, bilgisayar
//! sesi ve mikrofon anahtarları var. Dişli düğmesi aynı pencerede ayar
//! sayfasını açar: süre, çözünürlük, FPS (30–144, sürgüyle), kalite, sıkıştırma ve iki kaynağın
//! ses düzeyi (sürgü + canlı gösterge, mikrofon testi).
//!
//! Panel hiç etkinleşmez: fare tıklamaları ve sürüklemeler çalışır ama odak
//! öndeki oyunda kalır (oyun alta inmez). Fareyi kilitleyen oyunlar için
//! panel açıkken oklar / Tab / Enter / Boşluk / Esc düşük düzeyli klavye
//! kancasıyla panele yönlendirilir (oyuna gitmez). Değişiklikler config.toml'a
//! yazılır; ana iş parçacığı dosya izleyicisiyle uygular. Çizim `draw.rs`'te.

mod draw;
mod meter;

use crate::{mixer, Status, STATUS};
use carlui_core::config::{self, Codec, Quality, ReplayConfig, Resolution};
use carlui_ui::{
    paint::R,
    view::{self, Ctx, Opts, View, WM_APP_TICK},
};
use std::{
    sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            Input::KeyboardAndMouse::*,
            Shell::ShellExecuteW,
            WindowsAndMessaging::*,
        },
    },
};

static PANEL: AtomicIsize = AtomicIsize::new(0);
static OPENING: AtomicBool = AtomicBool::new(false);

const W: f32 = 340.0;
const H_MAIN: f32 = 248.0;
const H_SETTINGS: f32 = 500.0;
const PAD: f32 = 16.0;

/// Paneli açar; açıksa kapatır.
pub fn toggle() {
    if PANEL.load(Ordering::SeqCst) != 0 {
        close();
        return;
    }
    if OPENING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("replay-panel".into()).spawn(|| {
        let r = run();
        meter::stop();
        PANEL.store(0, Ordering::SeqCst);
        OPENING.store(false, Ordering::SeqCst);
        if let Err(e) = r {
            warn!("anlık tekrar paneli açılamadı: {e}");
        }
    });
}

pub fn close() {
    let h = PANEL.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(HWND(h as _), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Durum değişti: panel açıksa hemen yeniden çizsin.
pub fn refresh() {
    let h = PANEL.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(HWND(h as _), WM_APP_TICK, WPARAM(0), LPARAM(0));
        }
    }
}

fn run() -> Result<()> {
    unsafe {
        // Birincil ekranın sağ üst köşesi.
        let mut wa = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let scale = GetDpiForSystem().max(96) as f32 / 96.0;
        let at = (wa.right - ((W + 24.0) * scale) as i32, wa.top + (24.0 * scale) as i32);
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), GetModuleHandleW(None)?, 0).ok();
        let r = view::run(Panel::new(), Opts { title: t!("Anlık tekrar", "Instant replay").into(), w: W, h: H_MAIN, at: Some(at), noactivate: true, timer_ms: 500, ..Default::default() });
        if let Some(h) = hook {
            let _ = UnhookWindowsHookEx(h);
        }
        r.map(|_| ())
    }
}

/// Panel açıkken gezinme tuşlarını oyundan alıp panele verir.
unsafe extern "system" fn key_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let panel = PANEL.load(Ordering::SeqCst);
    if code == HC_ACTION as i32 && panel != 0 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        let nav = [VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_TAB, VK_RETURN, VK_SPACE, VK_ESCAPE].contains(&vk);
        let held = |v: VIRTUAL_KEY| GetAsyncKeyState(v.0 as i32) as u16 & 0x8000 != 0;
        // Alt+Tab, Ctrl+… gibi birleşimler oyuna / Windows'a kalır.
        let mods = held(VK_MENU) || held(VK_CONTROL) || held(VK_LWIN) || held(VK_RWIN);
        let injected = k.flags.0 & LLKHF_INJECTED.0 != 0;
        if nav && !mods && !injected {
            if wp.0 as u32 == WM_KEYDOWN {
                let _ = PostMessageW(HWND(panel as _), WM_KEYDOWN, WPARAM(vk.0 as usize), LPARAM(0));
            }
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

#[derive(Clone, Copy, PartialEq)]
enum Src {
    Sys,
    Mic,
}

impl Src {
    fn gain(self) -> &'static AtomicU32 {
        match self {
            Src::Sys => &mixer::GAIN_SYS,
            Src::Mic => &mixer::GAIN_MIC,
        }
    }

    fn level(self) -> &'static AtomicU32 {
        match self {
            Src::Sys => &mixer::LEVEL_SYS,
            Src::Mic => &mixer::LEVEL_MIC,
        }
    }

    fn get(self, c: &ReplayConfig) -> u32 {
        match self {
            Src::Sys => c.audio_volume,
            Src::Mic => c.mic_volume,
        }
        .min(ReplayConfig::MAX_VOLUME)
    }

    fn set(self, c: &mut ReplayConfig, v: u32) {
        match self {
            Src::Sys => c.audio_volume = v,
            Src::Mic => c.mic_volume = v,
        }
    }
}

/// Sürgü: kaynak düzeyi ya da kare hızı.
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Vol(Src),
    Fps,
}

#[derive(Clone, PartialEq)]
enum A {
    Record,
    Save,
    Audio,
    Mic,
    Minutes(u32),
    Res(Resolution),
    Quality(Quality),
    Codec(Codec),
    Settings,
    Back,
    AllSettings,
    Folder,
    Close,
    Slide(Slot),
    Test,
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Main,
    Settings,
}

struct Panel {
    st: Status,
    page: Page,
    hwnd: HWND,
    /// Kendi değişikliğimizden sonra izleyici yetişene dek eski durumu alma.
    hold: Option<Instant>,
    blink: bool,
    /// Klavye ile gezinme: (satır, alan, eylem) çizim sırasıyla.
    nav: Vec<(u32, R, A)>,
    focus: usize,
    keyboard: bool,
    /// Sürgülerin çizildiği izler (fareden değere çevirmek için).
    sliders: Vec<(Slot, R)>,
    dragging: Option<Slot>,
    /// Gösterilen düzeyler (0..1, yavaşça düşer).
    levels: [f32; 2],
}

impl Panel {
    fn new() -> Self {
        let st = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
        Self { st, page: Page::Main, hwnd: HWND::default(), hold: None, blink: false, nav: Vec::new(), focus: 0, keyboard: false, sliders: Vec::new(), dragging: None, levels: [0.0; 2] }
    }

    fn pull(&mut self) {
        let Some(s) = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        if self.dragging.is_some() || self.hold.is_some_and(|t| t.elapsed().as_millis() < 1500) {
            // Durum bilgisi (süre, boyut) gelsin; ayarlar bizimki kalsın.
            let cfg = std::mem::take(&mut self.st.cfg);
            self.st = Status { cfg, ..s };
        } else {
            self.hold = None;
            self.st = s;
        }
    }

    /// Ayarı değiştirip kaydeder (izleyici ana iş parçacığına iletir).
    fn edit(&mut self, f: impl Fn(&mut ReplayConfig)) {
        f(&mut self.st.cfg);
        let mut c = config::load();
        f(&mut c.replay);
        config::save(&c);
        self.hold = Some(Instant::now());
    }

    fn recording(&self) -> bool {
        self.st.cfg.enabled && self.st.started.is_some()
    }

    /// Sürgü değerini canlı uygular (kaydetmeden); bırakınca `commit`.
    /// Düzey hemen duyulur; kare hızı kayıt yeniden kurulacağı için
    /// yalnızca bırakınca uygulanır.
    fn set_slot(&mut self, s: Slot, v: u32) {
        match s {
            Slot::Vol(k) => {
                let v = v.min(ReplayConfig::MAX_VOLUME);
                k.set(&mut self.st.cfg, v);
                k.gain().store(v, Ordering::SeqCst);
            }
            Slot::Fps => self.st.cfg.fps = v,
        }
        self.hold = Some(Instant::now());
    }

    fn slot_value(&self, s: Slot) -> u32 {
        match s {
            Slot::Vol(k) => k.get(&self.st.cfg),
            Slot::Fps => self.st.cfg.fps(),
        }
    }

    fn commit(&mut self, s: Slot) {
        let v = self.slot_value(s);
        match s {
            Slot::Vol(k) => self.edit(|c| k.set(c, v)),
            Slot::Fps => self.edit(|c| c.fps = v),
        }
    }

    fn value_at(&self, s: Slot, x: f32) -> Option<u32> {
        let (_, r) = self.sliders.iter().find(|(k, _)| *k == s)?;
        let f = ((x - r.x) / r.w).clamp(0.0, 1.0);
        Some(match s {
            Slot::Vol(_) => {
                // 5'in katlarına yapışır; %100 civarı biraz daha yapışkan.
                let v = (f * ReplayConfig::MAX_VOLUME as f32 / 5.0).round() as u32 * 5;
                if v.abs_diff(100) <= 5 {
                    100
                } else {
                    v
                }
            }
            Slot::Fps => ReplayConfig::FPS[(f * (ReplayConfig::FPS.len() - 1) as f32).round() as usize],
        })
    }

    /// Klavye / tekerlek: bir adım sola (-1) ya da sağa (+1).
    fn nudge(&mut self, s: Slot, d: i32) {
        let v = match s {
            Slot::Vol(k) => (k.get(&self.st.cfg) as i32 + d * 5).clamp(0, ReplayConfig::MAX_VOLUME as i32) as u32,
            Slot::Fps => {
                let fps = &ReplayConfig::FPS;
                let i = fps.iter().position(|&f| f == self.st.cfg.fps()).unwrap_or(2) as i32;
                fps[(i + d).clamp(0, fps.len() as i32 - 1) as usize]
            }
        };
        if v != self.slot_value(s) {
            self.set_slot(s, v);
            self.commit(s);
        }
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        self.nav.clear();
        self.focus = 0;
        self.sliders.clear();
        if page == Page::Main {
            meter::stop();
        }
        unsafe {
            let h = if page == Page::Main { H_MAIN } else { H_SETTINGS };
            let scale = GetDpiForWindow(self.hwnd).max(96) as f32 / 96.0;
            let mut r = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut r);
            let _ = SetWindowPos(self.hwnd, None, 0, 0, r.right - r.left, (h * scale) as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            // Ayar sayfasında göstergeler akıcı olsun; ana sayfada seyrek uyan.
            SetTimer(self.hwnd, 1, if page == Page::Main { 500 } else { 50 }, None);
        }
    }

    fn mv(&mut self, dr: i32) {
        let Some((row, r, _)) = self.nav.get(self.focus).cloned() else { return };
        let cx = r.x + r.w / 2.0;
        let mut rows: Vec<u32> = self.nav.iter().map(|n| n.0).collect();
        rows.dedup();
        let Some(i) = rows.iter().position(|&x| x == row) else { return };
        let j = i as i32 + dr;
        if j < 0 || j >= rows.len() as i32 {
            return;
        }
        let target = rows[j as usize];
        // Hedef satırda yatayda en yakın öğe.
        if let Some((k, _)) = self.nav.iter().enumerate().filter(|(_, n)| n.0 == target).min_by(|a, b| {
            let da = (a.1 .1.x + a.1 .1.w / 2.0 - cx).abs();
            let db = (b.1 .1.x + b.1 .1.w / 2.0 - cx).abs();
            da.total_cmp(&db)
        }) {
            self.focus = k;
        }
    }
}

impl View for Panel {
    type A = A;

    fn opened(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        PANEL.store(hwnd.0 as isize, Ordering::SeqCst);
    }

    fn tick(&mut self, _ctx: &mut Ctx) {
        self.pull();
        if self.page == Page::Settings {
            for (i, s) in [Src::Sys, Src::Mic].into_iter().enumerate() {
                let pk = s.level().swap(0, Ordering::Relaxed) as f32 / 32767.0;
                // -60 dB … 0 dB arası doğrusal gösterge.
                let v = if pk <= 0.001 { 0.0 } else { (1.0 + 20.0 * pk.log10() / 60.0).clamp(0.0, 1.0) };
                self.levels[i] = v.max(self.levels[i] - 0.05);
            }
        } else {
            self.blink = !self.blink;
        }
    }

    fn key(&mut self, vk: u16, ctx: &mut Ctx) -> bool {
        let vk = VIRTUAL_KEY(vk);
        if vk == VK_ESCAPE {
            if self.page == Page::Settings {
                self.go(Page::Main);
            } else {
                ctx.close = true;
            }
            return true;
        }
        if self.nav.is_empty() {
            return false;
        }
        // İlk tuş yalnızca odak halkasını gösterir.
        if !self.keyboard {
            self.keyboard = true;
            return true;
        }
        let n = self.nav.len();
        let cur = self.nav.get(self.focus).map(|x| x.2.clone());
        match (vk, cur) {
            (VK_LEFT, Some(A::Slide(s))) => self.nudge(s, -1),
            (VK_RIGHT, Some(A::Slide(s))) => self.nudge(s, 1),
            (VK_TAB, _) if view::shift() => self.focus = (self.focus + n - 1) % n,
            (VK_TAB, _) => self.focus = (self.focus + 1) % n,
            (VK_LEFT, _) => self.focus = self.focus.saturating_sub(1),
            (VK_RIGHT, _) => self.focus = (self.focus + 1).min(n - 1),
            (VK_UP, _) => self.mv(-1),
            (VK_DOWN, _) => self.mv(1),
            (VK_RETURN | VK_SPACE, Some(a)) => self.click(a, ctx),
            _ => return false,
        }
        true
    }

    fn press(&mut self, a: &A, at: (f32, f32), _ctx: &mut Ctx) -> bool {
        let A::Slide(s) = *a else { return false };
        if let Some(v) = self.value_at(s, at.0) {
            self.set_slot(s, v);
        }
        self.dragging = Some(s);
        true
    }

    fn drag(&mut self, at: (f32, f32), _ctx: &mut Ctx) {
        if let Some(s) = self.dragging {
            if let Some(v) = self.value_at(s, at.0) {
                self.set_slot(s, v);
            }
        }
    }

    fn release(&mut self, _at: (f32, f32), _ctx: &mut Ctx) {
        if let Some(s) = self.dragging.take() {
            self.commit(s);
        }
    }

    fn wheel(&mut self, delta: f32, at: (f32, f32), _ctx: &mut Ctx) {
        let hit = self.sliders.iter().find(|(_, r)| r.inset(-8.0).contains(at)).map(|x| x.0);
        if let Some(s) = hit {
            self.nudge(s, if delta > 0.0 { 1 } else { -1 });
        }
    }

    fn click(&mut self, a: A, ctx: &mut Ctx) {
        match a {
            A::Record => {
                let on = !self.st.cfg.enabled;
                self.edit(|c| c.enabled = on);
                if !on {
                    self.st.started = None;
                }
            }
            A::Save => crate::post_save(),
            A::Audio => {
                let on = !self.st.cfg.audio;
                self.edit(|c| c.audio = on);
            }
            A::Mic => {
                let on = !self.st.cfg.mic;
                self.edit(|c| c.mic = on);
            }
            A::Minutes(m) => self.edit(|c| c.minutes = m),
            A::Res(r) => self.edit(|c| c.resolution = r),
            A::Quality(q) => self.edit(|c| c.quality = q),
            A::Codec(k) => self.edit(|c| c.codec = k),
            A::Settings => self.go(Page::Settings),
            A::Back => self.go(Page::Main),
            A::Test => {
                if meter::on() {
                    meter::stop();
                } else {
                    meter::start();
                }
            }
            A::Slide(_) => {}
            A::AllSettings => {
                let _ = std::process::Command::new(carlui_core::paths::sibling_exe("carlui-settings.exe")).args(["--page", "replay"]).spawn();
                ctx.close = true;
            }
            A::Folder => {
                let dir = self.st.cfg.folder();
                let _ = std::fs::create_dir_all(&dir);
                unsafe {
                    ShellExecuteW(None, w!("open"), &HSTRING::from(dir.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                }
            }
            A::Close => ctx.close = true,
        }
    }

    fn paint(&mut self, p: &mut carlui_ui::paint::Painter<A>, w: f32, _h: f32) {
        self.nav.clear();
        self.sliders.clear();
        match self.page {
            Page::Main => self.draw_main(p, w),
            Page::Settings => self.draw_settings(p, w),
        }
        // Klavye odağı.
        if self.keyboard {
            self.focus = self.focus.min(self.nav.len().saturating_sub(1));
            if let Some((_, r, _)) = self.nav.get(self.focus) {
                p.outline(r.inset(-2.0), 8.0, p.accent, 0.95);
                p.outline(r.inset(-3.0), 9.0, p.accent, 0.5);
            }
        }
    }
}
