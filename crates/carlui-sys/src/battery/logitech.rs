//! Logitech HID++ 2.0: Unifying / Bolt / Lightspeed alıcılarına bağlı ya da
//! kabloyla takılı fare, klavye ve kulaklıklar.
//!
//! Model tablosu gerekmez: cihaza "şu özellik hangi sırada?" diye sorulur
//! (kök özellik 0x0000) ve desteklediği pil özelliği okunur:
//! - 0x1004 Unified Battery → yüzde + şarj durumu
//! - 0x1000 Battery Status  → yüzde + şarj durumu
//! - 0x1001 Battery Voltage → mV (Li-ion eğrisiyle yüzdeye çevrilir)
//!
//! Ad ve tür 0x0005 Device Name & Type özelliğinden gelir. Mesajlar uzun HID++
//! raporudur: `[0x11, cihaz, özellik_sırası, işlev<<4 | yazılım_kimliği, …]`.

use super::*;
use std::{collections::HashMap, sync::Mutex};

const VID: &str = "vid_046d";
const LONG: u8 = 0x11;
const SW_ID: u8 = 0x0A;
const ROOT: u8 = 0x00;
/// Doğrudan bağlı (kablolu) cihaz; alıcı arkasındakiler 1..=6.
const DIRECT: u8 = 0xFF;

const F_NAME: u16 = 0x0005;
const F_UNIFIED: u16 = 0x1004;
const F_STATUS: u16 = 0x1000;
const F_VOLTAGE: u16 = 0x1001;

/// Alıcı arkasındaki cihaz numaraları yavaş taranır (her boş numara bir zaman
/// aşımı); bulunanlar hatırlanır, tam tarama ara sıra yinelenir.
const RESCAN: Duration = Duration::from_secs(600);
const TIMEOUT: u64 = 300;

#[derive(Clone)]
struct Known {
    index: u8,
    name: String,
    kind: Kind,
}

static CACHE: Mutex<Option<HashMap<String, (Instant, Vec<Known>)>>> = Mutex::new(None);

/// Tek bir HID++ sorusu: cevabın parametre baytları (`r[4..]`) ya da hata.
fn call(d: &Dev, i: &Iface, dev: u8, feat: u8, func: u8, params: &[u8]) -> Option<Vec<u8>> {
    let mut req = vec![0u8; i.out_len.max(20)];
    req[..4].copy_from_slice(&[LONG, dev, feat, (func << 4) | SW_ID]);
    req[4..4 + params.len()].copy_from_slice(params);
    let ok = d.ask(&req, i.in_len, TIMEOUT, |r: &[u8]| {
        if r.len() < 7 || r[1] != dev {
            return None;
        }
        // HID++ 2.0 hatası: [.., 0xFF, özellik, işlev, kod]; 1.0 hatası: 0x8F.
        if (r[2] == 0xFF && r[3] == feat) || r[2] == 0x8F {
            return Some(None);
        }
        (r[0] == LONG && r[2] == feat && r[3] == (func << 4) | SW_ID).then(|| Some(r[4..].to_vec()))
    });
    ok.flatten()
}

/// Özelliğin sıra numarası (yoksa `None`).
fn feature(d: &Dev, i: &Iface, dev: u8, id: u16) -> Option<u8> {
    let r = call(d, i, dev, ROOT, 0, &[(id >> 8) as u8, id as u8])?;
    (r[0] != 0).then_some(r[0])
}

fn name_and_kind(d: &Dev, i: &Iface, dev: u8) -> (String, Kind) {
    let Some(f) = feature(d, i, dev, F_NAME) else { return ("Logitech".into(), Kind::Other) };
    let len = call(d, i, dev, f, 0, &[]).map_or(0, |r| r[0] as usize);
    let mut name = Vec::new();
    while name.len() < len.min(64) {
        let Some(r) = call(d, i, dev, f, 1, &[name.len() as u8]) else { break };
        let chunk: Vec<u8> = r.iter().copied().take(len - name.len()).take_while(|&b| b != 0).collect();
        if chunk.is_empty() {
            break;
        }
        name.extend(chunk);
    }
    let name = String::from_utf8_lossy(&name).trim().to_string();
    // 3 fare, 4 dokunmatik yüzey, 5 iztopu, 8 kulaklık.
    let kind = match call(d, i, dev, f, 2, &[]).map(|r| r[0]) {
        Some(3..=5) => Kind::Mouse,
        Some(8) => Kind::Headset,
        _ => kind_of(&name),
    };
    (if name.is_empty() { "Logitech".into() } else { name }, kind)
}

/// Li-ion hücre gerilimi → yaklaşık yüzde.
fn volt_pct(mv: u16) -> u8 {
    const CURVE: [(u16, u8); 11] =
        [(4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50), (3778, 40), (3751, 30), (3717, 20), (3671, 10), (3500, 0)];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((hv, hp), (lv, lp)) = (w[0], w[1]);
        if mv >= lv {
            return lp + ((mv - lv) as u32 * (hp - lp) as u32 / (hv - lv) as u32) as u8;
        }
    }
    0
}

/// Unified Battery seviye bitleri (yüzde bildirmeyen cihazlar için).
fn level_pct(bits: u8) -> u8 {
    match bits {
        b if b & 8 != 0 => 100,
        b if b & 4 != 0 => 50,
        b if b & 2 != 0 => 20,
        b if b & 1 != 0 => 5,
        _ => 0,
    }
}

/// (yüzde, şarjda); cihaz uyuyorsa `None`.
fn battery(d: &Dev, i: &Iface, dev: u8) -> Option<(u8, bool)> {
    if let Some(f) = feature(d, i, dev, F_UNIFIED) {
        let r = call(d, i, dev, f, 1, &[])?;
        let pct = if r[0] > 0 { r[0].min(100) } else { level_pct(r[1]) };
        return Some((pct, matches!(r[2], 1..=3)));
    }
    if let Some(f) = feature(d, i, dev, F_STATUS) {
        let r = call(d, i, dev, f, 0, &[])?;
        return Some((r[0].min(100), matches!(r[2], 1..=4)));
    }
    if let Some(f) = feature(d, i, dev, F_VOLTAGE) {
        let r = call(d, i, dev, f, 0, &[])?;
        return Some((volt_pct(u16::from_be_bytes([r[0], r[1]])), r[2] & 0x80 != 0));
    }
    None
}

/// Bu koleksiyonun arkasında HID++ 2.0 konuşan cihazlar.
fn discover(d: &Dev, i: &Iface) -> Vec<Known> {
    let alive = |dev: u8| call(d, i, dev, ROOT, 0, &[0, 0]).is_some();
    let indexes: Vec<u8> = if alive(DIRECT) { vec![DIRECT] } else { (1..=6).filter(|&n| alive(n)).collect() };
    indexes
        .into_iter()
        .map(|index| {
            let (name, kind) = name_and_kind(d, i, index);
            Known { index, name, kind }
        })
        .collect()
}

pub(super) fn read() -> Vec<Reading> {
    let mut out = Vec::new();
    let all = interfaces(VID);
    // Uzun HID++ raporu taşıyan koleksiyonlar (kulaklıklarda 0xFF43:0x0202).
    let cands = all.iter().filter(|i| matches!((i.page, i.usage), (0xFF00, 2) | (0xFF43, 0x0202)) && i.out_len >= 20 && i.in_len >= 7);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    for i in cands {
        if i.path.to_lowercase().contains("00001124-0000-1000-8000-00805f9b34fb") || i.path.to_lowercase().contains("00001812-0000-1000-8000-00805f9b34fb") {
            continue; // Bluetooth: Windows'tan zaten geliyor
        }
        let Some(d) = Dev::open(&i.path) else { continue };
        d.drain(i.in_len);
        let fresh = cache.get(&i.path).is_some_and(|(at, k)| !k.is_empty() && at.elapsed() < RESCAN);
        if !fresh {
            let found = discover(&d, i);
            // Uyuyan cihaz taramada görünmeyebilir: önceki liste korunur.
            if !found.is_empty() || !cache.contains_key(&i.path) {
                cache.insert(i.path.clone(), (Instant::now(), found));
            }
        }
        for k in cache.get(&i.path).map(|c| c.1.clone()).unwrap_or_default() {
            let got = battery(&d, i, k.index);
            out.push(Reading { kind: k.kind, name: k.name, percent: got.map(|g| g.0), charging: got.is_some_and(|g| g.1) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gerilim() {
        assert_eq!(volt_pct(4200), 100);
        assert_eq!(volt_pct(3811), 50);
        assert_eq!(volt_pct(3400), 0);
        let mid = volt_pct(3840);
        assert!((50..60).contains(&mid), "{mid}");
    }

    #[test]
    fn seviye_bitleri() {
        assert_eq!(level_pct(0b1000), 100);
        assert_eq!(level_pct(0b0100), 50);
        assert_eq!(level_pct(0), 0);
    }
}
