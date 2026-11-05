//! Bu modül "metni bilgisayarda, internete göndermeden nasıl seslendiririz?"
//! sorusunu çözüyor. İki tür ses modeli çalıştırılır:
//! - Piper (VITS): fonem kimlikleri `^ _ f1 _ f2 _ … $` dizisiyle verilir,
//!   22 050 Hz ses döner. Türkçe ve İngilizce sesler.
//! - Kokoro (82M): misaki fonemleri ve bir ses kimliği (256 boyutlu stil)
//!   verilir, 24 000 Hz ses döner. İngilizcede en doğal ses.
//! Metin cümlelere bölünür; her cümle ayrı üretilir ki okuma hemen başlasın.

use crate::{g2p_en, g2p_tr, runtime, store};
use ort::{session::Session, value::Tensor};
use std::{collections::HashMap, path::Path};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Tr,
    En,
}

enum Kind {
    Piper { ids: HashMap<char, Vec<i64>>, noise: f32, length: f32, noise_w: f32 },
    Kokoro { vocab: HashMap<char, i64>, style: Vec<f32> },
}

pub struct Voice {
    sess: Session,
    kind: Kind,
    lex: Option<g2p_en::Lexicon>,
    pub lang: Lang,
    pub rate: u32,
}

fn json(p: &Path) -> Result<serde_json::Value, String> {
    serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| e.to_string())
}

fn one_char(k: &str) -> Option<char> {
    let mut it = k.chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

impl Voice {
    /// Model klasöründen sesi açar (Piper ya da Kokoro, dosyalara bakarak).
    pub fn open(dir: &Path) -> Result<Voice, String> {
        let onnx = store::find(dir, "model.onnx").ok_or("model.onnx yok")?;
        if let Some(tok) = store::find(dir, "tokenizer.json") {
            let t = json(&tok)?;
            let vocab = t["model"]["vocab"].as_object().ok_or("tokenizer.json: vocab yok")?.iter().filter_map(|(k, v)| Some((one_char(k)?, v.as_i64()?))).collect();
            let vb = std::fs::read(store::find(dir, "voice.bin").ok_or("voice.bin yok")?).map_err(|e| e.to_string())?;
            let style = vb.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect::<Vec<_>>();
            if style.len() < 256 {
                return Err("voice.bin bozuk".into());
            }
            // DirectML Kokoro'nun ConvTranspose katmanında hata veriyor; işlemci gerçek zamana yakın.
            let lex = g2p_en::Lexicon::load(dir)?;
            let sess = runtime::session_cpu(runtime::Src::File(&onnx))?;
            return Ok(Voice { sess, kind: Kind::Kokoro { vocab, style }, lex: Some(lex), lang: Lang::En, rate: 24_000 });
        }
        let cfg = json(&store::find(dir, "model.onnx.json").ok_or("model.onnx.json yok")?)?;
        let ids = cfg["phoneme_id_map"].as_object().ok_or("phoneme_id_map yok")?.iter().filter_map(|(k, v)| Some((one_char(k)?, v.as_array()?.iter().filter_map(|x| x.as_i64()).collect()))).collect();
        let inf = &cfg["inference"];
        let f = |k: &str, d: f64| inf[k].as_f64().unwrap_or(d) as f32;
        let lang = if cfg["espeak"]["voice"].as_str().unwrap_or("").starts_with("tr") { Lang::Tr } else { Lang::En };
        let lex = if lang == Lang::En { Some(g2p_en::Lexicon::load(dir)?) } else { None };
        let rate = cfg["audio"]["sample_rate"].as_u64().unwrap_or(22_050) as u32;
        let sess = runtime::session_cpu(runtime::Src::File(&onnx))?;
        Ok(Voice { sess, kind: Kind::Piper { ids, noise: f("noise_scale", 0.667), length: f("length_scale", 1.0), noise_w: f("noise_w", 0.8) }, lex, lang, rate })
    }

    /// Metnin fonemleri (bu sesin beklediği işaretlerle).
    pub fn phonemes(&self, text: &str) -> String {
        match (&self.kind, self.lang, &self.lex) {
            (_, Lang::Tr, _) => g2p_tr::phonemize(text),
            (Kind::Kokoro { .. }, _, Some(l)) => g2p_en::phonemize(l, text),
            (Kind::Piper { .. }, _, Some(l)) => g2p_en::to_espeak(&g2p_en::phonemize(l, text)),
            _ => String::new(),
        }
    }

    /// Bir cümleyi seslendirir. `speed`: 1.0 normal, 2.0 iki kat hızlı.
    pub fn synth(&mut self, text: &str, speed: f32) -> Result<Vec<f32>, String> {
        let ph = self.phonemes(text);
        if ph.trim().is_empty() {
            return Ok(Vec::new());
        }
        let speed = speed.clamp(0.5, 2.0);
        let e = |e: ort::Error| e.to_string();
        match &self.kind {
            Kind::Piper { ids, noise, length, noise_w } => {
                let mut seq: Vec<i64> = ids.get(&'^').cloned().unwrap_or(vec![1]);
                let pad = ids.get(&'_').cloned().unwrap_or(vec![0]);
                seq.extend(&pad);
                for c in ph.chars() {
                    if let Some(v) = ids.get(&c) {
                        seq.extend(v);
                        seq.extend(&pad);
                    }
                }
                seq.extend(ids.get(&'$').cloned().unwrap_or(vec![2]));
                let n = seq.len();
                let input = Tensor::from_array(([1usize, n], seq)).map_err(e)?;
                let lens = Tensor::from_array(([1usize], vec![n as i64])).map_err(e)?;
                let scales = Tensor::from_array(([3usize], vec![*noise, length / speed, *noise_w])).map_err(e)?;
                let out = self.sess.run(ort::inputs!["input" => input, "input_lengths" => lens, "scales" => scales]).map_err(e)?;
                let (_, d) = out[0].try_extract_tensor::<f32>().map_err(e)?;
                Ok(d.to_vec())
            }
            Kind::Kokoro { vocab, style } => {
                let mut toks: Vec<i64> = ph.chars().filter_map(|c| vocab.get(&c).copied()).collect();
                toks.truncate(510);
                let rows = style.len() / 256;
                let row = toks.len().min(rows - 1);
                let s = style[row * 256..row * 256 + 256].to_vec();
                let mut seq = vec![0i64];
                seq.extend(&toks);
                seq.push(0);
                let n = seq.len();
                let ids = Tensor::from_array(([1usize, n], seq)).map_err(e)?;
                let st = Tensor::from_array(([1usize, 256], s)).map_err(e)?;
                let sp = Tensor::from_array(([1usize], vec![speed])).map_err(e)?;
                let out = self.sess.run(ort::inputs!["input_ids" => ids, "style" => st, "speed" => sp]).map_err(e)?;
                let (_, d) = out[0].try_extract_tensor::<f32>().map_err(e)?;
                Ok(d.to_vec())
            }
        }
    }
}

/// Metni okunacak parçalara böler: cümle sonlarında, çok uzunsa virgülde.
pub fn sentences(text: &str) -> Vec<String> {
    const MAX: usize = 220;
    let mut out = Vec::new();
    let mut cur = String::new();
    let cs: Vec<char> = text.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if c == '\n' && cs.get(i + 1).map_or(true, |n| *n == '\n' || n.is_uppercase()) {
            // Paragraf ya da madde sonu
            push(&mut out, &mut cur);
            continue;
        }
        cur.push(if c.is_whitespace() { ' ' } else { c });
        let next = cs.get(i + 1).copied().unwrap_or(' ');
        let end = matches!(c, '.' | '!' | '?' | '…') && next.is_whitespace() && !is_abbrev(&cur);
        let long = cur.chars().count() > MAX && matches!(c, ',' | ';' | ':') && next.is_whitespace();
        if end || long || cur.chars().count() > MAX * 2 && c == ' ' {
            push(&mut out, &mut cur);
        }
    }
    push(&mut out, &mut cur);
    out
}

fn push(out: &mut Vec<String>, cur: &mut String) {
    let t = cur.trim();
    if t.chars().any(|c| c.is_alphanumeric()) {
        out.push(t.to_string());
    }
    cur.clear();
}

/// "Dr.", "vb.", "Mr." gibi kısaltmada cümle bitmez.
fn is_abbrev(s: &str) -> bool {
    let w = s.trim_end_matches('.').rsplit(' ').next().unwrap_or("").to_lowercase();
    matches!(w.as_str(), "dr" | "mr" | "mrs" | "ms" | "prof" | "vb" | "vs" | "örn" | "bkz" | "st" | "no" | "sn" | "av" | "doç" | "yrd" | "e.g" | "i.e" | "etc") || (w.chars().count() == 1 && w.chars().all(|c| c.is_alphabetic()))
}

/// 16 bit PCM tek kanal WAV.
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let n = samples.len() as u32 * 2;
    let mut b = Vec::with_capacity(44 + n as usize);
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    // Tepe değere göre hafif normalleştirme (kısık modeller için).
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs())).max(0.01);
    let g = if peak < 0.5 { 0.9 / peak } else { 1.0 };
    for s in samples {
        b.extend((((s * g).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        let s = sentences("Merhaba dünya. Dr. Ali geldi! Nasılsın?\nYeni satır");
        assert_eq!(s, vec!["Merhaba dünya.", "Dr. Ali geldi!", "Nasılsın?", "Yeni satır"]);
        assert!(sentences("  ...  ").is_empty());
    }

    #[test]
    fn wav_header() {
        let w = wav(&[0.0, 0.5, -0.5], 22050);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(crate::audio::read_wav(&w).map(|v| v.is_empty()), Ok(false));
    }
}
