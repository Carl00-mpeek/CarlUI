//! Bu modül adanın "üstüne gelince açılan" hâlini çiziyor. Sorun: saat
//! hapı küçük ve sade kalmalı, ama sistem durumu ve kaçırılan bildirimler bir
//! bakışta görülebilmeli. Çözüm tek bir genişleyen görünüm:
//! - Üst satır: tarih + ayarlarda açık olan göstergeler (CPU/GPU sıcaklığı, RAM).
//! - Şarkı çalıyorsa altında medya paneli (aynı denetimlerle).
//! - En altta saklanan bildirimler: her birinde çöp kutusu, ekran görüntüsünde
//!   Aç / Düzenle / Sil; sağ üstte hepsini silen X. Liste belli bir sayıda
//!   kesilir ki ada ekranı kaplamasın.

use super::*;

/// Listede en fazla bu kadar bildirim görünür; fazlası "+N" satırına düşer.
pub const NOTES_MAX: usize = 4;
const W: f32 = 380.0;
const PAD: f32 = 14.0;
const TOP_H: f32 = 38.0;
const ROW_H: f32 = 52.0;
const SHOT_H: f32 = 80.0;

/// Gösterge: (kısa ad, değer, uyarı düzeyi 0 normal / 1 ılık / 2 sıcak).
pub type Stat = (&'static str, String, u8);

pub struct HubView<'a> {
    /// false: yalnızca üst satır (fare yeni geldi). true: bildirimler de.
    pub full: bool,
    pub stats: Vec<Stat>,
    pub notes: &'a [Note],
    pub media: bool,
    /// Silinme animasyonundaki bildirimler: görünmez çizilir, birazdan kalkar.
    pub dying: &'a [u64],
    /// Daraltılmış uygulama grupları (yalnızca başlık görünür).
    pub collapsed: &'a std::collections::HashSet<String>,
}

/// Bir grupta en fazla bu kadar bildirim açık görünür.
const GROUP_MAX: usize = 3;

/// Bildirimleri uygulamaya göre gruplar: (uygulama adı, en yeniden eskiye
/// liste sıraları). Gruplar en yeni bildirimine göre sıralıdır. Listedeki sıra
/// `notes.iter().rev()` sırasıdır (en yeni 0), dokunma kimlikleri onu kullanır.
pub fn groups(notes: &[Note]) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, n) in notes.iter().rev().enumerate() {
        let name = if n.app.trim().is_empty() { t!("Diğer", "Other").to_string() } else { n.app.trim().to_string() };
        match out.iter_mut().find(|(a, _)| *a == name) {
            Some((_, v)) => v.push(i),
            None => out.push((name, vec![i])),
        }
    }
    out
}

impl Island {
    fn hub_dot(&self, kids: &VisualCollection, x: f32, y: f32, level: u8) -> Result<()> {
        let c = match level {
            2 => Rgba(255, 92, 92, 255),
            1 => Rgba(255, 176, 64, 255),
            _ => Rgba(92, 214, 140, 255),
        };
        let d = (6.0 * self.s).round();
        let (dot, _) = self.rrect(d, d, d / 2.0, &self.solid(c)?)?;
        dot.SetOffset(v3(x, y))?;
        kids.InsertAtTop(&dot)
    }

    /// Üst satırı kendi kabında kurar; kap saklanır ki göstergeler her saniye
    /// değişince yalnızca bu satır yeniden çizilsin (bkz. `hub_restat`).
    fn hub_top(&mut self, gfx: &Gfx, kids: &VisualCollection, w: f32, stats: &[Stat]) -> Result<()> {
        let row = self.comp.CreateContainerVisual()?;
        kids.InsertAtTop(&row)?;
        self.hub_top_fill(gfx, &row.Children()?, w, stats)?;
        self.hub_stats = Some((row, w, stats.len()));
        Ok(())
    }

    /// Yalnızca üst satırı (tarih + göstergeler) yeniler. Göstergeler saniyede
    /// bir değişiyor; eskiden bu, bildirimler, medya paneli, kapak ve dalgayla
    /// birlikte bütün hub'ı baştan kuruyordu. Satır sayısı değiştiyse (yükseklik
    /// değişir) false döner, çağıran tam yenilemeye düşer.
    pub fn hub_restat(&mut self, gfx: &Gfx, stats: &[Stat]) -> Result<bool> {
        let Some((row, w, n)) = self.hub_stats.clone() else { return Ok(false) };
        if self.view != View::Hub || n != stats.len() {
            return Ok(false);
        }
        let kids = row.Children()?;
        kids.RemoveAll()?;
        self.hub_top_fill(gfx, &kids, w, stats)?;
        Ok(true)
    }

    /// Üst satır: solda tarih, sağda göstergeler.
    fn hub_top_fill(&self, gfx: &Gfx, kids: &VisualCollection, w: f32, stats: &[Stat]) -> Result<()> {
        let s = self.s;
        let h = (TOP_H * s).round();
        let (_, _, _, date) = now_hm();
        let (d, dw, dh) = self.label(gfx, &date, 12.5, false, 170.0 * s, 0.72)?;
        Self::place(&d, PAD * s, (h - dh) / 2.0)?;
        kids.InsertAtTop(&d)?;
        let mut x = w - PAD * s;
        for (name, value, level) in stats.iter().rev() {
            let (v, vw, vh) = self.label(gfx, value, 13.0, true, 90.0 * s, 0.95)?;
            let (n, nw, nh) = self.label(gfx, name, 10.5, false, 60.0 * s, 0.5)?;
            // Sığmayan gösterge tarihin üstüne binmesin (listenin başındakiler önce düşer).
            if x - vw - nw - 27.0 * s < PAD * s + dw + 12.0 * s {
                break;
            }
            x -= vw;
            Self::place(&v, x, (h - vh) / 2.0)?;
            x -= 4.0 * s + nw;
            Self::place(&n, x, (h - nh) / 2.0 + 0.5 * s)?;
            x -= 9.0 * s;
            self.hub_dot(kids, x, ((h - 6.0 * s) / 2.0).round(), *level)?;
            x -= 14.0 * s;
            kids.InsertAtTop(&v)?;
            kids.InsertAtTop(&n)?;
        }
        Ok(())
    }

    fn hub_icon(&mut self, gfx: &Gfx, n: &Note, px: f32) -> Result<Visual> {
        if let Some(b) = &n.logo {
            if let Some(v) = self.art(gfx, b, px, 8.0 * self.s)? {
                return v.cast();
            }
        }
        let c = self.comp.CreateContainerVisual()?;
        let (bg, _) = self.rrect(px, px, 8.0 * self.s, &self.accent_brush()?)?;
        c.Children()?.InsertAtTop(&bg)?;
        let glyph = if n.shot.is_some() { '\u{E722}' } else { '\u{EA8F}' };
        let g = self.text.glyph(gfx, glyph, (px * 0.5).round(), 0.95)?;
        c.Children()?.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, (px - g.width) / 2.0, (px - g.height) / 2.0)?)?;
        c.cast()
    }

    /// Küçük yazılı düğme (Aç / Düzenle / Sil).
    fn hub_pill(&self, gfx: &Gfx, kids: &VisualCollection, text: &str, x: f32, y: f32, danger: bool) -> Result<f32> {
        let s = self.s;
        let (t, tw, th) = self.label(gfx, text, 11.5, true, 100.0 * s, 0.95)?;
        let (pw, ph) = ((tw + 20.0 * s).round(), (24.0 * s).round());
        let bg = if danger { self.solid(Rgba(255, 80, 80, 0x40))? } else { self.solid(Rgba(255, 255, 255, 0x1F))? };
        let (p, _) = self.rrect(pw, ph, ph / 2.0, &bg)?;
        p.SetOffset(v3(x.round(), y.round()))?;
        kids.InsertAtTop(&p)?;
        Self::place(&t, x + (pw - tw) / 2.0, y + (ph - th) / 2.0)?;
        kids.InsertAtTop(&t)?;
        Ok(pw)
    }

    fn hub_rect(x: f32, y: f32, w: f32, h: f32) -> RECT {
        RECT { left: x as i32, top: y as i32, right: (x + w) as i32, bottom: (y + h) as i32 }
    }

    pub(super) fn build_hub(&mut self, gfx: &Gfx, kids: &VisualCollection, m: &Model) -> Result<(f32, f32)> {
        let s = self.s;
        let Some(hv) = m.hub.as_ref() else { return Ok((84.0 * s, 30.0 * s)) };
        let w = (W * s).round();
        let mut y = 0.0;
        self.hub_stats = None;
        // Medya paneli kendi görünümüyle kurulur, sonra aşağı kaydırılır.
        let mut media_hits = Vec::new();
        let media = if hv.media && m.media.is_some() {
            let (mc, _, mh) = self.build(gfx, View::MediaFull, m)?;
            media_hits = std::mem::take(&mut self.hits);
            Some((mc, mh))
        } else {
            None
        };
        if !hv.stats.is_empty() || media.is_none() {
            self.hub_top(gfx, kids, w, &hv.stats)?;
            y += (TOP_H * s).round();
        }
        if let Some((mc, mh)) = media {
            let dy = if y > 0.0 { y - 8.0 * s } else { 0.0 };
            mc.SetOffset(v3(0.0, dy.round()))?;
            kids.InsertAtTop(&mc)?;
            for (hit, mut r) in media_hits {
                r.top += dy as i32;
                r.bottom += dy as i32;
                self.hits.push((hit, r));
            }
            y = dy + mh;
        }
        let notes = hv.notes;
        if notes.is_empty() {
            return Ok((w, (y + if media_is_last(hv) { 0.0 } else { 2.0 * s }).round().max(30.0 * s)));
        }
        if !hv.full {
            // Henüz açılmadı: kaç bildirim beklediğini söyle.
            let (t, tw, th) = self.label(gfx, &t!(format!("{} bildirim", notes.len()), if notes.len() == 1 { "1 notification".to_string() } else { format!("{} notifications", notes.len()) }), 11.5, false, w, 0.55)?;
            let rh = (24.0 * s).round();
            Self::place(&t, (w - tw) / 2.0, y + (rh - th) / 2.0 - 2.0 * s)?;
            kids.InsertAtTop(&t)?;
            return Ok((w, (y + rh).round()));
        }
        // Başlık + hepsini sil.
        let hh = (26.0 * s).round();
        let line = self.comp.CreateSpriteVisual()?;
        line.SetSize(v2(w - 2.0 * PAD * s, 1.0))?;
        line.SetOffset(v3(PAD * s, y))?;
        line.SetBrush(&self.comp.CreateColorBrushWithColor(color(Rgba(255, 255, 255, 0x1A)))?)?;
        kids.InsertAtTop(&line)?;
        let (t, _, th) = self.label(gfx, t!("Bildirimler", "Notifications"), 11.0, true, 200.0 * s, 0.55)?;
        Self::place(&t, PAD * s, y + (hh - th) / 2.0 + 2.0 * s)?;
        kids.InsertAtTop(&t)?;
        let g = self.text.glyph(gfx, '\u{E711}', (11.0 * s).round(), 0.7)?;
        let (bx, bs) = (w - PAD * s - 22.0 * s, (22.0 * s).round());
        let (bb, _) = self.rrect(bs, bs, bs / 2.0, &self.solid(Rgba(255, 255, 255, 0x1A))?)?;
        bb.SetOffset(v3(bx.round(), (y + (hh - bs) / 2.0 + 2.0 * s).round()))?;
        kids.InsertAtTop(&bb)?;
        kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, bx + (bs - g.width) / 2.0, y + (hh - g.height) / 2.0 + 2.0 * s)?)?;
        self.hits.push((Hit::HubClear, Self::hub_rect(bx - 4.0 * s, y, bs + 8.0 * s, hh + 4.0 * s)));
        y += hh + 2.0 * s;

        // Uygulamaya göre gruplar; en yeniler üstte.
        self.hub_rows.clear();
        let mut shown = 0;
        let mut hidden = 0;
        let mut fresh = 0;
        for (g, (name, idx)) in groups(notes).iter().enumerate() {
            if shown >= NOTES_MAX {
                hidden += idx.len();
                continue;
            }
            let open = !hv.collapsed.contains(name);
            y += self.hub_group_head(gfx, kids, w, y, g, name, idx.len(), open)?;
            if !open {
                continue;
            }
            for &i in idx.iter().take(GROUP_MAX) {
                if shown >= NOTES_MAX {
                    hidden += 1;
                    continue;
                }
                let n = &notes[notes.len() - 1 - i];
                y += self.hub_note_row(gfx, kids, hv, w, y, i, n, &mut fresh)?;
                shown += 1;
            }
            hidden += idx.len().saturating_sub(GROUP_MAX);
        }
        if hidden > 0 {
            let (t, tw, th) = self.label(gfx, &t!(format!("+{hidden} bildirim daha"), if hidden == 1 { "+1 more notification".to_string() } else { format!("+{hidden} more notifications") }), 11.0, false, w, 0.45)?;
            let rh = (22.0 * s).round();
            Self::place(&t, (w - tw) / 2.0, y + (rh - th) / 2.0)?;
            kids.InsertAtTop(&t)?;
            y += rh;
        }
        Ok((w, (y + 6.0 * s).round()))
    }

    /// Uygulama grubu başlığı: ad · sayı, daralt oku ve grubu silen X.
    #[allow(clippy::too_many_arguments)]
    fn hub_group_head(&mut self, gfx: &Gfx, kids: &VisualCollection, w: f32, y: f32, g: usize, name: &str, count: usize, open: bool) -> Result<f32> {
        let s = self.s;
        let hh = (24.0 * s).round();
        let x = PAD * s;
        let (t, tw, th) = self.label(gfx, name, 11.5, true, w * 0.55, 0.85)?;
        Self::place(&t, x, y + (hh - th) / 2.0)?;
        kids.InsertAtTop(&t)?;
        let (c, cw, ch) = self.label(gfx, &count.to_string(), 10.5, false, 40.0 * s, 0.45)?;
        Self::place(&c, x + tw + 6.0 * s, y + (hh - ch) / 2.0 + 0.5 * s)?;
        kids.InsertAtTop(&c)?;
        // Ok: açıkken aşağı, kapalıyken sağa bakar.
        let arrow = self.text.glyph(gfx, if open { '\u{E70D}' } else { '\u{E76C}' }, (8.5 * s).round(), 0.5)?;
        let ax = x + tw + 6.0 * s + cw + 6.0 * s;
        kids.InsertAtTop(&self.sprite(&arrow.surface, arrow.width, arrow.height, ax, y + (hh - arrow.height) / 2.0)?)?;
        let xg = self.text.glyph(gfx, '\u{E711}', (9.5 * s).round(), 0.5)?;
        let bx = w - PAD * s - 22.0 * s;
        kids.InsertAtTop(&self.sprite(&xg.surface, xg.width, xg.height, bx + (22.0 * s - xg.width) / 2.0, y + (hh - xg.height) / 2.0)?)?;
        // İlk eşleşen kazanır: önce X, sonra başlığın geri kalanı.
        self.hits.push((Hit::GroupClear(g), Self::hub_rect(bx - 4.0 * s, y, 30.0 * s, hh)));
        self.hits.push((Hit::GroupToggle(g), Self::hub_rect(0.0, y, w, hh)));
        Ok(hh)
    }

    /// Tek bildirim satırı; yüksekliğini döndürür. Her satır kendi katmanında:
    /// ilk kez görünen solarak gelir, silinen `hub_vanish` ile kayarak gider.
    #[allow(clippy::too_many_arguments)]
    fn hub_note_row(&mut self, gfx: &Gfx, kids: &VisualCollection, hv: &HubView, w: f32, y: f32, i: usize, n: &Note, fresh: &mut i64) -> Result<f32> {
        let s = self.s;
        {
            let row = self.comp.CreateContainerVisual()?;
            kids.InsertAtTop(&row)?;
            let kids = &row.Children()?;
            if hv.dying.contains(&n.id) {
                row.SetOpacity(0.0)?;
            } else if self.hub_seen.insert(n.id) {
                row.SetOpacity(0.0)?;
                row.SetOffset(v3(0.0, -8.0 * s))?;
                let delay = 40 * *fresh;
                *fresh += 1;
                self.motion.fade(&row, h!("Opacity"), 1.0, 200, delay)?;
                self.motion.fade(&row, h!("Offset.Y"), 0.0, 220, delay)?;
            }
            self.hub_rows.push((n.id, row));
            let rh = if n.shot.is_some() { SHOT_H } else { ROW_H } * s;
            let ipx = (32.0 * s).round();
            let icon = self.hub_icon(gfx, n, ipx)?;
            Self::place(&icon, PAD * s, y + 9.0 * s)?;
            kids.InsertAtTop(&icon)?;
            let tx = PAD * s + ipx + 11.0 * s;
            let tmax = w - tx - PAD * s - 34.0 * s;
            let (at, aw, _) = self.label(gfx, &n.at, 10.5, false, 60.0 * s, 0.42)?;
            let (ti, _, tih) = self.label(gfx, &n.title, 13.0, true, tmax - aw - 6.0 * s, 0.95)?;
            Self::place(&ti, tx, y + 8.0 * s)?;
            Self::place(&at, tx + tmax - aw, y + 10.0 * s)?;
            kids.InsertAtTop(&ti)?;
            kids.InsertAtTop(&at)?;
            // Uygulama adı grup başlığında; satırda yalnızca içerik.
            let sub = if n.body.is_empty() { n.app.clone() } else { n.body.clone() };
            let (b, _, _) = self.label(gfx, &sub, 11.5, false, tmax, 0.6)?;
            Self::place(&b, tx, y + 10.0 * s + tih)?;
            kids.InsertAtTop(&b)?;
            // Çöp kutusu
            let g = self.text.glyph(gfx, '\u{E74D}', (13.0 * s).round(), 0.6)?;
            let gx = w - PAD * s - 22.0 * s;
            kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, gx + (22.0 * s - g.width) / 2.0, y + 12.0 * s)?)?;
            self.hits.push((Hit::NoteDel(i), Self::hub_rect(gx - 6.0 * s, y + 2.0 * s, 34.0 * s, 36.0 * s)));
            if n.shot.is_some() {
                let py = y + 46.0 * s;
                let mut px = tx;
                for (k, (label, danger)) in [(t!("Aç", "Open"), false), (t!("Düzenle", "Edit"), false), (t!("Sil", "Delete"), true)].into_iter().enumerate() {
                    let pw = self.hub_pill(gfx, kids, label, px, py, danger)?;
                    self.hits.push((Hit::NoteAct(i, k as u8), Self::hub_rect(px, py, pw, 24.0 * s)));
                    px += pw + 6.0 * s;
                }
            }
            // Satırın geri kalanı: bildirimi açar.
            self.hits.push((Hit::NoteRow(i), Self::hub_rect(0.0, y, w, rh)));
            Ok(rh)
        }
    }

    /// Silinen bildirim satırlarını sağa kaydırıp soldurur. Ekranda satırı
    /// olan yoksa false (hemen silinebilir).
    pub fn hub_vanish(&self, ids: &[u64]) -> bool {
        let mut any = false;
        for (id, row) in &self.hub_rows {
            if ids.contains(id) {
                let _ = self.motion.fade(row, h!("Opacity"), 0.0, 150, 0);
                let _ = self.motion.fade(row, h!("Offset.X"), 36.0 * self.s, 170, 0);
                any = true;
            }
        }
        any
    }

    /// Hapın küçük hâlinde, saklanan bildirim sayısı (saatin sağında).
    pub(super) fn badge(&self, gfx: &Gfx, kids: &VisualCollection, x: f32, h: f32, n: usize) -> Result<f32> {
        let s = self.s;
        let txt = if n > 9 { "9+".to_string() } else { n.to_string() };
        let (t, tw, th) = self.label(gfx, &txt, 10.0, true, 30.0 * s, 0.95)?;
        let bh = (16.0 * s).round();
        let bw = (tw + 9.0 * s).max(bh).round();
        let (b, _) = self.rrect(bw, bh, bh / 2.0, &self.accent_brush()?)?;
        let by = ((h - bh) / 2.0).round();
        b.SetOffset(v3(x.round(), by))?;
        kids.InsertAtTop(&b)?;
        Self::place(&t, x + (bw - tw) / 2.0, by + (bh - th) / 2.0)?;
        kids.InsertAtTop(&t)?;
        Ok(bw)
    }
}

fn media_is_last(hv: &HubView) -> bool {
    hv.media
}
