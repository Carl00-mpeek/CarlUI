//! Bu modül "ekrandaki yabancı metni internetsiz nasıl çeviririz?" sorusunu
//! çözüyor: Opus-MT (Helsinki-NLP, CC-BY-4.0) Marian dönüştürücüsü. Resmi
//! ağırlıklardan (`model.pth` ya da `model.safetensors`) iki ONNX grafiği
//! kurarız:
//! - Kodlayıcı: simgeler → gömme × √d + sinüs konum → katmanlar (son-norm,
//!   ReLU) → her kod çözücü katmanı için çapraz dikkat K/V.
//! - Kod çözücü: her adımda tek simge; öz dikkat K/V önbelleği giriş/çıkış.
//! Metin `source.spm` ile parçalanır, `vocab.json` ile numaralanır. Çözme
//! ışın aramasıyla (beam) yapılır; uzun metin cümlelere bölünüp sırayla çevrilir.

use crate::{
    nn::B,
    onnx::{Dim, A, F32, G, I64},
    runtime,
    spm::{self, Spm},
    weights::{self, Weights},
};
use ort::{
    session::{Session, SessionInputValue},
    value::{DynValue, Tensor as OT},
};
use std::{
    borrow::Cow,
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

/// Bir cümlede en fazla bu kadar kaynak simge (fazlası kesilir).
const MAX_SRC: usize = 200;
const BEAMS: usize = 4;

struct Cfg {
    d: usize,
    heads: usize,
    enc_layers: usize,
    dec_layers: usize,
    vocab: usize,
    max_pos: usize,
}

fn err(e: ort::Error) -> String {
    e.to_string()
}

/// Marian'ın sabit konum tablosu: ilk yarı sin, ikinci yarı cos.
fn positions(n: usize, d: usize) -> Vec<f32> {
    let mut t = vec![0f32; n * d];
    let h = d / 2;
    for p in 0..n {
        for i in 0..h {
            let a = p as f64 / 10000f64.powf(2.0 * i as f64 / d as f64);
            t[p * d + i] = a.sin() as f32;
            t[p * d + h + i] = a.cos() as f32;
        }
    }
    t
}

fn emb_name(ws: &Weights) -> Result<&'static str, String> {
    ["model.shared.weight", "model.encoder.embed_tokens.weight", "model.decoder.embed_tokens.weight"].into_iter().find(|k| ws.contains_key(*k)).ok_or_else(|| "ağırlık yok: model.shared.weight".to_string())
}

impl B<'_> {
    /// Son-norm artık bağlantı: LN(x + y).
    fn post(&mut self, x: &str, y: &str, name: &str) -> Result<String, String> {
        let s = self.g.add(x, y);
        self.ln(&s, name)
    }
    fn ffn(&mut self, x: &str, p: &str) -> Result<String, String> {
        let h = self.lin(x, &format!("{p}.fc1"))?;
        let h = self.g.op("Relu", &[&h], &[]);
        let h = self.lin(&h, &format!("{p}.fc2"))?;
        self.post(x, &h, &format!("{p}.final_layer_norm"))
    }
}

fn encoder(c: &Cfg, ws: &Weights) -> Result<Vec<u8>, String> {
    let mut b = B { g: G::new(), ws, half: true };
    let (d, h) = (c.d, c.heads);
    let dh = d / h;
    let ids = b.g.input("ids", I64, &[Dim::N(1), Dim::P("src")]);
    let pos = b.g.input("pos", I64, &[Dim::P("src")]);
    let e = b.param(emb_name(ws)?)?;
    let pt = b.konst(&[c.max_pos, d], &positions(c.max_pos, d));
    let x0 = b.g.op("Gather", &[&e, &ids], &[]);
    let x0 = b.g.scale(&x0, (d as f32).sqrt());
    let p0 = b.g.op("Gather", &[&pt, &pos], &[]);
    let mut x = b.g.add(&x0, &p0);
    let sc = (dh as f32).powf(-0.5);
    for l in 0..c.enc_layers {
        let p = format!("model.encoder.layers.{l}");
        let q = b.lin(&x, &format!("{p}.self_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let kk = b.lin(&x, &format!("{p}.self_attn.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        let v = b.lin(&x, &format!("{p}.self_attn.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        let s = b.g.matmul(&q, &kk);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &v);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.self_attn.out_proj"))?;
        x = b.post(&x, &o, &format!("{p}.self_attn_layer_norm"))?;
        x = b.ffn(&x, &p)?;
    }
    for l in 0..c.dec_layers {
        let p = format!("model.decoder.layers.{l}.encoder_attn");
        let kk = b.lin(&x, &format!("{p}.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        b.g.output(&kk, &format!("ck{l}"), F32, &[Dim::N(1), Dim::N(h as i64), Dim::N(dh as i64), Dim::P("src")]);
        let v = b.lin(&x, &format!("{p}.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        b.g.output(&v, &format!("cv{l}"), F32, &[Dim::N(1), Dim::N(h as i64), Dim::P("src"), Dim::N(dh as i64)]);
    }
    Ok(b.g.finish())
}

fn decoder(c: &Cfg, ws: &Weights) -> Result<Vec<u8>, String> {
    let mut b = B { g: G::new(), ws, half: true };
    let (d, h) = (c.d, c.heads);
    let dh = d / h;
    let (hi, dhi) = (h as i64, dh as i64);
    let tok = b.g.input("tok", I64, &[Dim::N(1), Dim::N(1)]);
    let pos = b.g.input("pos", I64, &[Dim::N(1)]);
    let mask = b.g.input("mask", F32, &[Dim::N(1), Dim::N(1), Dim::N(1), Dim::P("past1")]);
    let e = b.param(emb_name(ws)?)?;
    let pt = b.konst(&[c.max_pos, d], &positions(c.max_pos, d));
    let x0 = b.g.op("Gather", &[&e, &tok], &[]);
    let x0 = b.g.scale(&x0, (d as f32).sqrt());
    let p0 = b.g.op("Gather", &[&pt, &pos], &[]);
    let mut x = b.g.add(&x0, &p0);
    let sc = (dh as f32).powf(-0.5);
    for l in 0..c.dec_layers {
        let p = format!("model.decoder.layers.{l}");
        let pk = b.g.input(&format!("pk{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::P("past")]);
        let pv = b.g.input(&format!("pv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::P("past"), Dim::N(dhi)]);
        let ck = b.g.input(&format!("ck{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::P("src")]);
        let cv = b.g.input(&format!("cv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::P("src"), Dim::N(dhi)]);
        // Öz dikkat (önbellekli).
        let q = b.lin(&x, &format!("{p}.self_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let kk = b.lin(&x, &format!("{p}.self_attn.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        let kk = b.g.concat(&[&pk, &kk], 3);
        b.g.output(&kk, &format!("nk{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::P("past1")]);
        let v = b.lin(&x, &format!("{p}.self_attn.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        let v = b.g.concat(&[&pv, &v], 2);
        b.g.output(&v, &format!("nv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::P("past1"), Dim::N(dhi)]);
        let s = b.g.matmul(&q, &kk);
        let s = b.g.add(&s, &mask);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &v);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.self_attn.out_proj"))?;
        x = b.post(&x, &o, &format!("{p}.self_attn_layer_norm"))?;
        // Çapraz dikkat.
        let q = b.lin(&x, &format!("{p}.encoder_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let s = b.g.matmul(&q, &ck);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &cv);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.encoder_attn.out_proj"))?;
        x = b.post(&x, &o, &format!("{p}.encoder_attn_layer_norm"))?;
        x = b.ffn(&x, &p)?;
    }
    let x = b.g.reshape(&x, &[1, d as i64]);
    let logits = match ws.get("final_logits_bias") {
        Some(t) => {
            let bias = b.g.f32(&[1, t.len()], &t.data);
            b.g.op("Gemm", &[&x, &e, &bias], &[("transB", A::I(1))])
        }
        None => b.g.op("Gemm", &[&x, &e], &[("transB", A::I(1))]),
    };
    b.g.output(&logits, "logits", F32, &[Dim::N(1), Dim::N(c.vocab as i64)]);
    Ok(b.g.finish())
}

/// Bir ışın: üretilen simgeler, toplam log-olasılık ve öz dikkat önbelleği.
struct Beam {
    toks: Vec<usize>,
    score: f32,
    past: Rc<Vec<DynValue>>,
}

pub struct Marian {
    enc: Session,
    dec: Session,
    cfg: Cfg,
    spm: Spm,
    ids: HashMap<String, usize>,
    pieces: Vec<String>,
    eos: usize,
    pad: usize,
    unk: usize,
}

fn log_softmax(x: &mut [f32]) {
    let m = x.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let l = m + x.iter().map(|v| (v - m).exp()).sum::<f32>().ln();
    x.iter_mut().for_each(|v| *v -= l);
}

impl Marian {
    /// `dir`: model.pth ya da model.safetensors, source.spm, vocab.json, config.json.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let rd = |n: &str| std::fs::read_to_string(dir.join(n)).map_err(|e| format!("{n}: {e}"));
        let cj: serde_json::Value = serde_json::from_str(&rd("config.json")?).map_err(|e| e.to_string())?;
        let u = |k: &str| cj[k].as_u64().map(|v| v as usize).ok_or_else(|| format!("config.json: {k} yok"));
        let cfg = Cfg { d: u("d_model")?, heads: u("encoder_attention_heads")?, enc_layers: u("encoder_layers")?, dec_layers: u("decoder_layers")?, vocab: u("vocab_size")?, max_pos: u("max_position_embeddings").unwrap_or(512) };
        if cj["activation_function"].as_str().unwrap_or("relu") != "relu" || cj["normalize_before"].as_bool().unwrap_or(false) {
            return Err("desteklenmeyen Marian yapısı".into());
        }
        let (eos, pad) = (u("eos_token_id")?, u("pad_token_id")?);
        let vj: serde_json::Value = serde_json::from_str(&rd("vocab.json")?).map_err(|e| e.to_string())?;
        let mut pieces = vec![String::new(); cfg.vocab];
        let mut ids = HashMap::new();
        for (k, v) in vj.as_object().ok_or("vocab.json bozuk")? {
            let Some(i) = v.as_u64().map(|i| i as usize) else { continue };
            if i < cfg.vocab {
                pieces[i] = k.clone();
                ids.insert(k.clone(), i);
            }
        }
        let unk = *ids.get("<unk>").unwrap_or(&1);
        let spm = Spm::load(&std::fs::read(dir.join("source.spm")).map_err(|e| format!("source.spm: {e}"))?)?;
        let ws = if dir.join("model.safetensors").is_file() {
            weights::safetensors(&std::fs::read(dir.join("model.safetensors")).map_err(|e| e.to_string())?)?
        } else {
            weights::pth(&std::fs::read(dir.join("model.pth")).map_err(|e| e.to_string())?)?
        };
        let e = encoder(&cfg, &ws)?;
        let dg = decoder(&cfg, &ws)?;
        drop(ws);
        let enc = runtime::session(runtime::Src::Bytes(&e))?;
        drop(e);
        let dec = runtime::session(runtime::Src::Bytes(&dg))?;
        Ok(Self { enc, dec, cfg, spm, ids, pieces, eos, pad, unk })
    }

    /// Metni çevirir; satır yapısını kabaca korur. `progress(0..1)`.
    pub fn translate(&mut self, text: &str, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<String, String> {
        let paras = paragraphs(text);
        let total: usize = paras.iter().map(|p| p.len()).sum::<usize>().max(1);
        let mut done = 0usize;
        let mut out = Vec::new();
        for p in paras {
            let mut line = Vec::new();
            for s in &p {
                if cancel.load(Ordering::Relaxed) {
                    return Err(t!("Durduruldu", "Stopped").into());
                }
                line.push(self.sentence(s, cancel)?);
                done += 1;
                progress(done as f32 / total as f32);
            }
            out.push(line.join(" "));
        }
        Ok(out.join("\n"))
    }

    fn sentence(&mut self, s: &str, cancel: &AtomicBool) -> Result<String, String> {
        let mut src: Vec<i64> = self.spm.encode(s).iter().map(|p| *self.ids.get(p).unwrap_or(&self.unk) as i64).collect();
        if src.is_empty() {
            return Ok(String::new());
        }
        src.truncate(MAX_SRC);
        src.push(self.eos as i64);
        let n = src.len();
        let ids = OT::from_array(([1usize, n], src)).map_err(err)?;
        let pos = OT::from_array(([n], (0..n as i64).collect::<Vec<_>>())).map_err(err)?;
        let mut out = self.enc.run(ort::inputs!["ids" => ids, "pos" => pos]).map_err(err)?;
        let mut cross = Vec::new();
        for l in 0..self.cfg.dec_layers {
            cross.push(out.remove(format!("ck{l}")).ok_or("ck yok")?);
            cross.push(out.remove(format!("cv{l}")).ok_or("cv yok")?);
        }
        drop(out);
        let toks = self.beam(&cross, (n * 2 + 10).min(self.cfg.max_pos - 1), cancel)?;
        let ps: Vec<&str> = toks.iter().filter(|&&t| t != self.eos && t != self.pad && t != self.unk).map(|&t| self.pieces[t].as_str()).collect();
        Ok(spm::join(&ps))
    }

    /// Bir adım: verilen simge ve önbellekle sonraki simgenin log-olasılıkları.
    fn step(&mut self, cross: &[DynValue], past: &[DynValue], tok: usize, pos: usize) -> Result<(Vec<f32>, Vec<DynValue>), String> {
        let (h, dh, nl) = (self.cfg.heads, self.cfg.d / self.cfg.heads, self.cfg.dec_layers);
        // Önbelleğin ilk sütunu boş (sıfır uzunluk yerine); maskeyle gizlenir.
        let mut mask = vec![0f32; pos + 2];
        mask[0] = -1e9;
        let mut ins: Vec<(Cow<str>, SessionInputValue)> = vec![
            ("tok".into(), OT::from_array(([1usize, 1], vec![tok as i64])).map_err(err)?.into_dyn().into()),
            ("pos".into(), OT::from_array(([1usize], vec![pos as i64])).map_err(err)?.into_dyn().into()),
            ("mask".into(), OT::from_array(([1usize, 1, 1, pos + 2], mask)).map_err(err)?.into_dyn().into()),
        ];
        let empty;
        let past: &[DynValue] = if past.is_empty() {
            let mut v = Vec::new();
            for _ in 0..nl {
                v.push(OT::from_array(([1usize, h, dh, 1], vec![0f32; h * dh])).map_err(err)?.into_dyn());
                v.push(OT::from_array(([1usize, h, 1, dh], vec![0f32; h * dh])).map_err(err)?.into_dyn());
            }
            empty = v;
            &empty
        } else {
            past
        };
        for l in 0..nl {
            ins.push((format!("pk{l}").into(), (&past[2 * l]).into()));
            ins.push((format!("pv{l}").into(), (&past[2 * l + 1]).into()));
            ins.push((format!("ck{l}").into(), (&cross[2 * l]).into()));
            ins.push((format!("cv{l}").into(), (&cross[2 * l + 1]).into()));
        }
        let mut out = self.dec.run(ins).map_err(err)?;
        let mut logits = out["logits"].try_extract_tensor::<f32>().map_err(err)?.1.to_vec();
        let mut np = Vec::new();
        for l in 0..nl {
            np.push(out.remove(format!("nk{l}")).ok_or("nk yok")?);
            np.push(out.remove(format!("nv{l}")).ok_or("nv yok")?);
        }
        logits[self.pad] = f32::NEG_INFINITY;
        log_softmax(&mut logits);
        Ok((logits, np))
    }

    /// Işın araması (HF'nin varsayılanı gibi 4 ışın, uzunluk cezası 1).
    fn beam(&mut self, cross: &[DynValue], max: usize, cancel: &AtomicBool) -> Result<Vec<usize>, String> {
        let mut beams = vec![Beam { toks: Vec::new(), score: 0.0, past: Rc::new(Vec::new()) }];
        let mut done: Vec<(Vec<usize>, f32)> = Vec::new();
        for pos in 0..max {
            if cancel.load(Ordering::Relaxed) {
                return Err(t!("Durduruldu", "Stopped").into());
            }
            let mut cands: Vec<(usize, usize, f32)> = Vec::new();
            let mut pasts = Vec::new();
            for (bi, b) in beams.iter().enumerate() {
                let last = b.toks.last().copied().unwrap_or(self.pad);
                let (lp, np) = self.step(cross, &b.past, last, pos)?;
                pasts.push(Rc::new(np));
                let mut idx: Vec<usize> = (0..lp.len()).collect();
                idx.select_nth_unstable_by(2 * BEAMS, |&a, &c| lp[c].total_cmp(&lp[a]));
                for &t in &idx[..2 * BEAMS] {
                    cands.push((bi, t, b.score + lp[t]));
                }
            }
            cands.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut next = Vec::new();
            for (bi, t, sc) in cands {
                if t == self.eos {
                    let mut toks = beams[bi].toks.clone();
                    toks.push(t);
                    let len = toks.len() as f32;
                    done.push((toks, sc / len));
                } else if next.len() < BEAMS {
                    let mut toks = beams[bi].toks.clone();
                    toks.push(t);
                    next.push(Beam { toks, score: sc, past: pasts[bi].clone() });
                }
                if next.len() >= BEAMS && done.len() >= BEAMS {
                    break;
                }
            }
            // Yeterince bitmiş ışın var ve kalanların en iyisi onları geçemiyorsa dur.
            let best_done = done.iter().map(|d| d.1).fold(f32::NEG_INFINITY, f32::max);
            let best_live = next.iter().map(|b| b.score / (b.toks.len() as f32 + 1.0)).fold(f32::NEG_INFINITY, f32::max);
            beams = next;
            if beams.is_empty() || done.len() >= BEAMS && best_done >= best_live {
                break;
            }
        }
        if done.is_empty() {
            done.extend(beams.into_iter().map(|b| {
                let s = b.score / b.toks.len().max(1) as f32;
                (b.toks, s)
            }));
        }
        Ok(done.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|d| d.0).unwrap_or_default())
    }
}

/// OCR satırlarını paragraflara, paragrafları cümlelere böler. Cümle ortasında
/// kırılmış satırlar birleşir; kısa, büyük harfle başlayan satırlar (menü,
/// başlık) ayrı kalır.
pub fn paragraphs(text: &str) -> Vec<Vec<String>> {
    let mut paras: Vec<String> = Vec::new();
    let mut open = false;
    let mut prev = 0usize;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            open = false;
            continue;
        }
        let starts_low = line.chars().next().map(|c| c.is_lowercase() || c.is_ascii_digit()).unwrap_or(false);
        match paras.last_mut() {
            Some(p) if open && (starts_low || !p.ends_with(['.', '!', '?', ':']) && prev > 50) => {
                if p.ends_with('-') && starts_low {
                    p.pop();
                } else {
                    p.push(' ');
                }
                p.push_str(line);
            }
            _ => paras.push(line.to_string()),
        }
        open = true;
        prev = line.chars().count();
    }
    paras.into_iter().map(|p| sentences(&p)).collect()
}

fn sentences(p: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let cs: Vec<char> = p.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        cur.push(c);
        let end = matches!(c, '.' | '!' | '?') && cs.get(i + 1).map(|n| *n == ' ').unwrap_or(false) && cs.get(i + 2).map(|n| n.is_uppercase() || *n == '"').unwrap_or(false);
        if end && cur.trim().chars().count() > 3 {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        let t = "This is a long sentence that was broken by the OCR engine because\nthe line ended here. Next one starts. Mr. x\nFile\nEdit\n\nNew paragraph.";
        let p = paragraphs(t);
        assert_eq!(p.len(), 4, "{p:?}");
        assert_eq!(p[0], vec!["This is a long sentence that was broken by the OCR engine because the line ended here.", "Next one starts.", "Mr. x"]);
        assert_eq!(p[1], vec!["File"]);
        assert_eq!(p[2], vec!["Edit"]);
    }

    #[test]
    fn sinusoid() {
        let t = positions(3, 4);
        assert_eq!(&t[0..4], &[0.0, 0.0, 1.0, 1.0]);
        assert!((t[4] - 1f32.sin()).abs() < 1e-6 && (t[6] - 1f32.cos()).abs() < 1e-6);
    }
}
