//! Bu modül "PawnIO yokken, kullanıcı zaten HWiNFO çalıştırıyorsa sıcaklığı
//! ondan alalım" sorununu çözüyor. HWiNFO'da "Shared Memory Support" açıkken
//! ölçümler `Global\HWiNFO_SENS_SM2` paylaşılan belleğine yazılır; bu bellek
//! yetkisiz süreçlerce okunabilir. CarlUI ek bir sürücü kurmaz, yalnızca
//! okur. Intel ve AMD işlemcilerde doğru (çekirdeğin kendi algılayıcısı)
//! değer verir.

use super::{Source, Temps};
use windows::{
    core::*,
    Win32::{Foundation::*, System::Memory::*},
};

const NAME: PCWSTR = w!("Global\\HWiNFO_SENS_SM2");
/// "HWiS" (küçük uçlu).
const SIGNATURE: u32 = 0x5369_5748;
/// Ölçüm türü: sıcaklık.
const TEMP: u32 = 1;
/// Okuma öğesinde değerin (f64) konumu: tür, algılayıcı, kimlik (3 × 4) +
/// iki etiket (2 × 128) + birim (16).
const VALUE_AT: usize = 12 + 128 + 128 + 16;
/// HWiNFO bu kadar saniyedir yazmıyorsa (kapalı / donmuş) yok sayılır.
const STALE_SECS: i64 = 15;

/// Paket sıcaklığının etiketleri, öncelik sırasıyla (Intel, AMD).
const PACKAGE: &[&str] = &["CPU Package", "CPU (Tctl/Tdie)", "CPU Die (average)", "CPU (Tdie)", "CPU (Tctl)"];

pub fn read() -> Option<Temps> {
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ.0, false, NAME).ok()?;
        let v = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        let out = if v.Value.is_null() {
            None
        } else {
            let mut info = MEMORY_BASIC_INFORMATION::default();
            let len = VirtualQuery(Some(v.Value), &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
            let out = (len != 0).then(|| std::slice::from_raw_parts(v.Value as *const u8, info.RegionSize)).and_then(|buf| parse(buf, now_unix()));
            let _ = UnmapViewOfFile(v);
            out
        };
        let _ = CloseHandle(h);
        out
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn label(b: &[u8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end]).unwrap_or("")
}

/// "Core 3", "P-core 0", "E-core 12" → çekirdek numarası.
fn core_no(label: &str) -> Option<u32> {
    let rest = ["P-core ", "E-core ", "Core "].iter().find_map(|p| label.strip_prefix(p))?;
    (!rest.is_empty() && rest.bytes().all(|c| c.is_ascii_digit())).then(|| rest.parse().ok()).flatten()
}

/// Paylaşılan belleğin içeriğini çözer (bkz. HWiNFO SDK, HWiNFO_SENSORS_SHARED_MEM2).
fn parse(b: &[u8], now: i64) -> Option<Temps> {
    if u32_at(b, 0)? != SIGNATURE {
        return None;
    }
    let poll = i64::from_le_bytes(b.get(12..20)?.try_into().ok()?);
    if (now - poll).abs() > STALE_SECS {
        return None;
    }
    let (off, size, n) = (u32_at(b, 32)? as usize, u32_at(b, 36)? as usize, u32_at(b, 40)? as usize);
    if size < VALUE_AT + 8 {
        return None;
    }
    let mut package: Option<(usize, f32)> = None;
    let mut cores: Vec<(u32, f32)> = Vec::new();
    for i in 0..n.min(4096) {
        let Some(e) = b.get(off + i * size..off + (i + 1) * size) else { break };
        if u32_at(e, 0) != Some(TEMP) {
            continue;
        }
        let name = label(&e[12..140]);
        let value = f64::from_le_bytes(e[VALUE_AT..VALUE_AT + 8].try_into().ok()?) as f32;
        if !(1.0..=125.0).contains(&value) {
            continue;
        }
        if let Some(rank) = PACKAGE.iter().position(|p| *p == name) {
            if package.map_or(true, |(r, _)| rank < r) {
                package = Some((rank, value));
            }
        } else if let Some(no) = core_no(name) {
            if !cores.iter().any(|c| c.0 == no) {
                cores.push((no, value));
            }
        }
    }
    cores.sort_by_key(|c| c.0);
    let cores: Vec<f32> = cores.into_iter().map(|c| c.1).collect();
    let package = package.map(|p| p.1).or_else(|| cores.iter().copied().reduce(f32::max))?;
    Some(Temps { package, cores, source: Source::Hwinfo })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = VALUE_AT + 32;

    fn reading(kind: u32, name: &str, value: f64) -> Vec<u8> {
        let mut e = vec![0u8; SIZE];
        e[0..4].copy_from_slice(&kind.to_le_bytes());
        e[12..12 + name.len()].copy_from_slice(name.as_bytes());
        e[VALUE_AT..VALUE_AT + 8].copy_from_slice(&value.to_le_bytes());
        e
    }

    fn memory(poll: i64, rows: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0u8; 44];
        b[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        b[12..20].copy_from_slice(&poll.to_le_bytes());
        b[32..36].copy_from_slice(&44u32.to_le_bytes());
        b[36..40].copy_from_slice(&(SIZE as u32).to_le_bytes());
        b[40..44].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        rows.iter().for_each(|r| b.extend_from_slice(r));
        b
    }

    #[test]
    fn paket_ve_cekirdekler_okunur() {
        let b = memory(1000, &[reading(TEMP, "E-core 4", 48.0), reading(TEMP, "P-core 0", 55.0), reading(6, "P-core 1", 4800.0), reading(TEMP, "CPU Package", 61.5), reading(TEMP, "Core 0 Distance to TjMAX", 45.0)]);
        let t = parse(&b, 1002).unwrap();
        assert_eq!(t.package, 61.5);
        assert_eq!(t.cores, vec![55.0, 48.0]);
        assert_eq!(t.source, Source::Hwinfo);
    }

    #[test]
    fn amd_etiketi_ve_paketsiz_durum() {
        let t = parse(&memory(10, &[reading(TEMP, "CPU (Tctl/Tdie)", 70.0)]), 10).unwrap();
        assert_eq!(t.package, 70.0);
        let t = parse(&memory(10, &[reading(TEMP, "Core 0", 40.0), reading(TEMP, "Core 1", 44.0)]), 10).unwrap();
        assert_eq!(t.package, 44.0);
    }

    #[test]
    fn eski_bozuk_ya_da_bos_bellek_yok_sayilir() {
        let rows = [reading(TEMP, "CPU Package", 60.0)];
        assert!(parse(&memory(0, &rows), 100).is_none());
        let mut bad = memory(5, &rows);
        bad[0] = 0;
        assert!(parse(&bad, 5).is_none());
        assert!(parse(&memory(5, &[]), 5).is_none());
        assert!(parse(&memory(5, &rows)[..60], 5).is_none());
        assert!(parse(&[], 5).is_none());
    }
}
