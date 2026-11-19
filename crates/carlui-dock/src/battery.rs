//! Pil sekmesi: dock'un üst kenarının ortasında küçük bir çıkıntı. İmleç
//! üstüne gelince dock'un arkasından yukarı doğru bir cam panel kayar; şarjlı
//! fare ve kulaklığın simgeleri, üstlerinde pil yüzdeleriyle görünür.
//!
//! Okuma (`carlui_sys::battery`) yavaştır, ayrı bir iş parçacığında yapılır:
//! dakikada bir ve panel açılırken (veri eskiyse). Uyuyan kablosuz fare soruya
//! cevap vermez; o durumda son bilinen değer gösterilir.
//!
//! Ayrı pencere kullanılır; tıklanabilir bölgesi kapalıyken yalnızca çıkıntıdır.

use carlui_core::{info, theme::Theme};
use carlui_sys::battery::{Kind, Reading};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3, Glass},
    motion::Motion,
    text::Text,
};
use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateRectRgn, SetWindowRgn},
        System::WinRT::Composition::ICompositorDesktopInterop,
        UI::WindowsAndMessaging::*,
    },
};

pub const WM_APP_BATTERY: u32 = WM_APP + 40;
/// Dock'un yerini izler (kayma, yeniden kurulma, genişlik değişimi).
pub const TIMER_FOLLOW: usize = 41;
/// Panel aşağı kaydıktan sonra pencere bölgesi çıkıntıya döner.
pub const TIMER_FOLD: usize = 42;

const FOLLOW_MS: u32 = 200;
const POLL: Duration = Duration::from_secs(60);
/// Panel açılırken veri bundan eskiyse yeniden okunur.
const FRESH: Duration = Duration::from_secs(8);
/// Cevap vermeyen (uyuyan) farenin son değeri bu kadar süre gösterilir.
const KEEP: Duration = Duration::from_secs(3 * 3600);

const G_MOUSE: char = '\u{E962}';
const G_HEADSET: char = '\u{E7F6}';
const G_BT: char = '\u{E702}';
const G_BOLT: char = '\u{E945}';

struct Shared {
    readings: Vec<Reading>,
    at: Option<Instant>,
}

pub struct BatteryTab {
    pub hwnd: HWND,
    comp: Compositor,
    motion: Motion,
    s: f32,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    text: Text,
    nub: Glass,
    panel: Glass,
    /// Pencere boyu (panel + boşluk) ve panel ölçüleri.
    win: (i32, i32),
    panel_size: (f32, f32),
    nub_size: (f32, f32),
    /// Pencerenin ekrandaki sol-üst köşesi; `None` = gizli.
    pos: Option<(i32, i32)>,
    pub open: bool,
    pub mouse_inside: bool,
    shared: Arc<Mutex<Shared>>,
    ask: Sender<()>,
    /// Gösterilen durum (aynıysa yeniden çizilmez).
    shown: Vec<(Kind, Option<u8>, bool)>,
    /// Son bilinen değerler: uyuyan fare "bilinmiyor" görünmesin.
    last: HashMap<String, (u8, bool, Instant)>,
}

fn glyph_of(k: Kind) -> char {
    match k {
        Kind::Mouse => G_MOUSE,
        Kind::Headset => G_HEADSET,
        Kind::Other => G_BT,
    }
}

impl BatteryTab {
    pub fn new(gfx: &Gfx, hwnd: HWND, theme: &Theme, s: f32) -> Result<Self> {
        let comp = gfx.comp.clone();
        let target = unsafe { comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let nub_size = ((44.0 * s).round(), (9.0 * s).round());
        let nub = Glass::new(&comp, theme, nub_size.1 / 2.0)?;
        let panel = Glass::new(&comp, theme, (14.0 * s * theme.radius_scale).round())?;
        root.Children()?.InsertAtTop(&panel.root)?;
        root.Children()?.InsertAtTop(&nub.root)?;
        panel.root.SetOpacity(0.0)?;

        let shared = Arc::new(Mutex::new(Shared { readings: Vec::new(), at: None }));
        let (ask, rx) = mpsc::channel::<()>();
        let (sh, raw) = (shared.clone(), hwnd.0 as isize);
        std::thread::Builder::new()
            .name("battery".into())
            .spawn(move || loop {
                let r = carlui_sys::battery::read_all();
                if let Ok(mut s) = sh.lock() {
                    s.readings = r;
                    s.at = Some(Instant::now());
                }
                unsafe {
                    if PostMessageW(HWND(raw as _), WM_APP_BATTERY, WPARAM(0), LPARAM(0)).is_err() {
                        return; // pencere kapandı
                    }
                }
                match rx.recv_timeout(POLL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {
                        while rx.try_recv().is_ok() {}
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            })
            .ok();
        unsafe {
            SetTimer(hwnd, TIMER_FOLLOW, FOLLOW_MS, None);
        }
        Ok(Self {
            hwnd,
            motion: Motion::new(&comp, theme),
            comp,
            s,
            _target: target,
            root,
            text: Text::new()?,
            nub,
            panel,
            win: (0, 0),
            panel_size: (0.0, 0.0),
            nub_size,
            pos: None,
            open: false,
            mouse_inside: false,
            shared,
            ask,
            shown: Vec::new(),
            last: HashMap::new(),
        })
    }

    /// Okunan değerleri son bilinenlerle birleştirir.
    fn merged(&mut self) -> Vec<(Kind, Option<u8>, bool)> {
        let readings = self.shared.lock().map(|s| s.readings.clone()).unwrap_or_default();
        let now = Instant::now();
        readings
            .into_iter()
            .take(4)
            .map(|r| match r.percent {
                Some(p) => {
                    self.last.insert(r.name.clone(), (p, r.charging, now));
                    (r.kind, Some(p), r.charging)
                }
                // Kulaklık "kapalı" diyebiliyor; fare ise yalnızca susar (uyuyor).
                None if r.kind == Kind::Mouse => match self.last.get(&r.name).filter(|l| now - l.2 < KEEP) {
                    Some(&(p, c, _)) => (r.kind, Some(p), c),
                    None => (r.kind, None, false),
                },
                None => (r.kind, None, false),
            })
            .collect()
    }

    /// Yeni okuma geldi: panel içeriği yeniden kurulur.
    pub fn update(&mut self, gfx: &Gfx) {
        let items = self.merged();
        if items == self.shown && self.win.0 > 0 {
            return;
        }
        info!("pil: {:?}", items);
        self.shown = items;
        if let Err(e) = self.build(gfx) {
            carlui_core::warn!("pil paneli çizilemedi: {e}");
        }
        // Boyut değişti: yerini yeniden hesapla.
        self.pos = None;
    }

    fn build(&mut self, gfx: &Gfx) -> Result<()> {
        let s = self.s;
        let content = self.panel.content.Children()?;
        content.RemoveAll()?;
        let n = self.shown.len().max(1) as f32;
        let (col, pad) = ((58.0 * s).round(), (8.0 * s).round());
        let (txt_h, icon_px, gap) = ((18.0 * s).round(), (22.0 * s).round(), (3.0 * s).round());
        let w = col * n + pad * 2.0;
        let h = pad + txt_h + gap + icon_px + pad;
        for (i, &(kind, pct, charging)) in self.shown.iter().enumerate() {
            let x0 = pad + i as f32 * col;
            let (label, alpha) = match pct {
                Some(p) => (format!("%{p}"), if p <= 15 { 1.0 } else { 0.95 }),
                None => ("—".to_string(), 0.5),
            };
            let t = self.text.render(gfx, &label, 13.0 * s, true, col, alpha)?;
            let tv = self.comp.CreateSpriteVisual()?;
            tv.SetSize(v2(t.width, t.height))?;
            tv.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(&t.surface)?)?;
            tv.SetOffset(v3((x0 + (col - t.width) / 2.0).round(), pad + ((txt_h - t.height) / 2.0).round()))?;
            content.InsertAtTop(&tv)?;

            let g = self.text.glyph(gfx, glyph_of(kind), icon_px * 0.85, if pct.is_some() { 0.95 } else { 0.45 })?;
            let gv = self.comp.CreateSpriteVisual()?;
            gv.SetSize(v2(g.width, g.height))?;
            gv.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(&g.surface)?)?;
            let gy = pad + txt_h + gap + ((icon_px - g.height) / 2.0).round();
            gv.SetOffset(v3((x0 + (col - g.width) / 2.0).round(), gy))?;
            content.InsertAtTop(&gv)?;
            if charging {
                let b = self.text.glyph(gfx, G_BOLT, 10.0 * s, 0.95)?;
                let bv = self.comp.CreateSpriteVisual()?;
                bv.SetSize(v2(b.width, b.height))?;
                bv.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(&b.surface)?)?;
                bv.SetOffset(v3((x0 + (col + g.width) / 2.0).round(), gy))?;
                content.InsertAtTop(&bv)?;
            }
        }
        self.panel_size = (w, h);
        self.panel.root.SetSize(v2(w, h))?;
        // Pencere: panel + üstte biraz boşluk (yaylanma taşmasın).
        let ww = w.max(self.nub_size.0).ceil() as i32;
        let wh = (h + 6.0 * s).ceil() as i32;
        self.win = (ww, wh);
        self.nub.root.SetSize(v2(self.nub_size.0, self.nub_size.1))?;
        self.nub.root.SetOffset(v3(((ww as f32 - self.nub_size.0) / 2.0).round(), wh as f32 - self.nub_size.1))?;
        let px = ((ww as f32 - w) / 2.0).round();
        let y = if self.open { self.open_y() } else { wh as f32 };
        self.panel.root.SetOffset(v3(px, y))?;
        Ok(())
    }

    fn open_y(&self) -> f32 {
        (self.win.1 as f32 - self.panel_size.1 - 2.0 * self.s).round()
    }

    /// Dock'un hap dikdörtgenine (ekran koordinatı) yapışır; `None` ise gizlenir.
    pub fn follow(&mut self, pill: Option<RECT>) {
        let want = pill.filter(|_| !self.shown.is_empty() && self.win.0 > 0).map(|r| ((r.left + r.right) / 2 - self.win.0 / 2, r.top - self.win.1));
        if want == self.pos {
            return;
        }
        let was = self.pos;
        self.pos = want;
        unsafe {
            match want {
                Some((x, y)) => {
                    let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, self.win.0, self.win.1, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                    self.set_region();
                    if was.is_none() {
                        let _ = self.root.SetOpacity(0.0);
                        let _ = self.motion.fade(&self.root, h!("Opacity"), 1.0, 160, 0);
                    }
                }
                None => {
                    self.open = false;
                    self.mouse_inside = false;
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                    let _ = self.panel.root.SetOpacity(0.0);
                    let _ = self.panel.root.SetOffset(v3(((self.win.0 as f32 - self.panel_size.0) / 2.0).round(), self.win.1 as f32));
                    let _ = self.nub.root.SetOpacity(1.0);
                }
            }
        }
    }

    /// Kapalıyken yalnızca çıkıntı, açıkken tüm pencere tıklanır.
    fn set_region(&self) {
        let (w, h) = self.win;
        unsafe {
            let rgn = if self.open {
                CreateRectRgn(0, 0, w, h)
            } else {
                // Çıkıntının biraz genişi: ince şeride denk getirmek kolay olsun.
                let (nw, nh) = (self.nub_size.0 as i32 + 8, self.nub_size.1 as i32 + 4);
                CreateRectRgn((w - nw) / 2, h - nh, (w + nw) / 2, h)
            };
            SetWindowRgn(self.hwnd, rgn, true);
        }
    }

    pub fn enter(&mut self) {
        self.mouse_inside = true;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FOLD);
        }
        if self.open {
            return;
        }
        self.open = true;
        let stale = self.shared.lock().map(|s| s.at.is_none_or(|t| t.elapsed() > FRESH)).unwrap_or(true);
        if stale {
            let _ = self.ask.send(());
        }
        self.set_region();
        let _ = self.panel.root.SetOpacity(1.0);
        let _ = self.motion.animate(&self.panel.root, h!("Offset.Y"), self.open_y(), 0.75, 60, 0);
        let _ = self.motion.fade(&self.nub.root, h!("Opacity"), 0.0, 90, 0);
    }

    pub fn leave(&mut self) {
        self.mouse_inside = false;
        if !self.open {
            return;
        }
        self.open = false;
        let _ = self.motion.animate(&self.panel.root, h!("Offset.Y"), self.win.1 as f32, 1.0, 55, 0);
        let _ = self.motion.fade(&self.nub.root, h!("Opacity"), 1.0, 160, 120);
        unsafe {
            SetTimer(self.hwnd, TIMER_FOLD, 260, None);
        }
    }

    /// Panel aşağı indi: bölge çıkıntıya döner (imleç tekrar gelmediyse).
    pub fn fold(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FOLD);
        }
        if !self.open {
            let _ = self.panel.root.SetOpacity(0.0);
            self.set_region();
        }
    }
}

impl Drop for BatteryTab {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FOLLOW);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
