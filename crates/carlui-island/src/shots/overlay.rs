//! Ekran görüntüsü düzenleyicisi: dondurulmuş kareyi tüm monitöre serer,
//! kullanıcı alanı sürükleyerek seçer, sonra kalemle çizer, yazı yazar,
//! silgiyle tek tek siler ya da geri alır; Kopyala / Kaydet ile biter.
//! Aynı pencere bildirimdeki "Düzenle" ile kayıtlı bir resmi de açar.
//!
//! Çizim Direct2D ile yapılır. Girdi soyut olaylarla gelir (`down/move/up/
//! key/char`): normalde pencere mesajlarından, oyun öndeyken (pencere odak
//! almaz) klavye/fare kancasından. Böylece oyun arka plana düşmez.

use super::capture::Frame;
use super::qr::{self, Qr};
use std::path::PathBuf;
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
            Gdi::{InvalidateRect, ValidateRect},
            Imaging::*,
        },
        UI::WindowsAndMessaging::*,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Pen,
    Text,
    Eraser,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Tool(Tool),
    Color(usize),
    Undo,
    Copy,
    /// Seçimdeki yazıyı oku (OCR) ve panoya kopyala.
    Ocr,
    Save,
    Close,
    /// Bulunan QR kodunun düğmesi: (sıra, eylem).
    Qr(usize, qr::Act),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    None,
    Cancel,
    Copy,
    Ocr,
    Save,
    Qr(usize, qr::Act),
}

const COLORS: [(u8, u8, u8); 4] = [(255, 59, 48), (255, 204, 0), (10, 132, 255), (255, 255, 255)];

#[derive(Clone, Debug)]
enum Mark {
    Stroke { pts: Vec<(f32, f32)>, color: usize, width: f32 },
    Text { x: f32, y: f32, s: String, color: usize, size: f32 },
}

pub struct Overlay {
    pub hwnd: HWND,
    pub frame: Frame,
    /// Düzenlenen dosya (bildirimden açıldıysa).
    pub editing: Option<PathBuf>,
    /// Oyun öndeyken: odak alınmaz, girdi kancadan gelir.
    pub passive: bool,
    s: f32,
    /// Resmin pencerede çizildiği yer ve ölçek (pencere px / resim px).
    ox: f32,
    oy: f32,
    k: f32,
    /// Seçim (resim koordinatı): x0, y0, x1, y1.
    sel: Option<(f32, f32, f32, f32)>,
    drag: Option<(f32, f32)>,
    tool: Tool,
    color: usize,
    marks: Vec<Mark>,
    typing: Option<usize>,
    hover: Option<Btn>,
    buttons: Vec<(Btn, D2D_RECT_F)>,
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    rt: Option<ID2D1HwndRenderTarget>,
    bmp: Option<ID2D1Bitmap>,
    round: Option<ID2D1StrokeStyle>,
    /// Seçimde bulunan QR kodları (seçim bitince taranır).
    pub qr: Vec<Qr>,
}

fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}

fn inside(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn col(c: (u8, u8, u8), a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Overlay {
    pub fn new(hwnd: HWND, frame: Frame, editing: Option<PathBuf>, passive: bool, s: f32) -> Result<Self> {
        let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let dw: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let round = unsafe {
            factory
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() },
                    None,
                )
                .ok()
        };
        let mut o = Self {
            hwnd,
            frame,
            editing,
            passive,
            s,
            ox: 0.0,
            oy: 0.0,
            k: 1.0,
            sel: None,
            drag: None,
            tool: Tool::Select,
            color: 0,
            marks: Vec::new(),
            typing: None,
            hover: None,
            buttons: Vec::new(),
            factory,
            dw,
            rt: None,
            bmp: None,
            round,
            qr: Vec::new(),
        };
        o.layout();
        if o.editing.is_some() {
            // Düzenleme: resmin tamamı seçili, kalemle başla.
            o.sel = Some((0.0, 0.0, o.frame.w as f32, o.frame.h as f32));
            o.tool = Tool::Pen;
            o.scan_qr();
        }
        Ok(o)
    }

    /// Seçili alandaki QR kodlarını bul (bkz. `qr`).
    fn scan_qr(&mut self) {
        self.qr = match self.sel_norm() {
            Some((a, b, c, d)) => qr::scan(&self.frame.px, self.frame.w, self.frame.h, (a as u32, b as u32, c.ceil() as u32, d.ceil() as u32)),
            None => Vec::new(),
        };
        if !self.qr.is_empty() {
            let kinds: Vec<_> = self.qr.iter().map(|q| format!("{:?}", q.kind)).collect();
            carlui_core::info!("ekran görüntüsü: QR bulundu ({})", kinds.join(", "));
        }
    }

    fn win_size(&self) -> (f32, f32) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut r);
        }
        ((r.right - r.left) as f32, (r.bottom - r.top) as f32)
    }

    /// Resmi pencereye yerleştir: ekran görüntüsü birebir, düzenlenen resim sığdırılır.
    fn layout(&mut self) {
        let (ww, wh) = self.win_size();
        let (iw, ih) = (self.frame.w as f32, self.frame.h as f32);
        self.k = if self.editing.is_some() { (ww * 0.86 / iw).min((wh * 0.8) / ih).min(1.0) } else { 1.0 };
        self.ox = ((ww - iw * self.k) / 2.0).round();
        self.oy = if self.editing.is_some() { ((wh - ih * self.k) / 2.0 - 30.0 * self.s).round().max(10.0) } else { 0.0 };
    }

    /// Monitörler (resim koordinatı); düzenlenen resimde resmin tamamı.
    fn mon_rects(&self) -> Vec<D2D_RECT_F> {
        let r = self.frame.rect;
        let v: Vec<D2D_RECT_F> = self
            .frame
            .monitors
            .iter()
            .map(|m| rect((m.left - r.left) as f32, (m.top - r.top) as f32, (m.right - r.left) as f32, (m.bottom - r.top) as f32))
            .collect();
        if v.is_empty() || self.editing.is_some() {
            vec![rect(0.0, 0.0, self.frame.w as f32, self.frame.h as f32)]
        } else {
            v
        }
    }

    /// Resim noktasının bulunduğu monitör (yoksa ilki).
    fn mon_at(&self, x: f32, y: f32) -> D2D_RECT_F {
        let v = self.mon_rects();
        v.iter().find(|m| inside(m, x, y)).copied().unwrap_or(v[0])
    }

    fn to_img(&self, x: f32, y: f32) -> (f32, f32) {
        (((x - self.ox) / self.k).clamp(0.0, self.frame.w as f32), ((y - self.oy) / self.k).clamp(0.0, self.frame.h as f32))
    }

    fn to_win(&self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + x * self.k, self.oy + y * self.k)
    }

    fn sel_norm(&self) -> Option<(f32, f32, f32, f32)> {
        self.sel.map(|(a, b, c, d)| (a.min(c), b.min(d), a.max(c), b.max(d)))
    }

    /// Seçili alan, görüntü pikseli olarak (x0, y0, x1, y1).
    pub fn sel_px(&self) -> Option<(u32, u32, u32, u32)> {
        self.sel_norm().map(|(a, b, c, d)| (a.max(0.0) as u32, b.max(0.0) as u32, c.ceil().max(0.0) as u32, d.ceil().max(0.0) as u32))
    }

    pub fn debug_sel(&self) -> Option<(f32, f32, f32, f32)> {
        self.sel
    }

    pub fn has_selection(&self) -> bool {
        self.sel_norm().is_some_and(|(a, b, c, d)| c - a >= 2.0 && d - b >= 2.0) && self.drag.is_none()
    }

    // ---- Girdi ----

    pub fn down(&mut self, x: f32, y: f32) -> Out {
        if let Some(b) = self.buttons.iter().find(|(_, r)| inside(r, x, y)).map(|b| b.0) {
            return self.press(b);
        }
        self.finish_typing();
        let (ix, iy) = self.to_img(x, y);
        let in_sel = self.sel_norm().is_some_and(|(a, b, c, d)| ix >= a && ix <= c && iy >= b && iy <= d);
        match self.tool {
            Tool::Pen if in_sel => self.marks.push(Mark::Stroke { pts: vec![(ix, iy)], color: self.color, width: 4.0 / self.k.max(0.5) }),
            Tool::Text if in_sel => {
                self.marks.push(Mark::Text { x: ix, y: iy - 14.0, s: String::new(), color: self.color, size: 28.0 });
                self.typing = Some(self.marks.len() - 1);
            }
            Tool::Eraser => self.erase_at(ix, iy),
            _ => {
                // Yeni seçim (çizim yoksa ya da seçim aracındaysa). Düzenlenen
                // resimde yalnızca Kırp aracıyla: seçilen alan kaydedilir.
                if (self.editing.is_none() || self.tool == Tool::Select) && (self.tool == Tool::Select || !in_sel) {
                    self.tool = Tool::Select;
                    self.sel = Some((ix, iy, ix, iy));
                    self.drag = Some((ix, iy));
                    self.qr.clear();
                }
            }
        }
        self.redraw();
        Out::None
    }

    pub fn moved(&mut self, x: f32, y: f32, button: bool) {
        let (ix, iy) = self.to_img(x, y);
        let mut dirty = false;
        if let (Some((sx, sy)), true) = (self.drag, button) {
            self.sel = Some((sx, sy, ix, iy));
            dirty = true;
        } else if button && self.tool == Tool::Pen {
            if let Some(Mark::Stroke { pts, .. }) = self.marks.last_mut() {
                if let Some((a, b, c, d)) = self.sel.map(|(a, b, c, d)| (a.min(c), b.min(d), a.max(c), b.max(d))) {
                    pts.push((ix.clamp(a, c), iy.clamp(b, d)));
                    dirty = true;
                }
            }
        }
        let hover = self.buttons.iter().find(|(_, r)| inside(r, x, y)).map(|b| b.0);
        if hover != self.hover {
            self.hover = hover;
            dirty = true;
        }
        if dirty {
            self.redraw();
        }
    }

    pub fn up(&mut self, _x: f32, _y: f32) {
        if self.drag.take().is_some() {
            if let Some((a, b, c, d)) = self.sel_norm() {
                // Sürüklemeden tıklandı: tıklanan monitörün tamamı.
                if c - a < 4.0 || d - b < 4.0 {
                    let m = self.mon_at(a, b);
                    self.sel = Some((m.left, m.top, m.right, m.bottom));
                }
            }
            self.scan_qr();
            self.redraw();
        }
    }

    pub fn key(&mut self, vk: u16, ctrl: bool) -> Out {
        if self.typing.is_some() {
            match vk {
                0x1B | 0x0D => self.finish_typing(),
                0x08 => {
                    if let Some(Mark::Text { s, .. }) = self.typing.and_then(|i| self.marks.get_mut(i)) {
                        s.pop();
                    }
                }
                _ => return Out::None,
            }
            self.redraw();
            return Out::None;
        }
        match (vk, ctrl) {
            (0x1B, _) => return Out::Cancel,
            (0x0D, _) | (0x43, true) if self.has_selection() => return Out::Copy,
            (0x53, true) if self.has_selection() => return Out::Save,
            (0x54, true) if self.has_selection() => return Out::Ocr,
            (0x5A, true) | (0x2E, false) => {
                self.marks.pop();
            }
            (0x43, false) => self.set_tool(Tool::Select),
            (0x50, false) => self.set_tool(Tool::Pen),
            (0x54, false) => self.set_tool(Tool::Text),
            (0x45, false) => self.set_tool(Tool::Eraser),
            _ => return Out::None,
        }
        self.redraw();
        Out::None
    }

    pub fn char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        if let Some(Mark::Text { s, .. }) = self.typing.and_then(|i| self.marks.get_mut(i)) {
            s.push(c);
            self.redraw();
        }
    }

    pub fn typing(&self) -> bool {
        self.typing.is_some()
    }

    fn finish_typing(&mut self) {
        if let Some(i) = self.typing.take() {
            if matches!(self.marks.get(i), Some(Mark::Text { s, .. }) if s.trim().is_empty()) {
                self.marks.remove(i);
            }
        }
    }

    fn set_tool(&mut self, t: Tool) {
        self.finish_typing();
        if self.has_selection() {
            self.tool = t;
        }
    }

    fn press(&mut self, b: Btn) -> Out {
        match b {
            Btn::Tool(t) => self.set_tool(if self.tool == t { Tool::Select } else { t }),
            Btn::Color(i) => {
                self.color = i;
                if let Some(Mark::Text { color, .. }) = self.typing.and_then(|i| self.marks.get_mut(i)) {
                    *color = i;
                }
            }
            Btn::Undo => {
                self.finish_typing();
                self.marks.pop();
            }
            Btn::Copy => {
                self.finish_typing();
                return Out::Copy;
            }
            Btn::Ocr => {
                self.finish_typing();
                return Out::Ocr;
            }
            Btn::Save => {
                self.finish_typing();
                return Out::Save;
            }
            Btn::Close => return Out::Cancel,
            Btn::Qr(i, a) => {
                self.finish_typing();
                return Out::Qr(i, a);
            }
        }
        self.redraw();
        Out::None
    }

    fn text_box(&self, x: f32, y: f32, s: &str, size: f32) -> (f32, f32) {
        unsafe {
            let w = wide(if s.is_empty() { " " } else { s });
            let Ok(fmt) = self.dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr")) else { return (x + 10.0, y + size) };
            let Ok(l) = self.dw.CreateTextLayout(&w, &fmt, 10_000.0, 10_000.0) else { return (x + 10.0, y + size) };
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            (x + m.widthIncludingTrailingWhitespace, y + m.height)
        }
    }

    fn erase_at(&mut self, x: f32, y: f32) {
        let tol = 12.0 / self.k;
        let hit = self.marks.iter().rposition(|m| match m {
            Mark::Stroke { pts, width, .. } => pts.iter().any(|p| (p.0 - x).hypot(p.1 - y) <= tol + width),
            Mark::Text { x: tx, y: ty, s, size, .. } => {
                let (r, b) = self.text_box(*tx, *ty, s, *size);
                x >= tx - tol && x <= r + tol && y >= ty - tol && y <= b + tol
            }
        });
        if let Some(i) = hit {
            self.marks.remove(i);
        }
    }

    // ---- Çizim ----

    pub fn redraw(&mut self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn ensure_target(&mut self) -> Result<()> {
        if self.rt.is_some() {
            return Ok(());
        }
        let (w, h) = self.win_size();
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: w as u32, height: h as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY };
        let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
        let bmp = unsafe { Self::bitmap(&rt.cast()?, &self.frame)? };
        self.rt = Some(rt);
        self.bmp = Some(bmp);
        Ok(())
    }

    unsafe fn bitmap(rt: &ID2D1RenderTarget, f: &Frame) -> Result<ID2D1Bitmap> {
        rt.CreateBitmap(
            D2D_SIZE_U { width: f.w, height: f.h },
            Some(f.px.as_ptr() as *const _),
            f.w * 4,
            &D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE }, dpiX: 96.0, dpiY: 96.0 },
        )
    }

    /// Çizimleri (resim koordinatında) verilen hedefe çizer.
    unsafe fn draw_marks(&self, rt: &ID2D1RenderTarget, caret: bool) -> Result<()> {
        for (i, m) in self.marks.iter().enumerate() {
            match m {
                Mark::Stroke { pts, color, width } => {
                    let b = rt.CreateSolidColorBrush(&col(COLORS[*color], 1.0), None)?;
                    if pts.len() == 1 {
                        let p = D2D_POINT_2F { x: pts[0].0, y: pts[0].1 };
                        rt.FillEllipse(&D2D1_ELLIPSE { point: p, radiusX: width / 2.0, radiusY: width / 2.0 }, &b);
                    }
                    for w in pts.windows(2) {
                        rt.DrawLine(D2D_POINT_2F { x: w[0].0, y: w[0].1 }, D2D_POINT_2F { x: w[1].0, y: w[1].1 }, &b, *width, self.round.as_ref());
                    }
                }
                Mark::Text { x, y, s, color, size } => {
                    let fmt = self.dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, *size, w!("tr-tr"))?;
                    let text = wide(s);
                    let (r, bt) = self.text_box(*x, *y, s, *size);
                    // Okunsun diye hafif gölge.
                    let sh = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.55 }, None)?;
                    rt.DrawText(&text, &fmt, &rect(x + 1.5, y + 1.5, r + 400.0, bt + 4.0), &sh, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    let b = rt.CreateSolidColorBrush(&col(COLORS[*color], 1.0), None)?;
                    rt.DrawText(&text, &fmt, &rect(*x, *y, r + 400.0, bt + 4.0), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    if caret && self.typing == Some(i) {
                        rt.FillRectangle(&rect(r + 1.0, y + size * 0.15, r + 3.0, bt - size * 0.1), &b);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn paint(&mut self) {
        if let Err(e) = self.paint_inner() {
            carlui_core::warn!("ekran görüntüsü çizilemedi: {e}");
            self.rt = None;
        }
        unsafe {
            let _ = ValidateRect(self.hwnd, None);
        }
    }

    fn paint_inner(&mut self) -> Result<()> {
        self.ensure_target()?;
        let rt: ID2D1RenderTarget = self.rt.as_ref().unwrap().cast()?;
        let bmp = self.bmp.clone().unwrap();
        let s = self.s;
        let (ww, wh) = self.win_size();
        let (iw, ih) = (self.frame.w as f32 * self.k, self.frame.h as f32 * self.k);
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.06, a: 1.0 }));
            let img = rect(self.ox, self.oy, self.ox + iw, self.oy + ih);
            rt.DrawBitmap(&bmp, Some(&img), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            let dim = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.45 }, None)?;
            let white = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
            let sel = self.sel_norm().filter(|(a, b, c, d)| c - a >= 1.0 && d - b >= 1.0);
            match sel {
                None => {
                    rt.FillRectangle(&img, &dim);
                    for m in self.mon_rects() {
                        let (l, t) = self.to_win(m.left, m.top);
                        let (r, _) = self.to_win(m.right, m.bottom);
                        self.label(&rt, t!("Alanı sürükleyerek seç · tıkla: bu ekran · Esc: iptal", "Drag to select an area · click: this screen · Esc: cancel"), (l + r) / 2.0, t + 54.0 * s, true)?;
                    }
                }
                Some((a, b, c, d)) => {
                    let (l, t) = self.to_win(a, b);
                    let (r, btm) = self.to_win(c, d);
                    rt.FillRectangle(&rect(img.left, img.top, img.right, t), &dim);
                    rt.FillRectangle(&rect(img.left, btm, img.right, img.bottom), &dim);
                    rt.FillRectangle(&rect(img.left, t, l, btm), &dim);
                    rt.FillRectangle(&rect(r, t, img.right, btm), &dim);
                    // Çizimler resim koordinatında, seçime kırpılarak.
                    rt.PushAxisAlignedClip(&rect(l, t, r, btm), D2D1_ANTIALIAS_MODE_ALIASED);
                    rt.SetTransform(&Matrix3x2 { M11: self.k, M12: 0.0, M21: 0.0, M22: self.k, M31: self.ox, M32: self.oy });
                    self.draw_marks(&rt, true)?;
                    rt.SetTransform(&Matrix3x2::identity());
                    rt.PopAxisAlignedClip();
                    rt.DrawRectangle(&rect(l - 0.5, t - 0.5, r + 0.5, btm + 0.5), &white, 1.5 * s, None);
                    // Köşe tutamakları
                    for (hx, hy) in [(l, t), (r, t), (l, btm), (r, btm)] {
                        rt.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x: hx, y: hy }, radiusX: 4.0 * s, radiusY: 4.0 * s }, &white);
                    }
                    let size = format!("{} × {}", (c - a).round(), (d - b).round());
                    self.label(&rt, &size, l + 40.0 * s, (t - 18.0 * s).max(16.0 * s), false)?;
                    if self.drag.is_none() {
                        // Araç çubuğu seçimin ortasındaki monitörde kalsın (ekranlar arası boşluğa düşmesin).
                        let m = if self.editing.is_some() { rect(0.0, 0.0, ww, wh) } else {
                            let m = self.mon_at((a + c) / 2.0, (b + d) / 2.0);
                            let (ml, mt) = self.to_win(m.left, m.top);
                            let (mr, mb) = self.to_win(m.right, m.bottom);
                            rect(ml, mt, mr, mb)
                        };
                        self.toolbar(&rt, l, t, r, btm, m)?;
                    } else {
                        self.buttons.clear();
                    }
                }
            }
            rt.EndDraw(None, None)?;
        }
        Ok(())
    }

    /// Koyu hap üzerinde ortalanmış kısa yazı.
    unsafe fn label(&self, rt: &ID2D1RenderTarget, text: &str, cx: f32, cy: f32, big: bool) -> Result<()> {
        let s = self.s;
        let size = if big { 14.0 } else { 12.0 } * s;
        let fmt = self.dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr"))?;
        let (r, b) = self.text_box(0.0, 0.0, text, size);
        let (pw, ph) = (r + 26.0 * s, b + 12.0 * s);
        let bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.1, g: 0.1, b: 0.12, a: 0.88 }, None)?;
        let box_ = rect(cx - pw / 2.0, cy - ph / 2.0, cx + pw / 2.0, cy + ph / 2.0);
        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: box_, radiusX: ph / 2.0, radiusY: ph / 2.0 }, &bg);
        let fg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None)?;
        rt.DrawText(&wide(text), &fmt, &rect(box_.left + 13.0 * s, box_.top + 6.0 * s, box_.right, box_.bottom), &fg, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        Ok(())
    }

    /// `m`: araç çubuğunun içinde kalacağı monitör (pencere koordinatı).
    #[allow(clippy::too_many_arguments)]
    unsafe fn toolbar(&mut self, rt: &ID2D1RenderTarget, l: f32, t: f32, r: f32, b: f32, m: D2D_RECT_F) -> Result<()> {
        let s = self.s;
        let bs = 36.0 * s;
        let gap = 4.0 * s;
        let items: Vec<(Btn, char)> = vec![
            (Btn::Tool(Tool::Select), '\u{E7A8}'),
            (Btn::Tool(Tool::Pen), '\u{EDFB}'),
            (Btn::Tool(Tool::Text), '\u{E8D2}'),
            (Btn::Tool(Tool::Eraser), '\u{E75C}'),
            (Btn::Undo, '\u{E7A7}'),
            (Btn::Color(0), ' '),
            (Btn::Color(1), ' '),
            (Btn::Color(2), ' '),
            (Btn::Color(3), ' '),
            (Btn::Copy, '\u{E8C8}'),
            (Btn::Ocr, '\u{EA37}'),
            (Btn::Save, '\u{E74E}'),
            (Btn::Close, '\u{E711}'),
        ];
        let seps = 2.0 * 10.0 * s;
        let tw = items.len() as f32 * (bs + gap) - gap + 16.0 * s + seps;
        let th = bs + 12.0 * s;
        let mut x = ((l + r) / 2.0 - tw / 2.0).clamp(m.left + 8.0 * s, (m.right - tw - 8.0 * s).max(m.left + 8.0 * s));
        let mut y = b + 10.0 * s;
        if y + th > m.bottom - 8.0 * s {
            y = t - th - 10.0 * s;
        }
        if y < m.top + 8.0 * s {
            y = b - th - 10.0 * s;
        }
        let bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.11, g: 0.11, b: 0.13, a: 0.94 }, None)?;
        let edge = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }, None)?;
        let bar = rect(x, y, x + tw, y + th);
        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bar, radiusX: th / 2.0, radiusY: th / 2.0 }, &bg);
        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: bar, radiusX: th / 2.0, radiusY: th / 2.0 }, &edge, 1.0, None);
        let icons = self
            .dw
            .CreateTextFormat(w!("Segoe Fluent Icons"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 16.0 * s, w!("tr-tr"))
            .or_else(|_| self.dw.CreateTextFormat(w!("Segoe MDL2 Assets"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 16.0 * s, w!("tr-tr")))?;
        icons.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        icons.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        let fg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.92 }, None)?;
        let dimfg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.35 }, None)?;
        let on = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 }, None)?;
        let hov = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.09 }, None)?;
        self.buttons.clear();
        x += 8.0 * s;
        y += 6.0 * s;
        for (i, (btn, glyph)) in items.iter().enumerate() {
            if i == 5 || i == 9 {
                rt.FillRectangle(&rect(x + 3.0 * s, y + 8.0 * s, x + 4.0 * s, y + bs - 8.0 * s), &edge);
                x += 10.0 * s;
            }
            let br = rect(x, y, x + bs, y + bs);
            let active = matches!(btn, Btn::Tool(t) if *t == self.tool) || matches!(btn, Btn::Color(c) if *c == self.color);
            let rr = D2D1_ROUNDED_RECT { rect: br, radiusX: bs / 2.0, radiusY: bs / 2.0 };
            if active && !matches!(btn, Btn::Color(_)) {
                rt.FillRoundedRectangle(&rr, &on);
            } else if self.hover == Some(*btn) {
                rt.FillRoundedRectangle(&rr, &hov);
            }
            match btn {
                Btn::Color(c) => {
                    let cb = rt.CreateSolidColorBrush(&col(COLORS[*c], 1.0), None)?;
                    let rad = if active { 9.0 } else { 7.0 } * s;
                    let p = D2D_POINT_2F { x: x + bs / 2.0, y: y + bs / 2.0 };
                    if active {
                        rt.DrawEllipse(&D2D1_ELLIPSE { point: p, radiusX: rad + 3.0 * s, radiusY: rad + 3.0 * s }, &fg, 1.5 * s, None);
                    }
                    rt.FillEllipse(&D2D1_ELLIPSE { point: p, radiusX: rad, radiusY: rad }, &cb);
                }
                _ => {
                    let g: Vec<u16> = glyph.to_string().encode_utf16().collect();
                    let usable = !matches!(btn, Btn::Undo) || !self.marks.is_empty();
                    rt.DrawText(&g, &icons, &br, if usable { &fg } else { &dimfg }, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
            self.buttons.push((*btn, br));
            x += bs + gap;
        }
        if !self.qr.is_empty() {
            self.qr_pills(rt, bar, m)?;
        }
        // Üstüne gelinen düğmenin adı.
        if let Some((b, r)) = self.hover.and_then(|h| self.buttons.iter().find(|x| x.0 == h && !matches!(x.0, Btn::Qr(..)))).copied() {
            let name = match b {
                Btn::Tool(Tool::Pen) => t!("Kalem (P)", "Pen (P)"),
                Btn::Tool(Tool::Text) => t!("Yazı (T)", "Text (T)"),
                Btn::Tool(Tool::Eraser) => t!("Silgi (E)", "Eraser (E)"),
                Btn::Tool(Tool::Select) => t!("Alan seç / kırp (C)", "Select / crop (C)"),
                Btn::Undo => t!("Geri al (Ctrl+Z)", "Undo (Ctrl+Z)"),
                Btn::Color(_) => t!("Renk", "Color"),
                Btn::Copy => t!("Kopyala (Enter)", "Copy (Enter)"),
                Btn::Ocr => t!("Yazıyı kopyala (Ctrl+T)", "Copy text (Ctrl+T)"),
                Btn::Save => if self.editing.is_some() { t!("Kaydet (Ctrl+S)", "Save (Ctrl+S)") } else { t!("Masaüstüne kaydet (Ctrl+S)", "Save to desktop (Ctrl+S)") },
                Btn::Close => t!("Kapat (Esc)", "Close (Esc)"),
                Btn::Qr(..) => "",
            };
            let above = r.top - 22.0 * s;
            self.label(rt, name, (r.left + r.right) / 2.0, if above > 10.0 { above } else { r.bottom + 22.0 * s }, false)?;
        }
        Ok(())
    }

    /// Bulunan QR kodları: araç çubuğunun altında (yer yoksa üstünde) birer
    /// hap; türü ve önizlemesi, yanında türe göre düğmeler (bkz. `qr::Qr::actions`).
    unsafe fn qr_pills(&mut self, rt: &ID2D1RenderTarget, bar: D2D_RECT_F, m: D2D_RECT_F) -> Result<()> {
        let s = self.s;
        let (ph, size, gap) = (40.0 * s, 13.0 * s, 6.0 * s);
        let items: Vec<(String, Vec<(qr::Act, &str)>)> = self.qr.iter().take(3).map(|q| (q.title(), q.actions())).collect();
        let need = items.len() as f32 * (ph + gap);
        let below = bar.bottom + 8.0 * s + need <= m.bottom - 8.0 * s;
        let mut y = if below { bar.bottom + 8.0 * s } else { bar.top - 8.0 * s - ph };
        let fmt = self.dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr"))?;
        fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let centered = self.dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr"))?;
        centered.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let icons = self
            .dw
            .CreateTextFormat(w!("Segoe Fluent Icons"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 16.0 * s, w!("tr-tr"))
            .or_else(|_| self.dw.CreateTextFormat(w!("Segoe MDL2 Assets"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 16.0 * s, w!("tr-tr")))?;
        icons.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        icons.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        let bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.11, g: 0.11, b: 0.13, a: 0.94 }, None)?;
        let edge = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }, None)?;
        let fg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.92 }, None)?;
        let btn = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }, None)?;
        let hov = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.22 }, None)?;
        // Carl Vault düğmesi kasanın mor vurgusuyla öne çıksın.
        let vault = rt.CreateSolidColorBrush(&col((176, 108, 230), 0.9), None)?;
        let vault_hov = rt.CreateSolidColorBrush(&col((196, 132, 245), 1.0), None)?;
        for (i, (title, acts)) in items.iter().enumerate() {
            let (tw, _) = self.text_box(0.0, 0.0, title, size);
            let tw = tw.min(420.0 * s);
            let widths: Vec<f32> = acts.iter().map(|(_, l)| self.text_box(0.0, 0.0, l, size).0 + 28.0 * s).collect();
            let total = 12.0 * s + 22.0 * s + 8.0 * s + tw + 14.0 * s + widths.iter().map(|w| w + gap).sum::<f32>();
            let x = ((bar.left + bar.right) / 2.0 - total / 2.0).clamp(m.left + 8.0 * s, (m.right - total - 8.0 * s).max(m.left + 8.0 * s));
            let pill = rect(x, y, x + total, y + ph);
            let rr = D2D1_ROUNDED_RECT { rect: pill, radiusX: ph / 2.0, radiusY: ph / 2.0 };
            rt.FillRoundedRectangle(&rr, &bg);
            rt.DrawRoundedRectangle(&rr, &edge, 1.0, None);
            let g: Vec<u16> = '\u{ED14}'.to_string().encode_utf16().collect();
            rt.DrawText(&g, &icons, &rect(x + 12.0 * s, y, x + 34.0 * s, y + ph), &fg, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            let tx = x + 42.0 * s;
            rt.PushAxisAlignedClip(&rect(tx, y, tx + tw, y + ph), D2D1_ANTIALIAS_MODE_ALIASED);
            rt.DrawText(&wide(title), &fmt, &rect(tx, y, tx + tw + 200.0, y + ph), &fg, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            rt.PopAxisAlignedClip();
            let mut bx = tx + tw + 14.0 * s;
            for ((act, label), w) in acts.iter().zip(widths) {
                let br = rect(bx, y + 6.0 * s, bx + w, y + ph - 6.0 * s);
                let hover = self.hover == Some(Btn::Qr(i, *act));
                let fill = match (*act == qr::Act::Vault, hover) {
                    (true, true) => &vault_hov,
                    (true, false) => &vault,
                    (false, true) => &hov,
                    (false, false) => &btn,
                };
                let r = (br.bottom - br.top) / 2.0;
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: br, radiusX: r, radiusY: r }, fill);
                rt.DrawText(&wide(label), &centered, &br, &fg, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                self.buttons.push((Btn::Qr(i, *act), br));
                bx += w + gap;
            }
            y += if below { ph + gap } else { -(ph + gap) };
        }
        Ok(())
    }

    /// Seçili alanı çizimleriyle birlikte PNG olarak üretir.
    pub fn render_png(&self) -> Option<(Vec<u8>, u32, u32)> {
        let (a, b, c, d) = self.sel_norm()?;
        let (x0, y0) = (a.floor().max(0.0), b.floor().max(0.0));
        let (w, h) = ((c - x0).round().max(1.0) as u32, (d - y0).round().max(1.0) as u32);
        let wic = carlui_clip::image::wic()?;
        unsafe {
            let target = wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad).ok()?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = self.factory.CreateWicBitmapRenderTarget(&target, &props).ok()?;
            let bmp = Self::bitmap(&rt, &self.frame).ok()?;
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: -x0, M32: -y0 });
            let full = rect(0.0, 0.0, self.frame.w as f32, self.frame.h as f32);
            rt.DrawBitmap(&bmp, Some(&full), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None);
            self.draw_marks(&rt, false).ok()?;
            rt.EndDraw(None, None).ok()?;
            let png = carlui_clip::image::encode_png(&wic, &target.cast().ok()?)?;
            Some((png, w, h))
        }
    }

    pub fn resized(&mut self) {
        self.rt = None;
        self.layout();
        self.redraw();
    }
}
