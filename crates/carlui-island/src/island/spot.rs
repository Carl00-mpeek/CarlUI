//! Bu modül adanın arama görünümünü çiziyor. Ada aşağı doğru
//! büyür; en üst şeritte saat ve çalan şarkı kalır, altında arama kutusu,
//! onun altında sonuçlar. Panel sonuç sayısına göre spring ile büyüyüp küçülür;
//! en fazla 8 satır görünür, fazlası kaydırılır. Seçim vurgusu satırdan satıra
//! kayar. Boş aramada sık kullanılan uygulamalar ve kısayollar gösterilir.
//!
//! Her değişiklikte (tuş, seçim) içerik yeniden kurulur; bu, birkaç küçük metin
//! yüzeyi demektir (milisaniyenin altında). Animasyonlar compositor'de çalışır.

use super::*;
use carlui_launcher::search::{Icon, Item};

pub const ROWS_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpotMode {
    /// Boş arama: kutucuklar + kısayollar.
    Home,
    /// Arama sonuçları ya da tüm uygulamalar.
    List,
    /// Seçili sonucun eylemleri.
    Actions,
}

pub struct SpotView<'a> {
    pub query: &'a str,
    /// İmlecin karakter konumu.
    pub caret: usize,
    pub all_selected: bool,
    pub mode: SpotMode,
    pub items: &'a [Item],
    pub tiles: &'a [Item],
    pub chips: &'a [Item],
    pub sel: usize,
    pub scroll: usize,
    /// Onay bekleyen sonuç (yeniden başlat/kapat).
    pub confirm: Option<usize>,
    pub hint: &'a str,
    pub placeholder: &'a str,
}

// Yerleşim (DIP)
const W: f32 = 640.0;
const PAD: f32 = 12.0;
const HEAD: f32 = 36.0;
const FIELD: f32 = 48.0;
const ROW: f32 = 46.0;
const TILE_W: f32 = 98.0;
const TILE_H: f32 = 88.0;
const CHIP_H: f32 = 36.0;
const HINT: f32 = 28.0;
/// Yazı kutusu en fazla bu kadar satıra büyür; fazlası imleçle kayar.
const FIELD_LINES: u32 = 4;

impl Island {
    /// Shell ikonunun yüzeyi; yoksa yükleme isteği bırakır.
    fn shell_icon(&mut self, path: &str, px: u32) -> Option<CompositionDrawingSurface> {
        let key = (path.to_string(), px);
        match self.icons.get(&key) {
            Some(s) => s.clone(),
            None => {
                if self.icon_pending.insert(key.clone()) {
                    self.icon_requests.push(key);
                }
                None
            }
        }
    }

    /// Yükleyici iş parçacığından gelen pikselleri yüzeye çevirip önbelleğe koyar.
    pub fn add_icon(&mut self, gfx: &Gfx, path: String, px: u32, w: u32, h: u32, pixels: &[u8]) {
        let surface = if pixels.is_empty() || w == 0 {
            None
        } else {
            let size = px as f32;
            gfx.surface(size, size, |dc| unsafe {
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    ..Default::default()
                };
                let bmp = dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(pixels.as_ptr() as _), w * 4, &props)?;
                let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: size, bottom: size };
                dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
                Ok(())
            })
            .ok()
        };
        self.icon_pending.remove(&(path.clone(), px));
        if self.icons.len() > 600 {
            self.icons.clear();
        }
        self.icons.insert((path, px), surface);
    }

    /// Sonuç ikonu: uygulama/dosya ikonu ya da vurgu renkli karede simge.
    fn item_icon(&mut self, gfx: &Gfx, icon: &Icon, px: f32) -> Result<Visual> {
        if let Icon::Shell(path) = icon {
            if let Some(surf) = self.shell_icon(path, px as u32) {
                return self.sprite(&surf, px, px, 0.0, 0.0)?.cast();
            }
            if !self.icons.contains_key(&(path.clone(), px as u32)) {
                // Yükleniyor: soluk yer tutucu (ikon gelince yerine oturur).
                let (ph, _) = self.rrect(px, px, px * 0.26, &self.solid(Rgba(255, 255, 255, 0x12))?)?;
                return ph.cast();
            }
        }
        let glyph = match icon {
            Icon::Glyph(c) => *c,
            Icon::Shell(p) if std::path::Path::new(p).is_dir() => '\u{E8B7}',
            Icon::Shell(_) => '\u{E7C3}',
        };
        let c = self.comp.CreateContainerVisual()?;
        let (bg, _) = self.rrect(px, px, (px * 0.28).round(), &self.accent_brush()?)?;
        c.Children()?.InsertAtTop(&bg)?;
        let g = self.text.glyph(gfx, glyph, (px * 0.5).round(), 0.95)?;
        c.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (px - g.width) / 2.0, (px - g.height) / 2.0)?)?;
        c.cast()
    }

    /// Seçim vurgusu: önceki konumundan yenisine spring ile kayar.
    pub(super) fn highlight(&mut self, kids: &VisualCollection, x: f32, y: f32, w: f32, h: f32, r: f32) -> Result<()> {
        let (hl, _) = self.rrect(w, h, r, &self.solid(Rgba(255, 255, 255, 0x1C))?)?;
        let (bar, _) = self.rrect((3.0 * self.s).round(), (h * 0.42).round(), 1.5 * self.s, &self.accent_brush()?)?;
        bar.SetOffset(v3((4.0 * self.s).round(), ((h - h * 0.42) / 2.0).round()))?;
        if w < h * 3.0 {
            bar.SetOpacity(0.0)?;
        }
        hl.Children()?.InsertAtTop(&bar)?;
        let (px, py) = self.spot_hl;
        let same_shape = self.spot_hl_size == (w, h);
        if same_shape && px >= 0.0 {
            hl.SetOffset(v3(px, py))?;
            self.motion.animate(&hl, h!("Offset.X"), x, 0.82, 45, 0)?;
            self.motion.animate(&hl, h!("Offset.Y"), y, 0.82, 45, 0)?;
        } else {
            hl.SetOffset(v3(x, y))?;
        }
        self.spot_hl = (x, y);
        self.spot_hl_size = (w, h);
        kids.InsertAtTop(&hl)
    }

    /// Arama görünümünü kurar; (genişlik, yükseklik) döndürür.
    pub(super) fn build_search(&mut self, gfx: &Gfx, kids: &VisualCollection, m: &Model) -> Result<(f32, f32)> {
        let s = self.s;
        let w = (W * s).round();
        let pad = (PAD * s).round();
        let Some(v) = m.spot.as_ref() else { return Ok((w, (HEAD * s).round())) };

        // ---- Üst şerit: saat solda, çalan şarkı sağda ----
        let head = (HEAD * s).round();
        let (_, _, time, date) = now_hm();
        let (t, _, th) = self.label(gfx, &time, 13.5, true, 120.0 * s, 0.95)?;
        Self::place(&t, pad + 8.0 * s, (head - th) / 2.0 + 2.0 * s)?;
        kids.InsertAtTop(&t)?;
        let right = w - pad - 8.0 * s;
        match m.media {
            Some(snap) => {
                let eq_w = 13.0 * s;
                self.equalizer(kids, right - eq_w, ((head - 13.0 * s) / 2.0 + 2.0 * s).round(), snap.playing)?;
                let text = if snap.artist.is_empty() { snap.title.clone() } else { format!("{} — {}", snap.title, snap.artist) };
                let (tl, tw, tlh) = self.label(gfx, &text, 12.0, false, 300.0 * s, 0.72)?;
                let tx = right - eq_w - 10.0 * s - tw;
                Self::place(&tl, tx, (head - tlh) / 2.0 + 2.0 * s)?;
                kids.InsertAtTop(&tl)?;
                let apx = (18.0 * s).round();
                let a = self.art_or_glyph(gfx, snap, apx, 5.0 * s)?;
                Self::place(&a, tx - 8.0 * s - apx, ((head - apx) / 2.0 + 2.0 * s).round())?;
                kids.InsertAtTop(&a)?;
            }
            None => {
                let (d, dw, dh) = self.label(gfx, &date, 12.0, false, 260.0 * s, 0.55)?;
                Self::place(&d, right - dw, (head - dh) / 2.0 + 2.0 * s)?;
                kids.InsertAtTop(&d)?;
            }
        }

        // ---- Arama kutusu ----
        // Uzun yazı kelimeden alt satıra geçer; kutu (ve altındaki her şey)
        // spring ile aşağı doğru büyür. En fazla FIELD_LINES satır görünür.
        let fy = head;
        let fh1 = (FIELD * s).round();
        let fw = w - 2.0 * pad;
        let tx = (pad + 46.0 * s).round();
        let tmax = fw - 46.0 * s - 16.0 * s;
        let size = 17.0;
        let ft = if v.query.is_empty() { None } else { Some(self.text.render_field(gfx, v.query, size * s, tmax, FIELD_LINES, v.caret, 0.96)?) };
        let line_h = ft.as_ref().map(|f| f.line_h).unwrap_or((size * s * 1.34).round());
        let block_h = ft.as_ref().map(|f| f.text.height).unwrap_or(line_h);
        let fh = (fh1 + block_h - line_h).round().max(fh1);
        let ty = (fy + (fh1 - line_h) / 2.0).round();
        let radius = (14.0 * s * self.theme.radius_scale).round();
        let (field, fgeom) = self.rrect(fw, fh, radius, &self.solid(Rgba(255, 255, 255, 0x12))?)?;
        Self::place(&field, pad, fy)?;
        kids.InsertAtTop(&field)?;
        let prev = self.spot_field_h;
        let grow = if prev > 0.0 && (prev - fh).abs() > 0.5 { fh - prev } else { 0.0 };
        if grow != 0.0 {
            field.SetSize(v2(fw, prev))?;
            fgeom.SetSize(v2(fw, prev))?;
            self.motion.animate(&field, h!("Size.Y"), fh, 0.9, 70, 0)?;
            self.motion.animate(&fgeom, h!("Size.Y"), fh, 0.9, 70, 0)?;
        }
        self.spot_field_h = fh;
        self.hits.push((Hit::Field, RECT { left: pad as i32, top: fy as i32, right: (pad + fw) as i32, bottom: (fy + fh) as i32 }));
        let g = self.text.glyph(gfx, '\u{E721}', (17.0 * s).round(), 0.6)?;
        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, pad + 16.0 * s, fy + (fh1 - g.height) / 2.0)?)?;
        let (cx, cy) = match &ft {
            None => {
                let (p, _, ph) = self.label(gfx, v.placeholder, size, false, tmax, 0.4)?;
                Self::place(&p, tx, fy + (fh1 - ph) / 2.0)?;
                kids.InsertAtTop(&p)?;
                (tx, ty)
            }
            Some(f) => {
                if v.all_selected {
                    let (sel, _) = self.rrect(f.text_w + 4.0 * s, f.text.height, 4.0 * s, &self.accent_brush()?)?;
                    sel.SetOpacity(0.45)?;
                    Self::place(&sel, tx - 2.0 * s, ty)?;
                    kids.InsertAtTop(&sel)?;
                }
                let q = self.sprite(&f.text.surface, f.text.width, f.text.height, tx, ty)?;
                kids.InsertAtTop(&q)?;
                (tx + f.caret.0.min(tmax), ty + f.caret.1)
            }
        };
        // İmleç: vurgu renkli ince çizgi, compositor'de yanıp söner.
        let (caret, _) = self.rrect((2.0 * s).round(), (22.0 * s).round(), 1.0 * s, &self.accent_brush()?)?;
        Self::place(&caret, cx.round(), cy + (line_h - 22.0 * s) / 2.0)?;
        let blink = self.comp.CreateScalarKeyFrameAnimation()?;
        let step = self.comp.CreateStepEasingFunction()?;
        blink.InsertKeyFrame(0.0, 1.0)?;
        blink.InsertKeyFrameWithEasingFunction(0.5, 0.0, &step)?;
        blink.InsertKeyFrameWithEasingFunction(1.0, 1.0, &step)?;
        blink.SetDuration(ts(1060))?;
        blink.SetDelayTime(ts(500))?;
        blink.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        caret.StartAnimation(h!("Opacity"), &blink)?;
        kids.InsertAtTop(&caret)?;

        // ---- Gövde ----
        // Ayrı katmanda: kutu büyürken gövde de onunla birlikte aşağı kayar.
        let body = self.comp.CreateContainerVisual()?;
        kids.InsertAtTop(&body)?;
        if grow != 0.0 {
            body.SetOffset(v3(0.0, -grow))?;
            self.motion.animate(&body, h!("Offset.Y"), 0.0, 0.9, 70, 0)?;
        }
        let kids = &body.Children()?;
        let mut y = fy + fh + (8.0 * s).round();
        match v.mode {
            SpotMode::Home => {
                let tw = (TILE_W * s).round();
                let tile_h = (TILE_H * s).round();
                let n = v.tiles.len().max(1) as f32;
                let gap = ((fw - n * tw) / (n - 1.0).max(1.0)).min(10.0 * s);
                let x0 = pad + (fw - (n * tw + (n - 1.0) * gap)) / 2.0;
                if v.sel < v.tiles.len() {
                    self.highlight(kids, x0 + v.sel as f32 * (tw + gap), y, tw, tile_h, 16.0 * s)?;
                }
                for (i, it) in v.tiles.iter().enumerate() {
                    let x = x0 + i as f32 * (tw + gap);
                    let ipx = (38.0 * s).round();
                    let icon = self.item_icon(gfx, &it.icon, ipx)?;
                    Self::place(&icon, x + (tw - ipx) / 2.0, y + 12.0 * s)?;
                    kids.InsertAtTop(&icon)?;
                    let (l, lw, _) = self.label(gfx, &it.title, 11.5, false, tw - 10.0 * s, 0.85)?;
                    Self::place(&l, x + (tw - lw) / 2.0, y + 12.0 * s + ipx + 8.0 * s)?;
                    kids.InsertAtTop(&l)?;
                    self.hits.push((Hit::Tile(i), RECT { left: x as i32, top: y as i32, right: (x + tw) as i32, bottom: (y + tile_h) as i32 }));
                }
                y += tile_h + (8.0 * s).round();

                // Kısayollar: solda metinli, sağda yalnızca simgeli (güç komutları).
                let ch = (CHIP_H * s).round();
                let mut lx = pad;
                let mut rx = pad + fw;
                for (i, it) in v.chips.iter().enumerate() {
                    let idx = v.tiles.len() + i;
                    let glyph = match it.icon {
                        Icon::Glyph(c) => c,
                        _ => '\u{E71D}',
                    };
                    let iconic = matches!(it.act, carlui_launcher::search::Act::System(_));
                    let g = self.text.glyph(gfx, glyph, (15.0 * s).round(), 0.9)?;
                    let (cw, x) = if iconic {
                        rx -= ch;
                        let x = rx;
                        rx -= 6.0 * s;
                        (ch, x)
                    } else {
                        let (l, lw, lh) = self.label(gfx, &it.title, 12.5, false, 200.0 * s, 0.88)?;
                        let cw = (lw + g.width + 34.0 * s).round();
                        Self::place(&l, lx + 14.0 * s + g.width + 8.0 * s, y + (ch - lh) / 2.0)?;
                        let (bg, _) = self.rrect(cw, ch, ch / 2.0, &self.solid(Rgba(255, 255, 255, 0x0E))?)?;
                        Self::place(&bg, lx, y)?;
                        kids.InsertAtTop(&bg)?;
                        kids.InsertAtTop(&l)?;
                        let x = lx;
                        lx += cw + 8.0 * s;
                        (cw, x)
                    };
                    if iconic {
                        let (bg, _) = self.rrect(cw, ch, ch / 2.0, &self.solid(Rgba(255, 255, 255, 0x0E))?)?;
                        Self::place(&bg, x, y)?;
                        kids.InsertAtTop(&bg)?;
                        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, x + (cw - g.width) / 2.0, y + (ch - g.height) / 2.0)?)?;
                    } else {
                        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, x + 14.0 * s, y + (ch - g.height) / 2.0)?)?;
                    }
                    if v.sel == idx {
                        self.highlight(kids, x, y, cw, ch, ch / 2.0)?;
                    }
                    self.hits.push((Hit::Chip(idx), RECT { left: x as i32, top: y as i32, right: (x + cw) as i32, bottom: (y + ch) as i32 }));
                }
                y += ch + (12.0 * s).round();
                if v.confirm.is_some_and(|c| c == v.sel) {
                    let (l, lw, lh) = self.label(gfx, t!("Onaylamak için tekrar Enter", "Press Enter again to confirm"), 11.5, false, fw, 0.75)?;
                    Self::place(&l, pad + fw - lw, y - 4.0 * s)?;
                    kids.InsertAtTop(&l)?;
                    y += lh + 4.0 * s;
                }
            }
            SpotMode::List | SpotMode::Actions => {
                let rh = (ROW * s).round();
                let shown = v.items.len().saturating_sub(v.scroll).min(ROWS_MAX);
                if v.sel >= v.scroll && v.sel < v.scroll + shown {
                    self.highlight(kids, pad, y + (v.sel - v.scroll) as f32 * rh, fw, rh, (12.0 * s).round())?;
                }
                for r in 0..shown {
                    let i = v.scroll + r;
                    let it = &v.items[i];
                    let ry = y + r as f32 * rh;
                    let ipx = (28.0 * s).round();
                    let icon = self.item_icon(gfx, &it.icon, ipx)?;
                    Self::place(&icon, pad + 14.0 * s, ry + (rh - ipx) / 2.0)?;
                    kids.InsertAtTop(&icon)?;
                    let x = pad + 14.0 * s + ipx + 12.0 * s;
                    let (tag, tgw, tgh) = if it.tag.is_empty() { (None, 0.0, 0.0) } else {
                        let (l, lw, lh) = self.label(gfx, it.tag_label(), 11.0, false, 90.0 * s, 0.4)?;
                        (Some(l), lw, lh)
                    };
                    let tmax = pad + fw - x - tgw - 26.0 * s;
                    let (t, _, th) = self.label(gfx, &it.title, 14.0, true, tmax, 0.96)?;
                    let confirm = v.confirm == Some(i);
                    let sub = if confirm { t!("Onaylamak için tekrar Enter", "Press Enter again to confirm") } else { it.sub.as_str() };
                    if sub.is_empty() {
                        Self::place(&t, x, ry + (rh - th) / 2.0)?;
                        kids.InsertAtTop(&t)?;
                    } else {
                        let (st, _, sh) = self.label(gfx, sub, 11.5, false, tmax, if confirm { 0.9 } else { 0.5 })?;
                        let top = ry + (rh - th - sh) / 2.0;
                        Self::place(&t, x, top)?;
                        Self::place(&st, x, top + th)?;
                        kids.InsertAtTop(&t)?;
                        kids.InsertAtTop(&st)?;
                    }
                    if let Some(l) = tag {
                        Self::place(&l, pad + fw - 14.0 * s - tgw, ry + (rh - tgh) / 2.0)?;
                        kids.InsertAtTop(&l)?;
                    }
                    self.hits.push((Hit::Row(i), RECT { left: pad as i32, top: ry as i32, right: (pad + fw) as i32, bottom: (ry + rh) as i32 }));
                }
                y += shown as f32 * rh;
                // Kaydırma göstergesi
                if v.items.len() > ROWS_MAX {
                    let track = shown as f32 * rh - 8.0 * s;
                    let th = (track * ROWS_MAX as f32 / v.items.len() as f32).max(18.0 * s);
                    let top = (track - th) * v.scroll as f32 / (v.items.len() - ROWS_MAX) as f32;
                    let (sb, _) = self.rrect((3.0 * s).round(), th.round(), 1.5 * s, &self.solid(Rgba(255, 255, 255, 0x40))?)?;
                    Self::place(&sb, pad + fw + 3.0 * s, y - shown as f32 * rh + 4.0 * s + top)?;
                    kids.InsertAtTop(&sb)?;
                }
                y += (4.0 * s).round();
            }
        }

        // ---- İpucu satırı ----
        if !v.hint.is_empty() {
            let hh = (HINT * s).round();
            let (sep, _) = self.rrect(fw, 1.0, 0.0, &self.solid(Rgba(255, 255, 255, 0x14))?)?;
            Self::place(&sep, pad, y)?;
            kids.InsertAtTop(&sep)?;
            let (l, _, lh) = self.label(gfx, v.hint, 11.0, false, fw - 16.0 * s, 0.42)?;
            Self::place(&l, pad + 8.0 * s, y + (hh - lh) / 2.0)?;
            kids.InsertAtTop(&l)?;
            y += hh;
        }
        Ok((w, (y + 4.0 * s).round()))
    }
}
