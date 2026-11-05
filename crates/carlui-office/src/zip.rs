//! Bu modül "Office dosyalarını (docx/xlsx/pptx = zip) değiştirirken
//! dokunmadığımız parçalar bozulmasın ve kaydetme hızlı olsun" sorununu
//! çözüyor: arşiv bir kez okunur, her girdinin sıkıştırılmış baytları olduğu
//! gibi saklanır. Kaydederken yalnızca değiştirilen girdiler yeniden
//! sıkıştırılır; geri kalanı ham kopyalanır (resimler, yazı tipleri…).
//! Zip64 desteklenmez (Office belgeleri için gerekmez).

use std::fmt;

#[derive(Debug)]
pub struct ZipError(pub String);

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(s: &str) -> Result<T, ZipError> {
    Err(ZipError(s.to_string()))
}

#[derive(Clone)]
enum Data {
    /// Dosyadaki gibi: (yöntem, crc, açık boyut, sıkıştırılmış baytlar).
    Raw { method: u16, crc: u32, size: u32, bytes: Vec<u8> },
    /// Yeni ya da değişmiş içerik; yazarken sıkıştırılır.
    Plain(Vec<u8>),
}

#[derive(Clone)]
struct Entry {
    name: String,
    data: Data,
}

#[derive(Clone, Default)]
pub struct Zip {
    entries: Vec<Entry>,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

pub fn crc32(data: &[u8]) -> u32 {
    let t = CRC_TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *v = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

impl Zip {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(b: &[u8]) -> Result<Self, ZipError> {
        // Merkezi dizinin sonu (EOCD) dosyanın son 64 KB'ında.
        let min = b.len().saturating_sub(22 + 65_535);
        let eocd = (min..b.len().saturating_sub(21)).rev().find(|&i| u32_at(b, i) == Some(0x0605_4b50));
        let Some(eocd) = eocd else { return err("zip dizini bulunamadı") };
        let count = u16_at(b, eocd + 10).unwrap_or(0) as usize;
        let mut p = u32_at(b, eocd + 16).unwrap_or(0) as usize;
        if p == 0xFFFF_FFFF {
            return err("zip64 desteklenmiyor");
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(b, p) != Some(0x0201_4b50) {
                return err("bozuk zip dizini");
            }
            let flags = u16_at(b, p + 8).unwrap_or(0);
            let method = u16_at(b, p + 10).unwrap_or(0);
            let crc = u32_at(b, p + 16).unwrap_or(0);
            let csize = u32_at(b, p + 20).unwrap_or(0) as usize;
            let size = u32_at(b, p + 24).unwrap_or(0);
            let nlen = u16_at(b, p + 28).unwrap_or(0) as usize;
            let xlen = u16_at(b, p + 30).unwrap_or(0) as usize;
            let clen = u16_at(b, p + 32).unwrap_or(0) as usize;
            let local = u32_at(b, p + 42).unwrap_or(0) as usize;
            let raw_name = b.get(p + 46..p + 46 + nlen).unwrap_or_default();
            let name = if flags & 0x800 != 0 { String::from_utf8_lossy(raw_name).into_owned() } else { raw_name.iter().map(|&c| c as char).collect() };
            p += 46 + nlen + xlen + clen;
            if u32_at(b, local) != Some(0x0403_4b50) {
                return err("bozuk zip girdisi");
            }
            let lname = u16_at(b, local + 26).unwrap_or(0) as usize;
            let lextra = u16_at(b, local + 28).unwrap_or(0) as usize;
            let start = local + 30 + lname + lextra;
            let Some(bytes) = b.get(start..start + csize) else { return err("zip girdisi kesik") };
            if name.ends_with('/') {
                continue;
            }
            entries.push(Entry { name, data: Data::Raw { method, crc, size, bytes: bytes.to_vec() } });
        }
        Ok(Self { entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.iter().find(|e| e.name == name)?;
        match &e.data {
            Data::Plain(v) => Some(v.clone()),
            Data::Raw { method: 0, bytes, .. } => Some(bytes.clone()),
            Data::Raw { method: 8, bytes, size, .. } => miniz_oxide::inflate::decompress_to_vec_with_limit(bytes, (*size as usize).max(1) * 2 + 1024).ok(),
            Data::Raw { .. } => None,
        }
    }

    pub fn read_str(&self, name: &str) -> Option<String> {
        let b = self.read(name)?;
        let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&b);
        Some(String::from_utf8_lossy(b).into_owned())
    }

    pub fn set(&mut self, name: &str, data: Vec<u8>) {
        match self.entries.iter_mut().find(|e| e.name == name) {
            Some(e) => e.data = Data::Plain(data),
            None => self.entries.push(Entry { name: name.to_string(), data: Data::Plain(data) }),
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|e| e.name != name);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for e in &self.entries {
            let (method, crc, size, bytes) = match &e.data {
                Data::Raw { method, crc, size, bytes } => (*method, *crc, *size, std::borrow::Cow::Borrowed(bytes.as_slice())),
                Data::Plain(v) => (8u16, crc32(v), v.len() as u32, std::borrow::Cow::Owned(miniz_oxide::deflate::compress_to_vec(v, 6))),
            };
            let name = e.name.as_bytes();
            let offset = out.len() as u32;
            let head = |sig: u32, out: &mut Vec<u8>, central: bool| {
                out.extend_from_slice(&sig.to_le_bytes());
                if central {
                    out.extend_from_slice(&20u16.to_le_bytes());
                }
                out.extend_from_slice(&20u16.to_le_bytes());
                out.extend_from_slice(&0x800u16.to_le_bytes());
                out.extend_from_slice(&method.to_le_bytes());
                // Saat/tarih: 1980-01-01 00:00.
                out.extend_from_slice(&0u16.to_le_bytes());
                out.extend_from_slice(&0x21u16.to_le_bytes());
                out.extend_from_slice(&crc.to_le_bytes());
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(&size.to_le_bytes());
                out.extend_from_slice(&(name.len() as u16).to_le_bytes());
                out.extend_from_slice(&0u16.to_le_bytes());
            };
            head(0x0403_4b50, &mut out, false);
            out.extend_from_slice(name);
            out.extend_from_slice(&bytes);
            head(0x0201_4b50, &mut central, true);
            central.extend_from_slice(&[0u8; 6]);
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let cd_start = out.len() as u32;
        out.extend_from_slice(&central);
        let n = self.entries.len() as u16;
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

/// Zip içindeki bir parçaya göre göreli yolu çözer: ("word/document.xml",
/// "media/a.png") → "word/media/a.png"; "/ppt/x.xml" kökten.
pub fn resolve(base: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Bir parçanın ilişki dosyasının yolu: "ppt/slides/slide1.xml" → "ppt/slides/_rels/slide1.xml.rels".
pub fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut z = Zip::new();
        z.set("a.txt", b"merhaba dunya merhaba dunya".to_vec());
        z.set("klasör/b.xml", b"<x/>".to_vec());
        let bytes = z.to_bytes();
        let z2 = Zip::parse(&bytes).unwrap();
        assert_eq!(z2.read("a.txt").unwrap(), b"merhaba dunya merhaba dunya");
        assert_eq!(z2.read_str("klasör/b.xml").unwrap(), "<x/>");
        // Ham kopya: değişmeyen girdi yeniden yazılınca aynı kalır.
        let z3 = Zip::parse(&z2.to_bytes()).unwrap();
        assert_eq!(z3.read("a.txt").unwrap(), b"merhaba dunya merhaba dunya");
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn paths() {
        assert_eq!(resolve("ppt/slides/slide1.xml", "../slideLayouts/slideLayout2.xml"), "ppt/slideLayouts/slideLayout2.xml");
        assert_eq!(resolve("word/document.xml", "media/image1.png"), "word/media/image1.png");
        assert_eq!(resolve("xl/workbook.xml", "/xl/worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(rels_path("xl/workbook.xml"), "xl/_rels/workbook.xml.rels");
    }
}
