//! Bu modül "çeviri modeline metni hangi parçalara bölerek veririz?" sorusunu
//! çözüyor: SentencePiece (unigram). Resmi `source.spm` dosyası bir protobuf;
//! içinden parçaları (piece) ve puanlarını okuyup en olası bölmeyi Viterbi ile
//! buluruz. Boşluklar "▁" olur, metnin başına da bir "▁" eklenir.

use std::collections::HashMap;

const SPACE: char = '▁';

pub struct Spm {
    pieces: HashMap<String, f32>,
    max_len: usize,
    unk_score: f32,
}

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for s in 0..10 {
        let c = *b.get(*i)?;
        *i += 1;
        v |= ((c & 0x7F) as u64) << (7 * s);
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Protobuf mesajının alanları: (alan no, tel türü, değer ya da bayt dilimi).
fn fields(b: &[u8]) -> Vec<(u32, u64, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(k) = varint(b, &mut i) else { break };
        let (f, w) = ((k >> 3) as u32, k & 7);
        match w {
            0 => {
                let Some(v) = varint(b, &mut i) else { break };
                out.push((f, v, &b[0..0]));
            }
            1 => {
                let Some(s) = b.get(i..i + 8) else { break };
                out.push((f, 0, s));
                i += 8;
            }
            2 => {
                let Some(n) = varint(b, &mut i) else { break };
                let Some(s) = b.get(i..i + n as usize) else { break };
                out.push((f, 0, s));
                i += n as usize;
            }
            5 => {
                let Some(s) = b.get(i..i + 4) else { break };
                out.push((f, 0, s));
                i += 4;
            }
            _ => break,
        }
    }
    out
}

impl Spm {
    pub fn load(buf: &[u8]) -> Result<Self, String> {
        let mut pieces = HashMap::new();
        let mut min = 0f32;
        for (f, _, msg) in fields(buf) {
            if f == 2 {
                // trainer_spec.model_type: 1 = unigram.
                if let Some((_, t, _)) = fields(msg).into_iter().find(|x| x.0 == 3) {
                    if t != 1 {
                        return Err(format!("SentencePiece türü desteklenmiyor ({t})"));
                    }
                }
            }
            if f != 1 {
                continue;
            }
            let (mut piece, mut score, mut ty) = (String::new(), 0f32, 1u64);
            for (pf, v, s) in fields(msg) {
                match pf {
                    1 => piece = String::from_utf8_lossy(s).into_owned(),
                    2 if s.len() == 4 => score = f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    3 => ty = v,
                    _ => {}
                }
            }
            // 1 normal, 4 kullanıcı tanımlı; denetim/bilinmeyen simgeler bölmeye girmez.
            if ty == 1 || ty == 4 {
                min = min.min(score);
                pieces.insert(piece, score);
            }
        }
        if pieces.is_empty() {
            return Err("source.spm boş ya da bozuk".into());
        }
        let max_len = pieces.keys().map(|p| p.chars().count()).max().unwrap_or(1);
        Ok(Self { pieces, max_len, unk_score: min - 10.0 })
    }

    /// Metni parçalara böler. Tanınmayan karakterler tek başına parça olur
    /// (sözlükte yoksa çağıran `<unk>` yapar).
    pub fn encode(&self, text: &str) -> Vec<String> {
        let norm: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if norm.is_empty() {
            return Vec::new();
        }
        let s: Vec<char> = std::iter::once(SPACE).chain(norm.chars().map(|c| if c == ' ' { SPACE } else { c })).collect();
        let n = s.len();
        // best[i]: s[..i] için en iyi puan ve son parçanın başı.
        let mut best = vec![(f32::NEG_INFINITY, 0usize); n + 1];
        best[0].0 = 0.0;
        let mut buf = String::new();
        for i in 0..n {
            if best[i].0 == f32::NEG_INFINITY {
                continue;
            }
            let mut any = false;
            buf.clear();
            for j in i..n.min(i + self.max_len) {
                buf.push(s[j]);
                if let Some(&sc) = self.pieces.get(&buf) {
                    any |= j == i;
                    let v = best[i].0 + sc;
                    if v > best[j + 1].0 {
                        best[j + 1] = (v, i);
                    }
                }
            }
            if !any {
                let v = best[i].0 + self.unk_score;
                if v > best[i + 1].0 {
                    best[i + 1] = (v, i);
                }
            }
        }
        let mut out = Vec::new();
        let mut e = n;
        while e > 0 {
            let b = best[e].1;
            out.push(s[b..e].iter().collect());
            e = b;
        }
        out.reverse();
        out
    }
}

/// Parçaları metne çevirir.
pub fn join(pieces: &[&str]) -> String {
    pieces.concat().replace(SPACE, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy() -> Spm {
        let mut pieces = HashMap::new();
        for (p, s) in [("▁hello", -1.0), ("▁he", -2.0), ("llo", -2.0), ("▁", -3.0), ("w", -4.0), ("o", -4.0), ("r", -4.0), ("l", -4.0), ("d", -4.0), ("▁wor", -2.5), ("ld", -2.5)] {
            pieces.insert(p.to_string(), s);
        }
        Spm { pieces, max_len: 6, unk_score: -14.0 }
    }

    #[test]
    fn viterbi() {
        let t = toy();
        assert_eq!(t.encode("  hello   world "), vec!["▁hello", "▁wor", "ld"]);
        assert_eq!(t.encode("hello!"), vec!["▁hello", "!"]);
        assert!(t.encode("   ").is_empty());
        assert_eq!(join(&["▁hello", "▁wor", "ld"]), "hello world");
    }

    #[test]
    fn protobuf_varint() {
        let b = [0x96, 0x01];
        let mut i = 0;
        assert_eq!(varint(&b, &mut i), Some(150));
    }
}
