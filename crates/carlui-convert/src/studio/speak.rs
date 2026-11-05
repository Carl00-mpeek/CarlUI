//! Stüdyonun "metinden sese" işleri: metni dinletmek ve ses dosyası
//! olarak kaydetmek (WAV; FFmpeg varsa MP3).

use super::job::{Job, Work};
use crate::{ffmpeg, file_name, translate};
use carlui_ai::{catalog, catalog::Model, store, tts};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::Ordering, mpsc, Arc},
};
use windows::{
    core::PCWSTR,
    Win32::{Media::Audio::*, System::Com::*},
};

/// Cümleler arasındaki sessizlik (sn).
const GAP: f32 = 0.25;

/// Seçime göre ses: `None` = otomatik (Türkçe metin Türkçe sesle).
pub fn pick(choice: Option<&'static Model>, text: &str) -> &'static Model {
    choice.unwrap_or_else(|| {
        let cfg = carlui_core::config::load();
        if translate::is_turkish(text) {
            catalog::get(catalog::TURKISH_VOICE).unwrap_or_else(|| catalog::chosen(&cfg.ai, catalog::Feature::Speak))
        } else {
            catalog::chosen(&cfg.ai, catalog::Feature::Speak)
        }
    })
}

/// Çalan sesi hemen keser.
pub fn hush() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), None, SND_PURGE);
    }
}

fn open(job: &Job, m: &'static Model) -> Result<tts::Voice, String> {
    job.prepare(m)?;
    job.update(|s| s.note = t!("Ses açılıyor…", "Opening the voice…").into());
    tts::Voice::open(&store::dir(m))
}

/// Metni cümle cümle okur (bir cümle çalarken sonraki üretilir).
pub fn play(job: Arc<Job>, m: &'static Model, text: String, speed: f32) {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut voice = match open(&job, m) {
            Ok(v) => v,
            Err(e) => return job.fail(e),
        };
        let parts = tts::sentences(&text);
        let total = parts.len();
        let (tx, rx) = mpsc::sync_channel::<(usize, Vec<u8>)>(2);
        {
            let job = job.clone();
            std::thread::spawn(move || {
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                for (i, p) in parts.iter().enumerate() {
                    if job.stopped() {
                        break;
                    }
                    match voice.synth(p, speed) {
                        Ok(s) if !s.is_empty() => {
                            if tx.send((i, tts::wav(&s, voice.rate))).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => carlui_core::warn!("stüdyo: üretilemedi: {e}"),
                    }
                }
            });
        }
        job.update(|s| s.note = t!("Hazırlanıyor…", "Preparing…").into());
        for (i, w) in rx {
            if job.stopped() {
                break;
            }
            job.update(|s| {
                s.note = t!(format!("Okunuyor {}/{}", i + 1, total), format!("Playing {}/{}", i + 1, total));
                s.frac = (i + 1) as f32 / total.max(1) as f32;
            });
            unsafe {
                let _ = PlaySoundW(PCWSTR(w.as_ptr() as *const u16), None, SND_MEMORY | SND_SYNC | SND_NODEFAULT);
            }
        }
        if job.stopped() {
            job.fail(String::new());
        } else {
            job.finish(t!("Okuma bitti", "Finished playing").into(), None);
        }
    });
}

/// Metnin tamamını seslendirip `dst`'ye yazar.
pub fn save(job: Arc<Job>, m: &'static Model, text: String, speed: f32, dst: PathBuf) {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        match render(&job, m, &text, speed, &dst) {
            Ok(()) => {
                let p = dst.to_string_lossy().to_string();
                job.finish(t!(format!("Kaydedildi: {}", file_name(&p)), format!("Saved: {}", file_name(&p))), Some(p));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dst);
                job.fail(e);
            }
        }
    });
}

fn render(job: &Job, m: &'static Model, text: &str, speed: f32, dst: &Path) -> Result<(), String> {
    let mut voice = open(job, m)?;
    job.update(|s| s.work = Work::Saving);
    let parts = tts::sentences(text);
    let gap = vec![0f32; (voice.rate as f32 * GAP) as usize];
    let mut all = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if job.stopped() {
            return Err(String::new());
        }
        job.update(|s| {
            s.note = t!(format!("Seslendiriliyor {}/{}", i + 1, parts.len()), format!("Synthesizing {}/{}", i + 1, parts.len()));
            s.frac = i as f32 / parts.len().max(1) as f32 * 0.95;
        });
        let s = voice.synth(p, speed)?;
        if !s.is_empty() {
            if !all.is_empty() {
                all.extend(&gap);
            }
            all.extend(s);
        }
    }
    if all.is_empty() {
        return Err(t!("Okunacak bir şey bulunamadı", "Nothing to read").into());
    }
    let wav = tts::wav(&all, voice.rate);
    let mp3 = dst.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    if !mp3 {
        return std::fs::write(dst, wav).map_err(|e| e.to_string());
    }
    let ff = ffmpeg::find().ok_or_else(|| t!("MP3 için FFmpeg gerekli", "MP3 needs FFmpeg").to_string())?;
    let tmp = std::env::temp_dir().join(format!("carlui-studio-{}.wav", std::process::id()));
    std::fs::write(&tmp, wav).map_err(|e| e.to_string())?;
    job.update(|s| s.note = t!("MP3'e çevriliyor…", "Encoding MP3…").into());
    let args: Vec<String> = ["-c:a", "libmp3lame", "-q:a", "3"].iter().map(|s| s.to_string()).collect();
    let r = ffmpeg::run(&ff, &tmp.to_string_lossy(), &args, &dst.to_string_lossy(), &job.stop, &|f| job.update(|s| s.frac = 0.95 + f * 0.05));
    let _ = std::fs::remove_file(&tmp);
    r
}

/// İptal: hem üretimi hem çalan sesi durdurur.
pub fn stop(job: &Job) {
    job.stop.store(true, Ordering::SeqCst);
    hush();
}
