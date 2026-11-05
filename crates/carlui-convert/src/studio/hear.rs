//! Stüdyonun "sesten metne" işleri: mikrofondan kayıt ya da bir ses/video
//! dosyası, Whisper ile yazıya dökülür. Sonuç (metin ve zamanlı parçalar)
//! pencereye verilir; oradan .txt ya da .srt olarak kaydedilir.

use super::job::{Job, Work};
use crate::{ai, ffmpeg};
use carlui_ai::{catalog::Model, mic::Mic, whisper, whisper::Whisper};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use windows::Win32::System::Com::*;

/// En uzun mikrofon kaydı (sn).
const MAX_REC: f32 = 60.0 * 60.0;
/// 100 ms'lik pencerelerin en yüksek RMS'i bunun altındaysa kayıt sessiz sayılır.
const SILENCE: f32 = 0.006;

fn loudest(pcm: &[f32]) -> f32 {
    pcm.chunks(whisper::SR / 10).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).fold(0.0, f32::max)
}

/// Mikrofondan kayıt; `end_rec` gelince yazıya döker.
pub fn record(job: Arc<Job>, m: &'static Model, lang: Option<String>) {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut mic = match Mic::open() {
            Ok(m) => m,
            Err(e) => return job.fail(t!(format!("Mikrofon açılamadı: {e}"), format!("Couldn't open the microphone: {e}"))),
        };
        let mut pcm = Vec::new();
        let mut level = 0f32;
        loop {
            std::thread::sleep(Duration::from_millis(40));
            match mic.read(&mut pcm) {
                Ok(peak) => level = if peak > level { peak } else { level * 0.85 },
                Err(e) => return job.fail(e),
            }
            let secs = pcm.len() as f32 / whisper::SR as f32;
            job.update(|s| {
                s.level = level;
                s.seconds = secs;
            });
            if job.stopped() {
                return job.fail(String::new());
            }
            if job.end_rec.load(Ordering::SeqCst) || secs >= MAX_REC {
                break;
            }
        }
        drop(mic);
        if loudest(&pcm) < SILENCE {
            return job.fail(t!("Konuşma duyulmadı", "No speech heard").into());
        }
        transcribe(&job, m, &pcm, lang.as_deref());
    });
}

/// Ses/video dosyasını yazıya döker. `need_ff`: FFmpeg önce indirilsin.
pub fn file(job: Arc<Job>, m: &'static Model, lang: Option<String>, src: PathBuf, need_ff: bool) {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let src = src.to_string_lossy().to_string();
        let mut ff = ffmpeg::find();
        if ff.is_none() && need_ff {
            job.update(|s| s.note = t!("FFmpeg indiriliyor…", "Downloading FFmpeg…").into());
            match ffmpeg::download(&job.stop) {
                Ok(p) => ff = Some(p),
                Err(e) => return job.fail(e),
            }
        }
        job.update(|s| s.note = t!("Ses okunuyor…", "Reading the audio…").into());
        let pcm = match ai::load_pcm(&job.stop, &src, ff.as_ref(), &|f| job.update(|s| s.frac = f * 0.1)) {
            Ok(p) => p,
            Err(e) => return job.fail(e),
        };
        if loudest(&pcm) < SILENCE {
            return job.fail(t!("Dosyada konuşma bulunamadı", "No speech found in the file").into());
        }
        transcribe(&job, m, &pcm, lang.as_deref());
    });
}

fn transcribe(job: &Job, m: &'static Model, pcm: &[f32], lang: Option<&str>) {
    job.update(|s| {
        s.work = Work::Transcribing;
        s.level = 0.0;
    });
    if let Err(e) = job.prepare(m) {
        return job.fail(e);
    }
    job.update(|s| {
        s.work = Work::Transcribing;
        s.note = t!("Yazıya dökülüyor…", "Transcribing…").into();
        s.frac = 0.1;
    });
    let mut wh = match ai::speech_dir(m).ok_or_else(|| t!("Model bulunamadı", "Model not found").to_string()).and_then(|d| Whisper::open(&d)) {
        Ok(w) => w,
        Err(e) => return job.fail(e),
    };
    let mins = pcm.len() as f32 / whisper::SR as f32 / 60.0;
    match wh.transcribe(pcm, lang, true, &job.stop, &|f| job.update(|s| s.frac = 0.1 + f * 0.9)) {
        Ok((segs, l)) if !segs.is_empty() => {
            carlui_core::info!("stüdyo: {:.1} dk yazıya döküldü, {} parça, dil {l}", mins, segs.len());
            let text = whisper::to_text(&segs).trim().to_string();
            job.update(|s| {
                s.result = Some((text, segs));
                s.work = Work::Idle;
                s.frac = 1.0;
                s.note = t!("Yazıya döküldü; metni düzenleyip kaydedebilirsin", "Transcribed; edit the text and save it").into();
            });
        }
        Ok(_) => job.fail(t!("Konuşma bulunamadı", "No speech found").into()),
        Err(e) => job.fail(e),
    }
}
