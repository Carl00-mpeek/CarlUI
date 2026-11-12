//! Sınırlı satır okuyucu. `BufRead::lines()` satır sonu gelene kadar sınırsız
//! bellek ayırır; kötü niyetli ya da bozuk bir uç tek satırla belleği
//! doldurabilirdi. Buradaki okuyucu `max` baytı aşan satırda hata döner ve
//! çağıran bağlantıyı düşürür.

use std::io::{self, BufRead};

/// Olay yolunda bir satırın en büyük boyu. Gerçek olaylar birkaç KB
/// (en büyükleri asistan yanıtı / ada notu metni); bol pay bırakıldı.
pub(crate) const MAX_LINE: usize = 256 * 1024;

/// Bir satır okur (`\n` ve varsa `\r` atılır). Akış bittiyse `Ok(None)`;
/// son satırda `\n` yoksa yine de döner. Satır `max`'ı aşarsa
/// `InvalidData` hatası — o zamana kadar en çok `max` bayt ayrılmış olur.
pub(crate) fn read_line<R: BufRead>(r: &mut R, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let avail = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if avail.is_empty() {
            return Ok((!out.is_empty()).then_some(out));
        }
        let (take, done) = match avail.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (avail.len(), false),
        };
        if out.len() + take > max {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "satır çok uzun"));
        }
        out.extend_from_slice(&avail[..take]);
        r.consume(if done { take + 1 } else { take });
        if done {
            if out.last() == Some(&b'\r') {
                out.pop();
            }
            return Ok(Some(out));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn all(data: &[u8], cap: usize, max: usize) -> Vec<io::Result<Option<Vec<u8>>>> {
        let mut r = BufReader::with_capacity(cap, Cursor::new(data.to_vec()));
        let mut v = Vec::new();
        loop {
            let x = read_line(&mut r, max);
            let stop = !matches!(x, Ok(Some(_)));
            v.push(x);
            if stop {
                return v;
            }
        }
    }

    #[test]
    fn satirlari_boler() {
        let v = all(b"a\r\nbc\n\nson", 2, 16);
        let got: Vec<_> = v.into_iter().map(|x| x.unwrap()).collect();
        assert_eq!(got, vec![Some(b"a".to_vec()), Some(b"bc".to_vec()), Some(Vec::new()), Some(b"son".to_vec()), None]);
    }

    #[test]
    fn bos_akis() {
        assert!(matches!(all(b"", 8, 8).as_slice(), [Ok(None)]));
    }

    #[test]
    fn sinir_tam_gecer_fazlasi_reddedilir() {
        let ok = all(b"12345678\nx", 3, 8);
        assert_eq!(ok[0].as_ref().unwrap().as_deref(), Some(&b"12345678"[..]));
        let bad = all(b"123456789\n", 3, 8);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].as_ref().unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn satir_sonu_gelmeyen_uzun_akis_durur() {
        // Sonsuz akış: sınır aşılınca hemen hata, bellek sınırlı.
        let mut r = BufReader::with_capacity(1024, io::repeat(b'x'));
        assert_eq!(read_line(&mut r, 64 * 1024).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
