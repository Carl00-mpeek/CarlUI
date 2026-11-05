//! Bu modül "dönüştürücü menüsünden resmin arka planını nasıl sileriz ya da
//! nasıl büyütürüz?" sorusunu çözüyor. Ayarlarda seçili model (yoksa önerilen)
//! kullanılır; model ya da yapay zekâ motoru kurulu değilse önce sorulur,
//! onay verilirse resmi kaynaktan indirilir (ilerleme aynı kartta görünür).
//! Sonuç orijinalin yanına yazılır; orijinale dokunulmaz.

use crate::{file_name, image, kinds, progress::Shared};
use carlui_ai::{
    catalog::{self, Feature, Model},
    esrgan::Upscaler,
    rmbg::Remover,
    runtime, store, Image,
};
use std::{path::PathBuf, sync::atomic::Ordering};

/// Büyütülmüş resim en fazla bu kadar piksel olabilir (bellek sınırı).
const MAX_OUT_PIXELS: u64 = 120_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Background,
    Upscale,
}

impl Op {
    pub fn feature(self) -> Feature {
        match self {
            Op::Background => Feature::Background,
            Op::Upscale => Feature::Upscale,
        }
    }

    pub fn model(self) -> &'static Model {
        catalog::chosen(&carlui_core::config::load().ai, self.feature())
    }

    /// Menüde görünen ad.
    pub fn name(self) -> String {
        match self {
            Op::Background => t!("Arka planı sil", "Remove background").into(),
            Op::Upscale => t!(format!("Büyüt (×{})", scale(self.model())), format!("Upscale (×{})", scale(self.model()))),
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Op::Background => '\u{E7C5}',
            Op::Upscale => '\u{E740}',
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

/// Motoru ve modeli (gerekirse) indirir, sonra her dosyayı işler.
pub fn run_all(w: &Shared, op: Op, files: &[String], ff: Option<&PathBuf>) {
    let m = op.model();
    let cancel = w.cancel.clone();
    let fail = |e: String| {
        w.update(|s| {
            s.title = e;
            s.failed = true;
            s.done = true;
            s.frac = 1.0;
        })
    };
    if !runtime::installed() {
        w.update(|s| {
            s.title = t!("Yapay zekâ motoru indiriliyor", "Downloading the AI engine").into();
            s.file = format!("ONNX Runtime {} · nuget.org", runtime::ORT_VERSION);
        });
        if let Err(e) = runtime::install(&cancel, &|f| w.update(|s| s.frac = f)) {
            return fail(e);
        }
    }
    if !store::installed(m) {
        w.update(|s| {
            s.title = t!(format!("{} indiriliyor", m.name), format!("Downloading {}", m.name));
            s.file = format!("{} · {}", mb(m.size()), m.source.trim_start_matches("https://"));
            s.frac = 0.0;
        });
        if let Err(e) = store::install(m, &cancel, &|f| w.update(|s| s.frac = f)) {
            return fail(e);
        }
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
    }
    let eng = match op {
        Op::Background => store::find(&dir, "model.onnx").ok_or_else(|| t!("Model dosyası bulunamadı", "Model file not found").to_string()).and_then(|p| Remover::open(&p)).map(Eng::Bg),
        Op::Upscale => store::find(&dir, "model.pth").ok_or_else(|| t!("Model dosyası bulunamadı", "Model file not found").to_string()).and_then(|p| Upscaler::open(&p)).map(Eng::Up),
    };
    let mut eng = match eng {
        Ok(e) => e,
        Err(e) => {
            carlui_core::warn!("model açılamadı ({}): {e}", m.id);
            return fail(t!("Model açılamadı", "Couldn't load the model").into());
        }
    };

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
        };
        w.update(|s| {
            s.title = if n > 1 { format!("{verb} · {}/{n}", i + 1) } else { verb.into() };
            s.file = name.clone();
            s.frac = i as f32 / n as f32;
        });
        let prog = |f: f32| w.update(|s| s.frac = (i as f32 + f) / n as f32);
        let dst = output_path(op, src, m);
        let r = image::decode(src, ff).and_then(|p| {
            let img = Image { w: p.w, h: p.h, bgra: p.data };
            let out = match &mut eng {
                Eng::Bg(r) => r.remove(&img, &cancel, &prog)?,
                Eng::Up(u) => {
                    let s = u.scale as u64;
                    if img.w as u64 * img.h as u64 * s * s > MAX_OUT_PIXELS {
                        return Err(t!("Resim büyütmek için fazla büyük", "The image is too large to upscale").into());
                    }
                    u.upscale(&img, &cancel, &prog)?
                }
            };
            image::encode(&image::Pixels { w: out.w, h: out.h, data: out.bgra }, &dst, &enc_ext(&dst), ff)
        });
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
                _ if errors.is_empty() => t!(format!("{ok} resim hazır"), format!("{ok} images done")),
                _ => t!(format!("{ok} resim hazır · {} hata", errors.len()), format!("{ok} images done · {} errors", errors.len())),
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
        assert_eq!(enc_ext("x.jpeg"), "jpg");
    }
}
