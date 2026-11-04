//! Bayt dizilerini kasa dosyasında onaltılık metin olarak yazar (serde `with`).

use serde::{de::Error, Deserialize, Deserializer, Serializer};

pub fn encode(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

pub fn decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&encode(b))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = String::deserialize(d)?;
    decode(&s).ok_or_else(|| D::Error::custom("hex"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let b = [0u8, 1, 0xAB, 0xFF];
        assert_eq!(super::encode(&b), "0001abff");
        assert_eq!(super::decode("0001abff").unwrap(), b);
        assert!(super::decode("0g").is_none());
        assert!(super::decode("abc").is_none());
    }
}
