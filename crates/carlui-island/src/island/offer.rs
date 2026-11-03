//! Bu modül adanın "İndirme bilgisi" kartını çiziyor. Sorun: tarayıcıda
//! başlayan bir indirmeyi kimin yapacağına karar vermek için ayrı, kaba bir
//! pencere açmak istemiyoruz. Çözüm: ada aşağı doğru kart olarak açılır;
//! dosya türü rozeti, ad, boyut · kaynak ve kayıt klasörü satır satır kayarak
//! belirir, altında üç hap düğme (İndir / Farklı kaydet… / Tarayıcıya bırak)
//! sırayla yerine oturur. Ana düğme hafifçe "nefes alır" ki karar beklendiği
//! belli olsun.

use super::*;

pub struct OfferView<'a> {
    pub name: &'a str,
    pub size: u64,
    pub host: &'a str,
    pub folder: &'a str,
    /// Sırada bekleyen başka teklif sayısı.
    pub more: usize,
}

const W: f32 = 400.0;
const PAD: f32 = 16.0;
const ICON: f32 = 46.0;
const BTN_H: f32 = 32.0;

/// Uzantıya göre rozet glifi.
fn glyph_for(name: &str) -> char {
    let ext = name.rsplit_once('.').map(|p| p.1.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "zip" | "rar" | "7z" | "gz" | "tar" | "xz" | "bz2" => '\u{F012}',
        "exe" | "msi" | "msix" | "appx" => '\u{ECAA}',
        "mp4" | "mkv" | "avi" | "mov" | "webm" => '\u{E714}',
        "mp3" | "flac" | "wav" | "m4a" | "ogg" => '\u{E8D6}',
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => '\u{EB9F}',
        "pdf" | "doc" | "docx" | "txt" | "xlsx" | "pptx" => '\u{E8A5}',
        "iso" | "img" => '\u{E958}',
        _ => '\u{E7C3}',
    }
}

impl Island {
    /// Satırı alttan kaydırarak ve soldurarak getirir (sıralı giriş).
    fn stagger(&self, v: &impl Interface, y: f32, delay: i64) -> Result<()> {
        let vis: Visual = v.cast()?;
        vis.SetOpacity(0.0)?;
        let off = vis.Offset()?;
        vis.SetOffset(Vector3 { X: off.X, Y: y + 8.0 * self.s, Z: 0.0 })?;
        self.motion.fade(&vis, h!("Opacity"), 1.0, 220, delay)?;
        self.motion.animate(&vis, h!("Offset.Y"), y, 0.78, 55, delay)
    }

    pub(super) fn build_offer(&mut self, gfx: &Gfx, kids: &VisualCollection, m: &Model) -> Result<(f32, f32)> {
        let s = self.s;
        let Some(o) = m.offer.as_ref() else { return Ok((84.0 * s, 30.0 * s)) };
        let (w, pad, ipx) = ((W * s).round(), (PAD * s).round(), (ICON * s).round());

        // Başlık satırı
        let head = if o.more > 0 { format!("İndirme bilgisi · +{} bekliyor", o.more) } else { "İndirme bilgisi".to_string() };
        let (hd, _, hh) = self.label(gfx, &head, 11.0, false, w - 2.0 * pad, 0.5)?;
        let hy = (12.0 * s).round();
        Self::place(&hd, pad, hy)?;
        kids.InsertAtTop(&hd)?;
        self.stagger(&hd, hy, 40)?;

        // Rozet: vurgu renginde yuvarlak kare + dosya türü glifi
        let iy = hy + hh + 10.0 * s;
        let badge = self.comp.CreateContainerVisual()?;
        let (bg, _) = self.rrect(ipx, ipx, 12.0 * s, &self.accent_brush()?)?;
        badge.Children()?.InsertAtTop(&bg)?;
        let g = self.text.glyph(gfx, glyph_for(o.name), (ipx * 0.46).round(), 0.96)?;
        badge.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (ipx - g.width) / 2.0, (ipx - g.height) / 2.0)?)?;
        badge.SetSize(v2(ipx, ipx))?;
        badge.SetCenterPoint(Vector3 { X: ipx / 2.0, Y: ipx / 2.0, Z: 0.0 })?;
        Self::place(&badge, pad, iy)?;
        kids.InsertAtTop(&badge)?;
        badge.SetScale(Vector3 { X: 0.4, Y: 0.4, Z: 1.0 })?;
        self.motion.scale(&badge, 1.0, 0.55, 60, 70)?;

        // Ad, boyut · kaynak, klasör
        let tx = pad + ipx + 12.0 * s;
        let tmax = w - tx - pad;
        let (nm, _, nh) = self.label(gfx, o.name, 14.0, true, tmax, 0.97)?;
        Self::place(&nm, tx, iy + 2.0 * s)?;
        kids.InsertAtTop(&nm)?;
        self.stagger(&nm, iy + 2.0 * s, 90)?;
        let size = if o.size > 0 { carlui_sys::bytes(o.size as f64) } else { "Boyut bilinmiyor".into() };
        let (meta, _, mh) = self.label(gfx, &format!("{size} · {}", o.host), 12.0, false, tmax, 0.66)?;
        let my = iy + 2.0 * s + nh + 2.0 * s;
        Self::place(&meta, tx, my)?;
        kids.InsertAtTop(&meta)?;
        self.stagger(&meta, my, 130)?;
        let (fd, _, fh) = self.label(gfx, &format!("\u{2192} {}", o.folder), 11.5, false, tmax, 0.48)?;
        let fy = my + mh + 2.0 * s;
        Self::place(&fd, tx, fy)?;
        kids.InsertAtTop(&fd)?;
        self.stagger(&fd, fy, 170)?;

        // Düğmeler: ana düğme vurgu renginde, diğerleri cam.
        let by = (iy + ipx).max(fy + fh) + 14.0 * s;
        let bh = (BTN_H * s).round();
        let gap = 8.0 * s;
        let labels = [("İndir", 1.15f32), ("Farklı kaydet…", 1.25), ("Tarayıcıya bırak", 1.35)];
        let total: f32 = labels.iter().map(|l| l.1).sum();
        let avail = w - 2.0 * pad - gap * (labels.len() as f32 - 1.0);
        let mut x = pad;
        for (k, (text, weight)) in labels.iter().enumerate() {
            let bw = (avail * weight / total).round();
            let brush = if k == 0 { self.accent_brush()? } else { self.solid(Rgba(255, 255, 255, 0x1C))? };
            let b = self.comp.CreateContainerVisual()?;
            let (rect, _) = self.rrect(bw, bh, bh / 2.0, &brush)?;
            b.Children()?.InsertAtTop(&rect)?;
            let (t, tw, th) = self.label(gfx, text, 12.5, k == 0, bw - 12.0 * s, if k == 0 { 0.98 } else { 0.85 })?;
            Self::place(&t, (bw - tw) / 2.0, (bh - th) / 2.0)?;
            b.Children()?.InsertAtTop(&t)?;
            b.SetSize(v2(bw, bh))?;
            b.SetCenterPoint(Vector3 { X: bw / 2.0, Y: bh / 2.0, Z: 0.0 })?;
            Self::place(&b, x, by)?;
            kids.InsertAtTop(&b)?;
            self.stagger(&b, by, 200 + 45 * k as i64)?;
            if k == 0 {
                self.breathe(&b)?;
            }
            self.hits.push((Hit::OfferBtn(k as u8), RECT { left: x as i32, top: by as i32, right: (x + bw) as i32, bottom: (by + bh) as i32 }));
            x += bw + gap;
        }
        Ok((w, (by + bh + pad).round()))
    }

    /// Ana düğme: yavaşça %3 büyüyüp küçülür.
    fn breathe(&self, v: &ContainerVisual) -> Result<()> {
        let a = self.comp.CreateVector3KeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
        a.InsertKeyFrame(0.5, Vector3 { X: 1.03, Y: 1.03, Z: 1.0 })?;
        a.InsertKeyFrame(1.0, Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
        a.SetDuration(ts(1800))?;
        a.SetDelayTime(ts(700))?;
        a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        v.StartAnimation(h!("Scale"), &a)
    }
}
