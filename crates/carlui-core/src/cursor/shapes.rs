//! Windows'un imleç rolleri (Normal, Metin, Meşgul, boyutlandırma… 17 tane) ve
//! her birinin 32×32'lik tuvaldeki şekli. Bir şekil "grup"lardan oluşur: her
//! grubun parçaları önce kenar rengiyle birleşik siluet olarak, sonra gövdeyle
//! doldurulur; sonraki grup (rozet: ?, konum, kişi, dönen yay) üstüne biner.

use windows::core::{Interface, Result};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Arrow,
    Help,
    Working,
    Busy,
    Cross,
    Text,
    Pen,
    No,
    SizeNS,
    SizeWE,
    SizeNWSE,
    SizeNESW,
    Move,
    Up,
    Hand,
    Pin,
    Person,
}

/// Dönen yayın kare sayısı ve kare süresi (1/60 sn).
pub const FRAMES: u32 = 24;
pub const JIFFIES: u32 = 2;

impl Role {
    pub const ALL: [Role; 17] = [
        Role::Arrow,
        Role::Help,
        Role::Working,
        Role::Busy,
        Role::Cross,
        Role::Text,
        Role::Pen,
        Role::No,
        Role::SizeNS,
        Role::SizeWE,
        Role::SizeNWSE,
        Role::SizeNESW,
        Role::Move,
        Role::Up,
        Role::Hand,
        Role::Pin,
        Role::Person,
    ];

    /// `SetSystemCursor` kimliği (OCR_*).
    pub fn ocr(self) -> u32 {
        match self {
            Role::Arrow => 32512,
            Role::Text => 32513,
            Role::Busy => 32514,
            Role::Cross => 32515,
            Role::Up => 32516,
            Role::Pen => 32631,
            Role::SizeNWSE => 32642,
            Role::SizeNESW => 32643,
            Role::SizeWE => 32644,
            Role::SizeNS => 32645,
            Role::Move => 32646,
            Role::No => 32648,
            Role::Hand => 32649,
            Role::Working => 32650,
            Role::Help => 32651,
            Role::Pin => 32671,
            Role::Person => 32672,
        }
    }

    pub fn file(self) -> &'static str {
        match self {
            Role::Arrow => "arrow.cur",
            Role::Help => "help.cur",
            Role::Working => "working.ani",
            Role::Busy => "busy.ani",
            Role::Cross => "cross.cur",
            Role::Text => "text.cur",
            Role::Pen => "pen.cur",
            Role::No => "no.cur",
            Role::SizeNS => "size-ns.cur",
            Role::SizeWE => "size-we.cur",
            Role::SizeNWSE => "size-nwse.cur",
            Role::SizeNESW => "size-nesw.cur",
            Role::Move => "move.cur",
            Role::Up => "up.cur",
            Role::Hand => "hand.cur",
            Role::Pin => "pin.cur",
            Role::Person => "person.cur",
        }
    }

    pub fn animated(self) -> bool {
        matches!(self, Role::Working | Role::Busy)
    }

    /// Sıcak nokta (32'lik tuvalde).
    pub fn hot(self) -> (f32, f32) {
        match self {
            Role::Arrow | Role::Help | Role::Working | Role::Pin | Role::Person => (5.5, 4.0),
            Role::Hand => (13.0, 3.0),
            Role::Up => (16.0, 3.0),
            Role::Pen => (8.6, 23.4),
            _ => (16.0, 16.0),
        }
    }
}

pub struct Group {
    pub parts: Vec<ID2D1Geometry>,
    /// Gövde dolgusunun opaklığı (dönen yayın izi soluk).
    pub fill: f32,
    /// Gövdenin üstüne tam opak dolgu (dönen yay).
    pub accent: Option<ID2D1Geometry>,
    /// Kenar renginde ince çizgiler (parmak araları, kalem bandı).
    pub lines: Option<ID2D1Geometry>,
    /// Kenar renginde yazı ve kutusu (yardım "?").
    pub text: Option<(&'static str, D2D_RECT_F)>,
    /// Gövde degradesi yerine düz renk (Zen sakurası).
    pub solid: Option<(f32, f32, f32)>,
}

impl Group {
    fn new(parts: Vec<ID2D1Geometry>) -> Self {
        Self { parts, fill: 1.0, accent: None, lines: None, text: None, solid: None }
    }
}

const ARROW: &str = "M4 2.5 L4 22.5 L9.2 17.8 L12.8 26 L16.4 24.5 L12.9 16.5 L20 16.3 Z";
const FINGERS: [(f32, f32, f32, f32, f32); 4] =
    [(10.4, 2.0, 4.2, 15.0, 2.1), (14.4, 9.2, 3.9, 8.5, 1.95), (18.1, 10.2, 3.7, 8.0, 1.85), (21.6, 11.6, 3.3, 7.0, 1.65)];
const PALM: &str = "M10.4 14 L24.9 14 L24.9 21 C24.9 25.5 22.5 28.5 18.6 28.5 L15.2 28.5 C12.6 28.5 11.4 27.3 10.2 25.6 L6.2 19.8 C5.3 18.5 5.6 17.1 6.8 16.6 C7.9 16.1 9 16.6 9.7 17.6 L10.4 18.6 Z";
const CREASES: &str = "M14.5 13.2 L14.5 17 M18.2 14.2 L18.2 17.6 M21.7 15.2 L21.7 18.2";
const DOUBLE: &str = "M16 3 L22.5 10 L18.3 10 L18.3 22 L22.5 22 L16 29 L9.5 22 L13.7 22 L13.7 10 L9.5 10 Z";
const MOVE: &str = "M16 2.5 L21 8 L17.6 8 L17.6 24 L21 24 L16 29.5 L11 24 L14.4 24 L14.4 8 L11 8 Z";
const UP: &str = "M16 3 L23.5 11.5 L18.6 11.5 L18.6 29 L13.4 29 L13.4 11.5 L8.5 11.5 Z";
const PEN_TIP: &str = "M13.4 19.5 L18.6 19.5 L16 26.5 Z";
const PIN: &str = "M24 30 C24 30 19 24.6 19 21 C19 18.2 21.2 16 24 16 C26.8 16 29 18.2 29 21 C29 24.6 24 30 24 30 Z";
const SHOULDERS: &str = "M18.5 29.5 C18.5 25.8 20.8 23.5 24 23.5 C27.2 23.5 29.5 25.8 29.5 29.5 Z";

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Bézier yaklaşımıyla çember (yol verisi). Aynı yolda iki çember halka olur.
fn circle(cx: f32, cy: f32, r: f32) -> String {
    let k = r * 0.5523;
    format!(
        "M {} {cy} C {} {} {} {} {cx} {} C {} {} {} {} {} {cy} C {} {} {} {} {cx} {} C {} {} {} {} {} {cy} Z ",
        cx + r,
        cx + r,
        cy + k,
        cx + k,
        cy + r,
        cy + r,
        cx - k,
        cy + r,
        cx - r,
        cy + k,
        cx - r,
        cx - r,
        cy - k,
        cx - k,
        cy - r,
        cy - r,
        cx + k,
        cy - r,
        cx + r,
        cy - k,
        cx + r
    )
}

/// Halkanın `a0`'dan başlayıp `sweep` derece süren dilimi (yol verisi).
fn sector(cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, sweep: f32) -> String {
    let steps = (sweep / 6.0).ceil() as i32;
    let p = |r: f32, a: f32| {
        let t = a.to_radians();
        (cx + r * t.cos(), cy + r * t.sin())
    };
    let mut s = String::new();
    for i in 0..=steps {
        let (x, y) = p(r1, a0 + sweep * i as f32 / steps as f32);
        s += &format!("{} {x} {y} ", if i == 0 { "M" } else { "L" });
    }
    for i in (0..=steps).rev() {
        let (x, y) = p(r0, a0 + sweep * i as f32 / steps as f32);
        s += &format!("L {x} {y} ");
    }
    s + "Z"
}

/// SVG yol verisinin M/L/C/Z alt kümesi. Çakışan figürler (alternatif dolgu) delik açar.
pub fn path(f: &ID2D1Factory, d: &str, closed: bool) -> Result<ID2D1Geometry> {
    unsafe {
        let g = f.CreatePathGeometry()?;
        let sink = g.Open()?;
        let mut toks = d.split_whitespace().flat_map(|t| {
            let (head, rest) = t.split_at(if t.starts_with(|c: char| c.is_ascii_alphabetic()) { 1 } else { 0 });
            [head, rest].into_iter().filter(|s| !s.is_empty())
        });
        let num = |toks: &mut dyn Iterator<Item = &str>| toks.next().and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let mut open = false;
        let (begin, end) = if closed { (D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED) } else { (D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN) };
        while let Some(t) = toks.next() {
            match t {
                "M" => {
                    if open {
                        sink.EndFigure(end);
                    }
                    let (x, y) = (num(&mut toks), num(&mut toks));
                    sink.BeginFigure(pt(x, y), begin);
                    open = true;
                }
                "L" => {
                    let (x, y) = (num(&mut toks), num(&mut toks));
                    sink.AddLine(pt(x, y));
                }
                "C" => {
                    let v: Vec<f32> = (0..6).map(|_| num(&mut toks)).collect();
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(v[0], v[1]), point2: pt(v[2], v[3]), point3: pt(v[4], v[5]) });
                }
                "Z" => {
                    sink.EndFigure(end);
                    open = false;
                }
                _ => {}
            }
        }
        if open {
            sink.EndFigure(end);
        }
        sink.Close()?;
        g.cast()
    }
}

fn rrect(f: &ID2D1Factory, x: f32, y: f32, w: f32, h: f32, r: f32) -> Result<ID2D1Geometry> {
    let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, radiusX: r, radiusY: r };
    unsafe { f.CreateRoundedRectangleGeometry(&rr)?.cast() }
}

/// (cx, cy) çevresinde saat yönünde `deg` derece döndürüp (dx, dy) kaydırır.
fn moved(f: &ID2D1Factory, g: ID2D1Geometry, deg: f32, (cx, cy): (f32, f32), (dx, dy): (f32, f32)) -> Result<ID2D1Geometry> {
    let (s, c) = deg.to_radians().sin_cos();
    let m = Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: cx - cx * c + cy * s + dx, M32: cy - cx * s - cy * c + dy };
    unsafe { f.CreateTransformedGeometry(&g, &m)?.cast() }
}

const MID: (f32, f32) = (16.0, 16.0);

/// Zen: okun içindeki mürekkep dalgası (ok koordinatlarında).
const ARROW_WAVES: &str = "M5.2 15.6 C7 14.2 9 16.6 11.4 15.2 C12.6 14.5 13.6 14.8 14.6 15.6 M5.2 11.4 C6.5 10.5 7.9 11.9 9.6 11.1";
const HAND_WAVES: &str = " M11.8 22.6 C13.6 21.2 15.6 23.6 18 22.2 C20 21 22 22.6 24.2 21.6";

fn arrow(f: &ID2D1Factory, zen: bool) -> Result<Group> {
    let mut g = Group::new(vec![moved(f, path(f, ARROW, true)?, 0.0, MID, (1.5, 1.5))?]);
    if zen {
        g.lines = Some(moved(f, path(f, ARROW_WAVES, false)?, 0.0, MID, (1.5, 1.5))?);
    }
    Ok(g)
}

/// Beş taç yapraklı sakura (uçları çentikli); ortası kenar renginde halka.
fn sakura(f: &ID2D1Factory, cx: f32, cy: f32, r: f32) -> Result<Group> {
    let p = |x: f32, y: f32| format!("{} {}", cx + x * r, cy + y * r);
    let petal = format!(
        "M {} C {} {} {} L {} L {} C {} {} {} Z",
        p(0.0, 0.0),
        p(-0.44, -0.2),
        p(-0.5, -0.78),
        p(-0.2, -1.0),
        p(0.0, -0.84),
        p(0.2, -1.0),
        p(0.5, -0.78),
        p(0.44, -0.2),
        p(0.0, 0.0)
    );
    let mut parts = Vec::new();
    for i in 0..5 {
        parts.push(moved(f, path(f, &petal, true)?, i as f32 * 72.0, (cx, cy), (0.0, 0.0))?);
    }
    let mut g = Group::new(parts);
    g.solid = Some((1.0, 1.0, 1.0));
    g.lines = Some(path(f, &circle(cx, cy, r * 0.2), false)?);
    Ok(g)
}

/// Zen sakurasının role göre yeri (yoksa None).
fn sakura_at(role: Role) -> Option<(f32, f32, f32)> {
    match role {
        Role::Arrow | Role::Help | Role::Working | Role::Pin | Role::Person => Some((9.8, 20.8, 3.3)),
        Role::Hand => Some((14.8, 24.8, 3.3)),
        Role::Busy => Some((23.6, 23.6, 3.3)),
        Role::No => Some((24.0, 24.0, 3.1)),
        Role::Text => Some((19.6, 12.4, 2.5)),
        _ => None,
    }
}

/// Dönen yay: soluk halka izi + `frame`'e göre konumlanan parlak dilim.
fn spinner(f: &ID2D1Factory, cx: f32, cy: f32, r: f32, t: f32, frame: u32) -> Result<Group> {
    let (r0, r1) = (r - t / 2.0, r + t / 2.0);
    let ring = path(f, &(circle(cx, cy, r1) + &circle(cx, cy, r0)), true)?;
    let a0 = frame as f32 * 360.0 / FRAMES as f32 - 90.0;
    let mut g = Group::new(vec![ring]);
    g.fill = 0.3;
    g.accent = Some(path(f, &sector(cx, cy, r0, r1, a0, 120.0), true)?);
    Ok(g)
}

/// `zen`: okta/elde mürekkep dalgası ve rol uygunsa sakura eklenir.
pub fn build(f: &ID2D1Factory, role: Role, frame: u32, zen: bool) -> Result<Vec<Group>> {
    let mut groups = shape(f, role, frame, zen)?;
    if let (true, Some((x, y, r))) = (zen, sakura_at(role)) {
        // Rozetli rollerde (?, konum, kişi, yay) sakura okun üstünde, rozetin altında kalsın.
        let at = if groups.len() > 1 { 1 } else { groups.len() };
        groups.insert(at, sakura(f, x, y, r)?);
    }
    Ok(groups)
}

fn shape(f: &ID2D1Factory, role: Role, frame: u32, zen: bool) -> Result<Vec<Group>> {
    let one = |parts: Vec<ID2D1Geometry>| Ok(vec![Group::new(parts)]);
    let arrow = |f: &ID2D1Factory| arrow(f, zen);
    match role {
        Role::Arrow => Ok(vec![arrow(f)?]),
        Role::Help => {
            let mut badge = Group::new(vec![path(f, &circle(24.0, 23.0, 5.4), true)?]);
            badge.text = Some(("?", D2D_RECT_F { left: 18.0, top: 17.2, right: 30.0, bottom: 29.0 }));
            Ok(vec![arrow(f)?, badge])
        }
        Role::Working => Ok(vec![arrow(f)?, spinner(f, 24.0, 23.0, 4.6, 2.8, frame)?]),
        Role::Busy => Ok(vec![spinner(f, 16.0, 16.0, 10.0, 4.2, frame)?]),
        Role::Cross => one(vec![
            rrect(f, 14.9, 3.5, 2.2, 9.0, 1.1)?,
            rrect(f, 14.9, 19.5, 2.2, 9.0, 1.1)?,
            rrect(f, 3.5, 14.9, 9.0, 2.2, 1.1)?,
            rrect(f, 19.5, 14.9, 9.0, 2.2, 1.1)?,
            path(f, &circle(16.0, 16.0, 1.3), true)?,
        ]),
        Role::Text => one(vec![rrect(f, 14.9, 6.5, 2.2, 19.0, 1.1)?, rrect(f, 11.2, 5.0, 9.6, 2.4, 1.2)?, rrect(f, 11.2, 24.6, 9.6, 2.4, 1.2)?]),
        Role::Pen => {
            let mut g = Group::new(vec![moved(f, rrect(f, 13.4, 3.0, 5.2, 17.0, 1.6)?, 45.0, MID, (0.0, 0.0))?, moved(f, path(f, PEN_TIP, true)?, 45.0, MID, (0.0, 0.0))?]);
            g.lines = Some(moved(f, path(f, "M13.8 19.5 L18.2 19.5 M13.8 7 L18.2 7", false)?, 45.0, MID, (0.0, 0.0))?);
            Ok(vec![g])
        }
        Role::No => one(vec![
            path(f, &(circle(16.0, 16.0, 11.0) + &circle(16.0, 16.0, 7.6)), true)?,
            moved(f, rrect(f, 14.4, 6.0, 3.2, 20.0, 1.6)?, -45.0, MID, (0.0, 0.0))?,
        ]),
        Role::SizeNS => one(vec![path(f, DOUBLE, true)?]),
        Role::SizeWE => one(vec![moved(f, path(f, DOUBLE, true)?, 90.0, MID, (0.0, 0.0))?]),
        Role::SizeNWSE => one(vec![moved(f, path(f, DOUBLE, true)?, -45.0, MID, (0.0, 0.0))?]),
        Role::SizeNESW => one(vec![moved(f, path(f, DOUBLE, true)?, 45.0, MID, (0.0, 0.0))?]),
        Role::Move => one(vec![path(f, MOVE, true)?, moved(f, path(f, MOVE, true)?, 90.0, MID, (0.0, 0.0))?]),
        Role::Up => one(vec![path(f, UP, true)?]),
        Role::Hand => {
            let d = (0.5, 1.0);
            let mut parts = Vec::new();
            for (x, y, w, h, r) in FINGERS {
                parts.push(moved(f, rrect(f, x, y, w, h, r)?, 0.0, MID, d)?);
            }
            parts.push(moved(f, path(f, PALM, true)?, 0.0, MID, d)?);
            let mut g = Group::new(parts);
            let lines = if zen { format!("{CREASES}{HAND_WAVES}") } else { CREASES.to_string() };
            g.lines = Some(moved(f, path(f, &lines, false)?, 0.0, MID, d)?);
            Ok(vec![g])
        }
        Role::Pin => Ok(vec![arrow(f)?, Group::new(vec![path(f, &(PIN.to_string() + " " + &circle(24.0, 21.0, 1.9)), true)?])]),
        Role::Person => Ok(vec![arrow(f)?, Group::new(vec![path(f, &circle(24.0, 19.0, 3.0), true)?, path(f, SHOULDERS, true)?])]),
    }
}
