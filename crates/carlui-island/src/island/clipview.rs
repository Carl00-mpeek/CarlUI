//! Bu modül adadaki pano geçmişini çiziyor: üstte süzgeç kutusu, solda kayıt
//! listesi (resimler küçük resimle), sağda büyük önizleme.
//!
//! Önizleme "fareyle sağa sola / yukarı aşağı bakamıyorum" sorununu şöyle
//! çözüyor: içerik (resim ya da metin) bir kez tam boyutlu bir yüzeye çizilir,
//! kırpılmış bir pencerenin içinde durur. Tekerlek, Shift+tekerlek (ya da yatay
//! tekerlek), sürükleme ve ok tuşları yalnızca bu yüzeyin konumunu değiştirir;
//! Ctrl+tekerlek imlecin altındaki noktayı sabit tutarak yakınlaştırır. Yeniden
//! çizim yoktur, kaydırma compositor'de yumuşak bir spring ile akar.
//!
//! "Önizleme küçük, düzenleme ayrı pencerede kötü duruyor" sorununu da kartla
//! çözüyor: resimde göz düğmesine (ya da resme) basınca resim önizleme
//! kutusundan büyüyerek panelin üstüne çıkar; orada sürüklenir, yakınlaştırılır.
//! Metne tıklayınca önizleme kutusu panelin dışına taşarak genişler ve metin
//! orada, kartın içinde düzenlenir. Kart, hapın üstündeki kırpılmayan katmanda
//! (overlay) durduğu için panel sınırının dışına çıkabilir.

use super::*;
use crate::clipper::{Act, ClipMode, ClipView, ROWS};
use carlui_clip::store::{ago, Clip, Kind};

const W: f32 = 440.0;
const PAD: f32 = 10.0;
const FIELD: f32 = 36.0;
const ROW: f32 = 46.0;
const LIST_W: f32 = 172.0;
const GAP: f32 = 8.0;
const PV_HEAD: f32 = 32.0;
const HINT: f32 = 26.0;
/// Önizleme yüzeyinin en uzun kenarı (daha büyük resimler küçültülür).
const MAX_SURFACE: f32 = 4096.0;
/// Önizlemede gösterilen en fazla karakter.
const MAX_CHARS: usize = 40_000;
/// Kartın zemini (düzenleyicinin arka planı da aynı renk olmalı).
const CARD_BG: Rgba = Rgba(30, 30, 32, 255);
const CARD_HEAD: f32 = 36.0;
/// Kart gölgesi için tıklama bölgesine eklenen pay.
const CARD_SHADOW: f32 = 40.0;

pub struct Card {
    root: LayerVisual,
    /// Kaydet (0) ve X (1) düğmeleri (içerik koordinatı).
    buttons: Vec<(u8, RECT)>,
    /// Hedef dikdörtgen (panel içerik koordinatı; eksi olabilir, panelden taşar).
    to: (f32, f32, f32, f32),
    /// Düzenleyici gelene kadar görünen metin.
    ghost: Option<SpriteVisual>,
    /// Düzenleyici penceresinin yeri (içerik koordinatı).
    edit: (f32, f32, f32, f32),
}

/// Panelin boyutu (fiziksel piksel); pencere fareye göre buna bakılarak yerleştirilir.
pub fn panel_size(s: f32) -> (f32, f32) {
    let h = (10.0 * s).round() + (FIELD * s).round() + (8.0 * s).round() + ROWS as f32 * (ROW * s).round() + (8.0 * s).round() + (HINT * s).round();
    ((W * s).round(), (h + 4.0 * s).round())
}

#[derive(Default)]
pub struct Preview {
    key: String,
    inner: Option<ContainerVisual>,
    vbar: Option<(ShapeVisual, CompositionRoundedRectangleGeometry)>,
    hbar: Option<(ShapeVisual, CompositionRoundedRectangleGeometry)>,
    /// İçerik boyutu (yakınlaştırma 1'de) ve görünen pencere.
    cw: f32,
    ch: f32,
    vw: f32,
    vh: f32,
    /// Görünen pencerenin hap içindeki sol üst köşesi.
    ox: f32,
    oy: f32,
    pub zoom: f32,
    fit: f32,
    pub x: f32,
    pub y: f32,
    pub image: bool,
    drag: Option<(i32, i32, f32, f32)>,
    surfaces: Vec<(String, CompositionDrawingSurface, f32, f32)>,
    thumbs: HashMap<String, Option<CompositionDrawingSurface>>,
    /// Kaydırma göstergelerinin bağlı olduğu katmanın içerik koordinatı
    /// (önizleme kutusunda 0, kartta kartın sol üstü).
    origin: (f32, f32),
    /// Önizleme kutusu (içerik koordinatı), içindeki yüzeyin anahtarı ve türü.
    pane: (f32, f32, f32, f32),
    pane_key: String,
    pub pane_image: bool,
    pub card: Option<Card>,
    card_old: Vec<LayerVisual>,
}

/// Resmi (gerekirse küçülterek) yüzeye çizer; (yüzey, genişlik, yükseklik).
fn picture(gfx: &Gfx, bytes: &[u8]) -> Option<(CompositionDrawingSurface, f32, f32)> {
    let (conv, w, h) = decode(gfx, bytes)?;
    let k = (MAX_SURFACE / w.max(h) as f32).min(1.0);
    let (sw, sh) = ((w as f32 * k).round().max(1.0), (h as f32 * k).round().max(1.0));
    let src: IWICBitmapSource = unsafe {
        if k < 1.0 {
            let sc = gfx.wic.CreateBitmapScaler().ok()?;
            sc.Initialize(&conv, sw as u32, sh as u32, WICBitmapInterpolationModeFant).ok()?;
            sc.cast().ok()?
        } else {
            conv.cast().ok()?
        }
    };
    let surf = gfx
        .surface(sw, sh, |dc| unsafe {
            let bmp = dc.CreateBitmapFromWicBitmap(&src, None)?;
            let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: sw, bottom: sh };
            dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            Ok(())
        })
        .ok()?;
    Some((surf, sw, sh))
}

/// Gizli kayıtların listede ve önizlemede görünen hâli.
const MASK: &str = "••••••••";

fn first_line(c: &Clip) -> String {
    if c.secret {
        return MASK.into();
    }
    match c.kind {
        Kind::Image => t!("Resim", "Image").into(),
        Kind::Files => {
            let n = c.text.lines().count();
            let first = c.text.lines().next().unwrap_or_default();
            let name = std::path::Path::new(first).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| first.to_string());
            if n > 1 { format!("{name} +{}", n - 1) } else { name }
        }
        Kind::Text => c.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(200).collect(),
        Kind::Vault => c.text.clone(),
    }
}

fn describe(c: &Clip) -> String {
    if c.secret {
        return if c.pinned { t!("gizli · sabit", "hidden · pinned").into() } else { t!("gizli · kısa süre sonra silinir", "hidden · deleted soon").into() };
    }
    match c.kind {
        Kind::Image => format!("{}×{}", c.w, c.h),
        Kind::Files => t!(format!("{} dosya", c.text.lines().count()), format!("{} files", c.text.lines().count())),
        Kind::Text => {
            let n = c.text.chars().count();
            let lines = c.text.lines().count();
            if lines > 1 { t!(format!("{n} karakter · {lines} satır"), format!("{n} characters · {lines} lines")) } else { t!(format!("{n} karakter"), format!("{n} characters")) }
        }
        Kind::Vault => {
            let left = ((c.expires - carlui_clip::store::now_ms()) / 1000).max(0);
            t!(format!("tek kullanımlık · {left} sn"), format!("one-time · {left} s"))
        }
    }
}

impl Island {
    fn caret(&self, kids: &VisualCollection, x: f32, y: f32, h: f32) -> Result<()> {
        let (caret, _) = self.rrect((2.0 * self.s).round(), h, 1.0 * self.s, &self.accent_brush()?)?;
        Self::place(&caret, x, y)?;
        let blink = self.comp.CreateScalarKeyFrameAnimation()?;
        let step = self.comp.CreateStepEasingFunction()?;
        blink.InsertKeyFrame(0.0, 1.0)?;
        blink.InsertKeyFrameWithEasingFunction(0.5, 0.0, &step)?;
        blink.InsertKeyFrameWithEasingFunction(1.0, 1.0, &step)?;
        blink.SetDuration(ts(1060))?;
        blink.SetDelayTime(ts(500))?;
        blink.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        caret.StartAnimation(h!("Opacity"), &blink)?;
        kids.InsertAtTop(&caret)
    }

    fn glyph_box(&self, gfx: &Gfx, glyph: char, px: f32) -> Result<Visual> {
        let c = self.comp.CreateContainerVisual()?;
        let (bg, _) = self.rrect(px, px, (px * 0.28).round(), &self.solid(Rgba(255, 255, 255, 0x16))?)?;
        c.Children()?.InsertAtTop(&bg)?;
        let g = self.text.glyph(gfx, glyph, (px * 0.5).round(), 0.85)?;
        c.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (px - g.width) / 2.0, (px - g.height) / 2.0)?)?;
        c.cast()
    }

    /// Satır simgesi: resimde küçük resim, diğerlerinde simge.
    fn clip_icon(&mut self, gfx: &Gfx, v: &ClipView, c: &Clip, px: f32) -> Result<Visual> {
        if c.kind == Kind::Image {
            if !self.clip_pv.thumbs.contains_key(&c.id) {
                let bytes = std::fs::read(v.store.thumb_path(c)).or_else(|_| std::fs::read(v.store.image_path(c))).ok();
                let surf = bytes.and_then(|b| image_surface(gfx, &b, px));
                if self.clip_pv.thumbs.len() > 300 {
                    self.clip_pv.thumbs.clear();
                }
                self.clip_pv.thumbs.insert(c.id.clone(), surf);
            }
            if let Some(Some(surf)) = self.clip_pv.thumbs.get(&c.id) {
                let sp = self.sprite(surf, px, px, 0.0, 0.0)?;
                let g = self.comp.CreateRoundedRectangleGeometry()?;
                g.SetSize(v2(px, px))?;
                g.SetCornerRadius(v2(7.0 * self.s, 7.0 * self.s))?;
                sp.SetClip(&self.comp.CreateGeometricClipWithGeometry(&g)?)?;
                return sp.cast();
            }
            return self.glyph_box(gfx, '\u{EB9F}', px);
        }
        let glyph = match c.kind {
            _ if c.secret => '\u{E72E}',
            Kind::Vault => '\u{E72E}',
            Kind::Files => '\u{E8B7}',
            _ => '\u{E8A5}',
        };
        self.glyph_box(gfx, glyph, px)
    }

    /// Önizleme içeriğinin yüzeyi (son birkaç tanesi saklanır).
    fn preview_surface(&mut self, gfx: &Gfx, v: &ClipView, c: &Clip, key: &str, wrap_w: f32) -> Option<(CompositionDrawingSurface, f32, f32)> {
        if let Some((_, s, w, h)) = self.clip_pv.surfaces.iter().find(|x| x.0 == key) {
            return Some((s.clone(), *w, *h));
        }
        let made = match c.kind {
            Kind::Image => std::fs::read(v.store.image_path(c)).ok().and_then(|b| picture(gfx, &b)),
            Kind::Vault => {
                let note = format!(
                    "{MASK}{MASK}\n\n{}",
                    t!(
                        "Carl Vault kaydı. Değer panoda değil, kasada.\nEnter: kasa değeri bu panodan önceki pencereye yazar.\nTek kullanımlık; 60 sn içinde geçersiz olur ve geçmişe kaydedilmez.",
                        "Carl Vault item. The value is in the vault, not on the clipboard.\nEnter: the vault types it into the window you came from.\nOne-time; it expires within 60 s and isn't saved to history."
                    )
                );
                self.text.render_block(gfx, &note, 13.0 * self.s, Some(wrap_w), 8192.0, 0.6).ok().map(|r| (r.surface, r.width, r.height))
            }
            _ if c.secret => {
                let note = format!(
                    "{MASK}{MASK}\n\n{}\n{}",
                    t!("Şifre ya da anahtar gibi göründüğü için gizli.\nEnter ile yine yapıştırabilirsin.", "Hidden because it looks like a password or key.\nYou can still paste it with Enter."),
                    if c.pinned { t!("Sabitlendiği için silinmez.", "Pinned, so it won't be deleted.") } else { t!("Sabitlemezsen kısa süre sonra geçmişten silinir.", "Unless pinned, it is removed from history soon.") }
                );
                self.text.render_block(gfx, &note, 13.0 * self.s, Some(wrap_w), 8192.0, 0.6).ok().map(|r| (r.surface, r.width, r.height))
            }
            _ => {
                let mut t: String = c.text.chars().take(MAX_CHARS).collect();
                if c.text.chars().count() > MAX_CHARS {
                    t.push_str("\n…");
                }
                let t = t.replace('\t', "    ");
                self.text
                    .render_block(gfx, &t, 13.5 * self.s, (v.wrap || c.kind == Kind::Files).then_some(wrap_w), 8192.0, 0.92)
                    .ok()
                    .map(|r| (r.surface, r.width, r.height))
            }
        }?;
        if self.clip_pv.surfaces.len() >= 4 {
            self.clip_pv.surfaces.remove(0);
        }
        self.clip_pv.surfaces.push((key.to_string(), made.0.clone(), made.1, made.2));
        Some(made)
    }

    pub(super) fn build_clip(&mut self, gfx: &Gfx, kids: &VisualCollection, m: &Model) -> Result<(f32, f32)> {
        let s = self.s;
        let w = (W * s).round();
        let pad = (PAD * s).round();
        let fw = w - 2.0 * pad;
        let Some(v) = m.clip.as_ref() else { return Ok((w, (36.0 * s).round())) };

        // ---- Süzgeç kutusu ----
        let fy = (10.0 * s).round();
        let fh = (FIELD * s).round();
        let (field, _) = self.rrect(fw, fh, (13.0 * s * self.theme.radius_scale).round(), &self.solid(Rgba(255, 255, 255, 0x12))?)?;
        Self::place(&field, pad, fy)?;
        kids.InsertAtTop(&field)?;
        let g = self.text.glyph(gfx, '\u{E77F}', (16.0 * s).round(), 0.6)?;
        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + 14.0 * s, fy + (fh - g.height) / 2.0)?)?;
        let total = v.store.items.len();
        let count = if v.filter.is_empty() { t!(format!("{total} kayıt"), format!("{total} items")) } else { format!("{} / {total}", v.items.len()) };
        let (cl, cw, ch) = self.label(gfx, &count, 11.5, false, 120.0 * s, 0.45)?;
        Self::place(&cl, pad + fw - 14.0 * s - cw, fy + (fh - ch) / 2.0)?;
        kids.InsertAtTop(&cl)?;
        let tx = (pad + 42.0 * s).round();
        let tmax = fw - 42.0 * s - cw - 30.0 * s;
        let size = 14.0;
        if v.filter.is_empty() {
            let (p, _, ph) = self.label(gfx, t!("Panoda ara…", "Search clipboard…"), size, false, tmax, 0.4)?;
            Self::place(&p, tx, fy + (fh - ph) / 2.0)?;
            kids.InsertAtTop(&p)?;
        } else {
            let (q, _, qh) = self.label(gfx, v.filter, size, false, tmax, 0.96)?;
            Self::place(&q, tx, fy + (fh - qh) / 2.0)?;
            kids.InsertAtTop(&q)?;
        }
        let cx = tx + self.text.measure(v.filter, size * s, false).min(tmax);
        self.caret(kids, cx.round(), fy + (fh - 20.0 * s) / 2.0, (20.0 * s).round())?;

        // ---- Liste ----
        let by = fy + fh + (8.0 * s).round();
        let rh = (ROW * s).round();
        let bh = ROWS as f32 * rh;
        let lw = (LIST_W * s).round();
        let n = match v.mode {
            ClipMode::List => v.items.len(),
            ClipMode::Actions => v.actions.len(),
        };
        let shown = n.saturating_sub(v.scroll).min(ROWS);
        let sel = if v.mode == ClipMode::List { v.sel } else { v.act_sel };
        if sel >= v.scroll && sel < v.scroll + shown {
            self.highlight(kids, pad, by + (sel - v.scroll) as f32 * rh, lw, rh, (12.0 * s).round())?;
        }
        for r in 0..shown {
            let i = v.scroll + r;
            let ry = by + r as f32 * rh;
            let ipx = (28.0 * s).round();
            let x = pad + 10.0 * s + ipx + 10.0 * s;
            let tmax = pad + lw - x - 10.0 * s;
            match v.mode {
                ClipMode::List => {
                    let c = v.items[i];
                    let icon = self.clip_icon(gfx, v, c, ipx)?;
                    Self::place(&icon, pad + 10.0 * s, ry + (rh - ipx) / 2.0)?;
                    kids.InsertAtTop(&icon)?;
                    let pin_w = if c.pinned { 18.0 * s } else { 0.0 };
                    let (t, _, th) = self.label(gfx, &first_line(c), 13.0, true, tmax - pin_w, 0.95)?;
                    let sub = format!("{} · {}", ago(c.time), describe(c));
                    let (st, _, sh) = self.label(gfx, &sub, 11.0, false, tmax, 0.5)?;
                    let top = ry + (rh - th - sh) / 2.0;
                    Self::place(&t, x, top)?;
                    Self::place(&st, x, top + th)?;
                    kids.InsertAtTop(&t)?;
                    kids.InsertAtTop(&st)?;
                    if c.pinned {
                        let g = self.text.glyph(gfx, '\u{E718}', (12.0 * s).round(), 0.8)?;
                        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + lw - 10.0 * s - g.width, top + (th - g.height) / 2.0)?)?;
                    }
                }
                ClipMode::Actions => {
                    let a = &v.actions[i];
                    let g = self.text.glyph(gfx, a.glyph, (15.0 * s).round(), 0.85)?;
                    kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + 10.0 * s + (ipx - g.width) / 2.0, ry + (rh - g.height) / 2.0)?)?;
                    let confirm = v.confirm_clear && a.act == Act::Clear && i == v.act_sel;
                    let (k, kw, kh) = if a.key.is_empty() { (None, 0.0, 0.0) } else {
                        let (l, lw2, lh) = self.label(gfx, a.key, 10.5, false, 90.0 * s, 0.4)?;
                        (Some(l), lw2, lh)
                    };
                    let label = if confirm { t!("Onaylamak için tekrar Enter", "Press Enter again to confirm") } else { a.label.as_str() };
                    let (t, _, th) = self.label(gfx, label, 12.5, false, tmax - kw - 6.0 * s, if confirm { 1.0 } else { 0.92 })?;
                    Self::place(&t, x, ry + (rh - th) / 2.0)?;
                    kids.InsertAtTop(&t)?;
                    if let Some(k) = k {
                        Self::place(&k, pad + lw - 10.0 * s - kw, ry + (rh - kh) / 2.0)?;
                        kids.InsertAtTop(&k)?;
                    }
                }
            }
            self.hits.push((Hit::ClipRow(i), RECT { left: pad as i32, top: ry as i32, right: (pad + lw) as i32, bottom: (ry + rh) as i32 }));
        }
        if n == 0 {
            let msg = if v.filter.is_empty() { t!("Pano geçmişi boş.\nBir şey kopyalayınca burada görünür.", "Clipboard history is empty.\nCopied items will appear here.") } else { t!("Eşleşen kayıt yok.", "No matching items.") };
            let r = self.text.render_block(gfx, msg, 12.5 * s, Some(lw - 20.0 * s), 400.0 * s, 0.5)?;
            kids.InsertAtTop(&self.sprite(&r.surface, r.width, r.height, pad + 10.0 * s, by + 14.0 * s)?)?;
        }
        if n > ROWS {
            let track = bh - 8.0 * s;
            let th = (track * ROWS as f32 / n as f32).max(18.0 * s);
            let top = (track - th) * v.scroll as f32 / (n - ROWS) as f32;
            let (sb, _) = self.rrect((3.0 * s).round(), th.round(), 1.5 * s, &self.solid(Rgba(255, 255, 255, 0x40))?)?;
            Self::place(&sb, pad + lw + 2.0 * s, by + 4.0 * s + top)?;
            kids.InsertAtTop(&sb)?;
        }

        // ---- Önizleme ----
        let px0 = pad + lw + (GAP * s).round();
        let pw = pad + fw - px0;
        let (pbg, _) = self.rrect(pw, bh, (14.0 * s).round(), &self.solid(Rgba(255, 255, 255, 0x0C))?)?;
        Self::place(&pbg, px0, by)?;
        kids.InsertAtTop(&pbg)?;
        let current = v.items.get(v.sel).copied();
        let ph = (PV_HEAD * s).round();
        let (vx, vy, vw, vh) = (px0 + 6.0 * s, by + ph, pw - 12.0 * s, bh - ph - 6.0 * s);
        // Kart açıkken kaydırma durumu karta aittir; önizleme yalnızca çizilir.
        let card_open = self.clip_pv.card.is_some();
        if !card_open {
            self.clip_pv.inner = None;
            self.clip_pv.vbar = None;
            self.clip_pv.hbar = None;
            self.clip_pv.origin = (0.0, 0.0);
        }
        self.clip_pv.pane = (px0, by, pw, bh);
        self.clip_pv.pane_image = current.is_some_and(|c| c.kind == Kind::Image);
        if let Some(c) = current {
            // Başlık şeridi: tür + boyut solda, düğmeler sağda.
            let kind = match c.kind {
                Kind::Text => t!("Metin", "Text"),
                Kind::Image => t!("Resim", "Image"),
                Kind::Files => t!("Dosyalar", "Files"),
                Kind::Vault => "Carl Vault",
            };
            let mut buttons: Vec<(u8, char, f32)> = Vec::new();
            if c.kind == Kind::Text {
                buttons.push((0, '\u{E70F}', 0.85));
            }
            if c.kind == Kind::Image {
                buttons.push((3, '\u{E890}', 0.9));
            }
            // Kasa belirteci sabitlenemez.
            if c.kind != Kind::Vault {
                buttons.push((1, '\u{E718}', if c.pinned { 1.0 } else { 0.55 }));
            }
            buttons.push((2, '\u{E74D}', 0.7));
            let bs = (26.0 * s).round();
            let mut bx = px0 + pw - 6.0 * s;
            for (id, glyph, alpha) in buttons.iter().rev() {
                bx -= bs;
                // Silme sorusu açıkken çöp kutusu kırmızımsı yanar.
                let fill = if *id == 1 && c.pinned {
                    self.accent_brush()?
                } else if *id == 2 && v.confirm_delete {
                    self.solid(Rgba(232, 72, 72, 0x55))?
                } else {
                    self.solid(Rgba(255, 255, 255, 0x10))?
                };
                let (bg, _) = self.rrect(bs, bs, 8.0 * s, &fill)?;
                Self::place(&bg, bx, by + (ph - bs) / 2.0 + 1.0 * s)?;
                kids.InsertAtTop(&bg)?;
                let g = self.text.glyph(gfx, *glyph, (13.0 * s).round(), *alpha)?;
                kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, bx + (bs - g.width) / 2.0, by + (ph - g.height) / 2.0 + 1.0 * s)?)?;
                self.hits.push((Hit::ClipBtn(*id), RECT { left: bx as i32, top: by as i32, right: (bx + bs) as i32, bottom: (by + ph) as i32 }));
                bx -= 4.0 * s;
            }
            let info = format!("{kind} · {}", describe(c));
            let (il, _, ih) = self.label(gfx, &info, 11.5, false, bx - px0 - 20.0 * s, 0.55)?;
            Self::place(&il, px0 + 12.0 * s, by + (ph - ih) / 2.0 + 1.0 * s)?;
            kids.InsertAtTop(&il)?;

            // İçerik: kırpılmış pencere içinde kaydırılan yüzey.
            let inset = if c.kind == Kind::Image { 0.0 } else { (8.0 * s).round() };
            let key = format!("{}:{}:{}:{}:{}", c.id, c.hash, v.wrap, vw as i32, c.secret as u8 + c.pinned as u8 * 2);
            if let Some((surf, sw, sh)) = self.preview_surface(gfx, v, c, &key, vw - 2.0 * inset) {
                let vp = self.comp.CreateContainerVisual()?;
                vp.SetSize(v2(vw, vh))?;
                Self::place(&vp, vx, vy)?;
                let g = self.comp.CreateRoundedRectangleGeometry()?;
                g.SetSize(v2(vw, vh))?;
                g.SetCornerRadius(v2(10.0 * s, 10.0 * s))?;
                vp.SetClip(&self.comp.CreateGeometricClipWithGeometry(&g)?)?;
                let inner = self.comp.CreateContainerVisual()?;
                let (cw, ch) = (sw + 2.0 * inset, sh + 2.0 * inset);
                inner.SetSize(v2(cw, ch))?;
                inner.Children()?.InsertAtTop(&self.sprite(&surf, sw, sh, inset, inset)?)?;
                vp.Children()?.InsertAtTop(&inner)?;
                kids.InsertAtTop(&vp)?;
                self.clip_pv.pane_key = key.clone();
                if c.kind == Kind::Image {
                    // Resim burada sabit, sığdırılmış durur; büyütmek için göz düğmesi.
                    let fit = (vw / cw).min(vh / ch).min(1.0);
                    inner.SetScale(Vector3 { X: fit, Y: fit, Z: 1.0 })?;
                    inner.SetOffset(v3(((vw - cw * fit) / 2.0).round(), ((vh - ch * fit) / 2.0).round()))?;
                } else if card_open {
                    let p = &self.clip_pv;
                    inner.SetOffset(v3(-p.x.max(0.0), -p.y.max(0.0)))?;
                } else {
                    let bar = |me: &Self| me.rrect((4.0 * s).round(), (4.0 * s).round(), 2.0 * s, &me.solid(Rgba(255, 255, 255, 0x55))?);
                    let vb = bar(self)?;
                    let hb = bar(self)?;
                    kids.InsertAtTop(&vb.0)?;
                    kids.InsertAtTop(&hb.0)?;
                    let p = &mut self.clip_pv;
                    if p.key != key {
                        p.key = key;
                        p.zoom = 1.0;
                        p.x = 0.0;
                        p.y = 0.0;
                    }
                    (p.cw, p.ch, p.vw, p.vh, p.ox, p.oy, p.fit, p.image) = (cw, ch, vw, vh, vx, vy, 1.0, false);
                    p.zoom = 1.0;
                    p.inner = Some(inner);
                    p.vbar = Some(vb);
                    p.hbar = Some(hb);
                    self.pv_apply(false);
                }
            } else {
                let (l, lw2, lh) = self.label(gfx, t!("Önizleme yüklenemedi", "Preview could not be loaded"), 12.5, false, vw, 0.5)?;
                Self::place(&l, vx + (vw - lw2) / 2.0, vy + (vh - lh) / 2.0)?;
                kids.InsertAtTop(&l)?;
            }
            if v.confirm_delete {
                self.delete_confirm(gfx, &kids, px0 + pw - 6.0 * s, by + ph + 2.0 * s)?;
            }
            self.hits.push((Hit::Preview, RECT { left: vx as i32, top: vy as i32, right: (vx + vw) as i32, bottom: (vy + vh) as i32 }));
        }

        // ---- İpucu satırı ----
        let mut y = by + bh + (8.0 * s).round();
        let hh = (HINT * s).round();
        let (sep, _) = self.rrect(fw, 1.0, 0.0, &self.solid(Rgba(255, 255, 255, 0x14))?)?;
        Self::place(&sep, pad, y)?;
        kids.InsertAtTop(&sep)?;
        if v.undo {
            // "Kayıt silindi   Ctrl+Z  [↶ Geri al]": birkaç saniye açık kalır.
            let g = self.text.glyph(gfx, '\u{E74D}', (11.0 * s).round(), 0.6)?;
            kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + 8.0 * s, y + (hh - g.height) / 2.0)?)?;
            let (l, _, lh) = self.label(gfx, v.hint, 11.0, false, fw * 0.5, 0.75)?;
            Self::place(&l, pad + 14.0 * s + g.width, y + (hh - lh) / 2.0)?;
            kids.InsertAtTop(&l)?;
            let (bl, blw, blh) = self.label(gfx, t!("Geri al", "Undo"), 11.0, true, 100.0 * s, 1.0)?;
            let ug = self.text.glyph(gfx, '\u{E7A7}', (11.0 * s).round(), 1.0)?;
            let bh2 = (20.0 * s).round();
            let bw = (blw + ug.width + 22.0 * s).round();
            let bx = pad + fw - 4.0 * s - bw;
            let bty = y + (hh - bh2) / 2.0;
            let (bg, _) = self.rrect(bw, bh2, bh2 / 2.0, &self.accent_brush()?)?;
            Self::place(&bg, bx, bty)?;
            kids.InsertAtTop(&bg)?;
            kids.InsertAtTop(&self.sprite(&ug.surface, ug.width, ug.height, bx + 8.0 * s, bty + (bh2 - ug.height) / 2.0)?)?;
            Self::place(&bl, bx + 12.0 * s + ug.width, bty + (bh2 - blh) / 2.0)?;
            kids.InsertAtTop(&bl)?;
            let (kl, klw, klh) = self.label(gfx, "Ctrl+Z", 10.5, false, 80.0 * s, 0.42)?;
            Self::place(&kl, bx - 8.0 * s - klw, y + (hh - klh) / 2.0)?;
            kids.InsertAtTop(&kl)?;
            self.hits.push((Hit::ClipBtn(6), RECT { left: (bx - 4.0 * s) as i32, top: y as i32, right: (bx + bw + 4.0 * s) as i32, bottom: (y + hh) as i32 }));
        } else {
            let (l, _, lh) = self.label(gfx, v.hint, 10.5, false, fw - 16.0 * s, 0.42)?;
            Self::place(&l, pad + 8.0 * s, y + (hh - lh) / 2.0)?;
            kids.InsertAtTop(&l)?;
        }
        y += hh;
        Ok((w, (y + 4.0 * s).round()))
    }

    /// Kaydırma/yakınlaştırma durumunu görsellere uygular.
    fn pv_apply(&mut self, animate: bool) {
        let s = self.s;
        let p = &mut self.clip_pv;
        let Some(inner) = p.inner.clone() else { return };
        let (sw, sh) = (p.cw * p.zoom, p.ch * p.zoom);
        p.x = p.x.clamp(0.0, (sw - p.vw).max(0.0));
        p.y = p.y.clamp(0.0, (sh - p.vh).max(0.0));
        let tx = if sw < p.vw { if p.image { ((p.vw - sw) / 2.0).round() } else { 0.0 } } else { -p.x };
        let ty = if sh < p.vh { if p.image { ((p.vh - sh) / 2.0).round() } else { 0.0 } } else { -p.y };
        let _ = inner.SetScale(Vector3 { X: p.zoom, Y: p.zoom, Z: 1.0 });
        let _ = inner.StopAnimation(h!("Offset.X"));
        let _ = inner.StopAnimation(h!("Offset.Y"));
        if animate {
            let _ = self.motion.animate(&inner, h!("Offset.X"), tx, 1.0, 30, 0);
            let _ = self.motion.animate(&inner, h!("Offset.Y"), ty, 1.0, 30, 0);
        } else {
            let _ = inner.SetOffset(v3(tx, ty));
        }
        // Kaydırma göstergeleri (yalnızca taşan yönde).
        let th = 4.0 * s;
        if let Some((vb, g)) = &p.vbar {
            if sh > p.vh + 0.5 {
                let len = (p.vh * p.vh / sh).max(20.0 * s);
                let pos = (p.vh - len) * p.y / (sh - p.vh);
                let _ = vb.SetSize(v2(th, len));
                let _ = g.SetSize(v2(th, len));
                let _ = vb.SetOffset(v3(p.ox + p.vw - th - 2.0 * s, p.oy + pos));
                let _ = vb.SetOpacity(1.0);
            } else {
                let _ = vb.SetOpacity(0.0);
            }
        }
        if let Some((hb, g)) = &p.hbar {
            if sw > p.vw + 0.5 {
                let len = (p.vw * p.vw / sw).max(20.0 * s);
                let pos = (p.vw - len) * p.x / (sw - p.vw);
                let _ = hb.SetSize(v2(len, th));
                let _ = g.SetSize(v2(len, th));
                let _ = hb.SetOffset(v3(p.ox + pos, p.oy + p.vh - th - 2.0 * s));
                let _ = hb.SetOpacity(1.0);
            } else {
                let _ = hb.SetOpacity(0.0);
            }
        }
    }

    /// Pencere koordinatı önizleme penceresinin içindeyse oradaki yerel nokta.
    pub fn preview_point(&self, x: i32, y: i32) -> Option<(f32, f32)> {
        let p = &self.clip_pv;
        p.inner.as_ref()?;
        let lx = x as f32 - (self.win_w - self.size.0) / 2.0 - p.origin.0 - p.ox;
        let ly = y as f32 - self.top - p.origin.1 - p.oy;
        (lx >= 0.0 && ly >= 0.0 && lx < p.vw && ly < p.vh).then_some((lx, ly))
    }

    /// Önizlemeyi kaydırır (piksel). Bir şey değiştiyse true.
    pub fn preview_scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.clip_pv.inner.is_none() {
            return false;
        }
        let (x, y) = (self.clip_pv.x, self.clip_pv.y);
        self.clip_pv.x += dx;
        self.clip_pv.y += dy;
        self.pv_apply(true);
        (x, y) != (self.clip_pv.x, self.clip_pv.y)
    }

    /// Yakınlaştır/uzaklaştır; `at`: sabit kalacak nokta (önizleme içinde), yoksa orta.
    pub fn preview_zoom(&mut self, factor: f32, at: Option<(f32, f32)>) {
        let p = &mut self.clip_pv;
        if p.inner.is_none() || !p.image {
            return;
        }
        let (ax, ay) = at.unwrap_or((p.vw / 2.0, p.vh / 2.0));
        let old = p.zoom;
        let new = if factor == 0.0 { p.fit } else { (old * factor).clamp(p.fit.min(1.0) * 0.5, 8.0) };
        // Görünen noktanın içerikteki karşılığı sabit kalsın.
        let (sw, sh) = (p.cw * old, p.ch * old);
        let cx = if sw < p.vw { (ax - (p.vw - sw) / 2.0) / old } else { (p.x + ax) / old };
        let cy = if sh < p.vh { (ay - (p.vh - sh) / 2.0) / old } else { (p.y + ay) / old };
        p.zoom = new;
        p.x = cx * new - ax;
        p.y = cy * new - ay;
        self.pv_apply(false);
    }

    pub fn preview_drag_start(&mut self, x: i32, y: i32) {
        self.clip_pv.drag = Some((x, y, self.clip_pv.x, self.clip_pv.y));
    }

    pub fn preview_drag(&mut self, x: i32, y: i32) {
        let Some((x0, y0, px, py)) = self.clip_pv.drag else { return };
        self.clip_pv.x = px - (x - x0) as f32;
        self.clip_pv.y = py - (y - y0) as f32;
        self.pv_apply(false);
    }

    /// Sürükleme bitti; fare neredeyse kımıldamadıysa (tıklamaysa) true.
    pub fn preview_drag_end(&mut self, x: i32, y: i32) -> bool {
        let Some((x0, y0, ..)) = self.clip_pv.drag.take() else { return true };
        let lim = (4.0 * self.s) as i32;
        (x - x0).abs() <= lim && (y - y0).abs() <= lim
    }

    pub fn preview_debug(&self) -> serde_json::Value {
        let p = &self.clip_pv;
        serde_json::json!({
            "has": p.inner.is_some(), "image": p.image, "zoom": p.zoom, "fit": p.fit,
            "x": p.x, "y": p.y, "content": [p.cw, p.ch], "viewport": [p.vw, p.vh],
            "pane_image": p.pane_image,
            "card": p.card.as_ref().map(|c| serde_json::json!({ "to": [c.to.0, c.to.1, c.to.2, c.to.3] })),
        })
    }

    // ---- Kart ----

    /// Kartın iskeleti: gölgeli katman, içinde yuvarlak kırpılmış koyu zemin.
    /// Zeminin ve kırpmanın boyutu katmanın boyutunu izler (boyut animasyonu için).
    /// Çöp kutusunun altında açılan ikinci onay: "Silinsin mi?" + ✓ (sil) / ✗ (vazgeç).
    /// `right`/`top`: balonun sağ üst köşesi (çöp kutusunun hemen altı).
    fn delete_confirm(&mut self, gfx: &Gfx, kids: &VisualCollection, right: f32, top: f32) -> Result<()> {
        let s = self.s;
        let (bs, gap, padx) = ((26.0 * s).round(), (6.0 * s).round(), (10.0 * s).round());
        let bh = (36.0 * s).round();
        let (l, lw, lh) = self.label(gfx, t!("Silinsin mi?", "Delete?"), 12.0, true, 120.0 * s, 0.92)?;
        let w = (padx + lw + 10.0 * s + bs * 2.0 + gap + 5.0 * s).round();
        let x = (right - w).round();
        let root = self.comp.CreateContainerVisual()?;
        root.SetSize(v2(w, bh))?;
        // Sahte gölge + ince kenar: iki katman yuvarlak dikdörtgen.
        for (k, a) in [(6.0, 0x14u8), (3.0, 0x22)] {
            let k = k * s;
            let (sh, _) = self.rrect(w + 2.0 * k, bh + 2.0 * k, 12.0 * s + k, &self.solid(Rgba(0, 0, 0, a))?)?;
            Self::place(&sh, -k, -k + 3.0 * s)?;
            root.Children()?.InsertAtTop(&sh)?;
        }
        let (edge, _) = self.rrect(w, bh, 12.0 * s, &self.solid(Rgba(255, 255, 255, 0x30))?)?;
        root.Children()?.InsertAtTop(&edge)?;
        let (bg, _) = self.rrect(w - 2.0 * s, bh - 2.0 * s, 11.0 * s, &self.solid(Rgba(36, 36, 40, 0xF6))?)?;
        Self::place(&bg, 1.0 * s, 1.0 * s)?;
        root.Children()?.InsertAtTop(&bg)?;
        Self::place(&l, padx, (bh - lh) / 2.0)?;
        root.Children()?.InsertAtTop(&l)?;
        // Sağdan sola: ✗ en sağda (çöp kutusunun tam altı), ✓ onun solunda.
        let by = (bh - bs) / 2.0;
        let mut bx = w - 5.0 * s;
        for (id, glyph, fill) in [(5u8, '\u{E711}', Rgba(255, 255, 255, 0x18)), (4u8, '\u{E73E}', Rgba(232, 72, 72, 0xE0))] {
            bx -= bs;
            let (b, _) = self.rrect(bs, bs, bs / 2.0, &self.solid(fill)?)?;
            Self::place(&b, bx, by)?;
            root.Children()?.InsertAtTop(&b)?;
            let g = self.text.glyph(gfx, glyph, (12.0 * s).round(), 1.0)?;
            root.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, bx + (bs - g.width) / 2.0, by + (bs - g.height) / 2.0)?)?;
            let (hx, hy) = (x + bx, top + by);
            self.hits.push((Hit::ClipBtn(id), RECT { left: (hx - 2.0 * s) as i32, top: (hy - 3.0 * s) as i32, right: (hx + bs + 2.0 * s) as i32, bottom: (hy + bs + 3.0 * s) as i32 }));
            bx -= gap;
        }
        // Balonun kendisi önizlemeye tıklama sayılmasın.
        self.hits.push((Hit::Card, RECT { left: x as i32, top: top as i32, right: (x + w) as i32, bottom: (top + bh) as i32 }));
        // Girişte çöp kutusundan aşağı hafifçe süzülür.
        root.SetOffset(v3(x, top - 6.0 * s))?;
        root.SetOpacity(0.0)?;
        kids.InsertAtTop(&root)?;
        self.motion.animate(&root, h!("Offset.Y"), top, 0.75, 40, 0)?;
        self.motion.fade(&root, h!("Opacity"), 1.0, 120, 0)?;
        Ok(())
    }

    fn card_shell(&self, w: f32, h: f32) -> Result<(LayerVisual, ContainerVisual)> {
        let s = self.s;
        let layer = self.comp.CreateLayerVisual()?;
        layer.SetSize(v2(w, h))?;
        let r = (18.0 * s).round();
        // Yumuşak gölge: kartın etrafında büyüyen, çok saydam halkalar.
        // (LayerVisual'ın DropShadow'u bu pencerede içeriği siyaha boyuyor.)
        for (k, a) in [(3.0, 0x22u8), (8.0, 0x18), (15.0, 0x10), (24.0, 0x09)] {
            let k = k * s;
            let grow = self.comp.CreateExpressionAnimationWithExpression(h!("r.Size + Vector2(K, K)"))?;
            grow.SetReferenceParameter(h!("r"), &layer)?;
            grow.SetScalarParameter(h!("K"), 2.0 * k)?;
            let g = self.comp.CreateRoundedRectangleGeometry()?;
            g.SetCornerRadius(v2(r + k, r + k))?;
            g.SetSize(v2(w + 2.0 * k, h + 2.0 * k))?;
            g.StartAnimation(h!("Size"), &grow)?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&g)?;
            shape.SetFillBrush(&self.solid(Rgba(0, 0, 0, a))?)?;
            let sv = self.comp.CreateShapeVisual()?;
            sv.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            sv.SetSize(v2(2.0 * k, 2.0 * k))?;
            sv.SetOffset(v3(-k, -k + 6.0 * s))?;
            sv.Shapes()?.Append(&shape)?;
            layer.Children()?.InsertAtTop(&sv)?;
        }
        let follow = self.comp.CreateExpressionAnimationWithExpression(h!("r.Size"))?;
        follow.SetReferenceParameter(h!("r"), &layer)?;
        let geom = || -> Result<CompositionRoundedRectangleGeometry> {
            let g = self.comp.CreateRoundedRectangleGeometry()?;
            g.SetSize(v2(w, h))?;
            g.SetCornerRadius(v2(r, r))?;
            g.StartAnimation(h!("Size"), &follow)?;
            Ok(g)
        };
        let body = self.comp.CreateContainerVisual()?;
        body.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        body.SetClip(&self.comp.CreateGeometricClipWithGeometry(&geom()?)?)?;
        let shape = self.comp.CreateSpriteShapeWithGeometry(&geom()?)?;
        shape.SetFillBrush(&self.solid(CARD_BG)?)?;
        shape.SetStrokeBrush(&self.solid(Rgba(255, 255, 255, 0x2E))?)?;
        shape.SetStrokeThickness(2.0 * s)?;
        let sv = self.comp.CreateShapeVisual()?;
        sv.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        sv.Shapes()?.Append(&shape)?;
        body.Children()?.InsertAtTop(&sv)?;
        layer.Children()?.InsertAtTop(&body)?;
        Ok((layer, body))
    }

    /// Düzenleme kartını açar: önizleme kutusu kendi yerinden biraz taşarak
    /// (panelin içinde kalarak) büyür. Açıldıysa true.
    pub fn open_card(&mut self, gfx: &Gfx) -> Result<bool> {
        if self.clip_pv.card.is_some() || self.view != View::Clip {
            return Ok(false);
        }
        let s = self.s;
        let (px, py, pw, ph) = self.clip_pv.pane;
        let key = self.clip_pv.pane_key.clone();
        let surf = self.clip_pv.surfaces.iter().find(|x| x.0 == key).map(|x| (x.1.clone(), x.2, x.3));
        self.drop_old_cards();
        let ox = ((self.win_w - self.size.0) / 2.0).round();
        self.overlay.SetOffset(v3(ox, self.top))?;
        let mo = self.motion.clone();
        let head = (CARD_HEAD * s).round();
        // Sola listeye doğru ve üst/alt birkaç piksel taşar; panelin kenarını geçmez.
        let tx = (px - 46.0 * s).round();
        let ty = (py - 12.0 * s).round();
        let tw = (self.size.0 - tx - 6.0 * s).round();
        let th = (ph + 24.0 * s).round();
        let (layer, body) = self.card_shell(pw, ph)?;
        let kids = body.Children()?;
        let (l, _, lh) = self.label(gfx, t!("Düzenle", "Edit"), 12.0, true, 120.0 * s, 0.85)?;
        Self::place(&l, 14.0 * s, (head - lh) / 2.0)?;
        kids.InsertAtTop(&l)?;
        // Sağ üstte Kaydet ve X; kartın sağ kenarına bağlı (genişlerken kenarla gelir).
        let bh = (24.0 * s).round();
        let by = ((head - bh) / 2.0).round();
        let (save_l, slw, slh) = self.label(gfx, t!("Kaydet", "Save"), 11.5, true, 100.0 * s, 1.0)?;
        let sw = (slw + 22.0 * s).round();
        let xw = bh;
        let gap = 6.0 * s;
        let right = 8.0 * s;
        let x_x = -(right + xw);
        let save_x = x_x - gap - sw;
        let (xbg, _) = self.rrect(xw, bh, bh / 2.0, &self.solid(Rgba(255, 255, 255, 0x18))?)?;
        xbg.SetRelativeOffsetAdjustment(v3(1.0, 0.0))?;
        xbg.SetOffset(v3(x_x.round(), by))?;
        kids.InsertAtTop(&xbg)?;
        let g = self.text.glyph(gfx, '\u{E711}', (10.0 * s).round(), 0.9)?;
        let gv = self.sprite(&g.surface, g.width, g.height, 0.0, 0.0)?;
        gv.SetRelativeOffsetAdjustment(v3(1.0, 0.0))?;
        gv.SetOffset(v3((x_x + (xw - g.width) / 2.0).round(), (by + (bh - g.height) / 2.0).round()))?;
        kids.InsertAtTop(&gv)?;
        let (sbg, _) = self.rrect(sw, bh, bh / 2.0, &self.accent_brush()?)?;
        sbg.SetRelativeOffsetAdjustment(v3(1.0, 0.0))?;
        sbg.SetOffset(v3(save_x.round(), by))?;
        kids.InsertAtTop(&sbg)?;
        save_l.SetRelativeOffsetAdjustment(v3(1.0, 0.0))?;
        save_l.SetOffset(v3((save_x + (sw - slw) / 2.0).round(), (by + (bh - slh) / 2.0).round()))?;
        kids.InsertAtTop(&save_l)?;
        let ghost = match surf {
            Some((surf, sw, sh)) => {
                let (gx, gy) = (self.clip_pv.x.max(0.0), self.clip_pv.y.max(0.0));
                let v = self.sprite(&surf, sw, sh, 14.0 * s - gx, head + 8.0 * s - gy)?;
                kids.InsertAtTop(&v)?;
                Some(v)
            }
            None => None,
        };
        layer.SetOffset(v3(px, py))?;
        mo.animate(&layer, h!("Offset.X"), tx, 0.78, 30, 0)?;
        mo.animate(&layer, h!("Offset.Y"), ty, 0.78, 30, 0)?;
        mo.animate(&layer, h!("Size.X"), tw, 0.78, 32, 0)?;
        mo.animate(&layer, h!("Size.Y"), th, 0.78, 34, 0)?;
        let edit = (tx + 6.0 * s, ty + head, tw - 12.0 * s, th - head - 8.0 * s);
        // Düğmelerin tıklama alanları (içerik koordinatı, hedef boyuta göre).
        let bx = |off: f32, w: f32| RECT { left: (tx + tw + off) as i32, top: (ty + by) as i32, right: (tx + tw + off + w) as i32, bottom: (ty + by + bh) as i32 };
        let buttons = vec![(0u8, bx(save_x, sw)), (1u8, bx(x_x, xw))];
        let card = Card { root: layer, to: (tx, ty, tw, th), ghost, edit, buttons };
        self.overlay.Children()?.InsertAtTop(&card.root)?;
        self.clip_pv.card = Some(card);
        if let Some(r) = self.card_region() {
            self.set_region(Some((r, false)));
        }
        Ok(true)
    }

    /// Kartı kapatır (`animate`: önizleme kutusuna geri küçülerek).
    pub fn close_card(&mut self, animate: bool) -> bool {
        let Some(card) = self.clip_pv.card.take() else { return false };
        let (px, py, pw, ph) = self.clip_pv.pane;
        let layer = card.root;
        if animate {
            let mo = self.motion.clone();
            let _ = mo.animate(&layer, h!("Offset.X"), px, 1.0, 28, 0);
            let _ = mo.animate(&layer, h!("Offset.Y"), py, 1.0, 28, 0);
            let _ = mo.animate(&layer, h!("Size.X"), pw, 1.0, 28, 0);
            let _ = mo.animate(&layer, h!("Size.Y"), ph, 1.0, 28, 0);
            let _ = mo.fade(&layer, h!("Opacity"), 0.0, 110, 60);
            self.clip_pv.card_old.push(layer);
            unsafe {
                SetTimer(self.hwnd, TIMER_REGION, 300, None);
            }
        } else {
            if let Ok(k) = self.overlay.Children() {
                let _ = k.Remove(&layer);
            }
            if self.view != View::Hidden {
                self.region_done();
            }
        }
        true
    }

    /// Panel kapanırken kart da onunla birlikte solarak gider (bir anda yok olmasın).
    pub fn fade_card(&mut self) -> bool {
        let Some(card) = self.clip_pv.card.take() else { return false };
        if let Some(g) = card.ghost.as_ref() {
            let _ = g.SetOpacity(1.0);
        }
        let _ = self.motion.fade(&card.root, h!("Opacity"), 0.0, 120, 0);
        self.clip_pv.card_old.push(card.root);
        true
    }

    pub fn preview_is_image(&self) -> bool {
        self.clip_pv.pane_image
    }

    /// Önizlemedeki resmin yüzeyi ve ekrandaki yeri (pencere istemci koordinatı).
    pub fn preview_picture(&self) -> Option<(CompositionDrawingSurface, f32, f32, RECT)> {
        let p = &self.clip_pv;
        if !p.pane_image {
            return None;
        }
        let (surf, w, h) = p.surfaces.iter().find(|x| x.0 == p.pane_key).map(|x| (x.1.clone(), x.2, x.3))?;
        let ox = ((self.win_w - self.size.0) / 2.0).round();
        let (px, py, pw, ph) = p.pane;
        let (vx, vy, vw, vh) = (px + 6.0 * self.s, py + (PV_HEAD * self.s).round(), pw - 12.0 * self.s, ph - (PV_HEAD * self.s).round() - 6.0 * self.s);
        let fit = (vw / w).min(vh / h).min(1.0);
        let (iw, ih) = (w * fit, h * fit);
        let (ix, iy) = (ox + vx + (vw - iw) / 2.0, self.top + vy + (vh - ih) / 2.0);
        Some((surf, w, h, RECT { left: ix as i32, top: iy as i32, right: (ix + iw) as i32, bottom: (iy + ih) as i32 }))
    }

    pub fn card_open(&self) -> bool {
        self.clip_pv.card.is_some()
    }

    /// Düzenleyici geldi: kartın içindeki metin görüntüsünü gizle.
    pub fn card_ghost(&self, on: bool) {
        if let Some(g) = self.clip_pv.card.as_ref().and_then(|c| c.ghost.as_ref()) {
            let _ = g.SetOpacity(if on { 1.0 } else { 0.0 });
        }
    }

    /// Düzenleyicinin yeri (pencere istemci koordinatı).
    pub fn card_edit_rect(&self) -> Option<RECT> {
        let c = self.clip_pv.card.as_ref()?;
        let ox = ((self.win_w - self.size.0) / 2.0).round();
        let (x, y, w, h) = c.edit;
        Some(RECT { left: (ox + x) as i32, top: (self.top + y) as i32, right: (ox + x + w) as i32, bottom: (self.top + y + h) as i32 })
    }

    pub(super) fn drop_old_cards(&mut self) {
        if let Ok(k) = self.overlay.Children() {
            for o in self.clip_pv.card_old.drain(..) {
                let _ = k.Remove(&o);
            }
        }
    }

    /// Kart açıkken tıklama bölgesi: hap + kart + gölge payı.
    pub(super) fn card_region(&self) -> Option<RECT> {
        let c = self.clip_pv.card.as_ref()?;
        let ox = ((self.win_w - self.size.0) / 2.0).round();
        let m = CARD_SHADOW * self.s;
        let pill = self.pill_rect();
        let (x, y, w, h) = c.to;
        Some(RECT {
            left: pill.left.min((ox + x - m).max(0.0) as i32),
            top: pill.top.min((self.top + y - m).max(0.0) as i32),
            right: pill.right.max((ox + x + w + m).min(self.win_w) as i32),
            bottom: pill.bottom.max((self.top + y + h + m).min(self.win_h) as i32),
        })
    }

    /// Kart açıkken: düğmeler `Hit::CardBtn`, kartın üstü `Hit::Card`, dışı `Hit::None`.
    pub(super) fn card_hit(&self, x: i32, y: i32) -> Option<Hit> {
        let c = self.clip_pv.card.as_ref()?;
        let lx = x - ((self.win_w - self.size.0) / 2.0).round() as i32;
        let ly = y - self.top as i32;
        if let Some((id, _)) = c.buttons.iter().find(|(_, r)| lx >= r.left && lx < r.right && ly >= r.top && ly < r.bottom) {
            return Some(Hit::CardBtn(*id));
        }
        let (tx, ty, tw, th) = c.to;
        let (lx, ly) = (lx as f32, ly as f32);
        Some(if lx >= tx && ly >= ty && lx < tx + tw && ly < ty + th { Hit::Card } else { Hit::None })
    }
}
