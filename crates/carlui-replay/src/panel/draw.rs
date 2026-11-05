//! Bu modül "anlık tekrar paneli nasıl görünür?" sorununu çözüyor: küçük ana
//! sayfa (durum, başlat/durdur, son N dakikayı kaydet, ses anahtarları) ve
//! dişliyle açılan ayar sayfası (süre, çözünürlük, FPS sürgüsü, kalite,
//! sıkıştırma, ses düzeyi sürgüleri ve canlı göstergeler) burada çizilir.

use super::{meter, Panel, Slot, Src, A, PAD};
use crate::SAVING;
use carlui_core::config::{Codec, Quality, ReplayConfig, Resolution};
use carlui_ui::paint::{mix, Align, Painter, R, GREEN, ORANGE, RED, TEXT, YELLOW};
use std::sync::atomic::Ordering;

fn mmss(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn size(mb: f64) -> String {
    let s = if mb >= 1000.0 { format!("{:.1} GB", mb / 1000.0) } else { format!("{mb:.0} MB") };
    t!(s.replace('.', ","), s)
}

fn screen() -> (u32, u32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN).max(640) as u32, GetSystemMetrics(SM_CYSCREEN).max(480) as u32) }
}

fn pct(v: u32) -> String {
    t!(format!("%{v}"), format!("{v}%"))
}

impl Panel {
    /// `from`'dan beri eklenen tıklanabilir alanları bir gezinme satırı yapar.
    fn mark(&mut self, p: &Painter<A>, from: usize, row: u32) {
        for (r, a) in &p.hits[from..] {
            self.nav.push((row, *r, a.clone()));
        }
    }

    fn big(p: &mut Painter<A>, r: R, glyph: char, label: &str, act: A, filled: bool, enabled: bool, glyph_color: u32) {
        let hot = enabled && p.hover(r);
        let k = if enabled { 1.0 } else { 0.4 };
        if filled {
            p.round(r, 8.0, p.accent, (if hot { 1.0 } else { 0.9 }) * k);
        } else {
            p.round(r, 8.0, 0xFFFFFF, if hot { 0.12 } else { 0.07 });
            p.outline(r, 8.0, 0xFFFFFF, if hot { 0.22 } else { 0.10 });
        }
        let fg = if filled { 0x000000 } else { TEXT };
        let tw = p.fonts.width(label, 13.0, true);
        let x = r.x + (r.w - tw - 24.0) / 2.0;
        p.glyph(glyph, R::new(x, r.y, 18.0, r.h), 14.0, if filled { fg } else { glyph_color }, 0.95 * k);
        p.text(label, R::new(x + 24.0, r.y, tw + 4.0, r.h), 13.0, true, fg, 0.95 * k, Align::Left);
        if enabled {
            p.hit(r, act);
        }
    }

    fn icon_btn(p: &mut Painter<A>, r: R, glyph: char, act: A) {
        if p.hover(r) {
            p.round(r, 6.0, 0xFFFFFF, 0.10);
        }
        p.glyph(glyph, r, 12.5, TEXT, 0.85);
        p.hit(r, act);
    }

    /// Yatay sürgü: `r` tıklama alanı, iz ortada. `stops` > 0 ise durak noktaları.
    fn slider(&mut self, p: &mut Painter<A>, r: R, frac: f32, slot: Slot, stops: usize) {
        let hot = p.hover(r) || self.dragging == Some(slot);
        let tr = R::new(r.x, r.y + r.h / 2.0 - 2.0, r.w, 4.0);
        p.round(tr, 2.0, 0xFFFFFF, 0.14);
        let kx = tr.x + tr.w * frac.clamp(0.0, 1.0);
        if kx > tr.x {
            p.round(R::new(tr.x, tr.y, kx - tr.x, tr.h), 2.0, p.accent, 0.95);
        }
        for i in 0..stops {
            let x = tr.x + tr.w * i as f32 / (stops - 1).max(1) as f32;
            p.round(R::new(x - 2.0, tr.y, 4.0, 4.0), 2.0, if x <= kx { 0x000000 } else { TEXT }, 0.45);
        }
        let k = if hot { 8.0 } else { 7.0 };
        p.round(R::new(kx - k, tr.y + 2.0 - k, k * 2.0, k * 2.0), k, 0xFFFFFF, 0.95);
        // Uçlardaki duraklar kenardan da tutulsun: alan düğme yarıçapı kadar geniş.
        p.hit(R::new(r.x - 10.0, r.y, r.w + 20.0, r.h), A::Slide(slot));
        self.sliders.push((slot, R::new(tr.x, r.y, tr.w, r.h)));
    }

    /// Canlı düzey göstergesi (0..1): yeşil, yükseldikçe turuncu / kırmızı.
    fn meter(p: &Painter<A>, r: R, v: f32) {
        p.round(r, r.h / 2.0, 0xFFFFFF, 0.07);
        if v > 0.0 {
            let c = if v > 0.92 {
                RED
            } else if v > 0.75 {
                mix(GREEN, ORANGE, (v - 0.75) / 0.17)
            } else {
                GREEN
            };
            p.round(R::new(r.x, r.y, (r.w * v).max(r.h), r.h), r.h / 2.0, c, 0.9);
        }
    }

    pub(super) fn draw_main(&mut self, p: &mut Painter<A>, w: f32) {
        let cfg = self.st.cfg.clone();
        let rec = self.recording();
        let saving = SAVING.load(Ordering::Relaxed);
        let x0 = PAD;
        let cw = w - PAD * 2.0;

        // Başlık: durum noktası, ad, durum satırı; sağda ayarlar ve kapat.
        let dot = if rec { RED } else { 0x8A8A8A };
        let da = if rec && self.blink { 0.45 } else { 1.0 };
        p.round(R::new(x0 + 2.0, 21.0, 9.0, 9.0), 4.5, dot, da);
        p.text(t!("Anlık tekrar", "Instant replay"), R::new(x0 + 18.0, 10.0, 180.0, 22.0), 14.5, true, TEXT, 0.95, Align::Left);
        let sub = if saving {
            t!("Kaydediliyor…", "Saving…")
        } else if rec {
            t!("Kayıt sürüyor", "Recording")
        } else if cfg.enabled {
            t!("Başlatılıyor…", "Starting…")
        } else {
            t!("Kayıt kapalı", "Recording is off")
        };
        p.text(sub, R::new(x0 + 18.0, 30.0, 200.0, 16.0), 11.5, false, TEXT, 0.6, Align::Left);
        let s0 = p.hits.len();
        Self::icon_btn(p, R::new(w - PAD - 60.0, 12.0, 28.0, 28.0), '\u{E713}', A::Settings);
        Self::icon_btn(p, R::new(w - PAD - 28.0, 12.0, 28.0, 28.0), '\u{E8BB}', A::Close);
        self.mark(p, s0, 0);

        // Ana düğmeler.
        let y = 56.0;
        let rw = 112.0;
        let s0 = p.hits.len();
        if cfg.enabled {
            Self::big(p, R::new(x0, y, rw, 40.0), '\u{E71A}', t!("Durdur", "Stop"), A::Record, false, true, RED);
        } else {
            Self::big(p, R::new(x0, y, rw, 40.0), '\u{E7C8}', t!("Başlat", "Start"), A::Record, false, true, RED);
        }
        let save_label = if saving { t!("Kaydediliyor…", "Saving…").to_string() } else { t!(format!("Son {} dk'yı kaydet", cfg.minutes()), format!("Save last {} min", cfg.minutes())) };
        Self::big(p, R::new(x0 + rw + 8.0, y, cw - rw - 8.0, 40.0), '\u{E74E}', &save_label, A::Save, true, rec && !saving, TEXT);
        self.mark(p, s0, 1);

        // Arabellek: ne kadarlık geçmiş hazır.
        let keep = cfg.minutes() as u64 * 60;
        let have = self.st.started.filter(|_| rec).map(|t| t.elapsed().as_secs().min(keep)).unwrap_or(0);
        let y = 106.0;
        p.text(t!("Arabellekte", "Buffered"), R::new(x0, y, 160.0, 16.0), 11.5, false, TEXT, 0.6, Align::Left);
        p.text(&format!("{} / {}", mmss(have), mmss(keep)), R::new(x0 + cw - 160.0, y, 160.0, 16.0), 11.5, false, TEXT, 0.75, Align::Right);
        p.bar(R::new(x0, y + 20.0, cw, 4.0), have as f32 / keep.max(1) as f32, if rec { p.accent } else { 0x8A8A8A });

        // Ses anahtarları.
        let y = 142.0;
        let card = R::new(x0, y, cw, 72.0);
        p.card(card);
        for (i, (g, label, on, act)) in [('\u{E767}', t!("Bilgisayar sesi", "System audio"), cfg.audio, A::Audio), ('\u{E720}', t!("Mikrofon", "Microphone"), cfg.mic, A::Mic)].into_iter().enumerate() {
            let ry = y + 4.0 + i as f32 * 32.0;
            let row = R::new(card.x + 4.0, ry, card.w - 8.0, 32.0);
            if p.hover(row) {
                p.round(row, 6.0, 0xFFFFFF, 0.04);
            }
            p.glyph(g, R::new(card.x + 10.0, ry, 20.0, 32.0), 13.0, TEXT, 0.85);
            p.text(label, R::new(card.x + 36.0, ry, 180.0, 32.0), 12.5, false, TEXT, 0.9, Align::Left);
            let s0 = p.hits.len();
            p.switch(card.right() - 46.0, ry + 7.0, on, true, act.clone());
            // Satırın tamamı da tıklanır (fare kilitli oyunlarda isabet kolay olsun).
            p.hit(R::new(row.x, row.y, row.w - 52.0, row.h), act);
            let n0 = self.nav.len();
            self.mark(p, s0, 2 + i as u32);
            // Gezinmede satır tek öğe sayılır.
            self.nav.truncate(n0 + 1);
        }

        let keys = format!("{} {}  ·  {} {}", self.st.save_key, t!("kaydet", "save"), self.st.panel_key, t!("panel", "panel"));
        p.text(&keys, R::new(x0, 222.0, cw, 16.0), 11.0, false, mix(TEXT, p.accent, 0.3), 0.55, Align::Center);
    }

    pub(super) fn draw_settings(&mut self, p: &mut Painter<A>, w: f32) {
        let cfg = self.st.cfg.clone();
        let x0 = PAD;
        let cw = w - PAD * 2.0;

        // Başlık: geri, ad, kapat.
        let s0 = p.hits.len();
        Self::icon_btn(p, R::new(x0 - 4.0, 12.0, 28.0, 28.0), '\u{E72B}', A::Back);
        p.text(t!("Kayıt ayarları", "Recording settings"), R::new(x0 + 32.0, 12.0, 200.0, 28.0), 14.5, true, TEXT, 0.95, Align::Left);
        Self::icon_btn(p, R::new(w - PAD - 28.0, 12.0, 28.0, 28.0), '\u{E8BB}', A::Close);
        self.mark(p, s0, 0);

        // Görüntü kartı.
        let y = 52.0;
        let card = R::new(x0, y, cw, 192.0);
        p.card(card);
        let lx = card.x + 12.0;
        let sx = card.x + 92.0;
        let sw = card.right() - 8.0 - sx;
        let label = |p: &Painter<A>, ry: f32, s: &str| p.text(s, R::new(lx, ry, 80.0, 30.0), 12.5, false, TEXT, 0.75, Align::Left);
        let minutes = [1u32, 2, 5, 10, 20, 30];
        let rows: [(&str, Vec<(String, A)>, usize); 4] = [
            (t!("Süre (dk)", "Length (min)"), minutes.iter().map(|&m| (m.to_string(), A::Minutes(m))).collect(), minutes.iter().position(|&m| m == cfg.minutes()).unwrap_or(usize::MAX)),
            (
                t!("Çözünürlük", "Resolution"),
                vec![(t!("Yerel", "Native").into(), A::Res(Resolution::Native)), ("1440p".into(), A::Res(Resolution::P1440)), ("1080p".into(), A::Res(Resolution::P1080)), ("720p".into(), A::Res(Resolution::P720))],
                [Resolution::Native, Resolution::P1440, Resolution::P1080, Resolution::P720].iter().position(|&r| r == cfg.resolution).unwrap_or(0),
            ),
            (
                t!("Kalite", "Quality"),
                vec![(t!("Düşük", "Low").into(), A::Quality(Quality::Low)), (t!("Orta", "Medium").into(), A::Quality(Quality::Medium)), (t!("Yüksek", "High").into(), A::Quality(Quality::High))],
                [Quality::Low, Quality::Medium, Quality::High].iter().position(|&q| q == cfg.quality).unwrap_or(1),
            ),
            (t!("Sıkıştırma", "Codec"), vec![("H.264".into(), A::Codec(Codec::H264)), ("H.265".into(), A::Codec(Codec::Hevc))], if cfg.codec == Codec::Hevc { 1 } else { 0 }),
        ];
        let mut ry = y + 8.0;
        for (i, (name, items, sel)) in rows.iter().enumerate() {
            if i == 2 {
                // FPS: sürgüyle 30–144.
                label(p, ry, "FPS");
                let fps = &ReplayConfig::FPS;
                let fi = fps.iter().position(|&f| f == cfg.fps()).unwrap_or(2);
                p.text(&cfg.fps().to_string(), R::new(card.right() - 44.0, ry, 34.0, 30.0), 12.5, true, TEXT, 0.9, Align::Right);
                let s0 = p.hits.len();
                self.slider(p, R::new(sx + 6.0, ry, sw - 56.0, 30.0), fi as f32 / (fps.len() - 1) as f32, Slot::Fps, fps.len());
                self.mark(p, s0, 3);
                ry += 36.0;
            }
            label(p, ry, name);
            let items: Vec<(&str, A)> = items.iter().map(|(s, a)| (s.as_str(), a.clone())).collect();
            let s0 = p.hits.len();
            p.segments(R::new(sx, ry, sw, 30.0), &items, *sel);
            self.mark(p, s0, 1 + i as u32 + (i >= 2) as u32);
            ry += 36.0;
        }

        // Tahmini boyut ve yüksek ayar uyarısı.
        let (sw, sh) = screen();
        let mb = cfg.mb_per_minute(sw, sh);
        let line = t!(
            format!("1 dk ≈ {}  ·  {} dk ≈ {}", size(mb), cfg.minutes(), size(mb * cfg.minutes() as f64)),
            format!("1 min ≈ {}  ·  {} min ≈ {}", size(mb), cfg.minutes(), size(mb * cfg.minutes() as f64))
        );
        p.text(t!("Tahmini boyut", "Estimated size"), R::new(x0 + 2.0, 250.0, 120.0, 18.0), 11.5, false, TEXT, 0.6, Align::Left);
        p.text(&line, R::new(x0 + 100.0, 250.0, cw - 102.0, 18.0), 11.5, true, TEXT, 0.9, Align::Right);
        if cfg.heavy(sw, sh) {
            let r = R::new(x0, 272.0, cw, 38.0);
            p.round(r, 6.0, YELLOW, 0.10);
            p.glyph('', R::new(r.x + 8.0, r.y, 18.0, r.h), 14.0, YELLOW, 1.0);
            p.text(t!("Yüksek FPS/çözünürlük çok yer kaplar.", "High FPS/resolution takes a lot of space."), R::new(r.x + 32.0, r.y + 3.0, r.w - 36.0, 16.0), 11.0, true, YELLOW, 0.95, Align::Left);
            p.text(t!("Kayıt oyunla aynı diskteyse donma yapabilir.", "On the game's disk, the game may stutter."), R::new(r.x + 32.0, r.y + 19.0, r.w - 36.0, 16.0), 11.0, false, YELLOW, 0.85, Align::Left);
        }

        // Ses düzeyi kartı.
        let y = 316.0;
        let card = R::new(x0, y, cw, 164.0);
        p.card(card);
        let srcs = [(Src::Sys, '\u{E767}', t!("Bilgisayar sesi", "System audio")), (Src::Mic, '\u{E720}', t!("Mikrofon", "Microphone"))];
        for (i, (src, g, name)) in srcs.into_iter().enumerate() {
            let ry = y + 6.0 + i as f32 * 60.0;
            let v = src.get(&cfg);
            p.glyph(g, R::new(card.x + 10.0, ry, 20.0, 26.0), 13.0, TEXT, 0.85);
            p.text(name, R::new(card.x + 36.0, ry, 150.0, 26.0), 12.5, false, TEXT, 0.9, Align::Left);
            p.text(&pct(v), R::new(card.right() - 60.0, ry, 48.0, 26.0), 12.5, true, TEXT, 0.9, Align::Right);
            if src == Src::Mic {
                // Mikrofon testi: kayıt kapalıyken de düzeyi gösterir.
                let on = meter::on();
                let r = R::new(card.right() - 132.0, ry + 2.0, 64.0, 22.0);
                let hot = p.hover(r);
                p.round(r, 11.0, if on { p.accent } else { 0xFFFFFF }, if on { 0.9 } else if hot { 0.14 } else { 0.08 });
                p.text(if on { t!("Durdur", "Stop") } else { t!("Test et", "Test") }, r, 11.5, true, if on { 0x000000 } else { TEXT }, 0.9, Align::Center);
                let s0 = p.hits.len();
                p.hit(r, A::Test);
                self.mark(p, s0, 7);
            }
            let s0 = p.hits.len();
            self.slider(p, R::new(card.x + 18.0, ry + 26.0, card.w - 36.0, 20.0), v as f32 / ReplayConfig::MAX_VOLUME as f32, Slot::Vol(src), 0);
            self.mark(p, s0, 6 + i as u32 * 2);
            Self::meter(p, R::new(card.x + 18.0, ry + 48.0, card.w - 36.0, 3.0), self.levels[i]);
        }
        // Gürültü engelleme: klavye, fan, uğultu mikrofondan süzülür.
        let row = R::new(card.x + 4.0, y + 126.0, card.w - 8.0, 34.0);
        if p.hover(row) {
            p.round(row, 6.0, 0xFFFFFF, 0.04);
        }
        p.glyph('', R::new(card.x + 10.0, row.y, 20.0, row.h), 13.0, TEXT, 0.85);
        p.text(t!("Gürültü engelleme", "Noise suppression"), R::new(card.x + 36.0, row.y + 1.0, 200.0, 18.0), 12.5, false, TEXT, 0.9, Align::Left);
        p.text(t!("Klavye, fan sesini mikrofondan siler", "Removes keyboard and fan noise from the mic"), R::new(card.x + 36.0, row.y + 17.0, 220.0, 15.0), 10.5, false, TEXT, 0.5, Align::Left);
        let s0 = p.hits.len();
        p.switch(card.right() - 46.0, row.y + 8.0, cfg.denoise, true, A::Denoise);
        p.hit(R::new(row.x, row.y, row.w - 52.0, row.h), A::Denoise);
        let n0 = self.nav.len();
        self.mark(p, s0, 9);
        self.nav.truncate(n0 + 1);
        if !meter::on() && !self.recording() {
            p.text(t!("Düzeyi görmek için Test et'e basın", "Press Test to see the level"), R::new(card.x, card.bottom() + 2.0, card.w, 14.0), 10.5, false, TEXT, 0.45, Align::Center);
        }

        // Alt düğmeler.
        let y = 496.0;
        let half = (cw - 8.0) / 2.0;
        let s0 = p.hits.len();
        p.button(R::new(x0, y, half, 30.0), Some('\u{E8B7}'), t!("Klasör", "Folder"), A::Folder, true, false);
        p.button(R::new(x0 + half + 8.0, y, half, 30.0), Some('\u{E713}'), t!("Tüm ayarlar", "All settings"), A::AllSettings, true, false);
        self.mark(p, s0, 10);
    }
}
