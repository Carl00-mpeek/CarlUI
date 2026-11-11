//! Görev çubuğu görünümünün sağ ucu: ağ, ses, pil simgeleri ve saat/tarih.
//!
//! Tek bir yüzeye çizilir; yalnızca gösterilen bir şey değişince (dakika,
//! ses düzeyi, kablo, pil) yeniden çizilir. Yoklama 2 sn'de bir, ağ 10 sn'de
//! bir bakılır (ucuz çağrılar; Wi-Fi radyosu sorgulanmaz).

use super::*;
use carlui_core::{quick::Volume, wired};
use windows::Win32::{
    Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
    System::{Power::*, SystemInformation::GetLocalTime},
};

pub const TIMER_STATUS: usize = 14;

/// Tıklanan bölüm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// Ağ/ses/pil simgeleri.
    Quick,
    /// Saat ve tarih.
    Clock,
}

#[derive(Clone, PartialEq, Default)]
struct Sig {
    wired: bool,
    vol: Option<(u8, bool)>,
    battery: Option<(u8, bool)>,
    time: String,
    date: String,
}

pub struct Status {
    dw: IDWriteFactory,
    volume: Option<Volume>,
    sig: Sig,
    net_at: Option<Instant>,
    pub visual: SpriteVisual,
    /// Yüzey genişliği ve simgelerin bittiği yer (saatin sol kenarı), piksel.
    pub width: f32,
    pub quick_w: f32,
    /// Full: sağ kenara yaslı; değilse dizilimin sonunda (`at`).
    pub full: bool,
    /// Ortadaki dizilimde sol kenarı (dock ortasına göre); dizilim yazar.
    pub at: f32,
    /// Saat biçimi (ayarlardaki 12/24 saat); her yoklamada ayardan alınır.
    pub hour: carlui_core::config::HourFormat,
}

fn battery() -> Option<(u8, bool)> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    // 128: pil yok, 255: bilinmiyor.
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some((s.BatteryLifePercent, s.ACLineStatus == 1))
}

fn now(hour: carlui_core::config::HourFormat) -> (String, String) {
    let t = unsafe { GetLocalTime() };
    (hour.time(t.wHour, t.wMinute), format!("{:02}.{:02}.{}", t.wDay, t.wMonth, t.wYear))
}

impl Status {
    pub fn new(comp: &Compositor, full: bool) -> Result<Self> {
        Ok(Self {
            dw: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? },
            volume: Volume::new(),
            sig: Sig::default(),
            net_at: None,
            visual: comp.CreateSpriteVisual()?,
            width: 0.0,
            quick_w: 0.0,
            full,
            at: 0.0,
            hour: Default::default(),
        })
    }

    fn read(&mut self) -> Sig {
        let wired = match self.net_at {
            Some(t) if t.elapsed() < Duration::from_secs(10) => self.sig.wired,
            _ => {
                self.net_at = Some(Instant::now());
                wired::wired_up()
            }
        };
        let vol = self.volume.as_ref().map(|v| {
            let (l, m) = v.get();
            ((l * 100.0).round() as u8, m)
        });
        let (time, date) = now(self.hour);
        Sig { wired, vol, battery: battery(), time, date }
    }

    /// Değişiklik varsa yeniden çizer; genişlik değiştiyse true döner.
    pub fn refresh(&mut self, gfx: &Gfx, h: f32, s: f32, force: bool) -> Result<bool> {
        let sig = self.read();
        if !force && sig == self.sig {
            return Ok(false);
        }
        self.sig = sig;
        let old = self.width;
        self.draw(gfx, h, s)?;
        Ok((self.width - old).abs() > 0.5)
    }

    fn glyphs(&self) -> Vec<char> {
        let g = &self.sig;
        let mut v = vec![if g.wired { '\u{E839}' } else { '\u{E701}' }];
        v.push(match g.vol {
            None => '\u{E767}',
            Some((_, true)) | Some((0, _)) => '\u{E74F}',
            Some((l, _)) if l < 34 => '\u{E993}',
            Some((l, _)) if l < 67 => '\u{E994}',
            Some(_) => '\u{E995}',
        });
        if let Some((p, charging)) = g.battery {
            let step = (p as u32 / 10).min(10);
            // Battery0..10: E850..E85A, BatteryCharging0..10: EBAB..EBB5.
            v.push(char::from_u32(if charging { 0xEBAB + step } else { 0xE850 + step }).unwrap_or('\u{E83F}'));
        }
        v
    }

    fn format(&self, family: PCWSTR, size: f32, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.dw.CreateTextFormat(family, None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr"))?;
            f.SetTextAlignment(align)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(f)
        }
    }

    fn text_w(&self, f: &IDWriteTextFormat, s: &str) -> Result<f32> {
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let l = self.dw.CreateTextLayout(&wide, f, 10_000.0, 100.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            l.GetMetrics(&mut m)?;
            Ok(m.widthIncludingTrailingWhitespace.ceil())
        }
    }

    fn draw(&mut self, gfx: &Gfx, h: f32, s: f32) -> Result<()> {
        let icon_f = self.format(w!("Segoe Fluent Icons"), (15.0 * s).round(), DWRITE_TEXT_ALIGNMENT_CENTER)?;
        let text_f = self.format(w!("Segoe UI Variable Text"), (12.0 * s).round(), DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let glyphs = self.glyphs();
        let cell = (26.0 * s).round();
        let gap = (10.0 * s).round();
        let quick_w = cell * glyphs.len() as f32 + gap;
        let text_w = self.text_w(&text_f, &self.sig.time)?.max(self.text_w(&text_f, &self.sig.date)?);
        let width = (quick_w + text_w + 6.0 * s).ceil();
        let (time, date) = (self.sig.time.clone(), self.sig.date.clone());
        let line = (h * 0.26).round();
        let surface = gfx.surface(width, h, |dc| unsafe {
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let (r, g, b) = carlui_ui::tone::unit(1.0, 1.0, 1.0);
            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a: carlui_ui::tone::ink_alpha(0.92) }, None)?;
            for (k, c) in glyphs.iter().enumerate() {
                let t: Vec<u16> = c.to_string().encode_utf16().collect();
                let x = k as f32 * cell;
                let rect = D2D_RECT_F { left: x, top: 0.0, right: x + cell, bottom: h };
                dc.DrawText(&t, &icon_f, &rect, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            let mid = h / 2.0;
            for (txt, top) in [(&time, mid - line - 1.0 * s), (&date, mid + 1.0 * s)] {
                let t: Vec<u16> = txt.encode_utf16().collect();
                let rect = D2D_RECT_F { left: quick_w, top, right: width, bottom: top + line };
                dc.DrawText(&t, &text_f, &rect, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            Ok(())
        })?;
        let brush = gfx.comp.CreateSurfaceBrushWithSurface(&surface)?;
        self.visual.SetBrush(&brush)?;
        self.visual.SetSize(v2(width, h))?;
        self.width = width;
        self.quick_w = quick_w;
        Ok(())
    }
}

impl Dock {
    /// Görev çubuğu görünümü mü (yalnızca altta)?
    pub fn taskbar(&self) -> bool {
        self.status.is_some()
    }

    /// Kenardan kenara (Full) görev çubuğu mu?
    pub(super) fn taskbar_full(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.full)
    }

    /// Durum alanının genişliği (yoksa 0).
    pub(super) fn status_w(&self) -> f32 {
        self.status.as_ref().map_or(0.0, |s| s.width)
    }

    /// Durum alanının sol kenarı (dock ortasına göre): Full'da sağ uçta,
    /// Compact'ta ortadaki dizilimin sonunda.
    pub(super) fn status_left(&self) -> f32 {
        match &self.status {
            Some(st) if st.full => self.len / 2.0 - self.m.pad - st.width,
            Some(st) => st.at,
            None => self.len / 2.0,
        }
    }

    /// Durum alanını yeniler (zamanlayıcıdan); genişlik değişirse yeniden dizer.
    pub fn status_tick(&mut self, gfx: &Gfx) {
        let (h, s) = (self.m.h, self.m.s);
        let Some(st) = self.status.as_mut() else { return };
        // Saat ayarı değişince dock baştan kurulmaz; bir sonraki yoklamada yeni biçimle çizilir.
        st.hour = self.cfg.clock.hour;
        match st.refresh(gfx, h, s, false) {
            Ok(true) => {
                let _ = self.relayout(true);
            }
            Ok(false) => {}
            Err(e) => carlui_core::warn!("durum alanı çizilemedi: {e}"),
        }
    }

    pub(super) fn place_status(&self) -> Result<()> {
        if let Some(st) = &self.status {
            st.visual.SetOffset(v3(self.status_left(), 0.0))?;
        }
        Ok(())
    }

    /// İstemci noktası durum alanının hangi bölümünde?
    pub fn status_hit(&self, x: i32, y: i32) -> Option<Part> {
        let st = self.status.as_ref()?;
        if self.slid_away {
            return None;
        }
        let lc = y as f32 - self.c0();
        if !(0.0..=self.m.h).contains(&lc) {
            return None;
        }
        let la = self.local_a(x) - self.status_left();
        if la < 0.0 || la > st.width {
            return None;
        }
        Some(if la < st.quick_w { Part::Quick } else { Part::Clock })
    }

    /// Durum alanının ekran dikdörtgeni ve dock'un üst kenarı (tray paneli için).
    pub fn status_rect(&self) -> (RECT, i32) {
        let w = self.status.as_ref().map(|s| s.quick_w).unwrap_or(0.0);
        let l = (self.len / 2.0 + self.status_left()) as i32 + self.wpos.0;
        let t = self.c0() as i32 + self.wpos.1;
        (RECT { left: l, top: t, right: l + w as i32, bottom: t + self.m.h as i32 }, t)
    }
}
