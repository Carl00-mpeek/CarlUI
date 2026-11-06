//! Bu modül dock'un üstünde beliren iki küçük yüzeyi çözüyor:
//! - **Etiket:** Fare bir ikonun üstündeyken uygulamanın adını gösteren cam hap.
//! - **Önizleme:** Açık penceresi olan bir ikonda kısa bir beklemeden sonra
//!   her pencere için başlık + canlı küçük resim. Küçük resimler DWM
//!   thumbnail'larıdır: içerik Windows'un compositor'ünde canlı çizilir, bu
//!   süreç kare kopyalamaz. Karta tıklamak pencereyi öne getirir, × kapatır.
//!
//! Ayrı bir pencere kullanılmasının sebebi, dock penceresinin tıklanabilir
//! bölgesinin pill ile sınırlı olmasıdır.

use carlui_core::theme::Theme;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3, Glass},
    icons,
    motion::Motion,
    text::Text,
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::WinRT::Composition::ICompositorDesktopInterop,
        UI::WindowsAndMessaging::*,
    },
};

pub const TIMER_THUMBS: usize = 21;
pub const TIMER_HIDE: usize = 22;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Hidden,
    Label,
    Preview,
}

pub enum Action {
    Activate(HWND),
    Close(HWND),
}

struct Card {
    src: HWND,
    rect: RECT,
    close_rect: RECT,
    thumb: isize,
    thumb_rect: RECT,
    hl: ShapeVisual,
    close: SpriteVisual,
}

pub struct Popup {
    pub hwnd: HWND,
    comp: Compositor,
    motion: Motion,
    theme: Theme,
    s: f32,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    text: Text,
    pub mode: Mode,
    pub key: String,
    pub windows: Vec<HWND>,
    cards: Vec<Card>,
    hovered: Option<usize>,
    pub mouse_inside: bool,
    close_surface: CompositionDrawingSurface,
}

fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

impl Popup {
    pub fn new(gfx: &Gfx, hwnd: HWND, theme: &Theme, s: f32) -> Result<Self> {
        let comp = gfx.comp.clone();
        let target = unsafe { comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let cs = (18.0 * s).round();
        let close_surface = gfx.surface(cs, cs, |dc| icons::draw_close(dc, cs, &gfx.round_stroke))?;
        Ok(Self {
            hwnd,
            motion: Motion::new(&comp, theme),
            comp,
            theme: *theme,
            s,
            _target: target,
            root,
            text: Text::new()?,
            mode: Mode::Hidden,
            key: String::new(),
            windows: Vec::new(),
            cards: Vec::new(),
            hovered: None,
            mouse_inside: false,
            close_surface,
        })
    }

    fn clear(&mut self) {
        for c in self.cards.drain(..) {
            unsafe {
                let _ = DwmUnregisterThumbnail(c.thumb);
            }
        }
        if let Ok(k) = self.root.Children() {
            let _ = k.RemoveAll();
        }
        self.hovered = None;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_HIDE);
            let _ = KillTimer(self.hwnd, TIMER_THUMBS);
        }
    }

    /// Pencereyi içeriğin etrafına yerleştirir: `anchor` = ikonun üst-orta noktası.
    fn place(&self, w: f32, h: f32, margin: f32, anchor: (i32, i32)) -> (i32, i32, i32, i32) {
        let ww = (w + margin * 2.0).ceil() as i32;
        let wh = (h + margin * 2.0).ceil() as i32;
        let mut x = anchor.0 - ww / 2;
        let y = anchor.1 - (6.0 * self.s) as i32 - wh;
        let mon = carlui_core::win::primary_monitor().map(|m| m.rect).unwrap_or_default();
        x = x.clamp(mon.left + 4, (mon.right - ww - 4).max(mon.left));
        (x, y, ww, wh)
    }

    fn reveal(&self, glass: &Glass, w: f32, h: f32, margin: f32, animate: bool) -> Result<()> {
        glass.root.SetSize(v2(w, h))?;
        glass.root.SetOffset(v3(margin, margin))?;
        glass.root.SetCenterPoint(Vector3 { X: w / 2.0, Y: h, Z: 0.0 })?;
        if animate {
            self.root.SetOpacity(0.0)?;
            self.motion.fade(&self.root, h!("Opacity"), 1.0, 120, 0)?;
            glass.root.SetScale(Vector3 { X: 0.92, Y: 0.92, Z: 1.0 })?;
            self.motion.scale(&glass.root, 1.0, 0.8, 40, 0)?;
        } else {
            self.root.SetOpacity(1.0)?;
        }
        Ok(())
    }

    fn show_window(&self, rect: (i32, i32, i32, i32)) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, rect.0, rect.1, rect.2, rect.3, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    pub fn show_label(&mut self, gfx: &Gfx, key: &str, name: &str, anchor: (i32, i32)) -> Result<()> {
        let animate = self.mode == Mode::Hidden;
        self.clear();
        self.mode = Mode::Label;
        self.key = key.to_string();
        self.windows.clear();
        let s = self.s;
        let t = self.text.render(gfx, name, 13.0 * s, false, 320.0 * s, 0.95)?;
        let (padx, h, margin) = ((12.0 * s).round(), (28.0 * s).round(), (8.0 * s).round());
        let w = t.width + padx * 2.0;
        let glass = Glass::new(&self.comp, &self.theme, h / 2.0)?;
        let label = self.comp.CreateSpriteVisual()?;
        label.SetSize(v2(t.width, t.height))?;
        label.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(&t.surface)?)?;
        label.SetOffset(v3(padx, ((h - t.height) / 2.0).round()))?;
        glass.content.Children()?.InsertAtTop(&label)?;
        self.root.Children()?.InsertAtTop(&glass.root)?;
        self.reveal(&glass, w, h, margin, animate)?;
        self.show_window(self.place(w, h, margin, anchor));
        Ok(())
    }

    pub fn show_preview(&mut self, gfx: &Gfx, key: &str, items: &[(HWND, String)], anchor: (i32, i32)) -> Result<()> {
        let animate = self.mode != Mode::Preview || self.key != key;
        self.clear();
        self.mode = Mode::Preview;
        self.key = key.to_string();
        self.windows = items.iter().map(|(h, _)| *h).collect();
        let s = self.s;
        let n = items.len().clamp(1, 6);
        let (card_w, thumb_h, title_h) = ((220.0 * s).round(), (124.0 * s).round(), (22.0 * s).round());
        let (pad, gap, inner, margin) = ((8.0 * s).round(), (6.0 * s).round(), (8.0 * s).round(), (8.0 * s).round());
        let close_px = (18.0 * s).round();
        let w = n as f32 * card_w + (n - 1) as f32 * gap + pad * 2.0;
        let card_h = inner + title_h + 4.0 * s + thumb_h + inner;
        let h = card_h + pad * 2.0;

        let glass = Glass::new(&self.comp, &self.theme, (18.0 * s * self.theme.radius_scale).round())?;
        let content = glass.content.Children()?;
        let hl_brush = self.comp.CreateColorBrushWithColor(carlui_ui::glass::exact(self.theme.hover))?;
        let close_brush = self.comp.CreateSurfaceBrushWithSurface(&self.close_surface)?;

        for (i, (src, title)) in items.iter().take(n).enumerate() {
            let x0 = pad + i as f32 * (card_w + gap);
            let hl_geom = self.comp.CreateRoundedRectangleGeometry()?;
            hl_geom.SetSize(v2(card_w, card_h))?;
            let r = 12.0 * s * self.theme.radius_scale;
            hl_geom.SetCornerRadius(v2(r, r))?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&hl_geom)?;
            shape.SetFillBrush(&hl_brush)?;
            let hl = self.comp.CreateShapeVisual()?;
            hl.SetSize(v2(card_w, card_h))?;
            hl.SetOffset(v3(x0, pad))?;
            hl.Shapes()?.Append(&shape)?;
            hl.SetOpacity(0.0)?;
            content.InsertAtTop(&hl)?;

            let t = self.text.render(gfx, title, 12.5 * s, false, card_w - inner * 2.0 - close_px - 4.0 * s, 0.92)?;
            let tv = self.comp.CreateSpriteVisual()?;
            tv.SetSize(v2(t.width, t.height))?;
            tv.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(&t.surface)?)?;
            tv.SetOffset(v3(x0 + inner, pad + inner + ((title_h - t.height) / 2.0).round()))?;
            content.InsertAtTop(&tv)?;

            let cx = x0 + card_w - inner - close_px;
            let cy = pad + inner + ((title_h - close_px) / 2.0).round();
            let close = self.comp.CreateSpriteVisual()?;
            close.SetSize(v2(close_px, close_px))?;
            close.SetBrush(&close_brush)?;
            close.SetOffset(v3(cx, cy))?;
            close.SetOpacity(0.0)?;
            content.InsertAtTop(&close)?;

            // Canlı küçük resim: kaynak oranı korunarak kart alanına sığdırılır.
            let (thumb, thumb_rect) = unsafe {
                let thumb = DwmRegisterThumbnail(self.hwnd, *src).unwrap_or(0);
                let size = DwmQueryThumbnailSourceSize(thumb).unwrap_or(SIZE { cx: 16, cy: 9 });
                let (aw, ah) = (card_w - inner * 2.0, thumb_h);
                let k = (aw / size.cx.max(1) as f32).min(ah / size.cy.max(1) as f32);
                let (tw, th) = (size.cx as f32 * k, size.cy as f32 * k);
                let left = margin + x0 + inner + (aw - tw) / 2.0;
                let top = margin + pad + inner + title_h + 4.0 * s + (ah - th) / 2.0;
                (thumb, RECT { left: left as i32, top: top as i32, right: (left + tw) as i32, bottom: (top + th) as i32 })
            };
            let to_win = |l: f32, t: f32, r: f32, b: f32| RECT {
                left: (margin + l) as i32,
                top: (margin + t) as i32,
                right: (margin + r) as i32,
                bottom: (margin + b) as i32,
            };
            self.cards.push(Card {
                src: *src,
                rect: to_win(x0, pad, x0 + card_w, pad + card_h),
                close_rect: to_win(cx - 2.0, cy - 2.0, cx + close_px + 2.0, cy + close_px + 2.0),
                thumb,
                thumb_rect,
                hl,
                close,
            });
        }
        self.root.Children()?.InsertAtTop(&glass.root)?;
        self.reveal(&glass, w, h, margin, animate)?;
        self.show_window(self.place(w, h, margin, anchor));
        // Küçük resimler DWM'de çizilir, animasyona katılamaz; panel yerine oturunca belirsinler.
        unsafe {
            SetTimer(self.hwnd, TIMER_THUMBS, if animate { 140 } else { 1 }, None);
        }
        Ok(())
    }

    pub fn show_thumbs(&self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_THUMBS);
            for c in &self.cards {
                let props = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
                    rcDestination: c.thumb_rect,
                    opacity: 255,
                    fVisible: TRUE,
                    fSourceClientAreaOnly: FALSE,
                    ..Default::default()
                };
                let _ = DwmUpdateThumbnailProperties(c.thumb, &props);
            }
        }
    }

    pub fn hide(&mut self) {
        if self.mode == Mode::Hidden {
            return;
        }
        self.mode = Mode::Hidden;
        self.key.clear();
        self.windows.clear();
        for c in self.cards.drain(..) {
            unsafe {
                let _ = DwmUnregisterThumbnail(c.thumb);
            }
        }
        let _ = self.motion.fade(&self.root, h!("Opacity"), 0.0, 100, 0);
        unsafe {
            SetTimer(self.hwnd, TIMER_HIDE, 110, None);
        }
    }

    pub fn finish_hide(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_HIDE);
        }
        if self.mode == Mode::Hidden {
            self.clear();
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_inside = true;
        self.set_hover(self.cards.iter().position(|c| contains(&c.rect, x, y)));
    }

    fn set_hover(&mut self, idx: Option<usize>) {
        if idx == self.hovered {
            return;
        }
        if let Some(old) = self.hovered.and_then(|i| self.cards.get(i)) {
            let _ = self.motion.animate(&old.hl, h!("Opacity"), 0.0, 1.0, 35, 0);
            let _ = self.motion.fade(&old.close, h!("Opacity"), 0.0, 100, 0);
        }
        if let Some(new) = idx.and_then(|i| self.cards.get(i)) {
            let _ = self.motion.animate(&new.hl, h!("Opacity"), 1.0, 1.0, 35, 0);
            let _ = self.motion.fade(&new.close, h!("Opacity"), 1.0, 100, 0);
        }
        self.hovered = idx;
    }

    pub fn mouse_leave(&mut self) {
        self.mouse_inside = false;
        self.set_hover(None);
    }

    pub fn click(&self, x: i32, y: i32) -> Option<Action> {
        let c = self.cards.iter().find(|c| contains(&c.rect, x, y))?;
        Some(if contains(&c.close_rect, x, y) { Action::Close(c.src) } else { Action::Activate(c.src) })
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        self.clear();
    }
}
