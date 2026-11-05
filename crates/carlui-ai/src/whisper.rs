//! Bu modül "konuşmayı bilgisayarda, internetsiz nasıl yazıya çeviririz?"
//! sorusunu çözüyor: Whisper (MIT, OpenAI). Resmi `model.safetensors`
//! ağırlıklarından iki ONNX grafiği kurarız:
//! - Kodlayıcı: 30 saniyelik ses penceresi → log-mel (STFT'yi Conv1d ile,
//!   mel süzgeçlerini MatMul ile grafiğin içinde yaparız) → konvolüsyonlar →
//!   dönüştürücü katmanlar → her kod çözücü katmanı için çapraz dikkat K/V.
//! - Kod çözücü: her adımda tek simge; öz dikkat K/V önbelleği giriş/çıkış.
//! Çözme açgözlü (greedy); OpenAI'nin zaman damgası kuralları ve pencere
//! kaydırma mantığı aynen uygulanır, böylece .srt için cümle zamanları çıkar.

use crate::{
    nn::B,
    onnx::{Dim, A, F32, G, I64},
    runtime,
    weights::{self, Weights},
};
use ort::{
    session::{Session, SessionInputValue},
    value::{DynValue, Tensor as OT},
};
use std::{
    borrow::Cow,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// Örnekleme hızı (Hz). Ses girişi 16 kHz tek kanal olmalı.
pub const SR: usize = 16_000;
const N_FFT: usize = 400;
const HOP: usize = 160;
const CHUNK: usize = 30 * SR;
const FRAMES: usize = CHUNK / HOP;
const SAMPLE_LEN: usize = 224;

/// Whisper dil kodları (simge sırasıyla).
pub const LANGS: [&str; 100] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha", "ba", "jw", "su", "yue",
];

#[derive(Clone, Debug)]
pub struct Segment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

struct Cfg {
    d: usize,
    heads: usize,
    enc_layers: usize,
    dec_layers: usize,
    mels: usize,
    vocab: usize,
}

/// Özel simge numaraları (çok dilli modeller).
struct Special {
    eot: usize,
    sot: usize,
    lang0: usize,
    nlang: usize,
    transcribe: usize,
    nospeech: usize,
    notimestamps: usize,
    ts_begin: usize,
}

impl Special {
    fn new(vocab: usize) -> Self {
        let nlang = if vocab >= 51866 { 100 } else { 99 };
        let lang0 = 50259;
        let tr = lang0 + nlang; // translate
        Self { eot: 50257, sot: 50258, lang0, nlang, transcribe: tr + 1, nospeech: tr + 4, notimestamps: tr + 5, ts_begin: tr + 6 }
    }
}

/// librosa'nın "slaney" mel süzgeçleri [mels, 201].
fn mel_filters(mels: usize) -> Vec<f32> {
    let (f_sp, min_log_hz, min_log_mel, logstep) = (200.0 / 3.0, 1000.0f64, 15.0f64, (6.4f64).ln() / 27.0);
    let hz2mel = |f: f64| if f >= min_log_hz { min_log_mel + (f / min_log_hz).ln() / logstep } else { f / f_sp };
    let mel2hz = |m: f64| if m >= min_log_mel { min_log_hz * (logstep * (m - min_log_mel)).exp() } else { m * f_sp };
    let nf = N_FFT / 2 + 1;
    let fft: Vec<f64> = (0..nf).map(|i| i as f64 * (SR as f64 / 2.0) / (nf - 1) as f64).collect();
    let (lo, hi) = (hz2mel(0.0), hz2mel(SR as f64 / 2.0));
    let pts: Vec<f64> = (0..mels + 2).map(|i| mel2hz(lo + (hi - lo) * i as f64 / (mels + 1) as f64)).collect();
    let mut w = vec![0f32; mels * nf];
    for m in 0..mels {
        let enorm = 2.0 / (pts[m + 2] - pts[m]);
        for (j, &f) in fft.iter().enumerate() {
            let lower = (f - pts[m]) / (pts[m + 1] - pts[m]);
            let upper = (pts[m + 2] - f) / (pts[m + 2] - pts[m + 1]);
            w[m * nf + j] = (lower.min(upper).max(0.0) * enorm) as f32;
        }
    }
    w
}

fn encoder(c: &Cfg, ws: &Weights, half: bool) -> Result<Vec<u8>, String> {
    let mut b = B { g: G::new(), ws, half };
    let (d, h) = (c.d, c.heads);
    let dh = d / h;
    let a = b.g.input("audio", F32, &[Dim::N(1), Dim::N(1), Dim::N((CHUNK + N_FFT) as i64)]);
    // STFT: periyodik Hann × cos / −sin, adım 160.
    let nf = N_FFT / 2 + 1;
    let mut k = vec![0f32; 2 * nf * N_FFT];
    for f in 0..nf {
        for n in 0..N_FFT {
            let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos();
            let ph = 2.0 * std::f64::consts::PI * (f * n) as f64 / N_FFT as f64;
            k[f * N_FFT + n] = (hann * ph.cos()) as f32;
            k[(nf + f) * N_FFT + n] = (-hann * ph.sin()) as f32;
        }
    }
    let kn = b.g.f32(&[2 * nf, 1, N_FFT], &k);
    let s = b.g.op("Conv", &[&a, &kn], &[("kernel_shape", A::Is(vec![N_FFT as i64])), ("strides", A::Is(vec![HOP as i64]))]);
    let re = b.slice(&s, 0, nf as i64, 1);
    let im = b.slice(&s, nf as i64, 2 * nf as i64, 1);
    let (r2, i2) = (b.g.mul(&re, &re), b.g.mul(&im, &im));
    let p = b.g.add(&r2, &i2);
    let p = b.slice(&p, 0, FRAMES as i64, 2);
    let fl = b.g.f32(&[c.mels, nf], &mel_filters(c.mels));
    let mel = b.g.matmul(&fl, &p);
    let eps = b.g.scalar(1e-10);
    let x = b.g.op("Max", &[&mel, &eps], &[]);
    let x = b.g.op("Log", &[&x], &[]);
    let x = b.g.scale(&x, 1.0 / std::f32::consts::LN_10);
    let m = b.g.op("ReduceMax", &[&x], &[("keepdims", A::I(1))]);
    let m8 = b.g.scalar(-8.0);
    let m = b.g.add(&m, &m8);
    let x = b.g.op("Max", &[&x, &m], &[]);
    let four = b.g.scalar(4.0);
    let x = b.g.add(&x, &four);
    let x = b.g.scale(&x, 0.25);
    // Konvolüsyonlar.
    let mut x = x;
    for (n, stride) in [("conv1", 1i64), ("conv2", 2)] {
        let w = b.param(&format!("model.encoder.{n}.weight"))?;
        let bi = b.param(&format!("model.encoder.{n}.bias"))?;
        let y = b.g.op("Conv", &[&x, &w, &bi], &[("kernel_shape", A::Is(vec![3])), ("pads", A::Is(vec![1, 1])), ("strides", A::Is(vec![stride]))]);
        x = b.gelu(&y);
    }
    let x = b.g.transpose(&x, &[0, 2, 1]);
    let pos = b.param("model.encoder.embed_positions.weight")?;
    let mut x = b.g.add(&x, &pos);
    let sc = (dh as f32).powf(-0.5);
    for l in 0..c.enc_layers {
        let p = format!("model.encoder.layers.{l}");
        let hn = b.ln(&x, &format!("{p}.self_attn_layer_norm"))?;
        let q = b.lin(&hn, &format!("{p}.self_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let kk = b.lin(&hn, &format!("{p}.self_attn.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        let v = b.lin(&hn, &format!("{p}.self_attn.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        let s = b.g.matmul(&q, &kk);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &v);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.self_attn.out_proj"))?;
        x = b.g.add(&x, &o);
        x = b.mlp(&x, &p)?;
    }
    let xa = b.ln(&x, "model.encoder.layer_norm")?;
    for l in 0..c.dec_layers {
        let p = format!("model.decoder.layers.{l}.encoder_attn");
        let kk = b.lin(&xa, &format!("{p}.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        b.g.output(&kk, &format!("ck{l}"), F32, &[Dim::N(1), Dim::N(h as i64), Dim::N(dh as i64), Dim::N(1500)]);
        let v = b.lin(&xa, &format!("{p}.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        b.g.output(&v, &format!("cv{l}"), F32, &[Dim::N(1), Dim::N(h as i64), Dim::N(1500), Dim::N(dh as i64)]);
    }
    Ok(b.g.finish())
}

fn decoder(c: &Cfg, ws: &Weights, half: bool) -> Result<Vec<u8>, String> {
    let mut b = B { g: G::new(), ws, half };
    let (d, h) = (c.d, c.heads);
    let dh = d / h;
    let (hi, dhi) = (h as i64, dh as i64);
    let tok = b.g.input("tok", I64, &[Dim::N(1), Dim::N(1)]);
    let pos = b.g.input("pos", I64, &[Dim::N(1)]);
    let mask = b.g.input("mask", F32, &[Dim::N(1), Dim::N(1), Dim::N(1), Dim::P("past1")]);
    let e = b.param("model.decoder.embed_tokens.weight")?;
    let pe = b.param("model.decoder.embed_positions.weight")?;
    let x0 = b.g.op("Gather", &[&e, &tok], &[]);
    let x1 = b.g.op("Gather", &[&pe, &pos], &[]);
    let mut x = b.g.add(&x0, &x1);
    let sc = (dh as f32).powf(-0.5);
    for l in 0..c.dec_layers {
        let p = format!("model.decoder.layers.{l}");
        let pk = b.g.input(&format!("pk{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::P("past")]);
        let pv = b.g.input(&format!("pv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::P("past"), Dim::N(dhi)]);
        let ck = b.g.input(&format!("ck{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::N(1500)]);
        let cv = b.g.input(&format!("cv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(1500), Dim::N(dhi)]);
        // Öz dikkat (önbellekli).
        let hn = b.ln(&x, &format!("{p}.self_attn_layer_norm"))?;
        let q = b.lin(&hn, &format!("{p}.self_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let kk = b.lin(&hn, &format!("{p}.self_attn.k_proj"))?;
        let kk = b.heads(&kk, h, dh, &[0, 2, 3, 1]);
        let kk = b.g.concat(&[&pk, &kk], 3);
        b.g.output(&kk, &format!("nk{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::N(dhi), Dim::P("past1")]);
        let v = b.lin(&hn, &format!("{p}.self_attn.v_proj"))?;
        let v = b.heads(&v, h, dh, &[0, 2, 1, 3]);
        let v = b.g.concat(&[&pv, &v], 2);
        b.g.output(&v, &format!("nv{l}"), F32, &[Dim::N(1), Dim::N(hi), Dim::P("past1"), Dim::N(dhi)]);
        let s = b.g.matmul(&q, &kk);
        let s = b.g.add(&s, &mask);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &v);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.self_attn.out_proj"))?;
        x = b.g.add(&x, &o);
        // Çapraz dikkat.
        let hn = b.ln(&x, &format!("{p}.encoder_attn_layer_norm"))?;
        let q = b.lin(&hn, &format!("{p}.encoder_attn.q_proj"))?;
        let q = b.g.scale(&q, sc);
        let q = b.heads(&q, h, dh, &[0, 2, 1, 3]);
        let s = b.g.matmul(&q, &ck);
        let s = b.g.op("Softmax", &[&s], &[("axis", A::I(-1))]);
        let o = b.g.matmul(&s, &cv);
        let o = b.merge(&o, d);
        let o = b.lin(&o, &format!("{p}.encoder_attn.out_proj"))?;
        x = b.g.add(&x, &o);
        x = b.mlp(&x, &p)?;
    }
    let x = b.ln(&x, "model.decoder.layer_norm")?;
    let x = b.g.reshape(&x, &[1, d as i64]);
    let logits = b.g.op("Gemm", &[&x, &e], &[("transB", A::I(1))]);
    b.g.output(&logits, "logits", F32, &[Dim::N(1), Dim::N(c.vocab as i64)]);
    Ok(b.g.finish())
}

// ------------------------------------------------------------------ simgeler

/// GPT-2 bayt ↔ unicode eşlemesinin tersi.
fn byte_decoder() -> std::collections::HashMap<char, u8> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32).chain(0xA1..=0xAC).chain(0xAE..=0xFF).collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    bs.into_iter().zip(cs).map(|(b, c)| (char::from_u32(c).unwrap(), b as u8)).collect()
}

fn load_vocab(p: &Path, n: usize) -> Result<Vec<Vec<u8>>, String> {
    let s = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    let bd = byte_decoder();
    let mut out = vec![Vec::new(); n];
    for (k, id) in v.as_object().ok_or("vocab.json bozuk")? {
        let Some(id) = id.as_u64() else { continue };
        if (id as usize) < n {
            out[id as usize] = k.chars().filter_map(|c| bd.get(&c).copied()).collect();
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ çalıştırma

pub struct Whisper {
    enc: Session,
    dec: Session,
    cfg: Cfg,
    sp: Special,
    vocab: Vec<Vec<u8>>,
    suppress: Vec<usize>,
    begin_suppress: Vec<usize>,
}

struct Decoded {
    tokens: Vec<usize>,
    avg_logprob: f32,
    no_speech: f32,
    lang: usize,
}

fn err(e: ort::Error) -> String {
    e.to_string()
}

fn log_softmax(x: &[f32]) -> Vec<f32> {
    let m = x.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let s: f32 = x.iter().map(|v| (v - m).exp()).sum();
    let l = m + s.ln();
    x.iter().map(|v| v - l).collect()
}

impl Whisper {
    /// `dir`: model.safetensors, config.json, vocab.json.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let cj: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let u = |k: &str| cj[k].as_u64().map(|v| v as usize).ok_or_else(|| format!("config.json: {k} yok"));
        let cfg = Cfg { d: u("d_model")?, heads: u("encoder_attention_heads")?, enc_layers: u("encoder_layers")?, dec_layers: u("decoder_layers")?, mels: u("num_mel_bins")?, vocab: u("vocab_size")? };
        let ids = |k: &str| -> Vec<usize> { cj[k].as_array().map(|a| a.iter().filter_map(|v| v.as_u64().map(|x| x as usize)).collect()).unwrap_or_default() };
        let (suppress, begin_suppress) = (ids("suppress_tokens"), ids("begin_suppress_tokens"));
        let buf = std::fs::read(dir.join("model.safetensors")).map_err(|e| e.to_string())?;
        let half = buf.len() > 1_000_000_000;
        let ws = weights::safetensors(&buf)?;
        drop(buf);
        let e = encoder(&cfg, &ws, half)?;
        let dg = decoder(&cfg, &ws, half)?;
        drop(ws);
        let enc = runtime::session(runtime::Src::Bytes(&e))?;
        drop(e);
        let dec = runtime::session(runtime::Src::Bytes(&dg))?;
        let vocab = load_vocab(&dir.join("vocab.json"), cfg.vocab)?;
        let sp = Special::new(cfg.vocab);
        Ok(Self { enc, dec, cfg, sp, vocab, suppress, begin_suppress })
    }

    pub fn lang_index(code: &str) -> Option<usize> {
        LANGS.iter().position(|l| *l == code)
    }

    fn encode(&mut self, pcm: &[f32], start: usize) -> Result<Vec<DynValue>, String> {
        // Pencere + iki yanda 200 örnek bağlam (yoksa yansıtma / sıfır).
        let half = N_FFT as isize / 2;
        let mut a = vec![0f32; CHUNK + N_FFT];
        for (i, v) in a.iter_mut().enumerate() {
            let j = (start as isize + i as isize - half).unsigned_abs();
            *v = pcm.get(j).copied().unwrap_or(0.0);
        }
        let t = OT::from_array(([1usize, 1, CHUNK + N_FFT], a)).map_err(err)?;
        let mut out = self.enc.run(ort::inputs!["audio" => t]).map_err(err)?;
        let mut v = Vec::new();
        for l in 0..self.cfg.dec_layers {
            v.push(out.remove(format!("ck{l}")).ok_or("ck yok")?);
            v.push(out.remove(format!("cv{l}")).ok_or("cv yok")?);
        }
        Ok(v)
    }

    /// Bir pencereyi çözer. `lang = None`: dil ilk adımda bulunur.
    fn decode(&mut self, cross: &[DynValue], lang: Option<usize>, ts: bool, first: bool, cancel: &AtomicBool) -> Result<Decoded, String> {
        let (h, dh, nl) = (self.cfg.heads, self.cfg.d / self.cfg.heads, self.cfg.dec_layers);
        let sp = &self.sp;
        let (eot, ts0) = (sp.eot, sp.ts_begin);
        let mut past: Vec<DynValue> = Vec::new();
        for _ in 0..nl {
            past.push(OT::from_array(([1usize, h, dh, 1], vec![0f32; h * dh])).map_err(err)?.into_dyn());
            past.push(OT::from_array(([1usize, h, 1, dh], vec![0f32; h * dh])).map_err(err)?.into_dyn());
        }
        let mut plen = 1usize;
        let mut prompt = vec![sp.sot];
        let mut lang = lang;
        let mut pi = 0usize;
        let mut tokens: Vec<usize> = Vec::new();
        let (mut sum_lp, mut no_speech) = (0f32, 0f32);
        let mut cur = sp.sot;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(t!("Durduruldu", "Stopped").into());
            }
            let pos = pi + tokens.len();
            let mut mask = vec![0f32; plen + 1];
            mask[0] = -1e9;
            let mut ins: Vec<(Cow<str>, SessionInputValue)> = vec![
                ("tok".into(), OT::from_array(([1usize, 1], vec![cur as i64])).map_err(err)?.into_dyn().into()),
                ("pos".into(), OT::from_array(([1usize], vec![pos as i64])).map_err(err)?.into_dyn().into()),
                ("mask".into(), OT::from_array(([1usize, 1, 1, plen + 1], mask)).map_err(err)?.into_dyn().into()),
            ];
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
            drop(out);
            past = np;
            plen += 1;

            if pi < prompt.len() {
                // İstem (prompt) adımları.
                if pi == 0 {
                    let p = log_softmax(&logits);
                    no_speech = p[sp.nospeech].exp();
                    if lang.is_none() {
                        let r = sp.lang0..sp.lang0 + sp.nlang;
                        lang = Some(r.clone().max_by(|&a, &b| logits[a].total_cmp(&logits[b])).unwrap() - sp.lang0);
                    }
                    prompt.push(sp.lang0 + lang.unwrap());
                    prompt.push(sp.transcribe);
                    if !ts {
                        prompt.push(sp.notimestamps);
                    }
                }
                pi += 1;
                if pi < prompt.len() {
                    cur = prompt[pi];
                    continue;
                }
            }

            // Süzgeçler.
            let ninf = f32::NEG_INFINITY;
            for &s in &self.suppress {
                if s < logits.len() {
                    logits[s] = ninf;
                }
            }
            for v in &mut logits[eot + 1..ts0] {
                *v = ninf;
            }
            if tokens.is_empty() {
                for &s in &self.begin_suppress {
                    if s < logits.len() {
                        logits[s] = ninf;
                    }
                }
            }
            if !ts {
                for v in &mut logits[ts0..] {
                    *v = ninf;
                }
            } else {
                let n = tokens.len();
                let last_ts = n >= 1 && tokens[n - 1] >= ts0;
                let pen_ts = n < 2 || tokens[n - 2] >= ts0;
                if last_ts {
                    if pen_ts {
                        logits[ts0..].iter_mut().for_each(|v| *v = ninf);
                    } else {
                        logits[..eot].iter_mut().for_each(|v| *v = ninf);
                    }
                }
                if let Some(&lt) = tokens.iter().rev().find(|&&t| t >= ts0) {
                    let upto = if last_ts && !pen_ts { lt } else { lt + 1 };
                    logits[ts0..upto].iter_mut().for_each(|v| *v = ninf);
                }
                if n == 0 {
                    logits[..ts0].iter_mut().for_each(|v| *v = ninf);
                    if first {
                        let last_allowed = ts0 + 50;
                        logits[last_allowed + 1..].iter_mut().for_each(|v| *v = ninf);
                    }
                }
                let lp = log_softmax(&logits);
                let m = lp[ts0..].iter().cloned().fold(ninf, f32::max);
                let tsl = if m == ninf { ninf } else { m + lp[ts0..].iter().map(|v| (v - m).exp()).sum::<f32>().ln() };
                let mx = lp[..ts0].iter().cloned().fold(ninf, f32::max);
                if tsl > mx {
                    logits[..ts0].iter_mut().for_each(|v| *v = ninf);
                }
            }
            let lp = log_softmax(&logits);
            let next = (0..lp.len()).max_by(|&a, &b| lp[a].total_cmp(&lp[b])).unwrap();
            if next == eot {
                break;
            }
            sum_lp += lp[next];
            tokens.push(next);
            // Aynı simge döngüsünü kır.
            let n = tokens.len();
            if n >= SAMPLE_LEN || (n >= 12 && tokens[n - 12..].iter().all(|&t| t == next)) {
                break;
            }
            cur = next;
        }
        let avg = if tokens.is_empty() { 0.0 } else { sum_lp / (tokens.len() + 1) as f32 };
        Ok(Decoded { tokens, avg_logprob: avg, no_speech, lang: lang.unwrap_or(0) })
    }

    pub fn text_of(&self, toks: &[usize]) -> String {
        let b: Vec<u8> = toks.iter().filter(|&&t| t < self.sp.eot).flat_map(|&t| self.vocab[t].iter().copied()).collect();
        String::from_utf8_lossy(&b).trim().to_string()
    }

    /// Sesi yazıya çevirir. `pcm`: 16 kHz tek kanal. `lang`: "tr", "en"…
    /// ya da `None` (otomatik). `timestamps`: cümle zamanları (.srt için).
    /// Dönüş: parçalar ve bulunan dil kodu.
    pub fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>, timestamps: bool, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<(Vec<Segment>, &'static str), String> {
        let mut lang = lang.and_then(Self::lang_index).filter(|&i| i < self.sp.nlang);
        let total = pcm.len().div_ceil(HOP);
        let mut seek = 0usize;
        let mut segs = Vec::new();
        let ts0 = self.sp.ts_begin;
        while seek < total {
            progress(seek as f32 / total as f32);
            let size = (total - seek).min(FRAMES);
            let off = seek as f32 * HOP as f32 / SR as f32;
            let dur = size as f32 * HOP as f32 / SR as f32;
            // Sonda 0,1 saniyeden kısa artık parça: atla.
            if size < 10 {
                break;
            }
            let cross = self.encode(pcm, seek * HOP)?;
            let r = self.decode(&cross, lang, timestamps, seek == 0, cancel)?;
            if lang.is_none() {
                lang = Some(r.lang);
            }
            if r.no_speech > 0.6 && r.avg_logprob < -1.0 || r.tokens.is_empty() {
                seek += size;
                continue;
            }
            let toks = &r.tokens;
            if !timestamps {
                let text = self.text_of(toks);
                if !text.is_empty() {
                    segs.push(Segment { start: off, end: off + dur, text });
                }
                seek += size;
                continue;
            }
            let is_ts = |t: usize| t >= ts0;
            let n = toks.len();
            let single_end = n >= 2 && !is_ts(toks[n - 2]) && is_ts(toks[n - 1]);
            let mut cons: Vec<usize> = (1..n).filter(|&i| is_ts(toks[i - 1]) && is_ts(toks[i])).collect();
            if !cons.is_empty() {
                if single_end {
                    cons.push(n);
                }
                let mut last = 0;
                for &c in &cons {
                    let sl = &toks[last..c];
                    if sl.len() >= 2 {
                        let (s, e) = (sl[0].saturating_sub(ts0), sl[sl.len() - 1].saturating_sub(ts0));
                        let text = self.text_of(sl);
                        if !text.is_empty() {
                            segs.push(Segment { start: off + s as f32 * 0.02, end: off + e as f32 * 0.02, text });
                        }
                    }
                    last = c;
                }
                let adv = if single_end { size } else { (toks[last - 1].saturating_sub(ts0)) * 2 };
                seek += if adv == 0 { size } else { adv.min(size) };
            } else {
                let mut d = dur;
                if let Some(&lt) = toks.iter().rev().find(|&&t| is_ts(t)) {
                    if lt != ts0 {
                        d = (lt - ts0) as f32 * 0.02;
                    }
                }
                let text = self.text_of(toks);
                if !text.is_empty() {
                    segs.push(Segment { start: off, end: off + d, text });
                }
                seek += size;
            }
        }
        progress(1.0);
        Ok((segs, LANGS[lang.unwrap_or(0)]))
    }
}

/// Parçaları SubRip (.srt) metnine çevirir.
pub fn to_srt(segs: &[Segment]) -> String {
    let f = |t: f32| {
        let ms = (t.max(0.0) * 1000.0).round() as u64;
        format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
    };
    let mut s = String::new();
    for (i, g) in segs.iter().enumerate() {
        s += &format!("{}\r\n{} --> {}\r\n{}\r\n\r\n", i + 1, f(g.start), f(g.end.max(g.start + 0.2)), g.text);
    }
    s
}

/// Düz metin: parçalar boşlukla birleşir.
pub fn to_text(segs: &[Segment]) -> String {
    segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_rows_positive() {
        let m = mel_filters(80);
        for r in 0..80 {
            assert!(m[r * 201..(r + 1) * 201].iter().any(|&v| v > 0.0), "satır {r}");
        }
    }

    #[test]
    fn bytes_map() {
        let bd = byte_decoder();
        assert_eq!(bd.len(), 256);
        assert_eq!(bd[&'Ġ'], b' ');
    }

    #[test]
    fn srt_time() {
        let s = to_srt(&[Segment { start: 61.5, end: 3725.25, text: "a".into() }]);
        assert!(s.contains("00:01:01,500 --> 01:02:05,250"));
    }

    #[test]
    fn specials() {
        let s = Special::new(51865);
        assert_eq!((s.transcribe, s.nospeech, s.notimestamps, s.ts_begin), (50359, 50362, 50363, 50364));
        assert_eq!(Whisper::lang_index("tr"), Some(9));
    }
}
