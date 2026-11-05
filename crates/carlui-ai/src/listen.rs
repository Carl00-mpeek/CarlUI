//! Bu modül "arka planda mikrofonu dinleyip sesli komutu nasıl yakalarız?"
//! sorusunu çözüyor. İki iş parçacığı çalışır: biri mikrofonu okur (WASAPI
//! tamponu taşmasın diye hiç beklemez), öteki sesi tanıyıcıya verir.
//! - Vosk: ses akış halinde beslenir; cümle bitince metin gelir.
//! - Whisper: enerji tabanlı konuşma algılama (VAD) kısa bir söz parçasını
//!   ayırır, yalnızca o parça yazıya dökülür; sessizken model hiç çalışmaz.
//! Metin `command::parse`'tan geçerse çağırana bildirilir. Ses hiçbir yere
//! kaydedilmez, gönderilmez.

use crate::{
    catalog::Model,
    command::{self, Cmd},
    mic::Mic,
    store,
    vosk::Recognizer,
    whisper::{Whisper, SR},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

pub enum Engine {
    Vosk(Recognizer),
    Whisper(Box<Whisper>, Option<String>, Vad),
}

impl Engine {
    pub fn open(m: &Model, lang: &str) -> Result<Self, String> {
        let dir = store::dir(m);
        if let Some(dll) = store::find(&dir, "libvosk.dll") {
            let mdl = store::find(&dir, "final.mdl").and_then(|p| p.parent().map(|d| d.to_path_buf())).ok_or("Vosk modeli bulunamadı")?;
            return Ok(Engine::Vosk(Recognizer::open(&dll, &mdl, command::WORDS_TR)?));
        }
        let d = store::find(&dir, "model.safetensors").and_then(|p| p.parent().map(|d| d.to_path_buf())).ok_or("Whisper modeli bulunamadı")?;
        // Kısa komutta otomatik dil bulma güvenilmez; arayüz dilini varsay.
        let lang = Some(if lang.is_empty() { t!("tr", "en") } else { lang }.to_string());
        Ok(Engine::Whisper(Box::new(Whisper::open(&d)?), lang, Vad::default()))
    }

    pub fn feed(&mut self, pcm: &[f32], stop: &AtomicBool) -> Option<String> {
        match self {
            Engine::Vosk(r) => r.feed(pcm),
            Engine::Whisper(wh, lang, vad) => {
                let utt = vad.push(pcm)?;
                let t0 = std::time::Instant::now();
                let r = wh.transcribe(&utt, lang.as_deref(), false, stop, &|_| {});
                let text = r.ok().map(|(s, _)| crate::whisper::to_text(&s).trim().to_string())?;
                carlui_core::info!("sesli komut: {:.1} sn söz, {} ms", utt.len() as f32 / SR as f32, t0.elapsed().as_millis());
                Some(text)
            }
        }
    }
}

/// Basit konuşma algılayıcı: 30 ms'lik çerçevelerin enerjisini yavaş izlenen
/// gürültü tabanıyla karşılaştırır.
pub struct Vad {
    floor: f32,
    frame: Vec<f32>,
    /// Konuşma başlamadan önceki kısa kuyruk (sözün başı kesilmesin).
    pre: std::collections::VecDeque<f32>,
    utt: Vec<f32>,
    speaking: bool,
    quiet: usize,
    voiced: usize,
}

const FRAME: usize = SR * 30 / 1000;
const PRE: usize = SR * 3 / 10;
/// Bu kadar sessizlik sözü bitirir.
const END_QUIET: usize = 20; // 600 ms
/// Daha kısa sesler (tık, öksürük) atılır.
const MIN_VOICED: usize = 8; // 240 ms
const MAX_UTT: usize = SR * 6;

impl Default for Vad {
    fn default() -> Self {
        Self { floor: 0.003, frame: Vec::with_capacity(FRAME), pre: Default::default(), utt: Vec::new(), speaking: false, quiet: 0, voiced: 0 }
    }
}

impl Vad {
    /// Ses ekler; bir söz bittiyse onu döndürür.
    pub fn push(&mut self, pcm: &[f32]) -> Option<Vec<f32>> {
        let mut out = None;
        for &v in pcm {
            self.frame.push(v);
            if self.frame.len() < FRAME {
                continue;
            }
            let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let voice = rms > (self.floor * 3.0).max(0.008);
            if !voice {
                // Taban sessizlikte izlenir; konuşma onu yukarı çekmesin.
                self.floor = self.floor * 0.95 + rms * 0.05;
            }
            let frame = std::mem::take(&mut self.frame);
            if self.speaking {
                self.utt.extend_from_slice(&frame);
                if voice {
                    self.voiced += 1;
                    self.quiet = 0;
                } else {
                    self.quiet += 1;
                }
                if self.quiet >= END_QUIET || self.utt.len() >= MAX_UTT {
                    self.speaking = false;
                    let utt = std::mem::take(&mut self.utt);
                    if self.voiced >= MIN_VOICED && out.is_none() {
                        out = Some(utt);
                    }
                }
            } else if voice {
                self.speaking = true;
                self.quiet = 0;
                self.voiced = 1;
                self.utt = self.pre.drain(..).collect();
                self.utt.extend_from_slice(&frame);
            } else {
                self.pre.extend(frame.iter());
                while self.pre.len() > PRE {
                    self.pre.pop_front();
                }
            }
            self.frame = frame;
            self.frame.clear();
        }
        out
    }
}

/// Çalışan dinleyici; düşürülünce durur.
pub struct Listener {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Listener {
    /// Modeli açar ve dinlemeye başlar. Komut tanınınca `on` çağrılır
    /// (dinleme iş parçacığında). Model ya da mikrofon açılamazsa hata döner.
    pub fn start(m: &'static Model, lang: &str, on: impl Fn(Cmd) + Send + 'static) -> Result<Self, String> {
        if !store::installed(m) {
            return Err("model indirilmemiş".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let lang = lang.to_string();
        let st = stop.clone();
        let rec = std::thread::Builder::new()
            .name("voice-rec".into())
            .spawn(move || {
                let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
                let mut eng = match Engine::open(m, &lang) {
                    Ok(e) => {
                        let _ = ready_tx.send(Ok(()));
                        e
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                carlui_core::info!("sesli komut dinleniyor ({})", m.id);
                while let Ok(pcm) = rx.recv() {
                    if st.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Some(text) = eng.feed(&pcm, &st) {
                        match command::parse(&text) {
                            Some(c) => {
                                carlui_core::info!("sesli komut: {text:?} → {c:?}");
                                on(c);
                            }
                            None => {}
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        ready_rx.recv().unwrap_or_else(|_| Err("tanıyıcı açılamadı".into()))?;
        let st = stop.clone();
        let (mic_tx, mic_rx) = mpsc::channel::<Result<(), String>>();
        let cap = std::thread::Builder::new()
            .name("voice-mic".into())
            .spawn(move || {
                let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
                let mut mic = match Mic::open() {
                    Ok(m) => {
                        let _ = mic_tx.send(Ok(()));
                        m
                    }
                    Err(e) => {
                        let _ = mic_tx.send(Err(e));
                        return;
                    }
                };
                let mut buf = Vec::new();
                while !st.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                    if let Err(e) = mic.read(&mut buf) {
                        carlui_core::warn!("sesli komut: mikrofon okunamadı: {e}");
                        break;
                    }
                    if !buf.is_empty() && tx.send(std::mem::take(&mut buf)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let mut me = Self { stop, threads: vec![rec, cap] };
        if let Err(e) = mic_rx.recv().unwrap_or_else(|_| Err("mikrofon".into())) {
            me.stop_now();
            return Err(e);
        }
        Ok(me)
    }

    fn stop_now(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vad_splits_speech() {
        let mut v = Vad::default();
        let quiet = vec![0.001f32; SR];
        let voice: Vec<f32> = (0..SR).map(|i| (i as f32 * 0.07).sin() * 0.2).collect();
        assert!(v.push(&quiet).is_none());
        assert!(v.push(&voice).is_none());
        let utt = v.push(&quiet).expect("söz bitmeli");
        // 1 sn konuşma + ön kuyruk + bitiş sessizliği.
        let secs = utt.len() as f32 / SR as f32;
        assert!(secs > 1.2 && secs < 2.0, "{secs}");
        // Kısa tık atılır.
        let click: Vec<f32> = (0..FRAME * 3).map(|i| (i as f32).sin() * 0.3).collect();
        assert!(v.push(&click).is_none());
        assert!(v.push(&quiet).is_none());
    }
}
