//! Bu modül "ses dosyasını modelin istediği biçime nasıl getiririz?" sorusunu
//! çözüyor: WAV (PCM 8/16/24/32 bit ya da float) okunur, kanallar
//! ortalanıp tek kanala indirilir, 16 kHz'e yeniden örneklenir.

/// WAV baytlarını 16 kHz tek kanal örneklere çevirir.
pub fn read_wav(b: &[u8]) -> Result<Vec<f32>, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("WAV değil".into());
    }
    let (mut ch, mut rate, mut bits, mut fmt) = (0usize, 0usize, 0usize, 0u16);
    let mut data: &[u8] = &[];
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &b[i + 8..(i + 8).saturating_add(n).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            fmt = u16::from_le_bytes([body[0], body[1]]);
            ch = u16::from_le_bytes([body[2], body[3]]) as usize;
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
            bits = u16::from_le_bytes([body[14], body[15]]) as usize;
            if fmt == 0xFFFE && body.len() >= 26 {
                fmt = u16::from_le_bytes([body[24], body[25]]);
            }
        } else if id == b"data" {
            data = body;
        }
        i += 8 + n + (n & 1);
    }
    if ch == 0 || rate == 0 {
        return Err("WAV biçimi okunamadı".into());
    }
    let s: Vec<f32> = match (fmt, bits) {
        (1, 8) => data.iter().map(|&v| (v as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        (1, 24) => data.chunks_exact(3).map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0).collect(),
        (1, 32) => data.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f32 / 2_147_483_648.0).collect(),
        (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect(),
        _ => return Err(format!("desteklenmeyen WAV ({fmt}, {bits} bit)")),
    };
    let mono: Vec<f32> = s.chunks_exact(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect();
    Ok(resample(&mono, rate, crate::whisper::SR))
}

/// Doğrusal yeniden örnekleme.
pub fn resample(x: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let n = (x.len() as u64 * to as u64 / from as u64) as usize;
    let last = x.len() - 1;
    (0..n)
        .map(|k| {
            let f = k as f64 * from as f64 / to as f64;
            let a = (f as usize).min(last);
            let t = (f - a as f64) as f32;
            x[a] + (x[(a + 1).min(last)] - x[a]) * t
        })
        .collect()
}
