//! Bu modül "resmi ağırlık dosyalarını Python'suz nasıl okuruz?" sorununu
//! çözüyor. İki biçim var:
//! - PyTorch `.pth`: bir zip; içinde `data.pkl` (pickle) ve her tensörün ham
//!   verisi (`data/<n>`). Pickle'ın torch'un kullandığı küçük bir alt kümesini
//!   çalıştırıp tensör adlarını, şekillerini ve verinin yerini çıkarıyoruz.
//!   Pickle'da kod çalıştırılmaz: yalnızca bilinen torch kurucuları tanınır.
//! - `.safetensors`: 8 baytlık başlık uzunluğu + JSON + ham veri.

use std::collections::HashMap;

pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

pub type Weights = HashMap<String, Tensor>;

pub fn f16_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) as u32) << 31;
    let e = ((h >> 10) & 0x1F) as u32;
    let m = (h & 0x3FF) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s
        } else {
            // alt-normal
            let mut e = 127 - 15 + 1;
            let mut m = m;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            s | (e << 23) | ((m & 0x3FF) << 13)
        }
    } else if e == 31 {
        s | 0x7F80_0000 | (m << 13)
    } else {
        s | ((e + 127 - 15) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

pub fn f32_to_f16(f: f32) -> u16 {
    let x = f.to_bits();
    let s = ((x >> 16) & 0x8000) as u16;
    let e = ((x >> 23) & 0xFF) as i32 - 127 + 15;
    let m = x & 0x7F_FFFF;
    if e >= 31 {
        return s | 0x7C00 | if (x & 0x7FFF_FFFF) > 0x7F80_0000 { 0x200 } else { 0 };
    }
    if e <= 0 {
        if e < -10 {
            return s;
        }
        let m = (m | 0x80_0000) >> (1 - e);
        return s | ((m + 0x1000) >> 13) as u16;
    }
    let v = ((e as u32) << 10) | (m >> 13);
    let r = m & 0x1FFF;
    let v = if r > 0x1000 || (r == 0x1000 && v & 1 == 1) { v + 1 } else { v };
    s | v as u16
}

fn decode(dtype: &str, raw: &[u8]) -> Result<Vec<f32>, String> {
    Ok(match dtype {
        "F32" => raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        "F16" => raw.chunks_exact(2).map(|c| f16_to_f32(u16::from_le_bytes([c[0], c[1]]))).collect(),
        "BF16" => raw.chunks_exact(2).map(|c| f32::from_bits((u16::from_le_bytes([c[0], c[1]]) as u32) << 16)).collect(),
        d => return Err(format!("tensör türü desteklenmiyor: {d}")),
    })
}

// ---------------------------------------------------------------- safetensors

pub fn safetensors(buf: &[u8]) -> Result<Weights, String> {
    let bad = || "safetensors bozuk".to_string();
    let n = u64::from_le_bytes(buf.get(..8).ok_or_else(bad)?.try_into().unwrap()) as usize;
    let head: serde_json::Value = serde_json::from_slice(buf.get(8..8 + n).ok_or_else(bad)?).map_err(|e| e.to_string())?;
    let data = &buf[8 + n..];
    let mut out = Weights::new();
    for (name, v) in head.as_object().ok_or_else(bad)? {
        if name == "__metadata__" {
            continue;
        }
        let dtype = v["dtype"].as_str().ok_or_else(bad)?;
        let shape: Vec<usize> = v["shape"].as_array().ok_or_else(bad)?.iter().map(|x| x.as_u64().unwrap_or(0) as usize).collect();
        let off = v["data_offsets"].as_array().ok_or_else(bad)?;
        let (a, b) = (off[0].as_u64().unwrap_or(0) as usize, off[1].as_u64().unwrap_or(0) as usize);
        let raw = data.get(a..b).ok_or_else(bad)?;
        out.insert(name.clone(), Tensor { shape, data: decode(dtype, raw)? });
    }
    Ok(out)
}

// ---------------------------------------------------------------- zip

fn u16le(b: &[u8], i: usize) -> usize {
    u16::from_le_bytes([b[i], b[i + 1]]) as usize
}
fn u32le(b: &[u8], i: usize) -> usize {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
}
fn u64le(b: &[u8], i: usize) -> usize {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap()) as usize
}

/// Sıkıştırılmamış (STORED) zip girdileri: ad → (başlangıç, uzunluk).
fn zip_entries(b: &[u8]) -> Result<HashMap<String, (usize, usize)>, String> {
    let bad = || "zip bozuk".to_string();
    let eocd = (0..b.len().saturating_sub(21)).rev().take(70_000).find(|&i| b[i..i + 4] == [0x50, 0x4B, 0x05, 0x06]).ok_or_else(bad)?;
    let mut count = u16le(b, eocd + 10);
    let mut cd = u32le(b, eocd + 16);
    if cd == 0xFFFF_FFFF || count == 0xFFFF {
        // zip64: EOCD konumlayıcısı hemen önde.
        let loc = eocd.checked_sub(20).ok_or_else(bad)?;
        let z = u64le(b, loc + 8);
        count = u64le(b, z + 32);
        cd = u64le(b, z + 48);
    }
    let mut out = HashMap::new();
    let mut p = cd;
    for _ in 0..count {
        if b.get(p..p + 4) != Some(&[0x50, 0x4B, 0x01, 0x02]) {
            return Err(bad());
        }
        let method = u16le(b, p + 10);
        let mut size = u32le(b, p + 20);
        let nl = u16le(b, p + 28);
        let el = u16le(b, p + 30);
        let cl = u16le(b, p + 32);
        let mut lho = u32le(b, p + 42);
        let name = String::from_utf8_lossy(&b[p + 46..p + 46 + nl]).to_string();
        // zip64 ek alanı
        let mut e = p + 46 + nl;
        let end = e + el;
        while e + 4 <= end {
            let (id, len) = (u16le(b, e), u16le(b, e + 2));
            if id == 1 {
                let mut q = e + 4;
                if u32le(b, p + 24) == 0xFFFF_FFFF {
                    q += 8;
                }
                if size == 0xFFFF_FFFF {
                    size = u64le(b, q);
                    q += 8;
                }
                if lho == 0xFFFF_FFFF {
                    lho = u64le(b, q);
                }
            }
            e += 4 + len;
        }
        if method == 0 {
            let start = lho + 30 + u16le(b, lho + 26) + u16le(b, lho + 28);
            out.insert(name, (start, size));
        }
        p = end + cl;
    }
    Ok(out)
}

// ---------------------------------------------------------------- pickle

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum V {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Tuple(Vec<V>),
    List(Vec<V>),
    Dict(Vec<(V, V)>),
    Global(String),
    Storage { key: String, dtype: String },
    Tensor { key: String, dtype: String, offset: usize, shape: Vec<usize> },
    Mark,
    Other,
}

fn reduce(f: &V, args: V) -> V {
    let a = match args {
        V::Tuple(a) => a,
        _ => return V::Other,
    };
    let ints = |v: &V| match v {
        V::Tuple(x) => x.iter().map(|i| if let V::Int(i) = i { *i as usize } else { 0 }).collect(),
        _ => vec![],
    };
    match f {
        V::Global(g) if g == "torch._utils._rebuild_tensor_v2" || g == "torch._utils._rebuild_tensor" => match a.first() {
            Some(V::Storage { key, dtype }) => V::Tensor { key: key.clone(), dtype: dtype.clone(), offset: if let Some(V::Int(o)) = a.get(1) { *o as usize } else { 0 }, shape: a.get(2).map(ints).unwrap_or_default() },
            _ => V::Other,
        },
        V::Global(g) if g == "torch._utils._rebuild_parameter" => a.into_iter().next().unwrap_or(V::Other),
        V::Global(g) if g == "collections.OrderedDict" => V::Dict(vec![]),
        _ => V::Other,
    }
}

fn pickle(p: &[u8]) -> Result<V, String> {
    let bad = |what: &str| format!("pickle okunamadı ({what})");
    let mut st: Vec<V> = Vec::new();
    let mut memo: HashMap<usize, V> = HashMap::new();
    let mut i = 0;
    let pop_mark = |st: &mut Vec<V>| -> Vec<V> {
        let m = st.iter().rposition(|v| matches!(v, V::Mark)).unwrap_or(0);
        let items = st.split_off(m + 1);
        st.pop();
        items
    };
    macro_rules! take {
        ($n:expr) => {{
            let s = p.get(i..i + $n).ok_or_else(|| bad("son"))?;
            i += $n;
            s
        }};
    }
    loop {
        let op = *p.get(i).ok_or_else(|| bad("son"))?;
        i += 1;
        match op {
            0x80 => i += 1,
            0x95 => i += 8,
            b'}' => st.push(V::Dict(vec![])),
            b']' => st.push(V::List(vec![])),
            b')' => st.push(V::Tuple(vec![])),
            b'(' => st.push(V::Mark),
            b'N' => st.push(V::None),
            0x88 => st.push(V::Bool(true)),
            0x89 => st.push(V::Bool(false)),
            b'K' => st.push(V::Int(take!(1)[0] as i64)),
            b'M' => st.push(V::Int(u16::from_le_bytes(take!(2).try_into().unwrap()) as i64)),
            b'J' => st.push(V::Int(i32::from_le_bytes(take!(4).try_into().unwrap()) as i64)),
            0x8a => {
                let n = take!(1)[0] as usize;
                let b = take!(n);
                let mut v: i64 = 0;
                for (k, x) in b.iter().enumerate().take(8) {
                    v |= (*x as i64) << (8 * k);
                }
                if n > 0 && n < 8 && b[n - 1] & 0x80 != 0 {
                    v -= 1i64 << (8 * n);
                }
                st.push(V::Int(v));
            }
            b'G' => st.push(V::Float(f64::from_be_bytes(take!(8).try_into().unwrap()))),
            b'X' => {
                let n = u32::from_le_bytes(take!(4).try_into().unwrap()) as usize;
                st.push(V::Str(String::from_utf8_lossy(take!(n)).into()));
            }
            0x8c | b'U' => {
                let n = take!(1)[0] as usize;
                st.push(V::Str(String::from_utf8_lossy(take!(n)).into()));
            }
            b'T' => {
                let n = u32::from_le_bytes(take!(4).try_into().unwrap()) as usize;
                st.push(V::Str(String::from_utf8_lossy(take!(n)).into()));
            }
            b'c' => {
                let nl1 = p[i..].iter().position(|&c| c == b'\n').ok_or_else(|| bad("global"))?;
                let m = String::from_utf8_lossy(&p[i..i + nl1]).to_string();
                i += nl1 + 1;
                let nl2 = p[i..].iter().position(|&c| c == b'\n').ok_or_else(|| bad("global"))?;
                let n = String::from_utf8_lossy(&p[i..i + nl2]).to_string();
                i += nl2 + 1;
                st.push(V::Global(format!("{m}.{n}")));
            }
            0x93 => {
                let n = st.pop();
                let m = st.pop();
                match (m, n) {
                    (Some(V::Str(m)), Some(V::Str(n))) => st.push(V::Global(format!("{m}.{n}"))),
                    _ => return Err(bad("stack_global")),
                }
            }
            b'Q' => {
                let pid = st.pop().ok_or_else(|| bad("persid"))?;
                if let V::Tuple(t) = pid {
                    let dtype = match t.get(1) {
                        Some(V::Global(g)) => g.rsplit('.').next().unwrap_or("").to_string(),
                        _ => String::new(),
                    };
                    let key = match t.get(2) {
                        Some(V::Str(k)) => k.clone(),
                        _ => String::new(),
                    };
                    st.push(V::Storage { key, dtype });
                } else {
                    st.push(V::Other);
                }
            }
            b't' => {
                let items = pop_mark(&mut st);
                st.push(V::Tuple(items));
            }
            0x85 => {
                let a = st.pop().ok_or_else(|| bad("t1"))?;
                st.push(V::Tuple(vec![a]));
            }
            0x86 => {
                let b = st.pop().ok_or_else(|| bad("t2"))?;
                let a = st.pop().ok_or_else(|| bad("t2"))?;
                st.push(V::Tuple(vec![a, b]));
            }
            0x87 => {
                let c = st.pop().ok_or_else(|| bad("t3"))?;
                let b = st.pop().ok_or_else(|| bad("t3"))?;
                let a = st.pop().ok_or_else(|| bad("t3"))?;
                st.push(V::Tuple(vec![a, b, c]));
            }
            b'R' | 0x81 => {
                let args = st.pop().ok_or_else(|| bad("reduce"))?;
                let f = st.pop().ok_or_else(|| bad("reduce"))?;
                st.push(reduce(&f, args));
            }
            b'b' => {
                st.pop();
            }
            b's' => {
                let v = st.pop().ok_or_else(|| bad("setitem"))?;
                let k = st.pop().ok_or_else(|| bad("setitem"))?;
                if let Some(V::Dict(d)) = st.last_mut() {
                    d.push((k, v));
                }
            }
            b'u' => {
                let items = pop_mark(&mut st);
                if let Some(V::Dict(d)) = st.last_mut() {
                    let mut it = items.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        d.push((k, v));
                    }
                }
            }
            b'a' => {
                let v = st.pop().ok_or_else(|| bad("append"))?;
                if let Some(V::List(l)) = st.last_mut() {
                    l.push(v);
                }
            }
            b'e' => {
                let items = pop_mark(&mut st);
                if let Some(V::List(l)) = st.last_mut() {
                    l.extend(items);
                }
            }
            b'q' => {
                let k = take!(1)[0] as usize;
                memo.insert(k, st.last().cloned().unwrap_or(V::None));
            }
            b'r' => {
                let k = u32::from_le_bytes(take!(4).try_into().unwrap()) as usize;
                memo.insert(k, st.last().cloned().unwrap_or(V::None));
            }
            0x94 => {
                let k = memo.len();
                memo.insert(k, st.last().cloned().unwrap_or(V::None));
            }
            b'h' => {
                let k = take!(1)[0] as usize;
                st.push(memo.get(&k).cloned().unwrap_or(V::None));
            }
            b'j' => {
                let k = u32::from_le_bytes(take!(4).try_into().unwrap()) as usize;
                st.push(memo.get(&k).cloned().unwrap_or(V::None));
            }
            b'.' => return st.pop().ok_or_else(|| bad("boş")),
            x => return Err(bad(&format!("işlem 0x{x:02x}"))),
        }
    }
}

/// PyTorch `.pth` dosyasından tensörleri okur. `params_ema` ya da `params`
/// altında duran durum sözlüklerini (Real-ESRGAN) de açar.
pub fn pth(buf: &[u8]) -> Result<Weights, String> {
    let entries = zip_entries(buf)?;
    let (pkl, _) = entries.iter().find(|(k, _)| k.ends_with("data.pkl")).ok_or("data.pkl yok")?;
    let prefix = pkl.trim_end_matches("data.pkl").to_string();
    let (s, n) = entries[pkl];
    let mut root = pickle(&buf[s..s + n])?;
    for k in ["params_ema", "params", "state_dict", "model"] {
        if let V::Dict(d) = &root {
            if let Some((_, v)) = d.iter().find(|(kk, _)| matches!(kk, V::Str(s) if s == k)) {
                root = v.clone();
                break;
            }
        }
    }
    let V::Dict(d) = root else { return Err("pth içinde sözlük yok".into()) };
    let mut out = Weights::new();
    for (k, v) in d {
        let (V::Str(name), V::Tensor { key, dtype, offset, shape }) = (k, v) else { continue };
        let (es, en) = *entries.get(&format!("{prefix}data/{key}")).ok_or("tensör verisi yok")?;
        let numel: usize = shape.iter().product::<usize>().max(1);
        let (sz, dt) = match dtype.as_str() {
            "FloatStorage" => (4, "F32"),
            "HalfStorage" => (2, "F16"),
            "BFloat16Storage" => (2, "BF16"),
            d => return Err(format!("tensör türü desteklenmiyor: {d}")),
        };
        let a = es + offset * sz;
        if a + numel * sz > es + en {
            return Err("tensör sınır dışında".into());
        }
        out.insert(name, Tensor { shape, data: decode(dt, &buf[a..a + numel * sz])? });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half() {
        for v in [0.0f32, 1.0, -2.5, 65504.0, 0.000061035156, 1e-7] {
            let r = f16_to_f32(f32_to_f16(v));
            assert!((r - v).abs() <= v.abs() * 1e-3 + 1e-7, "{v} {r}");
        }
    }
}
