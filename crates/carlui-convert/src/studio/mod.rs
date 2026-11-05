//! Bu modül "metni sese, sesi metne bilgisayarda nasıl çeviririz ve dosya
//! olarak saklarız?" sorununu çözüyor (Ses stüdyosu, `--studio`). İki sekme:
//! - Metinden sese: metin yazılır/yapıştırılır, ses ve hız seçilir; dinlenir
//!   ya da WAV/MP3 olarak kaydedilir.
//! - Sesten metne: mikrofonla kayıt ya da ses/video dosyası Whisper ile
//!   yazıya dökülür; metin düzenlenip .txt ya da altyazı (.srt) kaydedilir.
//! Her şey yerel modellerle çalışır; hiçbir şey buluta gitmez.

mod dialog;
mod hear;
mod job;
mod speak;

use carlui_ai::{catalog, catalog::Feature, catalog::Model, whisper, whisper::Segment};
use carlui_ui::{
    paint::{Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    textarea::TextArea,
    view::{self, Ctx, Opts, View},
};
use job::{Job, Work};
use std::sync::{atomic::Ordering, Arc};
use windows::{core::Result, Win32::Foundation::HWND};

const SPEEDS: [f32; 4] = [0.8, 1.0, 1.25, 1.5];
const LANGS: [&str; 3] = ["", "tr", "en"];
const PAD: f32 = 20.0;

#[derive(Clone, PartialEq)]
enum Act {
    Mode(usize),
    Voice(usize),
    Speed(usize),
    Lang(usize),
    Play,
    Save,
    Record,
    Open,
    Copy,
    Cancel,
    Reveal,
    Area,
}

struct Studio {
    mode: usize,
    tts: TextArea,
    stt: TextArea,
    voices: Vec<Option<&'static Model>>,
    voice: usize,
    speed: usize,
    lang: usize,
    segs: Vec<Segment>,
    job: Arc<Job>,
    hwnd: HWND,
    accent: Option<u32>,
}

pub fn run(accent: Option<u32>) -> Result<()> {
    let cfg = carlui_core::config::load();
    let mut voices: Vec<Option<&'static Model>> = vec![None];
    voices.extend(catalog::get(catalog::TURKISH_VOICE).map(Some));
    voices.extend(catalog::of(Feature::Speak).filter(|m| m.id != catalog::TURKISH_VOICE).map(Some));
    let speed = SPEEDS.iter().position(|s| (s - cfg.ai.speak_speed).abs() < 0.01).unwrap_or(1);
    let lang = LANGS.iter().position(|l| *l == cfg.ai.speech_lang).unwrap_or(0);
    let mut tts = TextArea::new(t!("Seslendirilecek metni buraya yaz ya da yapıştır…", "Type or paste the text to speak here…"));
    tts.max = 50_000;
    let stt = TextArea::new(t!("Kayıt ya da dosya yazıya döküldüğünde metin burada görünür; düzenleyebilirsin.", "The transcript appears here; you can edit it."));
    let s = Studio { mode: 0, tts, stt, voices, voice: 0, speed, lang, segs: Vec::new(), job: Job::new(), hwnd: HWND::default(), accent };
    view::run(s, Opts { title: t!("Ses stüdyosu", "Voice studio").into(), w: 820.0, h: 620.0, resizable: true, ..Default::default() })?;
    Ok(())
}

fn voice_label(m: Option<&Model>) -> String {
    match m.map(|m| m.id) {
        None => t!("Otomatik", "Automatic").into(),
        Some(catalog::TURKISH_VOICE) => t!("Türkçe", "Turkish").into(),
        Some("kokoro-q8") => "Kokoro".into(),
        Some("kokoro-fp32") => "Kokoro HQ".into(),
        Some(_) => m.map(|m| m.name.split(" (").next().unwrap_or(m.name).trim_start_matches("Piper ").to_string()).unwrap_or_default(),
    }
}

impl Studio {
    fn area(&mut self) -> &mut TextArea {
        if self.mode == 0 {
            &mut self.tts
        } else {
            &mut self.stt
        }
    }

    /// Model eksikse sorar (lisans onayı dahil); vazgeçilirse false.
    fn ask(&self, m: &'static Model) -> bool {
        let need = crate::ai::missing_bytes(m);
        need == 0 || crate::ask_model(self.hwnd, self.accent, m, need)
    }

    fn note(&self, s: String, failed: bool) {
        self.job.update(|st| {
            st.note = s;
            st.failed = failed;
            st.saved = None;
        });
    }

    fn tts_model(&self) -> &'static Model {
        speak::pick(self.voices[self.voice], &self.tts.text())
    }

    fn play(&mut self) {
        if self.job.work() == Work::Playing {
            return speak::stop(&self.job);
        }
        if self.tts.is_empty() {
            return self.note(t!("Önce bir metin yaz", "Type some text first").into(), true);
        }
        let m = self.tts_model();
        if !self.ask(m) {
            return;
        }
        let text = self.text_for_voice();
        if self.job.begin(Work::Playing, t!("Hazırlanıyor…", "Preparing…").into()) {
            speak::play(self.job.clone(), m, text, SPEEDS[self.speed]);
        }
    }

    /// Seçim varsa yalnızca seçili kısım okunur.
    fn text_for_voice(&self) -> String {
        let sel = self.tts.selected();
        if sel.chars().any(|c| c.is_alphanumeric()) {
            sel
        } else {
            self.tts.text()
        }
    }

    fn save_voice(&mut self) {
        if self.tts.is_empty() {
            return self.note(t!("Önce bir metin yaz", "Type some text first").into(), true);
        }
        let m = self.tts_model();
        let mp3 = crate::ffmpeg::find().is_some();
        let mut filters = vec![(t!("WAV sesi", "WAV audio"), "*.wav")];
        if mp3 {
            filters.push((t!("MP3 sesi", "MP3 audio"), "*.mp3"));
        }
        let name = suggested_name(&self.tts.text());
        let Some((dst, _)) = dialog::save(self.hwnd, &name, &filters) else { return };
        if !self.ask(m) {
            return;
        }
        if self.job.begin(Work::Saving, t!("Hazırlanıyor…", "Preparing…").into()) {
            speak::save(self.job.clone(), m, self.tts.text(), SPEEDS[self.speed], dst);
        }
    }

    fn stt_model(&self) -> &'static Model {
        catalog::chosen(&carlui_core::config::load().ai, Feature::Speech)
    }

    fn lang(&self) -> Option<String> {
        Some(LANGS[self.lang].to_string()).filter(|l| !l.is_empty())
    }

    fn record(&mut self) {
        match self.job.work() {
            Work::Recording => {
                self.job.end_rec.store(true, Ordering::SeqCst);
                return;
            }
            Work::Idle => {}
            _ => return,
        }
        let m = self.stt_model();
        if !self.ask(m) {
            return;
        }
        if self.job.begin(Work::Recording, t!("Dinleniyor… bitirince \"Kaydı bitir\"e bas", "Listening… press \"Stop recording\" when done").into()) {
            hear::record(self.job.clone(), m, self.lang());
        }
    }

    fn open_file(&mut self) {
        let filters = [(t!("Ses ve video", "Audio and video"), "*.wav;*.mp3;*.m4a;*.aac;*.flac;*.ogg;*.opus;*.wma;*.mp4;*.mkv;*.mov;*.avi;*.webm;*.wmv"), (t!("Tüm dosyalar", "All files"), "*.*")];
        let Some(src) = dialog::open(self.hwnd, &filters) else { return };
        let m = self.stt_model();
        let s = src.to_string_lossy().to_string();
        let mut need_ff = false;
        if !crate::ai::readable_without_ffmpeg(&s) && crate::ffmpeg::find().is_none() {
            if !crate::ask_ffmpeg(self.hwnd, self.accent) {
                return;
            }
            need_ff = true;
        }
        if !self.ask(m) {
            return;
        }
        if self.job.begin(Work::Transcribing, t!("Hazırlanıyor…", "Preparing…").into()) {
            hear::file(self.job.clone(), m, self.lang(), src, need_ff);
        }
    }

    fn save_text(&mut self) {
        if self.stt.is_empty() {
            return self.note(t!("Kaydedilecek metin yok", "There's no text to save").into(), true);
        }
        let mut filters = vec![(t!("Metin", "Text"), "*.txt")];
        if !self.segs.is_empty() {
            filters.push((t!("Altyazı", "Subtitles"), "*.srt"));
        }
        let name = suggested_name(&self.stt.text());
        let Some((dst, _)) = dialog::save(self.hwnd, &name, &filters) else { return };
        let srt = dst.extension().is_some_and(|e| e.eq_ignore_ascii_case("srt"));
        let body = if srt { format!("\u{FEFF}{}", whisper::to_srt(&self.segs)) } else { self.stt.text().replace('\n', "\r\n") + "\r\n" };
        let p = dst.to_string_lossy().to_string();
        match std::fs::write(&dst, body) {
            Ok(()) => self.job.finish(t!(format!("Kaydedildi: {}", crate::file_name(&p)), format!("Saved: {}", crate::file_name(&p))), Some(p)),
            Err(e) => self.note(e.to_string(), true),
        }
    }

    // ---- Çizim parçaları ----

    fn primary(p: &mut Painter<Act>, r: R, glyph: char, label: &str, act: Act, enabled: bool) {
        let hv = enabled && p.hover(r);
        p.round(r, 6.0, p.accent, if !enabled { 0.35 } else if hv { 1.0 } else { 0.88 });
        let tw = p.fonts.width(label, 13.0, true);
        let x = r.x + (r.w - tw - 22.0) / 2.0;
        p.glyph(glyph, R::new(x, r.y, 18.0, r.h), 13.0, 0x000000, 0.85);
        p.text(label, R::new(x + 22.0, r.y, tw + 4.0, r.h), 13.0, true, 0x000000, 0.85, Align::Left);
        if enabled {
            p.hit(r, act);
        }
    }

    fn label(p: &mut Painter<Act>, s: &str, x: f32, y: f32) {
        p.text(s, R::new(x, y, 60.0, 32.0), 13.0, false, TEXT, 0.6, Align::Left);
    }

    fn paint_tts(&mut self, p: &mut Painter<Act>, w: f32, h: f32, work: Work) {
        let lw = 46.0;
        Self::label(p, t!("Ses", "Voice"), PAD, 62.0);
        let labels: Vec<String> = self.voices.iter().map(|m| voice_label(*m)).collect();
        let items: Vec<(&str, Act)> = labels.iter().enumerate().map(|(i, l)| (l.as_str(), Act::Voice(i))).collect();
        let vw = (w - 2.0 * PAD - lw).min(110.0 * items.len() as f32);
        p.segments(R::new(PAD + lw, 62.0, vw, 32.0), &items, self.voice);

        Self::label(p, t!("Hız", "Speed"), PAD, 102.0);
        let sl = ["0,8×", "1×", "1,25×", "1,5×"];
        let sitems: Vec<(&str, Act)> = sl.iter().enumerate().map(|(i, l)| (*l, Act::Speed(i))).collect();
        p.segments(R::new(PAD + lw, 102.0, 260.0, 32.0), &sitems, self.speed);
        // Ticari olmayan lisanslı ses seçiliyse (ya da otomatik seçilecekse) uyarı.
        let m = self.tts_model();
        let info = format!("{} · {}", m.name, m.license);
        let nc = m.license.contains("NC");
        let ix = PAD + lw + 276.0;
        if w - ix - PAD > 80.0 {
            if nc {
                p.glyph('\u{E7BA}', R::new(ix, 102.0, 18.0, 32.0), 13.0, ORANGE, 0.95);
            }
            let tx = ix + if nc { 22.0 } else { 0.0 };
            p.text(&info, R::new(tx, 102.0, w - tx - PAD, 32.0), 12.0, false, if nc { ORANGE } else { TEXT }, if nc { 0.95 } else { 0.55 }, Align::Left);
        }

        let area = R::new(PAD, 146.0, w - 2.0 * PAD, (h - 146.0 - 70.0).max(60.0));
        self.tts.paint(p, area, Act::Area);

        let by = h - 54.0;
        let playing = work == Work::Playing;
        let idle = work == Work::Idle;
        Self::primary(p, R::new(PAD, by, 132.0, 36.0), if playing { '\u{E71A}' } else { '\u{E768}' }, if playing { t!("Durdur", "Stop") } else { t!("Dinle", "Listen") }, Act::Play, idle || playing);
        p.button(R::new(PAD + 142.0, by, 132.0, 36.0), Some('\u{E74E}'), t!("Kaydet…", "Save…"), Act::Save, idle, false);
        self.status(p, PAD + 290.0, by, w, Some(format!("{} {}", self.tts.len(), t!("karakter", "chars"))));
    }

    fn paint_stt(&mut self, p: &mut Painter<Act>, w: f32, h: f32, work: Work) {
        let recording = work == Work::Recording;
        let idle = work == Work::Idle;
        let rl = if recording { t!("Kaydı bitir", "Stop recording") } else { t!("Kayda başla", "Start recording") };
        Self::primary(p, R::new(PAD, 62.0, 160.0, 34.0), if recording { '\u{E71A}' } else { '\u{E720}' }, rl, Act::Record, idle || recording);
        p.button(R::new(PAD + 170.0, 62.0, 150.0, 34.0), Some('\u{E8E5}'), t!("Dosya aç…", "Open file…"), Act::Open, idle, false);
        if recording {
            let (level, secs) = self.job.state.lock().map(|s| (s.level, s.seconds)).unwrap_or_default();
            let x = PAD + 336.0;
            p.round(R::new(x, 75.0, 10.0, 10.0), 5.0, RED, 0.9);
            let t = secs as u32;
            p.text(&format!("{:02}:{:02}", t / 60, t % 60), R::new(x + 18.0, 62.0, 56.0, 34.0), 13.0, true, TEXT, 0.9, Align::Left);
            let mw = (w - x - 84.0 - PAD).clamp(0.0, 220.0);
            if mw > 20.0 {
                p.bar(R::new(x + 80.0, 76.0, mw, 6.0), (level * 4.0).sqrt().min(1.0), GREEN);
            }
        }

        Self::label(p, t!("Dil", "Language"), PAD, 106.0);
        let items = [(t!("Otomatik", "Automatic"), Act::Lang(0)), ("Türkçe", Act::Lang(1)), ("English", Act::Lang(2))];
        p.segments(R::new(PAD + 66.0, 106.0, 300.0, 32.0), &items, self.lang);
        let m = self.stt_model();
        let ix = PAD + 382.0;
        if w - ix - PAD > 80.0 {
            p.text(&format!("{} · {}", m.name, m.license), R::new(ix, 106.0, w - ix - PAD, 32.0), 12.0, false, TEXT, 0.55, Align::Left);
        }

        let area = R::new(PAD, 150.0, w - 2.0 * PAD, (h - 150.0 - 70.0).max(60.0));
        self.stt.paint(p, area, Act::Area);

        let by = h - 54.0;
        let has = !self.stt.is_empty();
        p.button(R::new(PAD, by, 120.0, 36.0), Some('\u{E8C8}'), t!("Kopyala", "Copy"), Act::Copy, has, false);
        p.button(R::new(PAD + 130.0, by, 132.0, 36.0), Some('\u{E74E}'), t!("Kaydet…", "Save…"), Act::Save, has && idle, false);
        self.status(p, PAD + 278.0, by, w, None);
    }

    /// Alt satırdaki durum: ilerleme, iptal, hata ya da "Klasörde göster".
    fn status(&self, p: &mut Painter<Act>, x: f32, y: f32, w: f32, right: Option<String>) {
        let Ok(s) = self.job.state.lock() else { return };
        let mut end = w - PAD;
        if let Some(r) = right.filter(|_| s.work == Work::Idle && s.note.is_empty()) {
            p.text(&r, R::new(x, y, end - x, 36.0), 12.0, false, TEXT, 0.5, Align::Right);
            return;
        }
        if s.work != Work::Idle && s.work != Work::Recording {
            p.button(R::new(end - 96.0, y, 96.0, 36.0), Some('\u{E711}'), t!("Vazgeç", "Cancel"), Act::Cancel, true, false);
            end -= 108.0;
            if s.frac > 0.0 {
                p.bar(R::new(x, y + 30.0, (end - x).max(0.0), 4.0), s.frac, p.accent);
            }
        } else if s.work == Work::Recording {
            p.button(R::new(end - 96.0, y, 96.0, 36.0), Some('\u{E711}'), t!("Vazgeç", "Cancel"), Act::Cancel, true, false);
            end -= 108.0;
        } else if s.saved.is_some() {
            p.button(R::new(end - 150.0, y, 150.0, 36.0), Some('\u{E838}'), t!("Klasörde göster", "Show in folder"), Act::Reveal, true, false);
            end -= 162.0;
        }
        let (c, a) = if s.failed { (RED, 0.95) } else { (TEXT, 0.75) };
        p.text(&s.note, R::new(x, y, (end - x).max(0.0), if s.frac > 0.0 && s.work != Work::Idle { 28.0 } else { 36.0 }), 12.5, false, c, a, Align::Left);
    }
}

/// Metnin ilk sözcüklerinden dosya adı önerisi.
fn suggested_name(text: &str) -> String {
    let words: Vec<String> = text.split_whitespace().take(6).map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '-').collect::<String>()).filter(|w| !w.is_empty()).collect();
    let n: String = words.join(" ").chars().take(48).collect();
    if n.is_empty() {
        t!("ses", "audio").into()
    } else {
        n
    }
}

impl View for Studio {
    type A = Act;

    fn paint(&mut self, p: &mut Painter<Act>, w: f32, h: f32) {
        // Yazıya dökme sonucu geldiyse metin alanına al.
        if let Some((text, segs)) = self.job.state.lock().ok().and_then(|mut s| s.result.take()) {
            self.stt.set_text(&text);
            self.segs = segs;
        }
        let work = self.job.work();
        let modes = [(t!("Metinden sese", "Text to speech"), Act::Mode(0)), (t!("Sesten metne", "Speech to text"), Act::Mode(1))];
        p.segments(R::new(PAD, 16.0, 340.0, 34.0), &modes, self.mode);
        if self.mode == 0 {
            self.paint_tts(p, w, h, work);
        } else {
            self.paint_stt(p, w, h, work);
        }
    }

    fn click(&mut self, a: Act, _ctx: &mut Ctx) {
        match a {
            Act::Mode(i) if i != self.mode => {
                self.mode = i;
                // Öbür sekmenin bitmiş işinin yazısı burada kalmasın.
                self.job.update(|s| {
                    if s.work == Work::Idle {
                        s.note.clear();
                        s.saved = None;
                        s.failed = false;
                    }
                });
            }
            Act::Mode(_) => {}
            Act::Voice(i) => self.voice = i,
            Act::Speed(i) => self.speed = i,
            Act::Lang(i) => self.lang = i,
            Act::Play => self.play(),
            Act::Save if self.mode == 0 => self.save_voice(),
            Act::Save => self.save_text(),
            Act::Record => self.record(),
            Act::Open => self.open_file(),
            Act::Copy => {
                let sel = self.stt.selected();
                let t = if sel.is_empty() { self.stt.text() } else { sel };
                if carlui_ui::textarea::set_clipboard(&t) {
                    self.job.finish(t!("Panoya kopyalandı", "Copied to the clipboard").into(), None);
                }
            }
            Act::Cancel => speak::stop(&self.job),
            Act::Reveal => {
                if let Some(p) = self.job.state.lock().ok().and_then(|s| s.saved.clone()) {
                    crate::reveal(&p);
                }
            }
            Act::Area => {}
        }
    }

    fn key(&mut self, vk: u16, _ctx: &mut Ctx) -> bool {
        // Ctrl+Enter: dinle / durdur.
        if vk == 0x0D && view::ctrl() && self.mode == 0 {
            self.play();
            return true;
        }
        self.area().key(vk)
    }

    fn char(&mut self, c: char, _ctx: &mut Ctx) {
        self.area().char(c);
    }

    fn press(&mut self, a: &Act, at: (f32, f32), _ctx: &mut Ctx) -> bool {
        if *a == Act::Area {
            self.area().press(at);
            return true;
        }
        false
    }

    fn drag(&mut self, at: (f32, f32), _ctx: &mut Ctx) {
        self.area().drag(at);
    }

    fn wheel(&mut self, delta: f32, at: (f32, f32), _ctx: &mut Ctx) {
        self.area().wheel(delta, at);
    }

    fn opened(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.job.set_hwnd(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(suggested_name("Merhaba, dünya! Bu bir deneme."), "Merhaba dünya Bu bir deneme");
        assert_eq!(suggested_name("  ...  "), t!("ses", "audio"));
    }
}
