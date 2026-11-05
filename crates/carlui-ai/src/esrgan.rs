//! Bu modül "fotoğrafı bulanıklaştırmadan nasıl büyütürüz?" sorusunu çözüyor:
//! Real-ESRGAN (BSD-3, xinntao). Resmi `.pth` ağırlıklarından ağı kurarız:
//! - SRVGGNetCompact (realesr-general-x4v3): conv + PReLU zinciri, sonunda
//!   piksel karıştırma ve girdinin en yakın komşu büyütmesiyle toplama.
//! - RRDBNet (x4plus, x2plus, anime 6B): 23 (ya da 6) RRDB bloğu, iki kez
//!   en yakın komşu büyütme + conv. x2'de girdi önce 2×2 piksel kümelerine
//!   bölünür (SpaceToDepth); ilk conv'un kanal sırası buna göre değiştirilir.
//! Büyük resimler kenar payı bırakılarak parçalar (karo) halinde işlenir.

use crate::{
    onnx::{Dim, A, F32, G},
    runtime,
    weights::{Tensor, Weights},
    Image,
};
use ort::{session::Session, value::Tensor as OTensor};
use std::sync::atomic::{AtomicBool, Ordering};

fn w<'a>(ws: &'a Weights, k: &str) -> Result<&'a Tensor, String> {
    ws.get(k).ok_or_else(|| format!("ağırlık yok: {k}"))
}

fn conv(g: &mut G, ws: &Weights, x: &str, name: &str) -> Result<String, String> {
    let wt = w(ws, &format!("{name}.weight"))?;
    let b = w(ws, &format!("{name}.bias"))?;
    let k = wt.shape[2] as i64;
    let wn = g.f32(&wt.shape, &wt.data);
    let bn = g.f32(&b.shape, &b.data);
    let p = k / 2;
    Ok(g.op("Conv", &[x, &wn, &bn], &[("kernel_shape", A::Is(vec![k, k])), ("pads", A::Is(vec![p, p, p, p]))]))
}

fn lrelu(g: &mut G, x: &str) -> String {
    g.op("LeakyRelu", &[x], &[("alpha", A::F(0.2))])
}

fn up2(g: &mut G, x: &str) -> String {
    let s = g.f32(&[4], &[1.0, 1.0, 2.0, 2.0]);
    g.op("Resize", &[x, "", &s], &[("mode", A::S("nearest"))])
}

fn compact(g: &mut G, ws: &Weights, x: &str) -> Result<(String, u32), String> {
    let mut h = x.to_string();
    let mut i = 0;
    let mut last_c = 0;
    while let Some(t) = ws.get(&format!("body.{i}.weight")) {
        if t.shape.len() == 4 {
            h = conv(g, ws, &h, &format!("body.{i}"))?;
            last_c = t.shape[0];
        } else {
            let s = g.f32(&[t.len(), 1, 1], &t.data);
            h = g.op("PRelu", &[&h, &s], &[]);
        }
        i += 1;
    }
    let s = ((last_c / 3) as f32).sqrt().round() as u32;
    if s < 1 || s as usize * s as usize * 3 != last_c {
        return Err("tanınmayan Real-ESRGAN ağı".into());
    }
    let y = g.op("DepthToSpace", &[&h], &[("blocksize", A::I(s as i64)), ("mode", A::S("CRD"))]);
    let sc = g.f32(&[4], &[1.0, 1.0, s as f32, s as f32]);
    let base = g.op("Resize", &[x, "", &sc], &[("mode", A::S("nearest"))]);
    Ok((g.add(&y, &base), s))
}

fn rrdb(g: &mut G, ws: &Weights, x: &str) -> Result<(String, u32), String> {
    let first = w(ws, "conv_first.weight")?;
    let inc = first.shape[1];
    let (scale, r) = match inc {
        3 => (4, 1),
        12 => (2, 2),
        48 => (1, 4),
        _ => return Err("tanınmayan RRDB girişi".into()),
    };
    let mut feat_in = x.to_string();
    let mut ws_first = None;
    if r > 1 {
        feat_in = g.op("SpaceToDepth", &[x], &[("blocksize", A::I(r as i64))]);
        // torch pixel_unshuffle kanalı: c*r*r + i*r + j; SpaceToDepth: (i*r + j)*3 + c.
        let (o, k) = (first.shape[0], first.shape[2] * first.shape[3]);
        let mut d = vec![0f32; first.len()];
        for oc in 0..o {
            for c in 0..3 {
                for ij in 0..r * r {
                    let src = c * r * r + ij;
                    let dst = ij * 3 + c;
                    d[(oc * inc + dst) * k..(oc * inc + dst + 1) * k].copy_from_slice(&first.data[(oc * inc + src) * k..(oc * inc + src + 1) * k]);
                }
            }
        }
        ws_first = Some(Tensor { shape: first.shape.clone(), data: d });
    }
    let feat = {
        let wt = ws_first.as_ref().unwrap_or(first);
        let b = w(ws, "conv_first.bias")?;
        let (wn, bn) = (g.f32(&wt.shape, &wt.data), g.f32(&b.shape, &b.data));
        g.op("Conv", &[&feat_in, &wn, &bn], &[("kernel_shape", A::Is(vec![3, 3])), ("pads", A::Is(vec![1, 1, 1, 1]))])
    };
    let mut h = feat.clone();
    let mut b = 0;
    while ws.contains_key(&format!("body.{b}.rdb1.conv1.weight")) {
        let blk_in = h.clone();
        for d in 1..=3 {
            let rdb_in = h.clone();
            let mut cat = vec![rdb_in.clone()];
            let mut x5 = String::new();
            for c in 1..=5 {
                let refs: Vec<&str> = cat.iter().map(|s| s.as_str()).collect();
                let inp = if refs.len() == 1 { refs[0].to_string() } else { g.concat(&refs, 1) };
                let y = conv(g, ws, &inp, &format!("body.{b}.rdb{d}.conv{c}"))?;
                if c < 5 {
                    cat.push(lrelu(g, &y));
                } else {
                    x5 = y;
                }
            }
            let s = g.scale(&x5, 0.2);
            h = g.add(&s, &rdb_in);
        }
        let s = g.scale(&h, 0.2);
        h = g.add(&s, &blk_in);
        b += 1;
    }
    let body = conv(g, ws, &h, "conv_body")?;
    let mut f = g.add(&feat, &body);
    for n in ["conv_up1", "conv_up2"] {
        let u = up2(g, &f);
        let c = conv(g, ws, &u, n)?;
        f = lrelu(g, &c);
    }
    let hr = conv(g, ws, &f, "conv_hr")?;
    let hr = lrelu(g, &hr);
    Ok((conv(g, ws, &hr, "conv_last")?, scale))
}

/// Ağırlıklardan ONNX modeli ve büyütme katsayısı.
pub fn build(ws: &Weights) -> Result<(Vec<u8>, u32), String> {
    let mut g = G::new();
    let x = g.input("x", F32, &[Dim::N(1), Dim::N(3), Dim::P("h"), Dim::P("w")]);
    let (y, s) = if ws.contains_key("conv_first.weight") { rrdb(&mut g, ws, &x)? } else { compact(&mut g, ws, &x)? };
    g.output(&y, "y", F32, &[Dim::N(1), Dim::N(3), Dim::P("H"), Dim::P("W")]);
    Ok((g.finish(), s))
}

pub struct Upscaler {
    sess: Session,
    pub scale: u32,
    tile: usize,
}

impl Upscaler {
    pub fn open(pth: &std::path::Path) -> Result<Self, String> {
        let buf = std::fs::read(pth).map_err(|e| e.to_string())?;
        let ws = crate::weights::pth(&buf)?;
        let heavy = ws.contains_key("conv_first.weight");
        let (model, scale) = build(&ws)?;
        drop(ws);
        let sess = runtime::session(runtime::Src::Bytes(&model))?;
        Ok(Self { sess, scale, tile: if heavy { 256 } else { 512 } })
    }

    fn run(&mut self, chw: Vec<f32>, h: usize, w: usize) -> Result<Vec<f32>, String> {
        let t = OTensor::from_array(([1usize, 3, h, w], chw)).map_err(|e| e.to_string())?;
        let out = self.sess.run(ort::inputs!["x" => t]).map_err(|e| e.to_string())?;
        let (_, d) = out["y"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(d.to_vec())
    }

    /// BGRA resmi büyütür; saydamlık iki doğrusal büyütülür.
    pub fn upscale(&mut self, img: &Image, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<Image, String> {
        let (w, h, s) = (img.w as usize, img.h as usize, self.scale as usize);
        let (ow, oh) = (w * s, h * s);
        let mut out = vec![0u8; ow * oh * 4];
        let pad = 16usize;
        let tile = self.tile;
        let (tx, ty) = (w.div_ceil(tile), h.div_ceil(tile));
        let px = |x: usize, y: usize, c: usize| img.bgra[(y * w + x) * 4 + c] as f32 / 255.0;
        for j in 0..ty {
            for i in 0..tx {
                if cancel.load(Ordering::Relaxed) {
                    return Err(t!("Durduruldu", "Stopped").into());
                }
                let (x0, y0) = (i * tile, j * tile);
                let (x1, y1) = ((x0 + tile).min(w), (y0 + tile).min(h));
                let (ax, ay) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
                let (bx, by) = ((x1 + pad).min(w), (y1 + pad).min(h));
                // x2 ağı çift boyut ister: kenarı tekrarlayarak tamamla.
                let (tw, th) = ((bx - ax).next_multiple_of(2), (by - ay).next_multiple_of(2));
                let mut chw = vec![0f32; 3 * tw * th];
                for yy in 0..th {
                    for xx in 0..tw {
                        let (sx, sy) = ((ax + xx).min(w - 1), (ay + yy).min(h - 1));
                        // BGRA → RGB
                        for (c, bc) in [(0, 2), (1, 1), (2, 0)] {
                            chw[c * tw * th + yy * tw + xx] = px(sx, sy, bc);
                        }
                    }
                }
                let y = self.run(chw, th, tw)?;
                let (yw, yh) = (tw * s, th * s);
                for oy in y0 * s..y1 * s {
                    for ox in x0 * s..x1 * s {
                        let (lx, ly) = (ox - ax * s, oy - ay * s);
                        let o = (oy * ow + ox) * 4;
                        for (c, bc) in [(0, 2), (1, 1), (2, 0)] {
                            out[o + bc] = (y[c * yw * yh + ly * yw + lx].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                }
                progress((j * tx + i + 1) as f32 / (tx * ty) as f32);
            }
        }
        // Saydamlık: iki doğrusal.
        for oy in 0..oh {
            let fy = ((oy as f32 + 0.5) / s as f32 - 0.5).clamp(0.0, (h - 1) as f32);
            let (y0, ty_) = (fy as usize, fy.fract());
            let y1 = (y0 + 1).min(h - 1);
            for ox in 0..ow {
                let fx = ((ox as f32 + 0.5) / s as f32 - 0.5).clamp(0.0, (w - 1) as f32);
                let (x0, tx_) = (fx as usize, fx.fract());
                let x1 = (x0 + 1).min(w - 1);
                let a = |x: usize, y: usize| img.bgra[(y * w + x) * 4 + 3] as f32;
                let v = (a(x0, y0) * (1.0 - tx_) + a(x1, y0) * tx_) * (1.0 - ty_) + (a(x0, y1) * (1.0 - tx_) + a(x1, y1) * tx_) * ty_;
                out[(oy * ow + ox) * 4 + 3] = (v + 0.5) as u8;
            }
        }
        Ok(Image { w: ow as u32, h: oh as u32, bgra: out })
    }
}
