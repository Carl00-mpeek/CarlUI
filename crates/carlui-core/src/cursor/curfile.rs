//! .cur dosya biçimi: ICONDIR (tür 2) + her boyut için 32 bit BGRA bitmap ve
//! boş AND maskesi. Sıcak nokta her girişte o boyuta ölçeklenmiş olarak durur.

pub struct Image {
    pub size: u32,
    /// Düz (ön çarpımsız) BGRA, satırlar yukarıdan aşağı.
    pub px: Vec<u8>,
    pub hot: (u16, u16),
}

pub fn encode(images: &[Image]) -> Vec<u8> {
    let mut head = Vec::new();
    let mut body = Vec::new();
    head.extend_from_slice(&0u16.to_le_bytes());
    head.extend_from_slice(&2u16.to_le_bytes());
    head.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for im in images {
        let n = im.size;
        let mask_row = n.div_ceil(32) * 4;
        let data_len = 40 + n * n * 4 + mask_row * n;
        let dim = if n >= 256 { 0 } else { n as u8 };
        head.extend_from_slice(&[dim, dim, 0, 0]);
        head.extend_from_slice(&im.hot.0.to_le_bytes());
        head.extend_from_slice(&im.hot.1.to_le_bytes());
        head.extend_from_slice(&data_len.to_le_bytes());
        head.extend_from_slice(&offset.to_le_bytes());
        offset += data_len;

        body.extend_from_slice(&40u32.to_le_bytes());
        body.extend_from_slice(&(n as i32).to_le_bytes());
        body.extend_from_slice(&(2 * n as i32).to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&32u16.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&(data_len - 40).to_le_bytes());
        body.extend_from_slice(&[0u8; 16]);
        let stride = n as usize * 4;
        for row in (0..n as usize).rev() {
            body.extend_from_slice(&im.px[row * stride..(row + 1) * stride]);
        }
        // AND maskesi: tam opak olmayan her piksel 1 (zemin korunur). Sıfır
        // bırakılırsa Windows zemini önce siyaha boyar, yarı saydam ışıma
        // kenarında siyah hale kalır.
        for row in (0..n as usize).rev() {
            let mut bits = vec![0u8; mask_row as usize];
            for x in 0..n as usize {
                if im.px[(row * n as usize + x) * 4 + 3] < 255 {
                    bits[x / 8] |= 0x80 >> (x % 8);
                }
            }
            body.extend_from_slice(&bits);
        }
    }
    head.extend_from_slice(&body);
    head
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// Animasyonlu imleç (.ani): RIFF/ACON, her kare tek resimli bir .cur.
/// `jiffies`: kare süresi (1/60 sn).
pub fn encode_ani(frames: &[Image], jiffies: u32) -> Vec<u8> {
    let mut anih = Vec::new();
    for v in [36, frames.len() as u32, frames.len() as u32, 0, 0, 0, 0, jiffies, 1] {
        anih.extend_from_slice(&v.to_le_bytes());
    }
    let mut list = b"fram".to_vec();
    for f in frames {
        chunk(&mut list, b"icon", &encode(std::slice::from_ref(f)));
    }
    let mut body = b"ACON".to_vec();
    chunk(&mut body, b"anih", &anih);
    chunk(&mut body, b"LIST", &list);
    let mut out = Vec::new();
    chunk(&mut out, b"RIFF", &body);
    out
}
