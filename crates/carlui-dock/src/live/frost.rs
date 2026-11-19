//! Kar yağarken fare ve klavye bir süre kullanılmazsa ekranın kenarlarında
//! yavaşça beliren hafif buzlanma; ilk dokunuşta erir. Tıklamayı geçiren,
//! en üstteki saydam pencerede tek bir bitmap'tir.

use super::{rng::Rng, window::overlay_window};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use std::{
    sync::Once,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Foundation::TimeSpan,
    Win32::{
        Foundation::*,
        Graphics::Direct2D::{Common::*, *},
        System::{SystemInformation::GetTickCount, WinRT::Composition::ICompositorDesktopInterop},
        UI::{
            Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
            WindowsAndMessaging::*,
        },
    },
    UI::Composition::{Desktop::DesktopWindowTarget, *},
};

/// Bu kadar boşta kalınca buz gelmeye başlar.
const IDLE: Duration = Duration::from_secs(20);
const CHECK: Duration = Duration::from_millis(400);
const IN_MS: f32 = 9000.0;
const OUT_MS: f32 = 450.0;

fn ts(ms: f32) -> TimeSpan {
    TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

fn col(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

fn idle_ms() -> u32 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if GetLastInputInfo(&mut li).as_bool() {
            GetTickCount().wrapping_sub(li.dwTime)
        } else {
            0
        }
    }
}

unsafe fn grad(dc: &ID2D1DeviceContext, a: D2D_POINT_2F, b: D2D_POINT_2F, c0: D2D1_COLOR_F, c1: D2D1_COLOR_F) -> Result<ID2D1LinearGradientBrush> {
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: c0 }, D2D1_GRADIENT_STOP { position: 1.0, color: c1 }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: a, endPoint: b }, None, &coll)
}

/// Eğrelti otu gibi dallanan buz çizgisi.
unsafe fn fern(dc: &ID2D1DeviceContext, b: &ID2D1SolidColorBrush, x: f32, y: f32, ang: f32, len: f32, w: f32, depth: u32, rng: &mut Rng) {
    let (mut x, mut y, mut ang) = (x, y, ang);
    let steps = 5;
    let seg = len / steps as f32;
    for i in 0..steps {
        ang += rng.range(-0.25, 0.25);
        let (nx, ny) = (x + ang.cos() * seg, y + ang.sin() * seg);
        dc.DrawLine(pt(x, y), pt(nx, ny), b, w, None);
        if depth > 0 && i % 2 == 0 {
            for side in [-1.0f32, 1.0] {
                fern(dc, b, nx, ny, ang + side * rng.range(0.5, 0.9), len * 0.38, w * 0.75, depth - 1, rng);
            }
        }
        x = nx;
        y = ny;
    }
}

/// `w`×`h` buz bitmap'i: kenarlarda beyazımsı bulanıklık, köşelerde yoğun dallar.
unsafe fn draw(dc: &ID2D1DeviceContext, w: f32, h: f32, rng: &mut Rng) -> Result<()> {
    let t = 0.12 * h;
    let edge = col(228, 238, 252, 0.42);
    let clear = col(228, 238, 252, 0.0);
    let rect = |l: f32, tp: f32, r: f32, b: f32| D2D_RECT_F { left: l, top: tp, right: r, bottom: b };
    dc.FillRectangle(&rect(0.0, 0.0, w, t), &grad(dc, pt(0.0, 0.0), pt(0.0, t), edge, clear)?);
    dc.FillRectangle(&rect(0.0, h - t, w, h), &grad(dc, pt(0.0, h), pt(0.0, h - t), edge, clear)?);
    dc.FillRectangle(&rect(0.0, 0.0, t, h), &grad(dc, pt(0.0, 0.0), pt(t, 0.0), edge, clear)?);
    dc.FillRectangle(&rect(w - t, 0.0, w, h), &grad(dc, pt(w, 0.0), pt(w - t, 0.0), edge, clear)?);
    // Köşeler: yumuşak beyaz lekeler.
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(240, 246, 255, 0.5) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(240, 246, 255, 0.0) }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let r = 0.32 * h;
    for (cx, cy) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: pt(cx, cy), gradientOriginOffset: pt(0.0, 0.0), radiusX: r, radiusY: r };
        let b = dc.CreateRadialGradientBrush(&props, None, &coll)?;
        dc.FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r }, &b);
    }
    // Buz dalları: kenarlardan içeri, köşelerde daha sık.
    let line = dc.CreateSolidColorBrush(&col(255, 255, 255, 0.33), None)?;
    let n = 110;
    for _ in 0..n {
        let side = (rng.f() * 4.0) as u32;
        // Köşeye yakın başlangıç daha olası.
        let u = { let v = rng.f(); if rng.f() < 0.5 { v * v * 0.5 } else { 1.0 - v * v * 0.5 } };
        let (x, y, ang) = match side {
            0 => (u * w, 0.0, std::f32::consts::FRAC_PI_2),
            1 => (u * w, h, -std::f32::consts::FRAC_PI_2),
            2 => (0.0, u * h, 0.0),
            _ => (w, u * h, std::f32::consts::PI),
        };
        let len = rng.range(0.04, 0.11) * h;
        fern(dc, &line, x, y, ang + rng.range(-0.6, 0.6), len, rng.range(0.6, 1.1), 2, rng);
    }
    // Buz kristali parıltıları.
    let spark = dc.CreateSolidColorBrush(&col(255, 255, 255, 0.7), None)?;
    for _ in 0..160 {
        let (x, y) = match (rng.f() * 4.0) as u32 {
            0 => (rng.f() * w, rng.f() * rng.f() * t),
            1 => (rng.f() * w, h - rng.f() * rng.f() * t),
            2 => (rng.f() * rng.f() * t, rng.f() * h),
            _ => (w - rng.f() * rng.f() * t, rng.f() * h),
        };
        let r = rng.range(0.4, 1.0);
        dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r }, &spark);
    }
    Ok(())
}

pub struct Frost {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    vis: SpriteVisual,
    on: bool,
    checked: Instant,
}

impl Frost {
    pub fn new(gfx: &Gfx, mon: RECT) -> Result<Self> {
        static CLASS: Once = Once::new();
        let hwnd = unsafe { overlay_window(w!("CarlUI.DockFrost"), &CLASS)? };
        let (w, h) = ((mon.right - mon.left).max(1), (mon.bottom - mon.top).max(1));
        let made = (|| -> Result<_> {
            unsafe {
                let _ = SetWindowPos(hwnd, HWND_TOPMOST, mon.left, mon.top, w, h, SWP_NOACTIVATE);
            }
            let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
            let root = gfx.comp.CreateContainerVisual()?;
            target.SetRoot(&root)?;
            // Yarım çözünürlükte çizilir; buz zaten yumuşak.
            let (bw, bh) = ((w as f32 / 2.0).round(), (h as f32 / 2.0).round());
            let mut rng = Rng::from_time();
            let surf = gfx.surface(bw, bh, |dc| unsafe { draw(dc, bw, bh, &mut rng) })?;
            let brush = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
            brush.SetStretch(CompositionStretch::Fill)?;
            let vis = gfx.comp.CreateSpriteVisual()?;
            vis.SetBrush(&brush)?;
            vis.SetSize(v2(w as f32, h as f32))?;
            vis.SetOffset(v3(0.0, 0.0))?;
            vis.SetOpacity(0.0)?;
            root.Children()?.InsertAtTop(&vis)?;
            Ok((target, vis))
        })();
        match made {
            Ok((target, vis)) => {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                Ok(Self { hwnd, _target: target, vis, on: false, checked: Instant::now() })
            }
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e)
            }
        }
    }

    /// Boşta kalma süresine göre buzu getirir ya da eritir.
    pub fn step(&mut self, gfx: &Gfx, now: Instant) {
        if now - self.checked < CHECK {
            return;
        }
        self.checked = now;
        let idle = idle_ms() as u64 >= IDLE.as_millis() as u64;
        if idle == self.on {
            return;
        }
        self.on = idle;
        let _ = (|| -> Result<()> {
            let a = gfx.comp.CreateScalarKeyFrameAnimation()?;
            a.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
            a.InsertKeyFrame(1.0, if idle { 0.85 } else { 0.0 })?;
            a.SetDuration(ts(if idle { IN_MS } else { OUT_MS }))?;
            self.vis.StartAnimation(h!("Opacity"), &a)
        })();
    }
}

impl Drop for Frost {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
