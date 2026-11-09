//! Bu modül dock'un görünümünü ve etkileşimini çözüyor: ekranın altında,
//! kenardan biraz yukarıda yüzen bir cam pill.
//!
//! - Sıra: Windows tuşu, arama | dosya yöneticisi, sabitlenmişler | açık
//!   uygulamalar | tray düğmesi.
//! - Her öğe kendi "slot" görselindedir ve konumu dock'un ortasına göredir.
//!   Uygulama açılıp kapandıkça slotlar ve cam gövde spring ile yeni yerlerine
//!   akar; yeni öğe büyüyerek belirir, giden öğe küçülerek kaybolur.
//! - Pencere başına bir gösterge noktası; öndeki uygulamanın noktası vurgu
//!   renginde bir haptır.
//! - Tıklanabilir bölge yalnızca pill'in kendisidir; geri kalan her piksel
//!   tıklamayı alttaki pencereye geçirir.

//!
//! Dosyalar: `layout` (dizilim, bölge, noktalar, açılış, gizleme), `mouse`
//! (hover, tıklama, sürükle-bırak, sabitleme), `actions` (başlat/öne getir),
//! `places` (yığınlar, çöp kutusu), `status` (görev çubuğu görünümünde sağ uç:
//! ağ, ses, pil, saat).

mod actions;
mod layout;
mod mouse;
mod places;
mod status;
pub use places::{open as open_uri, TIMER_TRASH};
pub use status::{Part, TIMER_STATUS};

use crate::{
    apps::{self, AppId},
    tracker::{self, Tracker},
};
use carlui_core::{
    config::{expand_env, Config, DockBehavior, DockPosition, Pinned, TaskbarSize},
    theme::Theme,
    win,
};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3, Glass},
    icons,
    motion::Motion,
};
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::*,
            Shell::ShellExecuteW,
            WindowsAndMessaging::*,
        },
    },
};

pub const TIMER_REGION: usize = 11;
pub const TIMER_CLEANUP: usize = 12;
pub const TIMER_AUTOHIDE: usize = 13;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Start,
    Search,
    Tray,
    App,
    /// İndirilenler yığını (tıklayınca son dosyalar menüsü).
    Folder,
    /// Geri Dönüşüm Kutusu.
    Trash,
}

/// İkon kaynakları sırayla denenir; hiçbiri olmazsa pencerenin kendi ikonu.
enum IconSrc {
    Glyph,
    Sources(Vec<String>, Option<HWND>),
}

pub struct Entry {
    pub key: String,
    pub kind: Kind,
    pub group: u8,
    pub name: String,
    /// Sabitlenmiş öğeler için başlatma bilgisi (dosya yöneticisi dahil).
    pub pin: Option<Pinned>,
    pub app: AppId,
    /// Sabitlenmemiş açık uygulama için özgün yazımıyla exe yolu.
    pub exe: String,
    pub windows: Vec<HWND>,
    slot: ContainerVisual,
    icon: SpriteVisual,
    hl: ShapeVisual,
    dots: ContainerVisual,
    dots_sig: (usize, bool),
    /// Dock'un ortasına göre merkez konumu.
    x: f32,
    leaving: Option<Instant>,
    fresh: bool,
}

impl Entry {
    /// Gezgin girdisi mi? Anahtar her zaman `explorer_key` olmuyor (klasör
    /// pencereleri kendi kimliğiyle gelebiliyor); pencerelerden biri
    /// explorer.exe'ninse de Gezgin sayılır — süreci masaüstü ve görev
    /// çubuğuyla ortak, durdurulmamalı ve kapatılmamalı.
    pub fn is_explorer(&self) -> bool {
        self.key == explorer_key() || self.windows.iter().any(|&h| carlui_core::win::is_explorer_window(h))
    }
}

#[derive(Clone, Copy)]
struct Metrics {
    s: f32,
    icon: f32,
    slot: f32,
    h: f32,
    pad: f32,
    sep: f32,
    /// Pill'in üstünde hover büyümesi için bırakılan boşluk.
    top: f32,
    bottom: f32,
    radius: f32,
    hl: f32,
}

struct Drag {
    index: usize,
    /// İmlecin slot merkezine göre yatay farkı.
    grab: f32,
    /// Bırakılırsa sabitlenmişler içindeki hedef sıra (None: sabitleme alanının dışı).
    insert: Option<usize>,
}

pub struct Dock {
    hwnd: HWND,
    comp: Compositor,
    motion: Motion,
    pub theme: Theme,
    pub cfg: Config,
    glass: Glass,
    items: ContainerVisual,
    pub entries: Vec<Entry>,
    seps: Vec<SpriteVisual>,
    dot_brush: CompositionSurfaceBrush,
    active_brush: CompositionSurfaceBrush,
    /// Çizgi göstergesi fırçaları, `(n - 1) * 2 + önde` sırasıyla; noktalı modda boş.
    line_brushes: Vec<CompositionSurfaceBrush>,
    hl_brush: CompositionColorBrush,
    m: Metrics,
    mon: RECT,
    /// Yaslandığı kenar; dikeyde "boyuna" eksen Y'dir (bkz. `pv`, `ps`).
    side: DockPosition,
    /// Pencerenin ekrandaki sol-üst köşesi.
    wpos: (i32, i32),
    win_w: f32,
    win_h: f32,
    /// Pencerenin boyuna uzunluğu (yatayda genişlik, dikeyde yükseklik).
    len: f32,
    width: f32,
    region_w: f32,
    hovered: Option<usize>,
    press: Option<(usize, i32, i32)>,
    drag: Option<Drag>,
    suppressed: bool,
    /// Otomatik gizlemede aşağı kaymış durumda mı?
    pub slid_away: bool,
    pub mouse_inside: bool,
    pub menu_open: bool,
    /// Çöp kutusu son bakıldığında dolu muydu (simge değişince yenilenir).
    trash_full: Option<bool>,
    /// Görev çubuğu görünümünde sağ uçtaki ağ/ses/pil ve saat alanı.
    status: Option<status::Status>,
}

fn explorer_key() -> String {
    format!("exe:{}", expand_env(r"%WINDIR%\explorer.exe").to_lowercase())
}

fn pin_key(p: &Pinned) -> String {
    let id = AppId::of_pin(p);
    if p.path.to_lowercase().ends_with("explorer.exe") {
        return explorer_key();
    }
    if !id.aumid.is_empty() {
        format!("id:{}", id.aumid)
    } else if !id.exe.is_empty() {
        format!("exe:{}", id.exe)
    } else {
        format!("path:{}", expand_env(&p.path).to_lowercase())
    }
}

/// Dock penceresinin dikdörtgeni (ekran koordinatı), intellihide için.
fn window_bounds(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32)
            .ok()
            .map(|_| r)
    }
}

impl Dock {
    /// `screen`: dock'un durduğu monitörün dikdörtgeni (ana dock'ta ana monitör).
    pub fn new(gfx: &Gfx, hwnd: HWND, target: &DesktopWindowTarget, cfg: &Config, tracker: &Tracker, screen: RECT) -> Result<Self> {
        let theme = Theme::from_config(&cfg.theme);
        let s = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
        let icon = (cfg.dock.icon_size.clamp(20.0, 64.0) * s).round();
        let k = icon / (30.0 * s); // diğer ölçüler ikon boyutuyla orantılı
        let taskbar = cfg.dock.taskbar && cfg.dock.position == DockPosition::Bottom;
        let mut m = Metrics {
            s,
            icon,
            slot: (50.0 * s * k).round(),
            h: (62.0 * s * k).round(),
            pad: (10.0 * s * k).round(),
            sep: (14.0 * s).round(),
            top: (14.0 * s * k).round(),
            bottom: (cfg.dock.bottom_margin.clamp(0.0, 200.0) * s).round(),
            radius: (24.0 * s * k * theme.radius_scale).round(),
            hl: (44.0 * s * k).round(),
        };
        let tsize = cfg.dock.taskbar_size;
        if taskbar {
            // Görev çubuğu: simgeler arası daha dar. Full kenara yapışık ve köşesiz;
            // Compact ortada yüzer, sağı solu kesik.
            m.slot = (42.0 * s * k).round();
            m.h = (50.0 * s * k).round().max((44.0 * s).round());
            m.pad = (8.0 * s).round();
            m.sep = (10.0 * s).round();
            m.hl = (38.0 * s * k).round();
            if tsize == TaskbarSize::Full {
                m.bottom = 0.0;
                m.radius = 0.0;
            } else {
                m.radius = (14.0 * s * k * theme.radius_scale).round();
            }
        }
        let comp = gfx.comp.clone();
        let motion = Motion::new(&comp, &theme);

        // Monitörün tamamı: görev çubuğu gizliyken çalışma alanı da tam ekran.
        let mon = screen;
        let side = cfg.dock.position;
        let thick = m.top + m.h + m.bottom;
        let (mw, mh) = ((mon.right - mon.left) as f32, (mon.bottom - mon.top) as f32);
        let (wpos, win_w, win_h, len) = match side {
            DockPosition::Bottom => ((mon.left, mon.bottom - thick as i32), mw, thick, mw),
            DockPosition::Left => ((mon.left, mon.top), thick, mh, mh),
            DockPosition::Right => ((mon.right - thick as i32, mon.top), thick, mh, mh),
        };
        unsafe {
            SetWindowPos(hwnd, HWND_TOPMOST, wpos.0, wpos.1, win_w as i32, win_h as i32, SWP_NOACTIVATE)?;
        }
        let vert = side.vertical();
        // Pill'in enine konumu: altta ve sağda üst/iç boşluktan sonra, solda kenar boşluğundan sonra.
        let c0 = if side == DockPosition::Left { m.bottom } else { m.top };

        let root = comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;

        let glass = Glass::new(&comp, &theme, m.radius)?;
        // Ortadan büyüsün diye anchor boyuna eksende ortada.
        if vert {
            glass.root.SetAnchorPoint(v2(0.0, 0.5))?;
            glass.root.SetOffset(v3(c0, len / 2.0))?;
        } else {
            glass.root.SetAnchorPoint(v2(0.5, 0.0))?;
            glass.root.SetOffset(v3(len / 2.0, c0))?;
        }
        glass.root.SetSize(v2(m.h, m.h))?;
        root.Children()?.InsertAtTop(&glass.root)?;

        // Öğe katmanı gövdenin ortasına bağlı; slotların konumu ortaya göre.
        let items = comp.CreateContainerVisual()?;
        let expr = HSTRING::from(if vert { "Vector3(0, r.Size.Y / 2, 0)" } else { "Vector3(r.Size.X / 2, 0, 0)" });
        let center = comp.CreateExpressionAnimationWithExpression(&expr)?;
        center.SetReferenceParameter(h!("r"), &glass.root)?;
        items.StartAnimation(h!("Offset"), &center)?;
        glass.content.Children()?.InsertAtTop(&items)?;

        let (dw, dh) = ((4.0 * s).round(), (4.0 * s).round());
        let dot = gfx.surface(dw, dh, |dc| icons::draw_dot(dc, dw, dh, None))?;
        let (aw, ah) = ((14.0 * s).round(), (4.0 * s).round());
        let (aw, ah) = if vert { (ah, aw) } else { (aw, ah) };
        let (a0, a1) = theme.accent;
        let accent = gfx.surface(aw, ah, |dc| icons::draw_dot(dc, aw, ah, Some(((a0.0, a0.1, a0.2), (a1.0, a1.1, a1.2)))))?;
        let dot_brush = comp.CreateSurfaceBrushWithSurface(&dot)?;
        let active_brush = comp.CreateSurfaceBrushWithSurface(&accent)?;
        let hl_brush = comp.CreateColorBrushWithColor(carlui_ui::glass::exact(theme.hover))?;
        // Çizgi göstergesi: 1-4 pencere x (arka plan, önde) için hazır yüzeyler.
        let mut line_brushes = Vec::new();
        if cfg.dock.line_indicator {
            let (len, thick) = ((26.0 * s).round(), (3.0 * s).round().max(2.0));
            let (lw, lh) = if vert { (thick, len) } else { (len, thick) };
            for n in 1..=4usize {
                for active in [false, true] {
                    let acc = active.then_some(((a0.0, a0.1, a0.2), (a1.0, a1.1, a1.2)));
                    let sf = gfx.surface(lw, lh, |dc| icons::draw_split_line(dc, len, thick, n, acc, vert))?;
                    line_brushes.push(comp.CreateSurfaceBrushWithSurface(&sf)?);
                }
            }
        }

        let mut dock = Self {
            hwnd,
            comp,
            motion,
            theme,
            cfg: cfg.clone(),
            glass,
            items,
            entries: Vec::new(),
            seps: Vec::new(),
            dot_brush,
            active_brush,
            line_brushes,
            hl_brush,
            m,
            mon,
            side,
            wpos,
            win_w,
            win_h,
            len,
            width: m.h,
            region_w: 0.0,
            hovered: None,
            press: None,
            drag: None,
            suppressed: false,
            slid_away: false,
            mouse_inside: false,
            menu_open: false,
            trash_full: None,
            status: None,
        };
        if taskbar {
            let mut st = status::Status::new(&dock.comp, tsize == TaskbarSize::Full)?;
            st.refresh(gfx, m.h, s, true)?;
            dock.items.Children()?.InsertAtTop(&st.visual)?;
            dock.status = Some(st);
            dock.place_status()?;
            unsafe {
                SetTimer(hwnd, TIMER_STATUS, 2000, None);
            }
        }
        dock.sync(gfx, tracker, false)?;
        dock.intro()?;
        Ok(dock)
    }

    // ---- eksen yardımcıları: a = boyuna (öğelerin dizildiği), c = enine ----

    pub fn side(&self) -> DockPosition {
        self.side
    }

    /// Çark dock'ta gösterilecek uygulamalar (dock sırasıyla, en çok `max`).
    pub fn radial_items(&self, max: usize) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].kind == Kind::App && self.entries[i].leaving.is_none()).take(max).collect()
    }

    /// Öğenin simge fırçası (çark dock aynı simgeyi yeniden çizmeden paylaşır).
    pub fn icon_brush(&self, i: usize) -> Option<CompositionBrush> {
        self.entries.get(i)?.icon.Brush().ok()
    }

    fn vert(&self) -> bool {
        self.side.vertical()
    }

    /// Boyuna/enine konumdan görsel konumu.
    fn pv(&self, a: f32, c: f32) -> Vector3 {
        if self.vert() { v3(c, a) } else { v3(a, c) }
    }

    /// Boyuna/enine boyuttan görsel boyutu.
    fn ps(&self, a: f32, c: f32) -> windows::Foundation::Numerics::Vector2 {
        if self.vert() { v2(c, a) } else { v2(a, c) }
    }

    fn along_prop(&self) -> HSTRING {
        HSTRING::from(if self.vert() { "Offset.Y" } else { "Offset.X" })
    }

    fn across_prop(&self) -> HSTRING {
        HSTRING::from(if self.vert() { "Offset.X" } else { "Offset.Y" })
    }

    fn size_prop(&self) -> HSTRING {
        HSTRING::from(if self.vert() { "Size.Y" } else { "Size.X" })
    }

    /// Pill'in pencere içindeki enine konumu (görünürken).
    fn c0(&self) -> f32 {
        if self.side == DockPosition::Left { self.m.bottom } else { self.m.top }
    }

    /// Otomatik gizlemede pill'in kaydığı enine konum (ekranın dışı).
    fn c_hidden(&self) -> f32 {
        let gap = 8.0 * self.m.s;
        match self.side {
            DockPosition::Bottom => self.win_h + gap,
            DockPosition::Left => -(self.m.h + gap),
            DockPosition::Right => self.win_w + gap,
        }
    }

    /// İstemci noktasının boyuna bileşeni.
    fn along_of(&self, x: i32, y: i32) -> i32 {
        if self.vert() { y } else { x }
    }

    /// Pill'in ekran dikdörtgeni (verilen boyuna uzunlukla).
    fn pill_rect(&self, w: f32) -> RECT {
        let a0 = ((self.len - w) / 2.0).floor() as i32;
        let c0 = self.c0() as i32;
        let (l, t, r, b) = if self.vert() {
            (c0, a0, c0 + self.m.h as i32, a0 + w.ceil() as i32)
        } else {
            (a0, c0, a0 + w.ceil() as i32, c0 + self.m.h as i32)
        };
        RECT { left: self.wpos.0 + l, top: self.wpos.1 + t, right: self.wpos.0 + r, bottom: self.wpos.1 + b }
    }

    fn pins(&self) -> Vec<Pinned> {
        self.cfg.dock.pinned.clone()
    }

    fn make_entry(&self, gfx: &Gfx, key: String, kind: Kind, group: u8, name: String, src: IconSrc) -> Result<Entry> {
        let m = self.m;
        let px = m.icon;
        let surface = match (&kind, &src) {
            (Kind::Start, _) => gfx.surface(px, px, |dc| icons::draw_start(dc, px))?,
            (Kind::Search, _) => gfx.surface(px, px, |dc| icons::draw_search(dc, px, &gfx.round_stroke))?,
            (Kind::Tray, _) => gfx.surface(px, px, |dc| icons::draw_chevron(dc, px, &gfx.round_stroke))?,
            (Kind::App | Kind::Folder | Kind::Trash, IconSrc::Sources(paths, window)) => {
                let loaded = paths
                    .iter()
                    .filter(|p| !p.is_empty())
                    .find_map(|p| icons::shell_icon(&gfx.wic, p, px as i32).ok())
                    .or_else(|| window.and_then(|h| icons::hicon_bitmap(&gfx.wic, tracker::window_icon(h)).ok()));
                match loaded {
                    Some(src) => gfx.surface(px, px, |dc| icons::draw_bitmap(dc, &src, px))?,
                    None => {
                        carlui_core::warn!("ikon yüklenemedi: {paths:?}");
                        gfx.surface(px, px, |_| Ok(()))?
                    }
                }
            }
            (_, IconSrc::Glyph) => gfx.surface(px, px, |_| Ok(()))?,
        };

        let comp = &self.comp;
        let slot = comp.CreateContainerVisual()?;
        let sz = self.ps(m.slot, m.h);
        let (sw, sh) = (sz.X, sz.Y);
        slot.SetSize(sz)?;
        slot.SetCenterPoint(Vector3 { X: sw / 2.0, Y: sh / 2.0, Z: 0.0 })?;
        if kind == Kind::Tray {
            slot.SetRotationAngleInDegrees(self.tray_angle(false))?;
        }
        let kids = slot.Children()?;

        let hl_geom = comp.CreateRoundedRectangleGeometry()?;
        hl_geom.SetSize(v2(m.hl, m.hl))?;
        let hr = m.hl * 0.32 * self.theme.radius_scale;
        hl_geom.SetCornerRadius(v2(hr, hr))?;
        let hl_shape = comp.CreateSpriteShapeWithGeometry(&hl_geom)?;
        hl_shape.SetFillBrush(&self.hl_brush)?;
        let hl = comp.CreateShapeVisual()?;
        hl.SetSize(v2(m.hl, m.hl))?;
        hl.Shapes()?.Append(&hl_shape)?;
        hl.SetOpacity(0.0)?;
        hl.SetOffset(v3((sw - m.hl) / 2.0, (sh - m.hl) / 2.0))?;
        kids.InsertAtTop(&hl)?;

        let brush = comp.CreateSurfaceBrushWithSurface(&surface)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        let icon = comp.CreateSpriteVisual()?;
        icon.SetSize(v2(px, px))?;
        icon.SetBrush(&brush)?;
        icon.SetOffset(v3((sw - px) / 2.0, (sh - px) / 2.0))?;
        // Hover büyümesi ekran kenarına yakın taraftan (yukarı/içeri doğru) olsun.
        let cp = match self.side {
            DockPosition::Bottom => (px / 2.0, px * 0.85),
            DockPosition::Left => (px * 0.15, px / 2.0),
            DockPosition::Right => (px * 0.85, px / 2.0),
        };
        icon.SetCenterPoint(Vector3 { X: cp.0, Y: cp.1, Z: 0.0 })?;
        kids.InsertAtTop(&icon)?;

        let dots = comp.CreateContainerVisual()?;
        kids.InsertAtTop(&dots)?;
        self.items.Children()?.InsertAtTop(&slot)?;

        Ok(Entry {
            key,
            kind,
            group,
            name,
            pin: None,
            app: AppId::default(),
            exe: String::new(),
            windows: Vec::new(),
            slot,
            icon,
            hl,
            dots,
            dots_sig: (0, false),
            x: 0.0,
            leaving: None,
            fresh: true,
        })
    }

    /// İstenen öğe listesini (sabitlenmişler + açık pencereler) mevcut görsellerle
    /// eşleştirir; yenileri oluşturur, gidenleri çıkış animasyonuna sokar.
    pub fn sync(&mut self, gfx: &Gfx, tracker: &Tracker, animate: bool) -> Result<()> {
        struct Want {
            key: String,
            kind: Kind,
            group: u8,
            name: String,
            pin: Option<Pinned>,
            app: AppId,
            exe: String,
            windows: Vec<HWND>,
            src: IconSrc,
        }
        let mut want: Vec<Want> = Vec::new();
        let glyph = |key: &str, kind, group, name: &str| Want {
            key: key.into(),
            kind,
            group,
            name: name.into(),
            pin: None,
            app: AppId::default(),
            exe: String::new(),
            windows: Vec::new(),
            src: IconSrc::Glyph,
        };
        want.push(glyph("start", Kind::Start, 0, t!("Başlat", "Start")));
        if self.cfg.dock.show_search {
            want.push(glyph("search", Kind::Search, 0, t!("Ara", "Search")));
        }

        let mut used = vec![false; tracker.windows.len()];
        for p in self.pins() {
            let app = AppId::of_pin(&p);
            let mut wins = Vec::new();
            for (i, w) in tracker.windows.iter().enumerate() {
                if !used[i] && app.matches(&w.app) {
                    used[i] = true;
                    wins.push(w.hwnd);
                }
            }
            let icon = if p.icon.is_empty() { expand_env(&p.path) } else { expand_env(&p.icon) };
            let first_window = wins.first().copied();
            want.push(Want {
                key: pin_key(&p),
                kind: Kind::App,
                group: 1,
                name: p.name.clone(),
                pin: Some(p),
                app,
                exe: String::new(),
                windows: wins,
                src: IconSrc::Sources(vec![icon], first_window),
            });
        }

        if self.cfg.dock.show_running_apps {
            for (i, w) in tracker.windows.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let key = w.app.key();
                if let Some(existing) = want.iter_mut().find(|x| x.pin.is_none() && x.kind == Kind::App && x.key == key) {
                    existing.windows.push(w.hwnd);
                    continue;
                }
                let exe = apps::process_exe_of_window(w.hwnd);
                let mut paths = Vec::new();
                if !w.app.aumid.is_empty() {
                    paths.push(format!("shell:AppsFolder\\{}", w.app.aumid));
                }
                paths.push(exe.clone());
                let src = IconSrc::Sources(paths, Some(w.hwnd));
                // Adı yalnızca yeni öğe için hesapla (sürüm bilgisi okumak ucuz değil).
                let name = match self.entries.iter().find(|e| e.key == key) {
                    Some(e) => e.name.clone(),
                    None => apps::display_name(&w.app, &exe),
                };
                // Açık uygulamalar sabitlenmişlerle aynı grupta: aralarında ayırıcı yok.
                want.push(Want { key, kind: Kind::App, group: 1, name, pin: None, app: w.app.clone(), exe, windows: vec![w.hwnd], src });
            }
        }
        // Sağ uç (macOS'taki gibi ayrı grup): İndirilenler yığını, Geri Dönüşüm Kutusu.
        let place = |key: &str, kind, name: &str, icon: String| Want {
            key: key.into(),
            kind,
            group: 2,
            name: name.into(),
            pin: None,
            app: AppId::default(),
            exe: String::new(),
            windows: Vec::new(),
            src: IconSrc::Sources(vec![icon], None),
        };
        if self.cfg.dock.downloads_stack {
            if let Some(d) = win::downloads_dir() {
                let p = d.display().to_string();
                let mut w = place("downloads", Kind::Folder, t!("İndirilenler", "Downloads"), p.clone());
                w.exe = p;
                want.push(w);
            }
        }
        // Klasör yığınları: Kind::Folder, klasör yolu `exe` alanında.
        for name in &self.cfg.dock.stacks {
            let name = name.trim();
            if name.is_empty() || name == "downloads" {
                continue;
            }
            let Some(dir) = win::stack_dir(name) else { continue };
            let p = dir.display().to_string();
            let key = format!("stack:{}", p.to_lowercase());
            if want.iter().any(|w| w.key == key) {
                continue;
            }
            let label = places::stack_label(name, &dir);
            let mut w = place(&key, Kind::Folder, &label, p.clone());
            w.exe = p;
            want.push(w);
        }
        if self.cfg.dock.recycle_bin {
            want.push(place("trash", Kind::Trash, t!("Geri Dönüşüm Kutusu", "Recycle Bin"), places::RECYCLE_BIN.into()));
        }
        // Tray paneli tek: düğmesi yalnızca ana dock'ta.
        if crate::screens::child().is_none() {
            want.push(glyph("tray", Kind::Tray, 3, "Tray"));
        }

        let hovered_key = self.hovered.and_then(|i| self.entries.get(i)).map(|e| e.key.clone());
        let mut old: Vec<Entry> = std::mem::take(&mut self.entries);
        let mut next: Vec<Entry> = Vec::with_capacity(want.len());
        for w in want {
            let mut e = match old.iter().position(|e| e.key == w.key) {
                Some(i) => {
                    let mut e = old.remove(i);
                    if e.leaving.take().is_some() {
                        e.fresh = true; // geri dönüyor: yeniden giriş animasyonu
                    }
                    e
                }
                None => self.make_entry(gfx, w.key.clone(), w.kind, w.group, w.name.clone(), w.src)?,
            };
            e.group = w.group;
            e.name = w.name;
            e.pin = w.pin;
            e.app = w.app;
            e.exe = w.exe;
            e.windows = w.windows;
            next.push(e);
        }
        for mut e in old {
            if e.leaving.is_none() {
                e.leaving = Some(Instant::now());
                if animate {
                    let _ = self.motion.fade(&e.slot, h!("Opacity"), 0.0, 160, 0);
                    let _ = self.motion.scale(&e.slot, 0.3, 1.0, 40, 0);
                } else {
                    let _ = e.slot.SetOpacity(0.0);
                }
                unsafe {
                    SetTimer(self.hwnd, TIMER_CLEANUP, 320, None);
                }
            }
            next.push(e);
        }
        self.entries = next;
        // Hover indeksi yeni listede aynı öğeyi göstersin (vurgu takılı kalmasın).
        self.hovered = hovered_key.and_then(|k| self.entries.iter().position(|e| e.key == k && e.leaving.is_none()));
        self.relayout(animate)?;
        self.update_dots(tracker.foreground)?;
        // Çöp kutusu yoklaması ilk eşitlemeden sonra başlar (sonra kendini 3 sn'de bir kurar).
        if self.trash_full.is_none() && self.cfg.dock.recycle_bin {
            unsafe {
                SetTimer(self.hwnd, TIMER_TRASH, 500, None);
            }
        }
        Ok(())
    }

    /// Çıkış animasyonu bitmiş öğelerin görsellerini kaldırır.
    pub fn cleanup(&mut self) {
        let items = self.items.Children();
        self.entries.retain(|e| {
            let done = e.leaving.is_some_and(|t| t.elapsed() > Duration::from_millis(300));
            if done {
                if let Ok(c) = &items {
                    let _ = c.Remove(&e.slot);
                }
            }
            !done
        });
    }
}
