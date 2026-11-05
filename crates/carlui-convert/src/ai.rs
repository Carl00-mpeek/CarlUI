//! Bu modül "dönüştürücü menüsünden resmin arka planını nasıl sileriz, nasıl
//! büyütürüz, ses/videoyu nasıl yazıya ya da altyazıya dökeriz?" sorusunu
//! çözüyor. Ayarlarda seçili model (yoksa önerilen) kullanılır; model ya da
//! yapay zekâ motoru kurulu değilse önce sorulur, onay verilirse resmi
//! kaynaktan indirilir (ilerleme aynı kartta görünür). Sonuç orijinalin
//! yanına yazılır; orijinale dokunulmaz.

use crate::{ffmpeg, file_name, image, kinds, kinds::Kind, progress::Shared};
use carlui_ai::{
    audio,
    catalog::{self, Feature, Model},
    esrgan::Upscaler,
    rmbg::Remover,
    runtime, store,
    whisper::{self, Whisper},
    Image,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Büyütülmüş resim en fazla bu kadar piksel olabilir (bellek sınırı).
const MAX_OUT_PIXELS: u64 = 120_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Background,
    Upscale,
    /// Ses/video → düz metin (.txt)
    Transcript,
    /// Ses/video → altyazı (.srt)
    Subtitles,
}

impl Op {
    pub fn feature(self) -> Feature {
        match self {
            Op::Background => Feature::Background,
            Op::Upscale => Feature::Upscale,
            Op::Transcript | Op::Subtitles => Feature::Speech,
        }
    }

    /// Bu işin uygulanabildiği dosya türleri.
    pub fn kinds(self) -> &'static [Kind] {
        match self {
            Op::Background | Op::Upscale => &[Kind::Image],
            Op::Transcript | Op::Subtitles => &[Kind::Audio, Kind::Video],
        }
    }

    pub fn is_speech(self) -> bool {
        self.feature() == Feature::Speech
    }

    pub fn model(self) -> &'static Model {
        catalog::chosen(&carlui_core::config::load().ai, self.feature())
    }

    /// Menüde görünen ad.
    pub fn name(self) -> String {
        match self {
            Op::Background => t!("Arka planı sil", "Remove background").into(),
            Op::Upscale => t!(format!("Büyüt (×{})", scale(self.model())), format!("Upscale (×{})", scale(self.model()))),
            Op::Transcript => t!("Yazıya dök (.txt)", "Transcribe (.txt)").into(),
            Op::Subtitles => t!("Altyazı çıkar (.srt)", "Generate subtitles (.srt)").into(),
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Op::Background => '\u{E7C5}',
            Op::Upscale => '\u{E740}',
            Op::Transcript => '\u{E8A5}',
            Op::Subtitles => '\u{ED1E}',
        }
    }
}

fn scale(m: &Model) -> u32 {
    if m.id == "esrgan-x2" {
        2
    } else {
        4
    }
}

/// İndirilmesi gereken boyut (motor + model); kuruluysa 0.
pub fn missing_bytes(m: &Model) -> u64 {
    let mut n = 0;
    if !runtime::installed() {
        n += 12_400_000;
    }
    if !store::installed(m) {
        n += m.size();
    }
    n
}

pub fn mb(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1} GB", n as f64 / 1e9)
    } else {
        format!("{} MB", (n as f64 / 1e6).ceil() as u64)
    }
}

/// Ses dosyası FFmpeg'siz okunabilir mi (WAV doğrudan okunur).
pub fn readable_without_ffmpeg(src: &str) -> bool {
    kinds::ext(src) == "wav"
}

fn output_path(op: Op, src: &str, m: &Model) -> String {
    let p = std::path::Path::new(src);
    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "resim".into());
    let (stem, ext) = match op {
        Op::Background => (format!("{stem} - {}", t!("arka plansız", "no background")), "png".to_string()),
        Op::Upscale => {
            let e = kinds::ext(src);
            // Okunup yazılamayan biçimler PNG olur.
            let e = if matches!(e.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "tga" | "dds" | "webp" | "avif" | "jxl") { e } else { "png".into() };
            (format!("{stem} (×{})", scale(m)), e)
        }
        Op::Transcript => (stem, "txt".into()),
        Op::Subtitles => (stem, "srt".into()),
    };
    kinds::output_path(&dir.join(format!("{stem}.{ext}")).to_string_lossy(), &ext)
}

fn enc_ext(dst: &str) -> String {
    match kinds::ext(dst).as_str() {
        "jpeg" => "jpg".into(),
        "tif" => "tiff".into(),
        e => e.into(),
    }
}

/// Ses/videodan 16 kHz tek kanal örnekler (WAV doğrudan, diğerleri FFmpeg ile).
pub fn load_pcm(cancel: &Arc<AtomicBool>, src: &str, ff: Option<&PathBuf>, prog: &dyn Fn(f32)) -> Result<Vec<f32>, String> {
    if readable_without_ffmpeg(src) {
        if let Ok(p) = std::fs::read(src).map_err(|e| e.to_string()).and_then(|b| audio::read_wav(&b)) {
            return Ok(p);
        }
    }
    let ff = ff.ok_or_else(|| t!("FFmpeg bulunamadı", "FFmpeg not found").to_string())?;
    let tmp = std::env::temp_dir().join(format!("carlui-asr-{}.wav", std::process::id()));
    let tmp_s = tmp.to_string_lossy().to_string();
    let args: Vec<String> = ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"].iter().map(|s| s.to_string()).collect();
    let r = ffmpeg::run(ff, src, &args, &tmp_s, cancel, prog).and_then(|_| std::fs::read(&tmp).map_err(|e| e.to_string())).and_then(|b| audio::read_wav(&b));
    let _ = std::fs::remove_file(&tmp);
    r
}

pub fn fail(w: &Shared, e: String) {
    w.update(|s| {
        s.title = e;
        s.failed = true;
        s.done = true;
        s.frac = 1.0;
    })
}

/// Motor ve model kurulu değilse ilerleme kartında indirir. Başarısızsa kartı
/// hata ile kapatır ve false döner.
pub fn ensure(w: &Shared, m: &Model) -> bool {
    let cancel = w.cancel.clone();
    if !runtime::installed() {
        w.update(|s| {
            s.title = t!("Yapay zekâ motoru indiriliyor", "Downloading the AI engine").into();
            s.file = format!("ONNX Runtime {} · nuget.org", runtime::ORT_VERSION);
        });
        if let Err(e) = runtime::install(&cancel, &|f| w.update(|s| s.frac = f)) {
            fail(w, e);
            return false;
        }
    }
    if !store::installed(m) {
        w.update(|s| {
            s.title = t!(format!("{} indiriliyor", m.name), format!("Downloading {}", m.name));
            s.file = format!("{} · {}", mb(m.size()), m.source.trim_start_matches("https://"));
            s.frac = 0.0;
        });
        if let Err(e) = store::install(m, &cancel, &|f| w.update(|s| s.frac = f)) {
            fail(w, e);
            return false;
        }
    }
    true
}

/// Konuşma modelinin klasörü (model.safetensors'ın bulunduğu yer).
pub fn speech_dir(m: &Model) -> Option<PathBuf> {
    store::find(&store::dir(m), "model.safetensors").and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Motoru ve modeli (gerekirse) indirir, sonra her dosyayı işler.
pub fn run_all(w: &Shared, op: Op, files: &[String], ff: Option<&PathBuf>) {
    let m = op.model();
    let cancel = w.cancel.clone();
    let fail = |e: String| fail(w, e);
    if !ensure(w, m) {
        return;
    }
    w.update(|s| {
        s.title = t!("Model yükleniyor", "Loading the model").into();
        s.file = m.name.into();
        s.frac = 0.0;
    });
    let dir = store::dir(m);
    enum Eng {
        Bg(Remover),
        Up(Upscaler),
        Asr(Box<Whisper>),
    }
    let missing = || t!("Model dosyası bulunamadı", "Model file not found").to_string();
    let eng = match op {
        Op::Background => store::find(&dir, "model.onnx").ok_or_else(missing).and_then(|p| Remover::open(&p)).map(Eng::Bg),
        Op::Upscale => store::find(&dir, "model.pth").ok_or_else(missing).and_then(|p| Upscaler::open(&p)).map(Eng::Up),
        Op::Transcript | Op::Subtitles => speech_dir(m).ok_or_else(missing).and_then(|d| Whisper::open(&d)).map(|w| Eng::Asr(Box::new(w))),
    };
    let mut eng = match eng {
        Ok(e) => e,
        Err(e) => {
            carlui_core::warn!("model açılamadı ({}): {e}", m.id);
            return fail(t!("Model açılamadı", "Couldn't load the model").into());
        }
    };
    let lang = carlui_core::config::load().ai.speech_lang;
    let lang = Some(lang.as_str()).filter(|l| !l.is_empty());

    let n = files.len();
    let (mut ok, mut errors, mut last) = (0, Vec::new(), None);
    for (i, src) in files.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let name = file_name(src);
        let verb = match op {
            Op::Background => t!("Arka plan siliniyor", "Removing background"),
            Op::Upscale => t!("Büyütülüyor", "Upscaling"),
            Op::Transcript => t!("Yazıya dökülüyor", "Transcribing"),
            Op::Subtitles => t!("Altyazı çıkarılıyor", "Generating subtitles"),
        };
        w.update(|s| {
            s.title = if n > 1 { format!("{verb} · {}/{n}", i + 1) } else { verb.into() };
            s.file = name.clone();
            s.frac = i as f32 / n as f32;
        });
        let prog = |f: f32| w.update(|s| s.frac = (i as f32 + f) / n as f32);
        let dst = output_path(op, src, m);
        let r = match &mut eng {
            Eng::Asr(wh) => load_pcm(&w.cancel, src, ff, &|f| prog(f * 0.1)).and_then(|pcm| {
                let (segs, l) = wh.transcribe(&pcm, lang, op == Op::Subtitles, &cancel, &|f| prog(0.1 + f * 0.9))?;
                carlui_core::info!("yazıya dökme: {} parça, dil {l}", segs.len());
                if segs.is_empty() {
                    return Err(t!("Konuşma bulunamadı", "No speech found").into());
                }
                let body = if op == Op::Subtitles { format!("\u{FEFF}{}", whisper::to_srt(&segs)) } else { whisper::to_text(&segs) + "\r\n" };
                std::fs::write(&dst, body).map_err(|e| e.to_string())
            }),
            eng => image::decode(src, ff).and_then(|p| {
                let img = Image { w: p.w, h: p.h, bgra: p.data };
                let out = match eng {
                    Eng::Bg(r) => r.remove(&img, &cancel, &prog)?,
                    Eng::Up(u) => {
                        let s = u.scale as u64;
                        if img.w as u64 * img.h as u64 * s * s > MAX_OUT_PIXELS {
                            return Err(t!("Resim büyütmek için fazla büyük", "The image is too large to upscale").into());
                        }
                        u.upscale(&img, &cancel, &prog)?
                    }
                    Eng::Asr(_) => unreachable!(),
                };
                image::encode(&image::Pixels { w: out.w, h: out.h, data: out.bgra }, &dst, &enc_ext(&dst), ff)
            }),
        };
        match r {
            Ok(()) => {
                ok += 1;
                carlui_core::info!("yapay zekâ ({op:?}, {}): {src} → {dst}", m.id);
                last = Some(dst);
            }
            Err(_) if cancel.load(Ordering::SeqCst) => break,
            Err(e) => {
                carlui_core::warn!("yapay zekâ ({op:?}) başarısız: {src}: {e}");
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    let stopped = cancel.load(Ordering::SeqCst);
    w.update(|s| {
        s.done = true;
        s.frac = 1.0;
        s.failed = ok == 0 && (!errors.is_empty() || stopped);
        s.title = if stopped && ok == 0 {
            t!("Durduruldu", "Stopped").into()
        } else if errors.is_empty() || ok > 0 {
            match (op, ok) {
                (Op::Background, 1) => t!("Arka plan silindi", "Background removed").into(),
                (Op::Upscale, 1) => t!("Büyütüldü", "Upscaled").into(),
                (Op::Transcript, 1) => t!("Yazıya döküldü", "Transcribed").into(),
                (Op::Subtitles, 1) => t!("Altyazı hazır", "Subtitles ready").into(),
                _ if errors.is_empty() => t!(format!("{ok} dosya hazır"), format!("{ok} files done")),
                _ => t!(format!("{ok} dosya hazır · {} hata", errors.len()), format!("{ok} files done · {} errors", errors.len())),
            }
        } else {
            t!("İşlenemedi", "Processing failed").into()
        };
        s.file = match (&errors.first(), &last) {
            (Some(e), _) => e.to_string(),
            (None, Some(l)) => format!("{} · {}", file_name(l), t!("göstermek için tıkla", "click to show")),
            _ => String::new(),
        };
        s.reveal = last.clone();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names() {
        let m = catalog::get("esrgan-x4").unwrap();
        let d = std::env::temp_dir().join("carlui-ai-out-test");
        let _ = std::fs::create_dir_all(&d);
        let src = d.join("foto.jpeg").to_string_lossy().to_string();
        assert!(output_path(Op::Upscale, &src, m).ends_with("foto (×4).jpeg"));
        assert!(output_path(Op::Upscale, &d.join("a.heic").to_string_lossy(), m).ends_with("a (×4).png"));
        assert!(output_path(Op::Background, &src, m).ends_with(".png"));
        assert!(output_path(Op::Subtitles, &d.join("v.mp4").to_string_lossy(), m).ends_with("v.srt"));
        assert!(output_path(Op::Transcript, &d.join("s.mp3").to_string_lossy(), m).ends_with("s.txt"));
        assert_eq!(enc_ext("x.jpeg"), "jpg");
    }
}
