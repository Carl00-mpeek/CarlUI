//! Bu modül "pano resim önizlemesi küçük; büyük görmek istiyorum" isteğini
//! çözüyor. Panodaki küçük önizlemenin üstünde biraz bekleyince ya da göz
//! düğmesine basınca resim, önizlemedeki yerinden kopup programın dışına
//! taşarak büyür ve ekranın ortasına gelir.
//!
//! Bunun için ekranın çalışma alanını kaplayan şeffaf bir katman penceresi
//! vardır; tıklama bölgesi yalnızca resim kartıdır, geri kalanı alttaki
//! pencerelere geçer. Pencere odak almaz (pano açık kalır). Yakınlaştırma ve
//! kaydırma compositor'de spring ile yumuşak akar, yeniden çizim yoktur.

use crate::*;
use carlui_core::theme::Rgba;
use carlui_ui::{
    glass::{color, v2, v3},
    motion::Motion,
    text::Text,
};
use windows::{
    Foundation::Numerics::Vector3,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::System::WinRT::Composition::ICompositorDesktopInterop,
};

const TIMER_GONE: usize = 21;
const PAD: f32 = 10.0;
const FOOT: f32 = 34.0;
const SHADOW: f32 = 36.0;
const BG: Rgba = Rgba(30, 30, 32, 255);

struct Card {
    layer: ContainerVisual,
    frame: Vec<Visual>,
    inner: ContainerVisual,
    /// Önizlemedeki resmin yeri (pencere koordinatı).
    from: RECT,
    /// Kart (pencere koordinatı).
    to: (f32, f32, f32, f32),
    /// Resmin doğal boyutu, görünen pencere.
    cw: f32,
    ch: f32,
    vw: f32,
    vh: f32,
    fit: f32,
    zoom: f32,
    x: f32,
    y: f32,
    drag: Option<(i32, i32, f32, f32)>,
    close_btn: RECT,
}

pub struct Viewer {
    pub hwnd: HWND,
    comp: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    motion: Motion,
    text: Text,
    s: f32,
    card: Option<Card>,
    old: Vec<ContainerVisual>,
}

impl Viewer {
    pub fn new(gfx: &Gfx, owner: HWND, theme: &carlui_core::theme::Theme, s: f32) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(viewer_proc),
                hInstance: instance.into(),
                lpszClassName: w!("CarlUI.ClipView"),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                w!("CarlUI.ClipView"),
                w!("CarlUI Image"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                owner,
                None,
                instance,
                None,
            )?;
            let on = BOOL(1);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, 4);
            SetWindowRgn(hwnd, CreateRectRgn(0, 0, 0, 0), false);
            let comp = gfx.comp.clone();
            let target = comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)?;
            let root = comp.CreateContainerVisual()?;
            target.SetRoot(&root)?;
            Ok(Self { hwnd, motion: Motion::new(&comp, theme), comp, _target: target, root, text: Text::new()?, s, card: None, old: Vec::new() })
        }
    }

    pub fn is_open(&self) -> bool {
        self.card.is_some()
    }

    fn solid(&self, c: Rgba) -> Result<CompositionBrush> {
        self.comp.CreateColorBrushWithColor(color(c))?.cast()
    }

    fn rrect(&self, w: f32, h: f32, r: f32, c: Rgba) -> Result<ShapeVisual> {
        let g = self.comp.CreateRoundedRectangleGeometry()?;
        g.SetSize(v2(w, h))?;
        g.SetCornerRadius(v2(r, r))?;
        let shape = self.comp.CreateSpriteShapeWithGeometry(&g)?;
        shape.SetFillBrush(&self.solid(c)?)?;
        let v = self.comp.CreateShapeVisual()?;
        v.SetSize(v2(w, h))?;
        v.Shapes()?.Append(&shape)?;
        Ok(v)
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

    /// Resmi açar. `from`: önizlemedeki resmin ekran dikdörtgeni, `work`: ekranın
    /// çalışma alanı (pencere buraya yerleşir).
    pub fn open(&mut self, gfx: &Gfx, surf: &CompositionDrawingSurface, cw: f32, ch: f32, from: RECT, work: RECT) -> Result<()> {
        self.close(false);
        self.drop_old();
        let s = self.s;
        let (ww, wh) = ((work.right - work.left) as f32, (work.bottom - work.top) as f32);
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, work.left, work.top, ww as i32, wh as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW)?;
        }
        let from = RECT { left: from.left - work.left, top: from.top - work.top, right: from.right - work.left, bottom: from.bottom - work.top };
        let pad = (PAD * s).round();
        let foot = (FOOT * s).round();
        // Ekranın ~%80'ine sığdır; küçük resimler en fazla 2 kat büyür.
        let fit = ((ww * 0.8 - 2.0 * pad) / cw).min((wh * 0.8 - pad - foot) / ch).min(2.0);
        let (iw, ih) = ((cw * fit).round(), (ch * fit).round());
        let (vw, vh) = (iw.max(320.0 * s), ih.max(180.0 * s));
        let (tw, th) = (vw + 2.0 * pad, vh + pad + foot);
        let (tx, ty) = (((ww - tw) / 2.0).round(), ((wh - th) / 2.0).round());

        let layer = self.comp.CreateContainerVisual()?;
        layer.SetSize(v2(tw, th))?;
        let mut frame: Vec<Visual> = Vec::new();
        let r = (18.0 * s).round();
        for (k, a) in [(3.0, 0x26u8), (9.0, 0x1A), (17.0, 0x10), (27.0, 0x08)] {
            let k = k * s;
            let sh = carlui_ui::tone::literal(|| self.rrect(tw + 2.0 * k, th + 2.0 * k, r + k, Rgba(0, 0, 0, a)))?;
            sh.SetOffset(v3(-k, -k + 8.0 * s))?;
            layer.Children()?.InsertAtTop(&sh)?;
            frame.push(sh.cast()?);
        }
        let bg = self.rrect(tw, th, r, BG)?;
        layer.Children()?.InsertAtTop(&bg)?;
        frame.push(bg.cast()?);
        let vp = self.comp.CreateContainerVisual()?;
        vp.SetSize(v2(vw, vh))?;
        vp.SetOffset(v3(pad, pad))?;
        let g = self.comp.CreateRoundedRectangleGeometry()?;
        g.SetSize(v2(vw, vh))?;
        g.SetCornerRadius(v2(12.0 * s, 12.0 * s))?;
        vp.SetClip(&self.comp.CreateGeometricClipWithGeometry(&g)?)?;
        let inner = self.comp.CreateContainerVisual()?;
        inner.SetSize(v2(cw, ch))?;
        inner.Children()?.InsertAtTop(&self.sprite(surf, cw, ch, 0.0, 0.0)?)?;
        vp.Children()?.InsertAtTop(&inner)?;
        layer.Children()?.InsertAtTop(&vp)?;
        // Alt şerit: ipucu solda, X sağda.
        let t = self.text.render(gfx, t!("Tekerlek ya da Ctrl +/- yakınlaştır · sürükle · Esc kapat", "Wheel or Ctrl +/- to zoom · drag · Esc to close"), 11.5 * s, false, tw - 80.0 * s, 0.55)?;
        let hint = self.sprite(&t.surface, t.width, t.height, pad + 6.0 * s, th - foot + (foot - t.height) / 2.0)?;
        layer.Children()?.InsertAtTop(&hint)?;
        frame.push(hint.cast()?);
        let bs = (26.0 * s).round();
        let (bx, by) = (tw - pad - bs, th - foot + (foot - bs) / 2.0);
        let xb = self.rrect(bs, bs, bs / 2.0, Rgba(255, 255, 255, 0x1C))?;
        xb.SetOffset(v3(bx.round(), by.round()))?;
        layer.Children()?.InsertAtTop(&xb)?;
        frame.push(xb.cast()?);
        let gl = self.text.glyph(gfx, '\u{E711}', (10.0 * s).round(), 0.9)?;
        let xg = self.sprite(&gl.surface, gl.width, gl.height, bx + (bs - gl.width) / 2.0, by + (bs - gl.height) / 2.0)?;
        layer.Children()?.InsertAtTop(&xg)?;
        frame.push(xg.cast()?);
        let close_btn = RECT { left: (tx + bx) as i32, top: (ty + by) as i32, right: (tx + bx + bs) as i32, bottom: (ty + by + bs) as i32 };

        // Önizlemedeki resim tam üst üste gelecek şekilde başla; çerçeve sonradan belirir.
        let k = (from.right - from.left) as f32 / iw;
        let (ix, iy) = (pad + (vw - iw) / 2.0, pad + (vh - ih) / 2.0);
        layer.SetOffset(v3(from.left as f32 - ix * k, from.top as f32 - iy * k))?;
        layer.SetScale(Vector3 { X: k, Y: k, Z: 1.0 })?;
        for f in &frame {
            f.SetOpacity(0.0)?;
            self.motion.fade(f, h!("Opacity"), 1.0, 170, 40)?;
        }
        let mo = self.motion.clone();
        mo.animate(&layer, h!("Offset.X"), tx, 0.8, 42, 0)?;
        mo.animate(&layer, h!("Offset.Y"), ty, 0.8, 42, 0)?;
        mo.scale(&layer, 1.0, 0.8, 42, 0)?;
        self.root.Children()?.InsertAtTop(&layer)?;
        let mut card = Card { layer, frame, inner, from, to: (tx, ty, tw, th), cw, ch, vw, vh, fit, zoom: fit, x: 0.0, y: 0.0, drag: None, close_btn };
        Self::apply(&self.motion, &mut card, false);
        self.card = Some(card);
        // Animasyon boyunca başlangıç + hedef; sonra yalnızca kart.
        let (fl, ft, fr, fb) = (from.left, from.top, from.right, from.bottom);
        let m = (SHADOW * s) as i32;
        self.set_region(Some(RECT {
            left: fl.min(tx as i32 - m),
            top: ft.min(ty as i32 - m),
            right: fr.max((tx + tw) as i32 + m),
            bottom: fb.max((ty + th) as i32 + m),
        }));
        unsafe {
            SetTimer(self.hwnd, TIMER_GONE, 450, None);
        }
        Ok(())
    }

    /// Kartı kapatır; `animate` ise önizlemedeki yerine küçülerek döner.
    pub fn close(&mut self, animate: bool) -> bool {
        let Some(card) = self.card.take() else { return false };
        if animate {
            let mo = self.motion.clone();
            let (_, _, tw, _) = card.to;
            let _ = tw;
            // Resim önizlemedeki boyutuna dönsün: yakınlaştırmayı da sıfırla.
            let iw = card.cw * card.fit;
            let k = (card.from.right - card.from.left) as f32 / iw;
            let pad = (PAD * self.s).round();
            let (ix, iy) = (pad + (card.vw - iw) / 2.0, pad + (card.vh - card.ch * card.fit) / 2.0);
            let mut c = card;
            c.zoom = c.fit;
            c.x = 0.0;
            c.y = 0.0;
            Self::apply(&mo, &mut c, true);
            let _ = mo.animate(&c.layer, h!("Offset.X"), c.from.left as f32 - ix * k, 1.0, 34, 0);
            let _ = mo.animate(&c.layer, h!("Offset.Y"), c.from.top as f32 - iy * k, 1.0, 34, 0);
            let _ = mo.scale(&c.layer, k, 1.0, 34, 0);
            for f in &c.frame {
                let _ = mo.fade(f, h!("Opacity"), 0.0, 110, 0);
            }
            let _ = mo.fade(&c.layer, h!("Opacity"), 0.0, 90, 150);
            self.old.push(c.layer);
            unsafe {
                SetTimer(self.hwnd, TIMER_GONE, 280, None);
            }
        } else {
            if let Ok(k) = self.root.Children() {
                let _ = k.Remove(&card.layer);
            }
            self.hide();
        }
        true
    }

    /// Önizlemesi artık olmayan resmi (panel kapandı) yerinde soldurarak kapatır.
    pub fn vanish(&mut self) {
        let Some(card) = self.card.take() else { return };
        let _ = self.motion.fade(&card.layer, h!("Opacity"), 0.0, 130, 0);
        self.old.push(card.layer);
        unsafe {
            SetTimer(self.hwnd, TIMER_GONE, 160, None);
        }
    }

    fn hide(&mut self) {
        self.drop_old();
        self.set_region(None);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn drop_old(&mut self) {
        if let Ok(k) = self.root.Children() {
            for o in self.old.drain(..) {
                let _ = k.Remove(&o);
            }
        }
    }

    /// Animasyon bitti: kapanıyorsa gizle, açıksa tıklama bölgesini karta daralt.
    fn timer_done(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_GONE);
        }
        match self.card.as_ref().map(|c| c.to) {
            None => self.hide(),
            Some((x, y, w, h)) => {
                self.drop_old();
                let m = SHADOW * self.s;
                self.set_region(Some(RECT { left: (x - m) as i32, top: (y - m) as i32, right: (x + w + m) as i32, bottom: (y + h + m) as i32 }));
            }
        }
    }

    fn set_region(&self, r: Option<RECT>) {
        unsafe {
            let rgn = match r {
                None => CreateRectRgn(0, 0, 0, 0),
                Some(r) => CreateRectRgn(r.left, r.top, r.right, r.bottom),
            };
            SetWindowRgn(self.hwnd, rgn, true);
        }
    }

    /// Yakınlaştırma/kaydırmayı resme uygular (`animate`: yumuşak spring).
    fn apply(mo: &Motion, c: &mut Card, animate: bool) {
        let (sw, sh) = (c.cw * c.zoom, c.ch * c.zoom);
        c.x = c.x.clamp(0.0, (sw - c.vw).max(0.0));
        c.y = c.y.clamp(0.0, (sh - c.vh).max(0.0));
        let tx = if sw < c.vw { ((c.vw - sw) / 2.0).round() } else { -c.x };
        let ty = if sh < c.vh { ((c.vh - sh) / 2.0).round() } else { -c.y };
        if animate {
            let _ = mo.animate(&c.inner, h!("Offset.X"), tx, 1.0, 40, 0);
            let _ = mo.animate(&c.inner, h!("Offset.Y"), ty, 1.0, 40, 0);
            let _ = mo.scale(&c.inner, c.zoom, 1.0, 40, 0);
        } else {
            let _ = c.inner.StopAnimation(h!("Offset.X"));
            let _ = c.inner.StopAnimation(h!("Offset.Y"));
            let _ = c.inner.SetOffset(v3(tx, ty));
            let _ = c.inner.SetScale(Vector3 { X: c.zoom, Y: c.zoom, Z: 1.0 });
        }
    }

    /// Yakınlaştır (`factor` 0: sığdır). `at`: ekran noktası (sabit kalır), yoksa orta.
    pub fn zoom(&mut self, factor: f32, at: Option<POINT>) {
        let pad = (PAD * self.s).round();
        let org = self.origin();
        let Some(c) = self.card.as_mut() else { return };
        let (ax, ay) = match at {
            Some(p) => ((p.x - org.0) as f32 - c.to.0 - pad, (p.y - org.1) as f32 - c.to.1 - pad),
            None => (c.vw / 2.0, c.vh / 2.0),
        };
        let (ax, ay) = (ax.clamp(0.0, c.vw), ay.clamp(0.0, c.vh));
        let old = c.zoom;
        let new = if factor == 0.0 { c.fit } else { (old * factor).clamp(c.fit.min(1.0) * 0.5, 8.0) };
        let (sw, sh) = (c.cw * old, c.ch * old);
        let cx = if sw < c.vw { (ax - (c.vw - sw) / 2.0) / old } else { (c.x + ax) / old };
        let cy = if sh < c.vh { (ay - (c.vh - sh) / 2.0) / old } else { (c.y + ay) / old };
        c.zoom = new;
        c.x = cx * new - ax;
        c.y = cy * new - ay;
        Self::apply(&self.motion, c, true);
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) {
        let Some(c) = self.card.as_mut() else { return };
        c.x += dx;
        c.y += dy;
        Self::apply(&self.motion, c, true);
    }

    fn origin(&self) -> (i32, i32) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        (r.left, r.top)
    }

    pub fn debug(&self) -> serde_json::Value {
        match &self.card {
            None => serde_json::Value::Null,
            Some(c) => serde_json::json!({ "to": [c.to.0, c.to.1, c.to.2, c.to.3], "zoom": c.zoom, "fit": c.fit, "x": c.x, "y": c.y, "from": [c.from.left, c.from.top, c.from.right, c.from.bottom] }),
        }
    }

    fn on_down(&mut self, x: i32, y: i32) {
        if let Some(c) = self.card.as_mut() {
            c.drag = Some((x, y, c.x, c.y));
        }
    }

    fn on_move(&mut self, x: i32, y: i32) {
        let Some(c) = self.card.as_mut() else { return };
        let Some((x0, y0, px, py)) = c.drag else { return };
        c.x = px - (x - x0) as f32;
        c.y = py - (y - y0) as f32;
        Self::apply(&self.motion, c, false);
    }

    /// Bırakıldı; X'e (kımıldamadan) tıklandıysa true.
    fn on_up(&mut self, x: i32, y: i32) -> bool {
        let Some(c) = self.card.as_mut() else { return false };
        let moved = c.drag.take().is_some_and(|(x0, y0, ..)| (x - x0).abs() > 4 || (y - y0).abs() > 4);
        let r = c.close_btn;
        let inside_card = {
            let (tx, ty, tw, th) = c.to;
            (x as f32) >= tx && (y as f32) >= ty && (x as f32) < tx + tw && (y as f32) < ty + th
        };
        !moved && ((x >= r.left && x < r.right && y >= r.top && y < r.bottom) || !inside_card)
    }
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

extern "system" fn viewer_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_TIMER if wp.0 == TIMER_GONE => {
                with_app(|a| a.viewer.timer_done());
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                SetCapture(hwnd);
                with_app(|a| a.viewer.on_down(x, y));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy(lp);
                with_app(|a| a.viewer.on_move(x, y));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                let _ = ReleaseCapture();
                with_app(|a| {
                    if a.viewer.on_up(x, y) {
                        a.viewer.close(true);
                    }
                });
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_app(|a| a.viewer.close(true));
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                let p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
                with_app(|a| a.viewer.zoom(if delta > 0.0 { 1.2 } else { 1.0 / 1.2 }, Some(p)));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
