//! Çark dock (ayarlardan açılır, varsayılan kapalı).
//!
//! Kısayol (`shortcuts.radial`, varsayılan Mouse5) basılınca imlecin çevresinde
//! uygulama simgelerinden bir halka açılır. Tuşu basılı tutup bir simgeye doğru
//! götürüp bırakmak o uygulamaya geçer; hiçbir simgenin üstünde bırakılırsa
//! halka açık kalır ve tıklamayla seçilir. Dışarı tıklamak, sağ tık ya da Esc
//! kapatır. Yalnızca ana dock'ta vardır; tam ekran oyun/videoda açılmaz.
//!
//! Simgeler dock'takilerle aynı fırçaları paylaşır (ayrıca yüklenmez).
//! Fare her 16 ms'de bir yoklanır; pencere odak çalmaz.

use crate::dock::Dock;
use carlui_core::theme::Theme;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3, Glass},
    motion::Motion,
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    UI::Composition::{ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual},
    Win32::{
        Foundation::*,
        System::WinRT::Composition::ICompositorDesktopInterop,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// Dock penceresine gelen kısayol kimliği.
pub const HOTKEY_ID: i32 = 0x5241;
/// Açıkken fareyi yoklayan zamanlayıcı (dock penceresinde).
pub const TIMER: usize = 34;
const MAX_ITEMS: usize = 12;

struct Slot {
    entry: usize,
    visual: SpriteVisual,
    /// Pencereye göre simge merkezi.
    cx: f32,
    cy: f32,
    angle: f32,
}

pub struct Radial {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    motion: Motion,
    slots: Vec<Slot>,
    /// Pencerenin ekrandaki sol-üst köşesi ve boyu.
    pos: (i32, i32),
    size: f32,
    pub open: bool,
    /// Tuş bırakıldı ama seçim yapılmadı: tıklama bekleniyor.
    sticky: bool,
    hovered: Option<usize>,
    /// Kısayolun ana tuşu (basılı mı diye yoklanır); 0: kısayol yok.
    pub vk: u32,
    lbutton: bool,
    rbutton: bool,
    s: f32,
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Tıklamalar alttaki pencereye geçmesin; seçim yoklamayla yapılır.
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl Radial {
    pub fn new(gfx: &Gfx, theme: &Theme) -> Result<Self> {
        let hwnd = crate::create_window(w!("CarlUI.Radial"), Some(proc), w!("CarlUI Radial"))?;
        let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = gfx.comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        Ok(Self {
            hwnd,
            _target: target,
            root,
            motion: Motion::new(&gfx.comp, theme),
            slots: Vec::new(),
            pos: (0, 0),
            size: 0.0,
            open: false,
            sticky: false,
            hovered: None,
            vk: 0,
            lbutton: false,
            rbutton: false,
            s: 1.0,
        })
    }

    /// İmlecin çevresinde açar. Gösterilecek uygulama yoksa açmaz.
    /// `at` verilirse imleç yerine orada açılır (test).
    pub fn show(&mut self, gfx: &Gfx, dock: &Dock, dock_hwnd: HWND, at: Option<(i32, i32)>) -> Result<()> {
        let items = dock.radial_items(MAX_ITEMS);
        if items.is_empty() {
            return Ok(());
        }
        let comp = &gfx.comp;
        let s = dock.scale();
        self.s = s;
        let n = items.len() as f32;
        let icon = 46.0 * s;
        let ring = (n * (icon + 18.0 * s) / std::f32::consts::TAU).max(64.0 * s);
        let size = ((ring + icon / 2.0 + 18.0 * s) * 2.0).ceil();
        // Büyüyen simge ve gölge payı.
        let margin = (14.0 * s).ceil();
        let win = size + margin * 2.0;
        self.size = win;

        let (cx, cy) = at.unwrap_or_else(cursor);
        let mon = carlui_core::win::monitors()
            .into_iter()
            .find(|m| cx >= m.rect.left && cx < m.rect.right && cy >= m.rect.top && cy < m.rect.bottom)
            .or_else(carlui_core::win::primary_monitor)
            .map(|m| m.rect)
            .unwrap_or_default();
        let half = (win / 2.0) as i32;
        let x = (cx - half).clamp(mon.left, (mon.right - win as i32).max(mon.left));
        let y = (cy - half).clamp(mon.top, (mon.bottom - win as i32).max(mon.top));
        self.pos = (x, y);

        let kids = self.root.Children()?;
        kids.RemoveAll()?;
        self.slots.clear();
        let glass = Glass::new(comp, &dock.theme, size / 2.0)?;
        glass.root.SetSize(v2(size, size))?;
        glass.root.SetOffset(v3(margin, margin))?;
        kids.InsertAtTop(&glass.root)?;

        let mid = win / 2.0;
        for (k, &i) in items.iter().enumerate() {
            let Some(brush) = dock.icon_brush(i) else { continue };
            // Saat 12'den başlayıp saat yönünde.
            let angle = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::TAU / n;
            let (sx, sy) = (mid + ring * angle.cos(), mid + ring * angle.sin());
            let v = comp.CreateSpriteVisual()?;
            v.SetBrush(&brush)?;
            v.SetSize(v2(icon, icon))?;
            v.SetOffset(v3(sx - icon / 2.0, sy - icon / 2.0))?;
            v.SetCenterPoint(Vector3 { X: icon / 2.0, Y: icon / 2.0, Z: 0.0 })?;
            kids.InsertAtTop(&v)?;
            self.slots.push(Slot { entry: i, visual: v, cx: sx, cy: sy, angle });
        }

        // Açılış: merkezden büyüyerek ve belirerek.
        self.root.SetCenterPoint(Vector3 { X: mid, Y: mid, Z: 0.0 })?;
        self.root.SetScale(Vector3 { X: 0.7, Y: 0.7, Z: 1.0 })?;
        self.root.SetOpacity(0.0)?;
        self.motion.scale(&self.root, 1.0, 0.75, 45, 0)?;
        self.motion.fade(&self.root, &HSTRING::from("Opacity"), 1.0, 110, 0)?;

        unsafe {
            let w = win as i32;
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, w, w, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(dock_hwnd, TIMER, 16, None);
        }
        self.open = true;
        self.sticky = false;
        self.hovered = None;
        self.lbutton = pressed(VK_LBUTTON);
        self.rbutton = pressed(VK_RBUTTON);
        Ok(())
    }

    pub fn hide(&mut self, dock_hwnd: HWND) {
        if !self.open {
            return;
        }
        self.open = false;
        self.hovered = None;
        unsafe {
            let _ = KillTimer(dock_hwnd, TIMER);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        if let Ok(k) = self.root.Children() {
            let _ = k.RemoveAll();
        }
        self.slots.clear();
    }

    pub fn set_sticky(&mut self) {
        self.sticky = true;
    }

    /// Ekran koordinatındaki imlecin üstünde durduğu dilim.
    fn pick(&self, p: (i32, i32)) -> Option<usize> {
        let mid = self.size / 2.0;
        let dx = (p.0 - self.pos.0) as f32 - mid;
        let dy = (p.1 - self.pos.1) as f32 - mid;
        let d = (dx * dx + dy * dy).sqrt();
        // Ortadaki boşlukta seçim yok; halkanın biraz dışı da sayılır.
        if d < 26.0 * self.s || d > mid + 70.0 * self.s || self.slots.is_empty() {
            return None;
        }
        let a = dy.atan2(dx);
        let diff = |s: &Slot| {
            let mut x = (a - s.angle).rem_euclid(std::f32::consts::TAU);
            if x > std::f32::consts::PI {
                x = std::f32::consts::TAU - x;
            }
            x
        };
        (0..self.slots.len()).min_by(|&i, &j| diff(&self.slots[i]).total_cmp(&diff(&self.slots[j])))
    }

    /// Simgenin üstündeki nokta (etiket için, ekran koordinatı).
    pub fn label_anchor(&self, slot: usize) -> (i32, i32) {
        let s = &self.slots[slot];
        (self.pos.0 + s.cx as i32, self.pos.1 + (s.cy - 30.0 * self.s) as i32)
    }

    pub fn hovered(&self) -> Option<(usize, usize)> {
        self.hovered.map(|k| (k, self.slots[k].entry))
    }

    /// Zamanlayıcı adımı. Ne yapılacağını döndürür.
    pub fn tick(&mut self) -> Tick {
        if !self.open {
            return Tick::Idle;
        }
        let p = cursor();
        let h = self.pick(p);
        let mut changed = false;
        if h != self.hovered {
            if let Some(o) = self.hovered.and_then(|k| self.slots.get(k)) {
                let _ = self.motion.scale(&o.visual, 1.0, 0.7, 40, 0);
            }
            if let Some(o) = h.and_then(|k| self.slots.get(k)) {
                let _ = self.motion.scale(&o.visual, 1.3, 0.6, 40, 0);
            }
            self.hovered = h;
            changed = true;
        }
        let entry = h.map(|k| self.slots[k].entry);

        if pressed(VK_ESCAPE) {
            return Tick::Close;
        }
        let r = pressed(VK_RBUTTON);
        let r_click = r && !self.rbutton;
        self.rbutton = r;
        let l = pressed(VK_LBUTTON);
        let l_up = !l && self.lbutton;
        self.lbutton = l;

        if !self.sticky {
            if !carlui_core::hotkey::held(self.vk) {
                // Tuş bırakıldı.
                return match entry {
                    Some(i) => Tick::Activate(i),
                    None => {
                        self.sticky = true;
                        if changed { Tick::Hover } else { Tick::Idle }
                    }
                };
            }
        } else {
            if r_click {
                return Tick::Close;
            }
            if l_up {
                return match entry {
                    Some(i) => Tick::Activate(i),
                    None => Tick::Close,
                };
            }
        }
        if changed {
            Tick::Hover
        } else {
            Tick::Idle
        }
    }
}

pub enum Tick {
    Idle,
    Hover,
    Activate(usize),
    Close,
}
