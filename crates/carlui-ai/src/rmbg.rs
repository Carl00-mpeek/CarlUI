//! Bu modül "fotoğrafın arka planını nasıl sileriz?" sorusunu çözüyor:
//! BiRefNet (MIT, ZhengPeng7). Resmi sürümler hazır ONNX olarak gelir; girdi
//! boyutunu oturumdan okuruz (1024² ya da 2K). Resim o boyuta iki doğrusal
//! küçültülür, ImageNet ortalamasıyla normalleştirilir; çıkan maske (gerekirse
//! sigmoid'den geçirilip) özgün boyuta büyütülür ve saydamlığa çarpılır.

use crate::{runtime, Image};
use ort::{
    session::{RunOptions, Session},
    value::Tensor as OTensor,
};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Remover {
    sess: Session,
    input: String,
    output: String,
    path: std::path::PathBuf,
    on_cpu: bool,
    pub w: usize,
    pub h: usize,
}

/// Tek kanallı düzlemi iki doğrusal yeniden örnekler.
fn resample(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let mut out = vec![0f32; dw * dh];
    let (rx, ry) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
    for y in 0..dh {
        let fy = ((y as f32 + 0.5) * ry - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * rx - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            let a = src[y0 * sw + x0] * (1.0 - tx) + src[y0 * sw + x1] * tx;
            let b = src[y1 * sw + x0] * (1.0 - tx) + src[y1 * sw + x1] * tx;
            out[y * dw + x] = a * (1.0 - ty) + b * ty;
        }
    }
    out
}

/// Küçültürken kenar kırılmasın diye önce kutu ortalamasıyla yaklaşık boyuta indirir.
fn shrink(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let k = (sw / dw).min(sh / dh).max(1);
    if k == 1 {
        return resample(src, sw, sh, dw, dh);
    }
    let (bw, bh) = (sw / k, sh / k);
    let mut b = vec![0f32; bw * bh];
    let inv = 1.0 / (k * k) as f32;
    for y in 0..bh {
        for x in 0..bw {
            let mut s = 0.0;
            for yy in 0..k {
                let row = (y * k + yy) * sw + x * k;
                s += src[row..row + k].iter().sum::<f32>();
            }
            b[y * bw + x] = s * inv;
        }
    }
    resample(&b, bw, bh, dw, dh)
}

impl Remover {
    pub fn open(onnx: &std::path::Path) -> Result<Self, String> {
        let sess = runtime::session_opts(runtime::Src::File(onnx), false)?;
        let inp = sess.inputs().first().ok_or("girdi yok")?;
        let input = inp.name().to_string();
        let (mut h, mut w) = (1024usize, 1024usize);
        if let Some(s) = inp.dtype().tensor_shape() {
            if s.len() == 4 && s[2] > 0 && s[3] > 0 {
                (h, w) = (s[2] as usize, s[3] as usize);
            }
        }
        // Birden çok çıkış varsa sonuncusu en ince maskedir.
        let output = sess.outputs().last().ok_or("çıkış yok")?.name().to_string();
        Ok(Self { sess, input, output, w, h, path: onnx.to_path_buf(), on_cpu: !runtime::gpu_allowed() })
    }

    /// Modeli bir kez çalıştırır. Çalışma saniyeler sürer; yanındaki gözcü
    /// iş parçacığı `cancel` kalkınca ONNX Runtime'a "dur" der, böylece
    /// "Durdur" hesaplamanın bitmesini beklemez.
    fn infer(&mut self, chw: Vec<f32>, mw: usize, mh: usize, cancel: &AtomicBool) -> Result<(usize, usize, Vec<f32>), String> {
        let t = OTensor::from_array(([1usize, 3, mh, mw], chw)).map_err(|e| e.to_string())?;
        let opts = RunOptions::new().map_err(|e| e.to_string())?;
        let done = AtomicBool::new(false);
        let (sess, input, output) = (&mut self.sess, self.input.as_str(), self.output.as_str());
        std::thread::scope(|sc| {
            sc.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = opts.terminate();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
            });
            let r = (|| {
                let out = sess.run_with_options(ort::inputs![input => t], &opts).map_err(|e| e.to_string())?;
                let (shape, d) = out[output].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
                let (oh, ow) = (shape[shape.len() - 2] as usize, shape[shape.len() - 1] as usize);
                Ok((ow, oh, d[..ow * oh].to_vec()))
            })();
            done.store(true, Ordering::Relaxed);
            if cancel.load(Ordering::Relaxed) {
                return Err(t!("Durduruldu", "Stopped").into());
            }
            r
        })
    }

    /// Arka planı saydam yapar. `progress(0..1)`.
    pub fn remove(&mut self, img: &Image, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<Image, String> {
        let (iw, ih) = (img.w as usize, img.h as usize);
        let (mw, mh) = (self.w, self.h);
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        let mut chw = Vec::with_capacity(3 * mw * mh);
        for (c, bc) in [(0usize, 2usize), (1, 1), (2, 0)] {
            let plane: Vec<f32> = img.bgra.chunks_exact(4).map(|p| p[bc] as f32 / 255.0).collect();
            let r = shrink(&plane, iw, ih, mw, mh);
            chw.extend(r.into_iter().map(|v| (v - mean[c]) / std[c]));
        }
        progress(0.2);
        if cancel.load(Ordering::Relaxed) {
            return Err(t!("Durduruldu", "Stopped").into());
        }
        let (ow, oh, mut m) = match self.infer(chw.clone(), mw, mh, cancel) {
            Ok(r) => r,
            // Durdurulduysa işlemciye geçip yeniden denemek anlamsız.
            Err(e) if !self.on_cpu && !cancel.load(Ordering::Relaxed) => {
                carlui_core::warn!("BiRefNet ekran kartında çalışmadı, işlemciye geçiliyor: {e}");
                self.sess = runtime::session_cpu(runtime::Src::File(&self.path))?;
                self.on_cpu = true;
                self.infer(chw, mw, mh, cancel)?
            }
            Err(e) => return Err(e),
        };
        progress(0.85);
        if m.iter().any(|&v| !(-0.001..=1.001).contains(&v)) {
            for v in &mut m {
                *v = 1.0 / (1.0 + (-*v).exp());
            }
        }
        let m = resample(&m, ow, oh, iw, ih);
        let mut bgra = img.bgra.clone();
        for (p, a) in bgra.chunks_exact_mut(4).zip(m) {
            p[3] = (p[3] as f32 * a.clamp(0.0, 1.0) + 0.5) as u8;
        }
        progress(1.0);
        Ok(Image { w: img.w, h: img.h, bgra })
    }
}
