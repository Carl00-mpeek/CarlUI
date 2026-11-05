//! Bu modül üstteki "ada"nın görünüşünü çözüyor: ekranın üst ortasında duran
//! tek bir cam haptır. İçeriği değişince (saat → bildirim → medya paneli)
//! hap spring ile yeni boyuta akar, köşeleri yeni yüksekliğe uyar, eski içerik
//! sönerken yenisi hafif gecikmeyle belirir. Tüm animasyonlar compositor'de
//! çalışır; şarkı ilerleme çubuğu bile tek bir doğrusal animasyondur.
//!
//! Pencere en büyük görünüm kadar şeffaf bir alandır; tıklamalar yalnızca
//! hapın üstünde alınır (pencere bölgesi), geri kalanı alttaki pencerelere geçer.

use crate::{media::Snap, notify::Note};
use carlui_core::theme::{Rgba, Theme};
use carlui_ui::{
    gfx::Gfx,
    glass::{color, v2, v3, Glass},
    motion::{ts, Motion},
    text::Text,
};
use std::collections::HashMap;
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    UI::Composition::{Desktop::DesktopWindowTarget, *},
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Gdi::*,
            Imaging::*,
        },
        System::{SystemInformation::GetLocalTime, WinRT::Composition::ICompositorDesktopInterop},
        UI::WindowsAndMessaging::*,
    },
};

mod clipview;
mod hub;
mod mood;
mod offer;
mod spot;
mod wave;

pub use clipview::panel_size as clip_panel_size;
pub use hub::{groups, HubView, Stat};
use mood::hsv_to_rgb;
pub use offer::OfferView;
pub use spot::{SpotMode, SpotView, ROWS_MAX};

pub const TIMER_REGION: usize = 51;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Hidden,
    Clock,
    Date,
    Media,
    MediaFull,
    Note,
    Search,
    Clip,
    /// Üstüne gelince: göstergeler, medya ve saklanan bildirimler.
    Hub,
    /// Tarayıcıdan gelen indirme: "İndirme bilgisi" kartı.
    Offer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// İndirme kartı düğmeleri: 0 İndir, 1 Farklı kaydet, 2 Tarayıcıya bırak.
    OfferBtn(u8),
    None,
    Pill,
    Prev,
    Play,
    Next,
    Seek,
    Volume,
    Source,
    Field,
    Row(usize),
    Tile(usize),
    Chip(usize),
    ClipRow(usize),
    /// Önizleme düğmeleri: 0 düzenle, 1 sabitle, 2 sil, 3 büyüt (göz).
    ClipBtn(u8),
    Preview,
    /// Metin düzenleme kartı ve düğmeleri (0 Kaydet, 1 X).
    Card,
    CardBtn(u8),
    /// Genişleyen ada: hepsini sil, bildirim satırı, çöp kutusu, ekran görüntüsü düğmesi (0 Aç, 1 Düzenle, 2 Sil).
    HubClear,
    NoteRow(usize),
    NoteDel(usize),
    NoteAct(usize, u8),
    /// Ekrana düşen ekran görüntüsü bildiriminin düğmeleri (0 Aç, 1 Düzenle, 2 Sil).
    BannerAct(u8),
    /// Genişleyen adadaki indirme satırı: duraklat/sürdür ve iptal (indirme kimliği).
    DlToggle(u64),
    DlCancel(u64),
    /// Uygulama grubu başlığı: daralt/genişlet ve o uygulamanın hepsini sil.
    GroupToggle(usize),
    GroupClear(usize),
}

/// Görünüm kurulurken gereken veriler.
pub struct Model<'a> {
    pub media: Option<&'a Snap>,
    pub note: Option<&'a Note>,
    /// Ekran görüntüsü bildirimine tıklandı: altında Aç / Düzenle / Sil.
    pub note_open: bool,
    pub app_volume: Option<f32>,
    pub spot: Option<SpotView<'a>>,
    pub clip: Option<crate::clipper::ClipView<'a>>,
    pub hub: Option<HubView<'a>>,
    /// Saklanan bildirim sayısı (küçük hapta rozet).
    pub badge: usize,
    /// Zamanlayıcı metni ("⏱ 04:32"): hapta saatin yerine geçer.
    pub timer: Option<String>,
    pub offer: Option<OfferView<'a>>,
}

struct Bar {
    /// İçerik koordinatlarında iz.
    x0: f32,
    w: f32,
    th: f32,
    geom: CompositionRoundedRectangleGeometry,
    knob: Option<(ShapeVisual, f32, f32)>,
}

const MONTHS: [&str; 12] = ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"];
const DAYS: [&str; 7] = ["Pazar", "Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

pub fn now_hm() -> (u16, u16, String, String) {
    let t = unsafe { GetLocalTime() };
    let time = format!("{:02}:{:02}", t.wHour, t.wMinute);
    let (m, d) = ((t.wMonth as usize).clamp(1, 12) - 1, t.wDayOfWeek as usize % 7);
    let date = t!(format!("{} {} {}", t.wDay, MONTHS[m], DAYS[d]), format!("{}, {} {}", DAYS_EN[d], MONTHS_EN[m], t.wDay));
    (t.wMinute, t.wSecond, time, date)
}

pub fn fmt_time(sec: f64) -> String {
    let s = sec.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn decode(gfx: &Gfx, bytes: &[u8]) -> Option<(IWICFormatConverter, u32, u32)> {
    unsafe {
        let stream = gfx.wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = gfx.wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = gfx.wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        (w > 0 && h > 0).then_some((conv, w, h))
    }
}

/// Görseli kare olacak şekilde ortadan kırpıp `px` boyutunda yüzeye çizer.
fn image_surface(gfx: &Gfx, bytes: &[u8], px: f32) -> Option<CompositionDrawingSurface> {
    let (conv, w, h) = decode(gfx, bytes)?;
    gfx.surface(px, px, |dc| unsafe {
        let bmp = dc.CreateBitmapFromWicBitmap(&conv, None)?;
        let side = w.min(h) as f32;
        let src = D2D_RECT_F {
            left: (w as f32 - side) / 2.0,
            top: (h as f32 - side) / 2.0,
            right: (w as f32 + side) / 2.0,
            bottom: (h as f32 + side) / 2.0,
        };
        let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: px, bottom: px };
        dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&src), None);
        Ok(())
    })
    .ok()
}

pub struct Island {
    pub hwnd: HWND,
    comp: Compositor,
    motion: Motion,
    theme: Theme,
    pub s: f32,
    _target: DesktopWindowTarget,
    glass: Glass,
    text: Text,
    pub view: View,
    /// Hapın hedef boyutu.
    size: (f32, f32),
    content: Option<ContainerVisual>,
    old: Vec<ContainerVisual>,
    pub win_w: f32,
    win_h: f32,
    top: f32,
    pub slid_up: bool,
    hits: Vec<(Hit, RECT)>,
    progress: Option<Bar>,
    volume: Option<Bar>,
    elapsed: Option<(SpriteVisual, f32)>,
    pub prog_duration: f64,
    art_cache: HashMap<(usize, u32), CompositionDrawingSurface>,
    /// Arama sonuç ikonları (yüklenemeyenler None).
    icons: HashMap<(String, u32), Option<CompositionDrawingSurface>>,
    /// Henüz yüklenmemiş, istenen ikonlar.
    pub icon_requests: Vec<(String, u32)>,
    icon_pending: std::collections::HashSet<(String, u32)>,
    spot_hl: (f32, f32),
    spot_hl_size: (f32, f32),
    /// Arama kutusunun son yüksekliği (uzun yazıda büyüme animasyonu için).
    spot_field_h: f32,
    /// Kapak rengi: (kapak anahtarı, ton/doygunluk). Vurgu çubukları da bu rengi alır.
    mood: Option<(usize, Option<(f32, f32)>)>,
    /// Pano önizlemesinin kaydırma/yakınlaştırma durumu ve önbellekleri.
    clip_pv: clipview::Preview,
    /// Bir sonraki açılış bu noktadan (pencere içi) küçük bir yuvarlak olarak
    /// doğup hızla büyür (fare yanında açılan pano).
    pub pop_from: Option<(f32, f32)>,
    /// Hapın üstünde, kırpılmayan katman: panelden taşan kartlar burada durur.
    overlay: ContainerVisual,
    /// Hub'daki bildirim satırları (kimlik, katman) ve bir kez gösterilenler.
    hub_rows: Vec<(u64, ContainerVisual)>,
    hub_seen: std::collections::HashSet<u64>,
    /// Medya dalgasının hiç durmayan saati (0→1 döngü): panel yeniden
    /// kurulunca dalga başa sarmasın.
    wave_clock: CompositionPropertySet,
    /// Hub'ın üst satırı: (kap, genişlik, gösterge sayısı); saniyelik yenileme için.
    hub_stats: Option<(ContainerVisual, f32, usize)>,
}

impl Island {
    pub fn new(gfx: &Gfx, hwnd: HWND, theme: &Theme, s: f32, top_margin: f32) -> Result<Self> {
        let comp = gfx.comp.clone();
        let target = unsafe { comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        // En büyük görünüm arama paneli; pencere onu kapsayacak kadar.
        let win_w = (680.0 * s).round();
        let win_h = (top_margin * s + 600.0 * s).round();
        let glass = Glass::new(&comp, theme, 15.0 * s)?;
        glass.root.SetAnchorPoint(v2(0.5, 0.0))?;
        glass.root.SetOffset(v3(win_w / 2.0, (top_margin * s).round()))?;
        glass.root.SetSize(v2(80.0 * s, 30.0 * s))?;
        glass.root.SetOpacity(0.0)?;
        let center = comp.CreateExpressionAnimationWithExpression(h!("Vector3(r.Size.X / 2, 0, 0)"))?;
        center.SetReferenceParameter(h!("r"), &glass.root)?;
        glass.root.StartAnimation(h!("CenterPoint"), &center)?;
        root.Children()?.InsertAtTop(&glass.root)?;
        let overlay = comp.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&overlay)?;
        let wave_clock = wave::clock(&comp)?;
        Ok(Self {
            hwnd,
            motion: Motion::new(&comp, theme),
            comp,
            theme: *theme,
            s,
            _target: target,
            glass,
            text: Text::new()?,
            view: View::Hidden,
            size: (80.0 * s, 30.0 * s),
            content: None,
            old: Vec::new(),
            win_w,
            win_h,
            top: (top_margin * s).round(),
            slid_up: false,
            hits: Vec::new(),
            progress: None,
            volume: None,
            elapsed: None,
            prog_duration: 0.0,
            art_cache: HashMap::new(),
            icons: HashMap::new(),
            icon_requests: Vec::new(),
            icon_pending: Default::default(),
            spot_hl: (-1.0, -1.0),
            spot_hl_size: (0.0, 0.0),
            spot_field_h: 0.0,
            mood: None,
            clip_pv: Default::default(),
            pop_from: None,
            overlay,
            hub_rows: Vec::new(),
            hub_seen: Default::default(),
            wave_clock,
            hub_stats: None,
        })
    }

    pub fn window_size(&self) -> (i32, i32) {
        (self.win_w as i32, self.win_h as i32)
    }

    pub fn apply_theme(&mut self, theme: &Theme) {
        self.theme = *theme;
        self.motion = Motion::new(&self.comp, theme);
        let _ = self.glass.apply_theme(theme);
    }

    pub fn set_top(&mut self, top_margin: f32) {
        self.top = (top_margin * self.s).round();
        if !self.slid_up {
            let _ = self.glass.root.SetOffset(v3(self.win_w / 2.0, self.top));
        }
    }

    // ---- Yardımcılar ----

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

    fn solid(&self, c: Rgba) -> Result<CompositionBrush> {
        self.comp.CreateColorBrushWithColor(color(c))?.cast()
    }

    fn accent_brush(&self) -> Result<CompositionBrush> {
        let b = self.comp.CreateLinearGradientBrush()?;
        b.SetStartPoint(v2(0.0, 0.0))?;
        b.SetEndPoint(v2(1.0, 0.0))?;
        let stops = b.ColorStops()?;
        let (c0, c1) = self.accent_colors();
        stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(0.0, color(c0))?)?;
        stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(1.0, color(c1))?)?;
        b.cast()
    }

    /// Vurgu renginin iki ucu: kapak rengi varsa ondan, yoksa temadan.
    fn accent_colors(&self) -> (Rgba, Rgba) {
        match self.mood {
            // Kapak rengi: aynı tonun parlak ve biraz kaymış iki hali.
            Some((_, Some((h, s)))) => {
                let s = s.clamp(0.5, 0.85);
                let a = hsv_to_rgb(h, s, 0.95);
                let b = hsv_to_rgb((h + 25.0).rem_euclid(360.0), s, 0.85);
                (Rgba(a.0, a.1, a.2, 255), Rgba(b.0, b.1, b.2, 255))
            }
            _ => self.theme.accent,
        }
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

    /// Metni çizip (görsel, genişlik, yükseklik) döndürür; konum sonradan verilir.
    fn label(&self, gfx: &Gfx, s: &str, size: f32, bold: bool, max_w: f32, alpha: f32) -> Result<(SpriteVisual, f32, f32)> {
        let t = self.text.render(gfx, s, size * self.s, bold, max_w, alpha)?;
        let v = self.sprite(&t.surface, t.width, t.height, 0.0, 0.0)?;
        Ok((v, t.width, t.height))
    }

    fn place(v: &impl Interface, x: f32, y: f32) -> Result<()> {
        let v: Visual = v.cast()?;
        v.SetOffset(v3(x.round(), y.round()))
    }

    fn art(&mut self, gfx: &Gfx, bytes: &std::sync::Arc<Vec<u8>>, px: f32, radius: f32) -> Result<Option<SpriteVisual>> {
        let key = (std::sync::Arc::as_ptr(bytes) as usize, px as u32);
        if !self.art_cache.contains_key(&key) {
            if self.art_cache.len() > 8 {
                self.art_cache.clear();
            }
            match image_surface(gfx, bytes, px) {
                Some(s) => {
                    self.art_cache.insert(key, s);
                }
                None => return Ok(None),
            }
        }
        let surf = self.art_cache[&key].clone();
        let v = self.sprite(&surf, px, px, 0.0, 0.0)?;
        let g = self.comp.CreateRoundedRectangleGeometry()?;
        g.SetSize(v2(px, px))?;
        g.SetCornerRadius(v2(radius, radius))?;
        v.SetClip(&self.comp.CreateGeometricClipWithGeometry(&g)?)?;
        Ok(Some(v))
    }

    /// Kapak yoksa vurgu renginde müzik notası.
    fn art_or_glyph(&mut self, gfx: &Gfx, snap: &Snap, px: f32, radius: f32) -> Result<Visual> {
        if let Some(bytes) = &snap.art {
            if let Some(v) = self.art(gfx, bytes, px, radius)? {
                return v.cast();
            }
        }
        let c = self.comp.CreateContainerVisual()?;
        let (bg, _) = self.rrect(px, px, radius, &self.accent_brush()?)?;
        c.Children()?.InsertAtTop(&bg)?;
        let g = self.text.glyph(gfx, '\u{EC4F}', (px * 0.5).round(), 0.95)?;
        c.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (px - g.width) / 2.0, (px - g.height) / 2.0)?)?;
        c.cast()
    }

    /// Çalarken zıplayan üç küçük çubuk (döngü compositor'de).
    fn equalizer(&self, kids: &VisualCollection, x: f32, y: f32, playing: bool) -> Result<()> {
        let s = self.s;
        let (bw, gap, mh) = ((3.0 * s).round(), (2.5 * s).round(), (13.0 * s).round());
        let brush = self.accent_brush()?;
        for i in 0..3 {
            let (bar, _) = self.rrect(bw, mh, bw / 2.0, &brush)?;
            bar.SetOffset(v3(x + i as f32 * (bw + gap), y))?;
            bar.SetCenterPoint(Vector3 { X: bw / 2.0, Y: mh, Z: 0.0 })?;
            if playing {
                let a = self.comp.CreateScalarKeyFrameAnimation()?;
                let lo = [0.35, 0.5, 0.3][i];
                a.InsertKeyFrame(0.0, lo)?;
                a.InsertKeyFrame(0.5, 1.0)?;
                a.InsertKeyFrame(1.0, lo)?;
                a.SetDuration(ts([520, 680, 600][i]))?;
                a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
                bar.StartAnimation(h!("Scale.Y"), &a)?;
            } else {
                bar.SetScale(Vector3 { X: 1.0, Y: 0.3, Z: 1.0 })?;
            }
            kids.InsertAtTop(&bar)?;
        }
        Ok(())
    }

    fn bar(&self, kids: &VisualCollection, x0: f32, y: f32, w: f32, value: f32, knob: bool) -> Result<Bar> {
        let s = self.s;
        let th = (4.0 * s).round();
        let (track, _) = self.rrect(w, th, th / 2.0, &self.solid(Rgba(255, 255, 255, 0x26))?)?;
        track.SetOffset(v3(x0, y))?;
        kids.InsertAtTop(&track)?;
        let (fill, geom) = self.rrect(w, th, th / 2.0, &self.accent_brush()?)?;
        fill.SetOffset(v3(x0, y))?;
        kids.InsertAtTop(&fill)?;
        let knob = if knob {
            let r = (7.0 * s).round();
            let (k, _) = self.rrect(r * 2.0, r * 2.0, r, &self.solid(Rgba(255, 255, 255, 0xF2))?)?;
            kids.InsertAtTop(&k)?;
            Some((k, r, y + th / 2.0))
        } else {
            None
        };
        let b = Bar { x0, w, th, geom, knob };
        Self::set_bar(&b, value);
        Ok(b)
    }

    fn set_bar(b: &Bar, v: f32) {
        let fw = (b.w * v.clamp(0.0, 1.0)).max(0.01);
        let _ = b.geom.StopAnimation(h!("Size"));
        let _ = b.geom.SetSize(v2(fw, b.th));
        if let Some((k, r, cy)) = &b.knob {
            let _ = k.SetOffset(v3(b.x0 + fw - r, cy - r));
        }
    }

    // ---- Görünümler ----

    fn build(&mut self, gfx: &Gfx, view: View, m: &Model) -> Result<(ContainerVisual, f32, f32)> {
        let s = self.s;
        let c = self.comp.CreateContainerVisual()?;
        let kids = c.Children()?;
        self.hits.clear();
        self.progress = None;
        self.volume = None;
        self.elapsed = None;
        let compact_h = (30.0 * s).round();
        let (_, _, time, date) = now_hm();
        let time = m.timer.clone().unwrap_or(time);
        let (w, h) = match view {
            View::Hidden => (80.0 * s, compact_h),
            View::Clock => {
                // İndirme hapı uzun olabilir (ad, boyut, hız, kalan süre).
                let (t, tw, th) = self.label(gfx, &time, 13.5, true, 560.0 * s, 0.95)?;
                let extra = if m.badge > 0 { 26.0 * s } else { 0.0 };
                let w = (tw + 40.0 * s + extra).max(84.0 * s).round();
                Self::place(&t, (w - tw - extra) / 2.0, (compact_h - th) / 2.0)?;
                kids.InsertAtTop(&t)?;
                if m.badge > 0 {
                    self.badge(gfx, &kids, (w + tw - extra) / 2.0 + 8.0 * s, compact_h, m.badge)?;
                }
                (w, compact_h)
            }
            View::Date => {
                let (d, dw, dh) = self.label(gfx, &date, 13.0, false, 260.0 * s, 0.7)?;
                let (t, tw, th) = self.label(gfx, &time, 13.5, true, 200.0 * s, 0.95)?;
                let gap = 12.0 * s;
                let w = (dw + gap + tw + 40.0 * s).round();
                let x = (w - dw - gap - tw) / 2.0;
                Self::place(&d, x, (compact_h - dh) / 2.0)?;
                Self::place(&t, x + dw + gap, (compact_h - th) / 2.0)?;
                kids.InsertAtTop(&d)?;
                kids.InsertAtTop(&t)?;
                (w, compact_h)
            }
            View::Media => {
                let Some(snap) = m.media else { return self.build(gfx, View::Clock, m) };
                let (t, tw, th) = self.label(gfx, &time, 13.5, true, 200.0 * s, 0.95)?;
                let w = (tw + 130.0 * s).max(196.0 * s).round();
                let apx = (20.0 * s).round();
                let a = self.art_or_glyph(gfx, snap, apx, 6.0 * s)?;
                Self::place(&a, (6.0 * s).round(), ((compact_h - apx) / 2.0).round())?;
                kids.InsertAtTop(&a)?;
                Self::place(&t, (w - tw) / 2.0, (compact_h - th) / 2.0)?;
                kids.InsertAtTop(&t)?;
                if m.badge > 0 {
                    self.badge(gfx, &kids, (w + tw) / 2.0 + 8.0 * s, compact_h, m.badge)?;
                }
                self.equalizer(&kids, w - 12.0 * s - 13.0 * s, ((compact_h - 13.0 * s) / 2.0).round(), snap.playing)?;
                (w, compact_h)
            }
            View::MediaFull => {
                let Some(snap) = m.media else { return self.build(gfx, View::Clock, m) };
                let w = (380.0 * s).round();
                let pad = (16.0 * s).round();
                let apx = (60.0 * s).round();
                let a = self.art_or_glyph(gfx, snap, apx, 12.0 * s)?;
                Self::place(&a, pad, pad)?;
                kids.InsertAtTop(&a)?;
                let tx = pad + apx + 14.0 * s;
                let tmax = w - tx - pad - 26.0 * s;
                let (t, _, th) = self.label(gfx, &snap.title, 14.5, true, tmax, 0.96)?;
                Self::place(&t, tx, pad + 6.0 * s)?;
                kids.InsertAtTop(&t)?;
                let artist = if snap.artist.is_empty() { t!("Bilinmeyen sanatçı", "Unknown artist") } else { &snap.artist };
                let (ar, _, _) = self.label(gfx, artist, 12.5, false, tmax, 0.62)?;
                Self::place(&ar, tx, pad + 6.0 * s + th + 2.0 * s)?;
                kids.InsertAtTop(&ar)?;
                self.equalizer(&kids, w - pad - 13.0 * s, pad + 8.0 * s, snap.playing)?;
                self.hits.push((Hit::Source, RECT { left: pad as i32, top: pad as i32, right: (w - pad) as i32, bottom: (pad + apx) as i32 }));

                // İlerleme: geçen süre | çubuk | toplam süre
                let py = pad + apx + 16.0 * s;
                let lw = 42.0 * s;
                if snap.end > 0.0 {
                    let pos = snap.position();
                    let (el, ew, eh) = self.label(gfx, &fmt_time(pos), 11.0, false, lw, 0.6)?;
                    Self::place(&el, pad, py)?;
                    kids.InsertAtTop(&el)?;
                    let (tot, tw2, _) = self.label(gfx, &fmt_time(snap.end), 11.0, false, lw, 0.6)?;
                    Self::place(&tot, w - pad - tw2, py)?;
                    kids.InsertAtTop(&tot)?;
                    let bx = pad + lw;
                    let bw = w - 2.0 * pad - 2.0 * lw;
                    let by = (py + eh / 2.0 - 2.0 * s).round();
                    let b = self.bar(&kids, bx, by, bw, (pos / snap.end) as f32, false)?;
                    self.wave(gfx, &kids, &b, by, snap.playing)?;
                    if snap.can_seek {
                        self.hits.push((Hit::Seek, RECT { left: bx as i32, top: (by - 9.0 * s) as i32, right: (bx + bw) as i32, bottom: (by + 13.0 * s) as i32 }));
                    }
                    self.progress = Some(b);
                    self.elapsed = Some((el, ew));
                    self.prog_duration = snap.end;
                    self.run_progress(snap);
                }

                // Düğmeler
                let by = py + 20.0 * s;
                let bh = (40.0 * s).round();
                let cx = w / 2.0;
                let buttons = [
                    (Hit::Prev, '\u{E892}', 18.0, snap.can_prev, cx - 64.0 * s),
                    (Hit::Play, if snap.playing { '\u{E769}' } else { '\u{E768}' }, 24.0, true, cx),
                    (Hit::Next, '\u{E893}', 18.0, snap.can_next, cx + 64.0 * s),
                ];
                for (hit, glyph, size, enabled, bx) in buttons {
                    let g = self.text.glyph(gfx, glyph, (size * s).round(), if enabled { 0.95 } else { 0.3 })?;
                    let v = self.sprite(&g.surface, g.width, g.height, bx - g.width / 2.0, by + (bh - g.height) / 2.0)?;
                    kids.InsertAtTop(&v)?;
                    if enabled {
                        let hw = 24.0 * s;
                        self.hits.push((hit, RECT { left: (bx - hw) as i32, top: by as i32, right: (bx + hw) as i32, bottom: (by + bh) as i32 }));
                    }
                }

                // Uygulamanın kendi ses seviyesi
                let vy = by + bh + 4.0 * s;
                let vh = (28.0 * s).round();
                let mut h = vy + 6.0 * s;
                if let Some(vol) = m.app_volume {
                    let g = self.text.glyph(gfx, '\u{E767}', (15.0 * s).round(), 0.85)?;
                    kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad, vy + (vh - g.height) / 2.0)?)?;
                    let vx = pad + 30.0 * s;
                    let vw = w - vx - pad - 6.0 * s;
                    let b = self.bar(&kids, vx, (vy + vh / 2.0 - 2.0 * s).round(), vw, vol, true)?;
                    self.hits.push((Hit::Volume, RECT { left: (vx - 8.0 * s) as i32, top: vy as i32, right: (vx + vw + 8.0 * s) as i32, bottom: (vy + vh) as i32 }));
                    self.volume = Some(b);
                    h = vy + vh + 10.0 * s;
                }
                (w, h.round())
            }
            View::Note => {
                let Some(n) = m.note else { return self.build(gfx, View::Clock, m) };
                let w = (380.0 * s).round();
                let pad = (14.0 * s).round();
                let ipx = (40.0 * s).round();
                let has_body = !n.body.is_empty();
                let head = if has_body { (74.0 * s).round() } else { (62.0 * s).round() };
                // Güncelleme bildiriminin düğmeleri hep açık; ekran görüntüsününkiler tıklayınca.
                let open = (m.note_open && n.shot.is_some()) || n.update.is_some();
                let h = if open { head + (34.0 * s).round() } else { head };
                let icon: Visual = match n.logo.as_ref().map(|b| self.art(gfx, b, ipx, 10.0 * s)) {
                    Some(Ok(Some(v))) => v.cast()?,
                    _ => {
                        let c = self.comp.CreateContainerVisual()?;
                        let (bg, _) = self.rrect(ipx, ipx, 10.0 * s, &self.accent_brush()?)?;
                        c.Children()?.InsertAtTop(&bg)?;
                        let g = self.text.glyph(gfx, '\u{EA8F}', (ipx * 0.5).round(), 0.95)?;
                        c.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (ipx - g.width) / 2.0, (ipx - g.height) / 2.0)?)?;
                        c.cast()?
                    }
                };
                Self::place(&icon, pad, ((head - ipx) / 2.0).round())?;
                kids.InsertAtTop(&icon)?;
                let tx = pad + ipx + 12.0 * s;
                let tmax = w - tx - pad;
                let (an, _, ah) = self.label(gfx, &n.app, 11.0, false, tmax - 50.0 * s, 0.55)?;
                let (nowl, nw, _) = self.label(gfx, t!("şimdi", "now"), 11.0, false, 60.0 * s, 0.45)?;
                let (t, _, th) = self.label(gfx, &n.title, 13.5, true, tmax, 0.96)?;
                let mut y = if has_body { 10.0 * s } else { 12.0 * s };
                Self::place(&an, tx, y)?;
                Self::place(&nowl, w - pad - nw, y)?;
                y += ah + 1.0 * s;
                Self::place(&t, tx, y)?;
                kids.InsertAtTop(&an)?;
                kids.InsertAtTop(&nowl)?;
                kids.InsertAtTop(&t)?;
                if has_body {
                    let (b, _, _) = self.label(gfx, &n.body, 12.5, false, tmax, 0.7)?;
                    Self::place(&b, tx, y + th + 1.0 * s)?;
                    kids.InsertAtTop(&b)?;
                }
                if open {
                    let py = head - 4.0 * s;
                    let mut px = tx;
                    let buttons: &[(&str, bool)] = if n.update.is_some() {
                        &[(t!("Güncelle", "Update"), false), (t!("Sonra", "Later"), false)]
                    } else {
                        &[(t!("Aç", "Open"), false), (t!("Düzenle", "Edit"), false), (t!("Sil", "Delete"), true)]
                    };
                    for (k, &(label, danger)) in buttons.iter().enumerate() {
                        let pw = self.hub_pill(gfx, &kids, label, px, py, danger)?;
                        self.hits.push((Hit::BannerAct(k as u8), RECT { left: px as i32, top: py as i32, right: (px + pw) as i32, bottom: (py + 24.0 * s) as i32 }));
                        px += pw + 6.0 * s;
                    }
                }
                (w, h)
            }
            View::Search => self.build_search(gfx, &kids, m)?,
            View::Clip => self.build_clip(gfx, &kids, m)?,
            View::Hub => self.build_hub(gfx, &kids, m)?,
            View::Offer => self.build_offer(gfx, &kids, m)?,
        };
        Ok((c, w, h))
    }

    /// İlerleme çubuğunu şimdiki konumdan sona doğru doğrusal olarak akıt.
    pub fn run_progress(&self, snap: &Snap) {
        let Some(b) = &self.progress else { return };
        if snap.end <= 0.0 {
            return;
        }
        let pos = snap.position();
        let frac = (pos / snap.end).clamp(0.0, 1.0) as f32;
        Self::set_bar(b, frac);
        if !snap.playing {
            return;
        }
        let remain = snap.end - pos;
        if remain <= 0.2 {
            return;
        }
        let run = || -> Result<()> {
            let a = self.comp.CreateVector2KeyFrameAnimation()?;
            let lin = self.comp.CreateLinearEasingFunction()?;
            a.InsertKeyFrameWithEasingFunction(0.0, v2((b.w * frac).max(0.01), b.th), &lin)?;
            a.InsertKeyFrameWithEasingFunction(1.0, v2(b.w, b.th), &lin)?;
            a.SetDuration(ts((remain * 1000.0) as i64))?;
            b.geom.StartAnimation(h!("Size"), &a)
        };
        let _ = run();
    }

    /// Geçen süre etiketini yeniler (panel açıkken saniyede bir).
    pub fn tick_elapsed(&mut self, gfx: &Gfx, snap: &Snap) {
        let Some((v, _)) = &self.elapsed else { return };
        let Ok(t) = self.text.render(gfx, &fmt_time(snap.position()), 11.0 * self.s, false, 42.0 * self.s, 0.6) else { return };
        if let Ok(b) = self.comp.CreateSurfaceBrushWithSurface(&t.surface) {
            let _ = b.SetStretch(CompositionStretch::Fill);
            let _ = v.SetBrush(&b);
            let _ = v.SetSize(v2(t.width, t.height));
        }
    }

    /// Sürüklerken çubuğu imlece göre ayarla; 0..1 değeri döndürür.
    pub fn drag_bar(&self, which: Hit, x: i32) -> Option<f32> {
        let b = match which {
            Hit::Seek => self.progress.as_ref()?,
            Hit::Volume => self.volume.as_ref()?,
            _ => return None,
        };
        let ox = (self.win_w - self.size.0) / 2.0;
        let v = ((x as f32 - ox - b.x0) / b.w).clamp(0.0, 1.0);
        Self::set_bar(b, v);
        Some(v)
    }

    /// Pencere koordinatındaki nokta neye denk geliyor?
    pub fn hit(&self, x: i32, y: i32) -> Hit {
        if self.view == View::Hidden || self.slid_up {
            return Hit::None;
        }
        if let Some(c) = self.card_hit(x, y) {
            return c;
        }
        let ox = ((self.win_w - self.size.0) / 2.0) as i32;
        let oy = self.top as i32;
        let (lx, ly) = (x - ox, y - oy);
        if lx < 0 || ly < 0 || lx >= self.size.0 as i32 || ly >= self.size.1 as i32 {
            return Hit::None;
        }
        self.hits
            .iter()
            .find(|(_, r)| lx >= r.left && lx < r.right && ly >= r.top && ly < r.bottom)
            .map(|(h, _)| *h)
            .unwrap_or(Hit::Pill)
    }

    /// Hapın pencere içindeki dikdörtgeni (hedef boyuta göre).
    pub fn pill_rect(&self) -> RECT {
        let ox = (self.win_w - self.size.0) / 2.0;
        RECT { left: ox as i32, top: self.top as i32, right: (ox + self.size.0) as i32, bottom: (self.top + self.size.1) as i32 }
    }

    fn radius_for(&self, h: f32) -> f32 {
        let compact = h <= 32.0 * self.s;
        if compact { h / 2.0 } else { (26.0 * self.s * self.theme.radius_scale).min(h / 2.0).round() }
    }

    /// Görünümü değiştirir. `animate` false ise içerik yerinde yenilenir
    /// (saat dakikası, şarkı durumu gibi küçük güncellemeler).
    pub fn set_view(&mut self, gfx: &Gfx, view: View, m: &Model, animate: bool) -> Result<()> {
        let was = self.view;
        let old_rect = self.pill_rect();
        // Eski içerikleri temizle (önceki geçişten kalanlar).
        for o in self.old.drain(..) {
            if let Ok(p) = self.glass.content.Children() {
                let _ = p.Remove(&o);
            }
        }
        self.view = view;
        let mo = self.motion.clone();
        if view == View::Hidden {
            if animate {
                self.fade_card();
            } else {
                self.close_card(false);
            }
            if let Some(c) = self.content.take() {
                let _ = mo.fade(&c, h!("Opacity"), 0.0, 90, 0);
                self.old.push(c);
            }
            if animate {
                mo.fade(&self.glass.root, h!("Opacity"), 0.0, 160, 0)?;
                mo.scale(&self.glass.root, 0.6, 1.0, 45, 0)?;
            } else {
                self.glass.root.SetOpacity(0.0)?;
            }
            self.hits.clear();
            self.set_region(None);
            return Ok(());
        }

        let (c, w, h) = self.build(gfx, view, m)?;
        let follow = self.comp.CreateExpressionAnimationWithExpression(h!("Vector3(r.Size.X / 2 - W / 2, 0, 0)"))?;
        follow.SetReferenceParameter(h!("r"), &self.glass.root)?;
        follow.SetScalarParameter(h!("W"), w)?;
        c.StartAnimation(h!("Offset"), &follow)?;
        c.SetSize(v2(w, h))?;
        self.glass.content.Children()?.InsertAtTop(&c)?;
        let radius = self.radius_for(h);
        let grow = view != was;

        if !animate || !grow {
            if let Some(o) = self.content.take() {
                self.glass.content.Children()?.Remove(&o)?;
            }
            if (w, h) != self.size && was != View::Hidden {
                mo.animate(&self.glass.root, h!("Size.X"), w, 0.85, 50, 0)?;
                mo.animate(&self.glass.root, h!("Size.Y"), h, 0.85, 50, 0)?;
                self.glass.animate_radius(&self.comp, radius, 200, 0)?;
            }
        } else {
            if let Some(o) = self.content.take() {
                mo.fade(&o, h!("Opacity"), 0.0, 90, 0)?;
                self.old.push(o);
            }
            c.SetOpacity(0.0)?;
            c.SetScale(Vector3 { X: 0.94, Y: 0.94, Z: 1.0 })?;
            c.SetCenterPoint(Vector3 { X: w / 2.0, Y: 0.0, Z: 0.0 })?;
            let pop = if was == View::Hidden { self.pop_from.take() } else { None };
            let delay = if pop.is_some() { 130 } else if was == View::Hidden { 60 } else { 110 };
            mo.fade(&c, h!("Opacity"), 1.0, if pop.is_some() { 110 } else { 170 }, delay)?;
            mo.scale(&c, 1.0, 0.75, 45, delay)?;
            if let Some((px, py)) = pop {
                // İki adım (~0,25 sn): farenin altında küçük bir yuvarlak doğar, yuvarlak
                // hâliyle sağa (panelin yerine) kayar, orada panele büyür.
                let d = (30.0 * self.s).round();
                self.glass.root.StopAnimation(h!("Offset"))?;
                self.glass.root.SetOffset(v3(px, py + 4.0 * self.s))?;
                self.glass.root.SetSize(v2(d, d))?;
                self.glass.set_radius(d / 2.0)?;
                self.glass.root.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
                mo.fade(&self.glass.root, h!("Opacity"), 1.0, 60, 0)?;
                mo.animate(&self.glass.root, h!("Offset.X"), self.win_w / 2.0, 1.0, 26, 0)?;
                mo.animate(&self.glass.root, h!("Offset.Y"), self.top, 1.0, 26, 0)?;
                mo.animate(&self.glass.root, h!("Size.X"), w, 0.8, 34, 95)?;
                mo.animate(&self.glass.root, h!("Size.Y"), h, 0.8, 36, 95)?;
                self.glass.animate_radius(&self.comp, radius, 140, 95)?;
                // Buzlu cam: içerik süt beyazı bir camın ardından belirir, panel
                // büyüdükçe cam açılır. Tek renkli bir katman; ek yük yok denecek kadar az.
                let veil = self.comp.CreateSpriteVisual()?;
                veil.SetSize(v2(w, h))?;
                veil.SetBrush(&self.comp.CreateColorBrushWithColor(color(Rgba(235, 238, 245, 0x55)))?)?;
                c.Children()?.InsertAtTop(&veil)?;
                mo.fade(&veil, h!("Opacity"), 0.0, 240, 150)?;
            } else if was == View::Hidden {
                // Ada belirirken: küçük bir haptan büyüyerek gelir.
                self.glass.root.SetSize(v2(w * 0.6, h))?;
                self.glass.set_radius(h / 2.0)?;
                mo.fade(&self.glass.root, h!("Opacity"), 1.0, 140, 0)?;
                self.glass.root.SetScale(Vector3 { X: 0.7, Y: 0.7, Z: 1.0 })?;
                mo.scale(&self.glass.root, 1.0, 0.65, 50, 0)?;
            }
            if pop.is_none() {
                // Genişlerken biraz esnek, küçülürken daha sakin.
                let expanding = h > self.size.1 || w > self.size.0;
                let damp = if expanding { 0.72 } else { 0.86 };
                mo.animate(&self.glass.root, h!("Size.X"), w, damp, 55, 0)?;
                mo.animate(&self.glass.root, h!("Size.Y"), h, damp, 60, 0)?;
                self.glass.animate_radius(&self.comp, radius, 220, 0)?;
            }
        }
        if was == View::Hidden && !(animate && grow) {
            self.glass.root.SetSize(v2(w, h))?;
            self.glass.set_radius(radius)?;
            self.glass.root.SetOpacity(1.0)?;
            self.glass.root.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
        }
        self.content = Some(c);
        self.size = (w, h);
        if !self.slid_up {
            let new_rect = self.pill_rect();
            let union = RECT {
                left: old_rect.left.min(new_rect.left),
                top: old_rect.top.min(new_rect.top),
                right: old_rect.right.max(new_rect.right),
                bottom: old_rect.bottom.max(new_rect.bottom),
            };
            // Animasyon boyunca eski ve yeni boyutu kapsa; bitince yeni boyuta daralt.
            let r = self.card_region().unwrap_or(if was == View::Hidden { new_rect } else { union });
            self.set_region(Some((r, false)));
            unsafe {
                SetTimer(self.hwnd, TIMER_REGION, 480, None);
            }
        }
        Ok(())
    }

    /// Öndeki pencere maksimize iken adayı yukarı sakla / geri indir.
    pub fn slide(&mut self, up: bool) {
        if self.slid_up == up {
            return;
        }
        self.slid_up = up;
        let y = if up { -(self.size.1 + 12.0 * self.s) } else { self.top };
        let _ = self.motion.animate(&self.glass.root, h!("Offset.Y"), y, if up { 1.0 } else { 0.75 }, 50, 0);
        if up {
            self.set_region(None);
        } else {
            self.region_done();
        }
    }

    /// Animasyon bitti: tıklama bölgesini yalnızca hapa daralt.
    pub fn region_done(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_REGION);
        }
        if let Ok(p) = self.glass.content.Children() {
            for o in self.old.drain(..) {
                let _ = p.Remove(&o);
            }
        }
        self.drop_old_cards();
        if self.view == View::Hidden || self.slid_up {
            self.set_region(None);
        } else if let Some(r) = self.card_region() {
            self.set_region(Some((r, false)));
        } else {
            self.set_region(Some((self.pill_rect(), true)));
        }
    }

    /// `None`: hiçbir yer tıklanmaz. `Some((r, round))`: yalnızca r (yuvarlak köşeli).
    fn set_region(&self, r: Option<(RECT, bool)>) {
        unsafe {
            let rgn = match r {
                None => CreateRectRgn(0, 0, 0, 0),
                Some((r, true)) => {
                    // GDI bölgesi kenar yumuşatmaz; hapın tam kenarından keserse köşeler
                    // merdiven gibi görünür. Bölgeyi 2 px dışarı taşırırız ki kesim
                    // görünmeyen saydam alana düşsün, yumuşak kenarı Composition çizsin.
                    const PAD: i32 = 2;
                    let d = (self.radius_for((r.bottom - r.top) as f32) * 2.0) as i32 + PAD * 2;
                    CreateRoundRectRgn(r.left - PAD, r.top - PAD, r.right + PAD + 1, r.bottom + PAD + 1, d, d)
                }
                Some((r, false)) => CreateRectRgn(r.left, r.top, r.right, r.bottom),
            };
            SetWindowRgn(self.hwnd, rgn, true);
        }
    }

    /// Arama açılırken seçim vurgusu baştan başlasın.
    pub fn reset_highlight(&mut self) {
        self.spot_hl = (-1.0, -1.0);
        self.spot_field_h = 0.0;
    }

    /// Basılan düğmeye küçük bir "basma" tepkisi.
    pub fn press(&self, down: bool) {
        let target = if down { 0.97 } else { 1.0 };
        let _ = self.motion.scale(&self.glass.root, target, 0.6, 35, 0);
    }
}
