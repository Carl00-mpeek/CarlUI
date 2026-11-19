//! Pil sekmesi: dock'un üst kenarında, saatin üstünde küçük bir çıkıntı. İmleç
//! üstüne gelince dock'un arkasından yukarı doğru bir cam panel kayar; şarjlı
//! fare ve kulaklığın simgeleri, üstlerinde pil yüzdeleriyle görünür.
//!
//! - Altta küçük bir not: "uykuda" / "kapalı" (son bilinen değer soluk),
//!   "şarjda", "dolu" ya da boşalma hızından kalan süre tahmini ("~6 sa").
//! - Pil %15'in altına inince çıkıntı kırmızılaşır ve panel bir kez birkaç
//!   saniyeliğine kendiliğinden açılır.
//! - Şarjdayken şimşek yavaşça yanıp söner; dolunca onay işaretine döner.
//! - Ayarlardan gizlenen cihazlar (`dock.battery_hidden`) gösterilmez.
//!
//! Okuma (`carlui_sys::battery`) yavaştır, ayrı bir iş parçacığında yapılır:
//! dakikada bir ve panel açılırken (veri eskiyse).
//!
//! Ayrı pencere kullanılır; tıklanabilir bölgesi kapalıyken yalnızca çıkıntıdır.

use carlui_core::{info, t, theme::{Rgba, Theme}};
use carlui_sys::battery::{Kind, Reading};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3, Glass},
    motion::{ts, Motion},
    text::Text,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
/// Düşük pil uyarısıyla kendiliğinden açılan panel kapanır.
pub const TIMER_PEEK: usize = 43;

const FOLLOW_MS: u32 = 200;
const PEEK_MS: u32 = 4000;
const POLL: Duration = Duration::from_secs(60);
/// Panel açılırken veri bundan eskiyse yeniden okunur.
const FRESH: Duration = Duration::from_secs(8);
/// Cevap vermeyen (uyuyan / kapalı) cihazın son değeri bu kadar süre gösterilir.
const KEEP: Duration = Duration::from_secs(3 * 3600);
/// Bu yüzde ve altı "düşük".
const LOW: u8 = 15;

const G_MOUSE: char = '\u{E962}';
const G_HEADSET: char = '\u{E7F6}';
const G_BT: char = '\u{E702}';
const G_BOLT: char = '\u{E945}';
const G_CHECK: char = '\u{E73E}';

const RED: Rgba = Rgba(0xFF, 0x3B, 0x30, 210);

struct Shared {
    readings: Vec<Reading>,
    at: Option<Instant>,
}

/// Bir cihazın son bilinen durumu ve süren boşalma dönemi.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Last {
    pct: u8,
    charging: bool,
    at: SystemTime,
    /// Şarjdan çıkıştan beri: (başlangıç yüzdesi, başlangıç anı).
    run: Option<(u8, SystemTime)>,
}

/// Panelde gösterilen bir cihaz.
#[derive(Clone, Debug, PartialEq)]
struct Item {
    kind: Kind,
    pct: Option<u8>,
    charging: bool,
    /// Cihaz şu an cevap vermiyor; değer son bilinendir.
    stale: bool,
    note: String,
}

impl Item {
    fn low(&self) -> bool {
        self.pct.is_some_and(|p| p <= LOW) && !self.charging
    }
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
    shown: Vec<Item>,
    /// Son bilinen değerler: uyuyan fare "bilinmiyor" görünmesin.
    last: HashMap<String, Last>,
    /// Ayarlardan gizlenen cihaz adları.
    hidden: Vec<String>,
    /// Düşük pil için panelin zaten kendiliğinden açıldığı cihazlar.
    warned: HashSet<String>,
    peek_due: bool,
    red: bool,
}

fn glyph_of(k: Kind) -> char {
    match k {
        Kind::Mouse => G_MOUSE,
        Kind::Headset => G_HEADSET,
        Kind::Other => G_BT,
    }
}

/// Boşalma hızından kalan süre: en az 20 dk ve %3 düşüş gözlenmiş olmalı.
fn estimate(run: (u8, SystemTime), pct: u8, now: SystemTime) -> Option<Duration> {
    let drop = run.0.checked_sub(pct).filter(|&d| d >= 3)?;
    let elapsed = now.duration_since(run.1).ok().filter(|e| *e >= Duration::from_secs(20 * 60))?;
    Some(Duration::from_secs(elapsed.as_secs() * pct as u64 / drop as u64))
}

fn fmt_left(d: Duration) -> String {
    let m = d.as_secs() / 60;
    if m < 60 {
        format!("~{} {}", m.max(1), t!("dk", "min"))
    } else if m < 48 * 60 {
        format!("~{} {}", (m + 30) / 60, t!("sa", "h"))
    } else {
        format!("~{} {}", (m + 12 * 60) / (24 * 60), t!("gün", "d"))
    }
}

/// Yeni (taze) okuma: son değeri ve boşalma dönemini günceller.
fn advance(prev: Option<Last>, pct: u8, charging: bool, now: SystemTime) -> Last {
    let run = match prev {
        _ if charging => None,
        Some(Last { run: Some((p0, t0)), at, .. }) if pct <= p0 + 1 => {
            // Cihaz uzun süre sustuysa (kapalı / uykuda) o süre boşalmaya sayılmaz.
            let gap = now.duration_since(at).unwrap_or_default();
            let idle = gap.saturating_sub(POLL * 2);
            Some((p0, t0 + idle))
        }
        _ => Some((pct, now)),
    };
    Last { pct, charging, at: now, run }
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
            last: load_last(),
            hidden: Vec::new(),
            warned: HashSet::new(),
            peek_due: false,
            red: false,
        })
    }

    /// Ayarlardan gizlenen cihazlar değişti: panel yeniden kurulur.
    pub fn set_hidden(&mut self, hidden: Vec<String>) {
        if hidden != self.hidden {
            self.hidden = hidden;
            unsafe {
                let _ = PostMessageW(self.hwnd, WM_APP_BATTERY, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Okunan değerleri son bilinenlerle birleştirir.
    fn merged(&mut self) -> Vec<Item> {
        let readings = self.shared.lock().map(|s| s.readings.clone()).unwrap_or_default();
        let now = SystemTime::now();
        let mut changed = false;
        let mut out = Vec::new();
        for r in readings.into_iter().filter(|r| !self.hidden.contains(&r.name)).take(4) {
            let item = match r.percent {
                Some(p) => {
                    let prev = self.last.get(&r.name).copied();
                    let cur = advance(prev, p, r.charging, now);
                    changed |= prev.map_or(true, |l| (l.pct, l.charging, l.run.map(|x| x.0)) != (p, r.charging, cur.run.map(|x| x.0)));
                    self.last.insert(r.name.clone(), cur);
                    let note = if r.charging && p >= 100 {
                        t!("dolu", "full").to_string()
                    } else if r.charging {
                        t!("şarjda", "charging").to_string()
                    } else if let Some(left) = cur.run.and_then(|run| estimate(run, p, now)) {
                        fmt_left(left)
                    } else if p <= LOW {
                        t!("düşük", "low").to_string()
                    } else {
                        String::new()
                    };
                    Item { kind: r.kind, pct: Some(p), charging: r.charging, stale: false, note }
                }
                // Cevap yok: son bilinen değer soluk gösterilir.
                None => {
                    let recent = self.last.get(&r.name).filter(|l| now.duration_since(l.at).map_or(true, |d| d < KEEP));
                    let note = if r.kind == Kind::Headset { t!("kapalı", "off") } else { t!("uykuda", "asleep") }.to_string();
                    Item { kind: r.kind, pct: recent.map(|l| l.pct), charging: false, stale: true, note }
                }
            };
            // Düşük pil: cihaz başına bir kez paneli kısa süre aç.
            if item.low() && !item.stale {
                if self.warned.insert(r.name.clone()) {
                    self.peek_due = true;
                }
            } else if item.charging || item.pct.is_some_and(|p| p > LOW + 5) {
                self.warned.remove(&r.name);
            }
            out.push(item);
        }
        if changed {
            save_last(&self.last);
        }
        out
    }

    /// Yeni okuma geldi: panel içeriği yeniden kurulur.
    pub fn update(&mut self, gfx: &Gfx) {
        let items = self.merged();
        let red = items.iter().any(Item::low);
        if red != self.red {
            self.red = red;
            let _ = self.nub.set_mood(&self.comp, red.then_some(RED), 400);
        }
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

    fn sprite(&self, surface: &CompositionDrawingSurface, size: (f32, f32), at: (f32, f32)) -> Result<SpriteVisual> {
        let v = self.comp.CreateSpriteVisual()?;
        v.SetSize(v2(size.0, size.1))?;
        v.SetBrush(&self.comp.CreateSurfaceBrushWithSurface(surface)?)?;
        v.SetOffset(v3(at.0.round(), at.1.round()))?;
        self.panel.content.Children()?.InsertAtTop(&v)?;
        Ok(v)
    }

    /// Şarj şimşeği: yavaş, sonsuz bir nefes alıp verme.
    fn breathe(&self, v: &SpriteVisual) -> Result<()> {
        let a = self.comp.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, 1.0)?;
        a.InsertKeyFrame(0.5, 0.3)?;
        a.InsertKeyFrame(1.0, 1.0)?;
        a.SetDuration(ts(1800))?;
        a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        v.StartAnimation(h!("Opacity"), &a)
    }

    fn build(&mut self, gfx: &Gfx) -> Result<()> {
        let s = self.s;
        self.panel.content.Children()?.RemoveAll()?;
        let n = self.shown.len().max(1) as f32;
        let (col, pad) = ((58.0 * s).round(), (8.0 * s).round());
        let (txt_h, icon_px, gap) = ((18.0 * s).round(), (22.0 * s).round(), (3.0 * s).round());
        let notes = self.shown.iter().any(|i| !i.note.is_empty());
        let note_h = if notes { (14.0 * s).round() } else { 0.0 };
        let w = col * n + pad * 2.0;
        let h = pad + txt_h + gap + icon_px + note_h + pad;
        for (i, it) in self.shown.iter().enumerate() {
            let x0 = pad + i as f32 * col;
            let (label, alpha) = match it.pct {
                Some(p) => (carlui_core::lang::pct(p), if it.stale { 0.5 } else { 0.95 }),
                None => ("—".to_string(), 0.5),
            };
            let t = self.text.render(gfx, &label, 13.0 * s, true, col, alpha)?;
            self.sprite(&t.surface, (t.width, t.height), (x0 + (col - t.width) / 2.0, pad + (txt_h - t.height) / 2.0))?;

            let g = self.text.glyph(gfx, glyph_of(it.kind), icon_px * 0.85, if it.stale || it.pct.is_none() { 0.45 } else { 0.95 })?;
            let gy = (pad + txt_h + gap + (icon_px - g.height) / 2.0).round();
            self.sprite(&g.surface, (g.width, g.height), (x0 + (col - g.width) / 2.0, gy))?;
            if it.charging {
                let full = it.pct.is_some_and(|p| p >= 100);
                let b = self.text.glyph(gfx, if full { G_CHECK } else { G_BOLT }, 10.0 * s, 0.95)?;
                let bv = self.sprite(&b.surface, (b.width, b.height), (x0 + (col + g.width) / 2.0, gy))?;
                if !full {
                    self.breathe(&bv)?;
                }
            }
            if !it.note.is_empty() {
                let nt = self.text.render(gfx, &it.note, 10.0 * s, false, col, if it.low() { 0.95 } else { 0.6 })?;
                let ny = pad + txt_h + gap + icon_px + (note_h - nt.height) / 2.0;
                self.sprite(&nt.surface, (nt.width, nt.height), (x0 + (col - nt.width) / 2.0, ny))?;
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
    /// `at`: sekmenin yatay ortası (saatin üstü). Ortada durunca uygulama
    /// simgelerine giderken imleç değip panel istemeden açılıyordu.
    pub fn follow(&mut self, pill: Option<RECT>, at: Option<i32>) {
        let want = pill.filter(|_| !self.shown.is_empty() && self.win.0 > 0).map(|r| {
            let cx = at.filter(|x| (r.left..=r.right).contains(x)).unwrap_or((r.left + r.right) / 2);
            (cx - self.win.0 / 2, r.top - self.win.1)
        });
        if want != self.pos {
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
        // Düşük pil: dock görünürken panel birkaç saniye kendiliğinden açılır.
        if self.peek_due && self.pos.is_some() {
            self.peek_due = false;
            if !self.open {
                self.show_panel();
                unsafe {
                    SetTimer(self.hwnd, TIMER_PEEK, PEEK_MS, None);
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

    fn show_panel(&mut self) {
        self.open = true;
        self.set_region();
        let _ = self.panel.root.SetOpacity(1.0);
        let _ = self.motion.animate(&self.panel.root, h!("Offset.Y"), self.open_y(), 0.75, 60, 0);
        let _ = self.motion.fade(&self.nub.root, h!("Opacity"), 0.0, 90, 0);
    }

    pub fn enter(&mut self) {
        self.mouse_inside = true;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FOLD);
            let _ = KillTimer(self.hwnd, TIMER_PEEK);
        }
        if self.open {
            return;
        }
        let stale = self.shared.lock().map(|s| s.at.is_none_or(|t| t.elapsed() > FRESH)).unwrap_or(true);
        if stale {
            let _ = self.ask.send(());
        }
        self.show_panel();
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

    /// Kendiliğinden açılan panelin süresi doldu (imleç üstünde değilse kapanır).
    pub fn end_peek(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_PEEK);
        }
        if !self.mouse_inside {
            self.leave();
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
            let _ = KillTimer(self.hwnd, TIMER_PEEK);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

// ---- son bilinen değerler ------------------------------------------------------
// Dock yeniden başlayınca uyuyan fare "—" görünmesin, kalan süre tahmini de
// sıfırlanmasın diye diske yazılır. Her satır:
// `ad<TAB>yüzde<TAB>şarjda<TAB>unix_saniye[<TAB>dönem_yüzdesi<TAB>dönem_saniyesi]`.
// Ayarlar uygulaması gizlenebilecek cihazları bu dosyadan listeler.

fn last_path() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("battery.txt")
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn parse_last(text: &str) -> HashMap<String, Last> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?.to_string();
            let pct: u8 = it.next()?.parse().ok()?;
            let charging = it.next()? == "1";
            let at = UNIX_EPOCH + Duration::from_secs(it.next()?.parse().ok()?);
            let run = (|| {
                let p: u8 = it.next()?.parse().ok()?;
                let s: u64 = it.next()?.parse().ok()?;
                Some((p, UNIX_EPOCH + Duration::from_secs(s)))
            })();
            (pct <= 100).then_some((name, Last { pct, charging, at, run }))
        })
        .collect()
}

fn format_last(last: &HashMap<String, Last>) -> String {
    let mut names: Vec<&String> = last.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let l = &last[n];
            let run = l.run.map(|(p, t)| format!("\t{p}\t{}", secs(t))).unwrap_or_default();
            format!("{}\t{}\t{}\t{}{run}\n", n.replace(['\t', '\n'], " "), l.pct, l.charging as u8, secs(l.at))
        })
        .collect()
}

fn load_last() -> HashMap<String, Last> {
    parse_last(&std::fs::read_to_string(last_path()).unwrap_or_default())
}

fn save_last(last: &HashMap<String, Last>) {
    let _ = std::fs::write(last_path(), format_last(last));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(min: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000 + min * 60)
    }

    #[test]
    fn kalan_sure() {
        // 60 dakikada %80 → %70: kalan %70 yaklaşık 7 saat.
        assert_eq!(estimate((80, at(0)), 70, at(60)), Some(Duration::from_secs(7 * 3600)));
        // Az veri: tahmin yok.
        assert_eq!(estimate((80, at(0)), 79, at(60)), None);
        assert_eq!(estimate((80, at(0)), 70, at(10)), None);
    }

    #[test]
    fn donem() {
        let a = advance(None, 90, false, at(0));
        assert_eq!(a.run, Some((90, at(0))));
        let b = advance(Some(a), 85, false, at(1));
        assert_eq!(b.run, Some((90, at(0))));
        // Uzun sessizlik boşalmaya sayılmaz: başlangıç ileri kayar.
        let c = advance(Some(b), 84, false, at(121));
        assert_eq!(c.run, Some((90, at(118))));
        // Şarj dönemi bitirir; şarjdan çıkınca yeni dönem.
        let d = advance(Some(c), 86, true, at(122));
        assert_eq!(d.run, None);
        assert_eq!(advance(Some(d), 100, false, at(200)).run, Some((100, at(200))));
    }

    #[test]
    fn dosya_bicimi() {
        let mut m = HashMap::new();
        m.insert("Fare".to_string(), Last { pct: 50, charging: false, at: at(5), run: Some((60, at(0))) });
        m.insert("Kulak".to_string(), Last { pct: 20, charging: true, at: at(5), run: None });
        assert_eq!(parse_last(&format_last(&m)), m);
        // Eski biçim (4 sütun) de okunur.
        assert_eq!(parse_last("X\t40\t0\t100\n")["X"].run, None);
    }

    #[test]
    fn sure_yazimi() {
        assert!(fmt_left(Duration::from_secs(40 * 60)).contains("40"));
        assert!(fmt_left(Duration::from_secs(6 * 3600)).contains('6'));
        assert!(fmt_left(Duration::from_secs(3 * 86400)).contains('3'));
    }
}
