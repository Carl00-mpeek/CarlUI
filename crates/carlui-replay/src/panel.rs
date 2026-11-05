//! Bu modül "anlık tekrarı oyundan çıkmadan nasıl yönetirim?" sorununu
//! çözüyor: kısayolla (varsayılan Alt+F8) açılan küçük kaplama paneli. Kaydı
//! başlatır/durdurur, tek tıkla son N dakikayı kaydeder, bilgisayar sesi ve
//! mikrofonu açıp kapatır; süre, çözünürlük, FPS, kalite ve sıkıştırma
//! buradan seçilir, ayrıntılar için Ayarlar açılır.
//!
//! Panel hiç etkinleşmez: fare tıklamaları çalışır ama odak öndeki oyunda
//! kalır (oyun alta inmez). Fareyi kilitleyen oyunlar için panel açıkken
//! oklar / Tab / Enter / Boşluk / Esc düşük düzeyli klavye kancasıyla panele
//! yönlendirilir (oyuna gitmez). Değişiklikler config.toml'a yazılır; ana
//! iş parçacığı dosya izleyicisiyle uygular.

use crate::{Status, SAVING, STATUS};
use carlui_core::config::{self, Codec, Quality, ReplayConfig, Resolution};
use carlui_ui::{
    paint::{mix, Align, Painter, R, RED, TEXT},
    view::{self, Ctx, Opts, View, WM_APP_TICK},
};
use std::{
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForSystem,
            Input::KeyboardAndMouse::*,
            Shell::ShellExecuteW,
            WindowsAndMessaging::*,
        },
    },
};

static PANEL: AtomicIsize = AtomicIsize::new(0);
static OPENING: AtomicBool = AtomicBool::new(false);

const W: f32 = 400.0;
const H: f32 = 560.0;
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
        let r = view::run(Panel::new(), Opts { title: t!("Anlık tekrar", "Instant replay").into(), w: W, h: H, at: Some(at), noactivate: true, timer_ms: 500, ..Default::default() });
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

#[derive(Clone, PartialEq)]
enum A {
    Record,
    Save,
    Audio,
    Mic,
    Minutes(u32),
    Res(Resolution),
    Fps(u32),
    Quality(Quality),
    Codec(Codec),
    Settings,
    Folder,
    Close,
}

struct Panel {
    st: Status,
    /// Kendi değişikliğimizden sonra izleyici yetişene dek eski durumu alma.
    hold: Option<Instant>,
    blink: bool,
    /// Klavye ile gezinme: (satır, alan, eylem) çizim sırasıyla.
    nav: Vec<(u32, R, A)>,
    focus: usize,
    keyboard: bool,
}

fn mmss(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

impl Panel {
    fn new() -> Self {
        let st = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
        Self { st, hold: None, blink: false, nav: Vec::new(), focus: 0, keyboard: false }
    }

    fn pull(&mut self) {
        let Some(s) = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        if self.hold.is_some_and(|t| t.elapsed().as_millis() < 1500) {
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

    /// `from`'dan beri eklenen tıklanabilir alanları bir gezinme satırı yapar.
    fn mark(&mut self, p: &Painter<A>, from: usize, row: u32) {
        for (r, a) in &p.hits[from..] {
            self.nav.push((row, *r, a.clone()));
        }
    }

    fn big(p: &mut Painter<A>, r: R, glyph: char, label: &str, act: A, filled: bool, enabled: bool, glyph_color: u32) {
        let hot = enabled && p.hover(r);
        let k = if enabled { 1.0 } else { 0.4 };
        if filled {
            p.round(r, 8.0, p.accent, (if hot { 1.0 } else { 0.9 }) * k);
        } else {
            p.round(r, 8.0, 0xFFFFFF, if hot { 0.12 } else { 0.07 });
            p.outline(r, 8.0, 0xFFFFFF, if hot { 0.22 } else { 0.10 });
        }
        let fg = if filled { 0x000000 } else { TEXT };
        let tw = p.fonts.width(label, 13.5, true);
        let x = r.x + (r.w - tw - 26.0) / 2.0;
        p.glyph(glyph, R::new(x, r.y, 20.0, r.h), 15.0, if filled { fg } else { glyph_color }, 0.95 * k);
        p.text(label, R::new(x + 26.0, r.y, tw + 4.0, r.h), 13.5, true, fg, 0.95 * k, Align::Left);
        if enabled {
            p.hit(r, act);
        }
    }

    fn icon_btn(p: &mut Painter<A>, r: R, glyph: char, act: A) {
        if p.hover(r) {
            p.round(r, 6.0, 0xFFFFFF, 0.10);
        }
        p.glyph(glyph, r, 13.0, TEXT, 0.85);
        p.hit(r, act);
    }

    fn row_label(p: &Painter<A>, y: f32, x: f32, label: &str) {
        p.text(label, R::new(x, y, 100.0, 32.0), 13.0, false, TEXT, 0.75, Align::Left);
    }

    fn mv(&mut self, dr: i32) {
        let Some((row, r, _)) = self.nav.get(self.focus).cloned() else { return };
        let cx = r.x + r.w / 2.0;
        let rows: Vec<u32> = {
            let mut v: Vec<u32> = self.nav.iter().map(|n| n.0).collect();
            v.dedup();
            v
        };
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
        PANEL.store(hwnd.0 as isize, Ordering::SeqCst);
    }

    fn tick(&mut self, _ctx: &mut Ctx) {
        self.blink = !self.blink;
        self.pull();
    }

    fn key(&mut self, vk: u16, ctx: &mut Ctx) -> bool {
        let vk = VIRTUAL_KEY(vk);
        if vk == VK_ESCAPE {
            ctx.close = true;
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
        match vk {
            VK_TAB if view::shift() => self.focus = (self.focus + n - 1) % n,
            VK_TAB => self.focus = (self.focus + 1) % n,
            VK_LEFT => self.focus = self.focus.saturating_sub(1),
            VK_RIGHT => self.focus = (self.focus + 1).min(n - 1),
            VK_UP => self.mv(-1),
            VK_DOWN => self.mv(1),
            VK_RETURN | VK_SPACE => {
                let a = self.nav[self.focus].2.clone();
                self.click(a, ctx);
            }
            _ => return false,
        }
        true
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
            A::Fps(f) => self.edit(|c| c.fps = f),
            A::Quality(q) => self.edit(|c| c.quality = q),
            A::Codec(k) => self.edit(|c| c.codec = k),
            A::Settings => {
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

    fn paint(&mut self, p: &mut Painter<A>, w: f32, _h: f32) {
        self.nav.clear();
        let cfg = self.st.cfg.clone();
        let rec = self.recording();
        let saving = SAVING.load(Ordering::Relaxed);
        let x0 = PAD;
        let cw = w - PAD * 2.0;

        // Başlık: durum noktası, ad, durum satırı; sağda ayarlar ve kapat.
        let dot = if rec { RED } else { 0x8A8A8A };
        let da = if rec && self.blink { 0.45 } else { 1.0 };
        p.round(R::new(x0 + 2.0, 24.0, 10.0, 10.0), 5.0, dot, da);
        p.text(t!("Anlık tekrar", "Instant replay"), R::new(x0 + 20.0, 14.0, 200.0, 22.0), 15.5, true, TEXT, 0.95, Align::Left);
        let sub = if saving {
            t!("Kaydediliyor…", "Saving…").to_string()
        } else if rec {
            t!("Kayıt sürüyor", "Recording").to_string()
        } else if cfg.enabled {
            t!("Başlatılıyor…", "Starting…").to_string()
        } else {
            t!("Kayıt kapalı", "Recording is off").to_string()
        };
        p.text(&sub, R::new(x0 + 20.0, 36.0, 220.0, 18.0), 12.0, false, TEXT, 0.6, Align::Left);
        let s0 = p.hits.len();
        Self::icon_btn(p, R::new(w - PAD - 64.0, 16.0, 30.0, 30.0), '\u{E713}', A::Settings);
        Self::icon_btn(p, R::new(w - PAD - 30.0, 16.0, 30.0, 30.0), '\u{E8BB}', A::Close);
        self.mark(p, s0, 0);

        // Ana düğmeler.
        let y = 68.0;
        let half = (cw - 10.0) / 2.0;
        let s0 = p.hits.len();
        if cfg.enabled {
            Self::big(p, R::new(x0, y, half, 46.0), '\u{E71A}', t!("Durdur", "Stop"), A::Record, false, true, RED);
        } else {
            Self::big(p, R::new(x0, y, half, 46.0), '\u{E7C8}', t!("Kaydı başlat", "Start"), A::Record, false, true, RED);
        }
        let save_label = if saving { t!("Kaydediliyor…", "Saving…").to_string() } else { t!(format!("Son {} dk'yı kaydet", cfg.minutes()), format!("Save last {} min", cfg.minutes())) };
        Self::big(p, R::new(x0 + half + 10.0, y, half, 46.0), '\u{E74E}', &save_label, A::Save, true, rec && !saving, TEXT);
        self.mark(p, s0, 1);

        // Arabellek: ne kadarlık geçmiş hazır.
        let keep = cfg.minutes() as u64 * 60;
        let have = self.st.started.filter(|_| rec).map(|t| t.elapsed().as_secs().min(keep)).unwrap_or(0);
        let y = 126.0;
        p.text(t!("Arabellekte", "Buffered"), R::new(x0, y, 160.0, 18.0), 12.0, false, TEXT, 0.6, Align::Left);
        p.text(&format!("{} / {}", mmss(have), mmss(keep)), R::new(x0 + cw - 160.0, y, 160.0, 18.0), 12.0, false, TEXT, 0.75, Align::Right);
        p.bar(R::new(x0, y + 22.0, cw, 5.0), have as f32 / keep.max(1) as f32, if rec { p.accent } else { 0x8A8A8A });

        // Ses kartı.
        let y = 168.0;
        let card = R::new(x0, y, cw, 84.0);
        p.card(card);
        for (i, (g, label, on, act)) in [('\u{E767}', t!("Bilgisayar sesi", "System audio"), cfg.audio, A::Audio), ('\u{E720}', t!("Mikrofon", "Microphone"), cfg.mic, A::Mic)].into_iter().enumerate() {
            let ry = y + 6.0 + i as f32 * 36.0;
            let row = R::new(card.x + 4.0, ry, card.w - 8.0, 36.0);
            if p.hover(row) {
                p.round(row, 6.0, 0xFFFFFF, 0.04);
            }
            p.glyph(g, R::new(card.x + 10.0, ry, 22.0, 36.0), 14.0, TEXT, 0.85);
            p.text(label, R::new(card.x + 40.0, ry, 200.0, 36.0), 13.0, false, TEXT, 0.9, Align::Left);
            let s0 = p.hits.len();
            p.switch(card.right() - 48.0, ry + 9.0, on, true, act.clone());
            // Satırın tamamı da tıklanır (fare kilitli oyunlarda isabet kolay olsun).
            p.hit(R::new(row.x, row.y, row.w - 56.0, row.h), act);
            p.hits.truncate(s0 + 2);
            let n0 = self.nav.len();
            self.mark(p, s0, 2 + i as u32);
            // Gezinmede satır tek öğe sayılır.
            self.nav.truncate(n0 + 1);
        }

        // Kayıt ayarları kartı.
        let y = 264.0;
        let card = R::new(x0, y, cw, 212.0);
        p.card(card);
        let lx = card.x + 12.0;
        let sx = card.x + 108.0;
        let sw = card.w - 120.0;
        let minutes = [1u32, 2, 5, 10, 20, 30];
        let mins: Vec<(String, A)> = minutes.iter().map(|&m| (m.to_string(), A::Minutes(m))).collect();
        let msel = minutes.iter().position(|&m| m == cfg.minutes()).unwrap_or(usize::MAX);
        let rows: [(&str, Vec<(String, A)>, usize); 5] = [
            (t!("Süre (dk)", "Length (min)"), mins, msel),
            (
                t!("Çözünürlük", "Resolution"),
                vec![(t!("Yerel", "Native").into(), A::Res(Resolution::Native)), ("1440p".into(), A::Res(Resolution::P1440)), ("1080p".into(), A::Res(Resolution::P1080)), ("720p".into(), A::Res(Resolution::P720))],
                [Resolution::Native, Resolution::P1440, Resolution::P1080, Resolution::P720].iter().position(|&r| r == cfg.resolution).unwrap_or(0),
            ),
            ("FPS", vec![("30".into(), A::Fps(30)), ("60".into(), A::Fps(60))], if cfg.fps() == 30 { 0 } else { 1 }),
            (
                t!("Kalite", "Quality"),
                vec![(t!("Düşük", "Low").into(), A::Quality(Quality::Low)), (t!("Orta", "Medium").into(), A::Quality(Quality::Medium)), (t!("Yüksek", "High").into(), A::Quality(Quality::High))],
                [Quality::Low, Quality::Medium, Quality::High].iter().position(|&q| q == cfg.quality).unwrap_or(1),
            ),
            (t!("Sıkıştırma", "Codec"), vec![("H.264".into(), A::Codec(Codec::H264)), ("H.265".into(), A::Codec(Codec::Hevc))], if cfg.codec == Codec::Hevc { 1 } else { 0 }),
        ];
        for (i, (label, items, sel)) in rows.iter().enumerate() {
            let ry = y + 10.0 + i as f32 * 40.0;
            Self::row_label(p, ry, lx, label);
            let items: Vec<(&str, A)> = items.iter().map(|(s, a)| (s.as_str(), a.clone())).collect();
            let s0 = p.hits.len();
            p.segments(R::new(sx, ry, sw, 32.0), &items, *sel);
            self.mark(p, s0, 4 + i as u32);
        }

        // Alt bilgi: kayıt boyutu, klasör, kısayollar.
        let y = 488.0;
        let info = if self.st.info.is_empty() || !rec { t!("Kayıt başlayınca boyut burada görünür", "Size shows once recording starts").to_string() } else { self.st.info.clone() };
        p.text(&info, R::new(x0, y, cw - 110.0, 32.0), 12.0, false, TEXT, 0.65, Align::Left);
        let s0 = p.hits.len();
        p.button(R::new(x0 + cw - 100.0, y, 100.0, 32.0), Some('\u{E8B7}'), t!("Klasör", "Folder"), A::Folder, true, false);
        self.mark(p, s0, 9);
        let keys = format!("{} {}  ·  {} {}", self.st.save_key, t!("kaydet", "save"), self.st.panel_key, t!("panel", "panel"));
        p.text(&keys, R::new(x0, y + 40.0, cw, 18.0), 11.5, false, mix(TEXT, p.accent, 0.3), 0.55, Align::Center);

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
