//! Bu modül tray panelini ve onun 5 adımlı açılışını çözüyor:
//! 1. Dock'taki yukarı ok tıklanır (dock basma animasyonunu oynatır).
//! 2. Düğmenin yerinde vurgu renginde küçük, yuvarlak bir cam balon belirir.
//! 3. Balon yukarı yükselip spring ile panel boyutuna genişler; köşeleri
//!    daireden panel yuvarlaklığına iner.
//! 4. Hızlı ayarlar ve ikonlar sırayla, hafif gecikmeli belirir.
//! 5. Panel bağımsız, yuvarlak bir cam yüzey olarak durur.
//! Kapanış aynı adımları tersine oynatır ve panel düğmeye geri iner.
//!
//! Balon tek bir cam yüzeydir: alt-ortadan sabitlenmiştir (anchor), boyutu ve
//! konumu compositor'de spring ile değişir. İçerik son panel konumuna bağlı
//! durduğu için balon büyüdükçe yerinden kaymadan açığa çıkar.
//!
//! Dosyalar: `content` (hızlı ayarlar, kaydırıcılar, ikon ızgarası), `mixer`
//! (ses bölümü: çıkış aygıtı ve uygulama sesleri), `anim` (açılış/kapanış ve
//! tıklanabilir bölge), `mouse` (hover, tıklama, sürükleme).

mod anim;
mod content;
mod mixer;
mod mouse;

use crate::{
    audio,
    host::Host,
    quick::{self, Radios, Volume},
};
use carlui_core::theme::Theme;
use carlui_ui::{
    gfx::Gfx,
    glass::{color, v2, v3, Glass},
    icons,
    motion::Motion,
    text::Text,
};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicI32, AtomicIsize, Ordering},
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::*,
        },
    },
};

pub const WM_APP_RADIOS: u32 = WM_APP + 30;
pub const WM_APP_BRIGHT: u32 = WM_APP + 31;
pub const WM_APP_OUTSIDE: u32 = WM_APP + 32;
pub const TIMER_HIDE: usize = 41;
pub const TIMER_BRIGHT: usize = 42;
pub const TIMER_REGION: usize = 43;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Closed,
    Open,
    Closing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    None,
    Cell(usize),
    Wifi,
    Bluetooth,
    VolumeIcon,
    Volume,
    Bright,
    Button,
    /// Ses kaydırıcısının yanındaki ses bölümü düğmesi.
    AudioBtn,
    Device(usize),
    AppIcon(usize),
    AppVol(usize),
}

struct Slider {
    /// Düğmenin satır içindeki dikey merkezi.
    cy: f32,
    hit: RECT,
    x0: f32,
    w: f32,
    fill: ShapeVisual,
    fill_geom: CompositionRoundedRectangleGeometry,
    knob: ShapeVisual,
    knob_r: f32,
    value: f32,
}

struct Tile {
    rect: RECT,
    on_layer: ShapeVisual,
    /// Yalnızca Wi-Fi kutucuğunda: [Wi-Fi yüzü, Ethernet yüzü] (simge + ad).
    faces: Option<[ContainerVisual; 2]>,
}

struct Cell {
    rect: RECT,
    icon: usize,
    hl: ShapeVisual,
}

// Dış tıklama kancası için panel ve düğmenin ekran dikdörtgenleri.
static OUT_L: AtomicI32 = AtomicI32::new(0);
static OUT_T: AtomicI32 = AtomicI32::new(0);
static OUT_R: AtomicI32 = AtomicI32::new(0);
static OUT_B: AtomicI32 = AtomicI32::new(0);
static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn outside_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wp.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
        let pt = (*(lp.0 as *const MSLLHOOKSTRUCT)).pt;
        let inside = pt.x >= OUT_L.load(Ordering::Relaxed)
            && pt.x < OUT_R.load(Ordering::Relaxed)
            && pt.y >= OUT_T.load(Ordering::Relaxed)
            && pt.y < OUT_B.load(Ordering::Relaxed);
        if !inside {
            let _ = PostMessageW(HWND(PANEL_HWND.load(Ordering::Relaxed) as _), WM_APP_OUTSIDE, WPARAM(0), LPARAM(0));
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

pub fn query_brightness_at_start(panel: HWND) {
    quick::query_brightness(panel.0 as isize, WM_APP_BRIGHT);
}

fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> RECT {
    RECT { left: x as i32, top: y as i32, right: (x + w) as i32, bottom: (y + h) as i32 }
}

/// Açık kutucuğun vurgu katmanı: açık temada yazı mürekkep kalır, zemin renklenir.
fn tile_on() -> f32 {
    if carlui_ui::tone::light() { 0.24 } else { 1.0 }
}

pub struct Panel {
    pub hwnd: HWND,
    comp: Compositor,
    motion: Motion,
    theme: Theme,
    s: f32,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    text: Text,
    pub state: State,
    glass: Option<Glass>,
    content: Option<ContainerVisual>,
    accent: Option<SpriteVisual>,
    chevron: Option<SpriteVisual>,
    /// Pencere içi koordinatlarda panel ve düğme.
    panel: RECT,
    button: RECT,
    bubble: f32,
    cells: Vec<Cell>,
    hovered: Option<usize>,
    pressed: Hit,
    wifi: Option<Tile>,
    bt: Option<Tile>,
    vol: Option<Slider>,
    bright: Option<Slider>,
    radios: Radios,
    volume: Option<Volume>,
    brightness: Option<f32>,
    dragging: Hit,
    icon_cache: HashMap<(isize, u32), CompositionDrawingSurface>,
    hook: HHOOK,
    width: f32,
    stagger: Vec<Visual>,
    last_open: Option<(RECT, i32)>,
    win_size: (i32, i32),
    animating: bool,
    refresh_due: bool,
    /// Ses bölümü (çıkış aygıtları + uygulama sesleri) açık mı.
    audio_open: bool,
    audio_btn: Option<RECT>,
    devices: Vec<audio::Device>,
    device_rows: Vec<RECT>,
    apps: Vec<audio::App>,
    app_sliders: Vec<Slider>,
    app_icons: Vec<(RECT, Option<SpriteVisual>)>,
    /// Panel içeriği yeniden kurulmalı (ana döngü `refresh` çağırır).
    pub rebuild: bool,
    height: f32,
    /// Yeniden kurulumda cam bu yükseklikten yeni yüksekliğe uzasın.
    grow_from: Option<f32>,
    /// Dock hangi kenarda: 0 alt, 1 sol, 2 sağ (panel dock'un iç yanında açılır).
    pub side: u8,
}

impl Panel {
    pub fn new(gfx: &Gfx, hwnd: HWND, theme: &Theme, s: f32) -> Result<Self> {
        let comp = gfx.comp.clone();
        let target = unsafe { comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        PANEL_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        Ok(Self {
            hwnd,
            motion: Motion::new(&comp, theme),
            comp,
            theme: *theme,
            s,
            _target: target,
            root,
            text: Text::new()?,
            state: State::Closed,
            glass: None,
            content: None,
            accent: None,
            chevron: None,
            panel: RECT::default(),
            button: RECT::default(),
            bubble: 0.0,
            cells: Vec::new(),
            hovered: None,
            pressed: Hit::None,
            wifi: None,
            bt: None,
            vol: None,
            bright: None,
            radios: Radios::default(),
            volume: Volume::new(),
            brightness: None,
            dragging: Hit::None,
            icon_cache: HashMap::new(),
            hook: HHOOK::default(),
            width: (320.0 * s).round(),
            stagger: Vec::new(),
            last_open: None,
            win_size: (0, 0),
            animating: false,
            refresh_due: false,
            audio_open: false,
            audio_btn: None,
            devices: Vec::new(),
            device_rows: Vec::new(),
            apps: Vec::new(),
            app_sliders: Vec::new(),
            app_icons: Vec::new(),
            rebuild: false,
            height: 0.0,
            grow_from: None,
            side: 0,
        })
    }

    fn rrect(&self, w: f32, h: f32, r: f32, brush: &CompositionBrush) -> Result<(ShapeVisual, CompositionRoundedRectangleGeometry)> {
        let g = self.comp.CreateRoundedRectangleGeometry()?;
        g.SetSize(v2(w, h))?;
        g.SetCornerRadius(v2(r, r))?;
        let shape = self.comp.CreateSpriteShapeWithGeometry(&g)?;
        shape.SetFillBrush(brush)?;
        let v = self.comp.CreateShapeVisual()?;
        v.SetSize(v2(w, h))?;
        v.Shapes()?.Append(&shape)?;
        Ok((v, g))
    }

    fn accent_brush(&self, horizontal: bool) -> Result<CompositionBrush> {
        let b = self.comp.CreateLinearGradientBrush()?;
        b.SetStartPoint(v2(0.0, 0.0))?;
        b.SetEndPoint(if horizontal { v2(1.0, 0.0) } else { v2(1.0, 1.0) })?;
        let stops = b.ColorStops()?;
        stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(0.0, carlui_ui::glass::exact(self.theme.accent.0))?)?;
        stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(1.0, carlui_ui::glass::exact(self.theme.accent.1))?)?;
        b.cast()
    }

    fn sprite(&self, surface: &CompositionDrawingSurface, w: f32, h: f32, x: f32, y: f32) -> Result<SpriteVisual> {
        let v = self.comp.CreateSpriteVisual()?;
        v.SetSize(v2(w, h))?;
        let b = self.comp.CreateSurfaceBrushWithSurface(surface)?;
        b.SetStretch(CompositionStretch::Fill)?;
        v.SetBrush(&b)?;
        v.SetOffset(v3(x.round(), y.round()))?;
        Ok(v)
    }

    fn icon_surface(&mut self, gfx: &Gfx, host: &mut Host, i: usize, px: f32) -> Option<CompositionDrawingSurface> {
        let icon = &mut host.icons[i];
        let key = (icon.hwnd.0 as isize, icon.uid);
        if icon.icon_dirty || !self.icon_cache.contains_key(&key) {
            icon.icon_dirty = false;
            let src = icons::hicon_bitmap(&gfx.wic, icon.hicon).ok()?;
            let surf = gfx.surface(px, px, |dc| icons::draw_bitmap(dc, &src, px)).ok()?;
            self.icon_cache.insert(key, surf);
        }
        self.icon_cache.get(&key).cloned()
    }

    pub fn set_radios(&mut self, r: Radios) {
        self.radios = r;
        // Kablolu bağlıyken ve Wi-Fi açık değilken kutucuk "Ethernet" olur ve yanar.
        let eth = r.ethernet_shown();
        let wifi_lit = if eth { Some(true) } else { r.wifi };
        for (tile, on) in [(&self.wifi, wifi_lit), (&self.bt, r.bluetooth)] {
            if let Some(t) = tile {
                let _ = self.motion.fade(&t.on_layer, h!("Opacity"), if on == Some(true) { tile_on() } else { 0.0 }, 160, 0);
            }
        }
        if let Some([wifi_face, eth_face]) = self.wifi.as_ref().and_then(|t| t.faces.as_ref()) {
            let _ = wifi_face.SetOpacity(if eth { 0.0 } else { 1.0 });
            let _ = eth_face.SetOpacity(if eth { 1.0 } else { 0.0 });
        }
    }

    pub fn has_brightness(&self) -> bool {
        self.brightness.is_some()
    }

    pub fn set_brightness_known(&mut self, v: Option<f32>) {
        self.brightness = v;
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.unhook();
    }
}
