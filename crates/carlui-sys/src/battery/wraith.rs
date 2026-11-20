//! Wraith W1 / W1 Freestyle (PixArt yongası, VID 093A).
//!
//! Alıcı 093A:522C, kablo 093A:622C. Aynı alıcı kimliğini başka markalar da
//! (ör. Incott) kullandığı için önce model sorulur. Hepsi FF05 sayfasındaki
//! koleksiyonlarda, rapor kimliği 0x09 ile konuşur:
//! - `SET_FEATURE [09 8F]` → `GET_FEATURE` yanıtı `[09 8F ? model]`; model 18 = W1,
//!   20 = W1 Freestyle (onluk).
//! - Fare pil durumunu kendiliğinden göndermez: beş tuşun ayarı sırayla
//!   (`[09 86 tuş]`, 0 1 2 4 3) okunduktan sonra giriş raporu olarak
//!   `[09 durum …]` yollar; durumun 7. biti şarjda, alt 7 biti yüzdedir.
//!
//! Protokol BatteryHub (Wireless-Device-Battery-Monitor) projesindeki gözlemlere
//! dayanır; kod bağımsız yazılmıştır.

use super::*;
use std::{collections::HashMap, sync::Mutex};

const VID: &str = "vid_093a";
/// (pid, bağlantı kablo mu)
const PIDS: [(&str, bool); 2] = [("pid_522c", false), ("pid_622c", true)];
const PAGE: u16 = 0xFF05;
const REPORT: u8 = 0x09;
const INFO: u8 = 0x8F;
const BUTTON: u8 = 0x86;
const SWEEP: [u8; 5] = [0, 1, 2, 4, 3];

/// PID başına bilinen model: uyuyan fare 8F'ye cevap vermese de W1 olarak kalır.
static MODELS: Mutex<Option<HashMap<&'static str, u8>>> = Mutex::new(None);

fn model_name(code: u8) -> Option<&'static str> {
    match code {
        18 => Some("Wraith W1"),
        20 => Some("Wraith W1 Freestyle"),
        _ => None,
    }
}

fn set(h: HANDLE, len: usize, body: &[u8]) -> bool {
    let mut b = vec![0u8; len.max(9)];
    b[0] = REPORT;
    b[1..1 + body.len()].copy_from_slice(body);
    unsafe { HidD_SetFeature(h, b.as_mut_ptr() as _, b.len() as u32).as_bool() }
}

fn get(h: HANDLE, len: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; len.max(9)];
    b[0] = REPORT;
    unsafe { HidD_GetFeature(h, b.as_mut_ptr() as _, b.len() as u32).as_bool() }.then_some(b)
}

/// 8F yanıtı: model kodu (yanıt bu soruya aitse).
fn info_parse(r: &[u8]) -> Option<u8> {
    (r.len() >= 4 && r[0] == REPORT && r[1] == INFO).then(|| r[3])
}

/// Durum raporu: (yüzde, şarjda).
fn status_parse(r: &[u8]) -> Option<(u8, bool)> {
    let s = *r.get(1)?;
    (r[0] == REPORT && (s & 0x7F) <= 100).then_some((s & 0x7F, s & 0x80 != 0))
}

/// Model kodu; cevap yoksa `None`.
fn ask_model(h: HANDLE, len: usize) -> Option<u8> {
    if !set(h, len, &[INFO]) {
        return None;
    }
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(m) = info_parse(&get(h, len)?) {
            return Some(m);
        }
    }
    None
}

/// Bir tuşun ayarını okur (yalnızca fareyi durum göndermeye dürtmek için).
fn read_button(h: HANDLE, len: usize, button: u8) -> bool {
    if !set(h, len, &[BUTTON, button]) {
        return false;
    }
    for wait in [150, 20] {
        std::thread::sleep(Duration::from_millis(wait));
        match get(h, len) {
            Some(r) if r[1] == BUTTON => break,
            Some(_) => {}
            None => break,
        }
    }
    true
}

/// Giriş koleksiyonlarını `ms` boyunca dinler.
fn listen(ins: &[(Dev, usize)], ms: u64) -> Option<(u8, bool)> {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        for (d, len) in ins {
            if let Some(v) = d.read(*len, 50).as_deref().and_then(status_parse) {
                return Some(v);
            }
        }
    }
    None
}

fn read_one(pid: &'static str, cable: bool, models: &mut HashMap<&'static str, u8>) -> Option<Reading> {
    let all: Vec<Iface> = interfaces(VID).into_iter().filter(|i| i.page == PAGE && i.path.to_lowercase().contains(pid)).collect();
    if all.is_empty() {
        return None;
    }
    // Kabloda ilk FF05 koleksiyonu SET_FEATURE'ı reddeder: sondan başlanır.
    let mut feat: Option<(HANDLE, usize)> = None;
    let mut model = models.get(pid).copied();
    for i in all.iter().rev().filter(|i| i.feat_len >= 9) {
        let Some(h) = open(&i.path, 0) else { continue };
        match ask_model(h, i.feat_len) {
            Some(m) if model_name(m).is_some() => {
                model = Some(m);
                feat = Some((h, i.feat_len));
                break;
            }
            // Başka bir PixArt faresi (aynı alıcı kimliği): bizim değil.
            Some(m) => {
                carlui_core::info!("wraith: {pid} model kodu {m} tanınmadı");
                unsafe {
                    let _ = CloseHandle(h);
                }
                models.remove(pid);
                return None;
            }
            None => unsafe {
                let _ = CloseHandle(h);
            },
        }
    }
    let model = model?;
    models.insert(pid, model);
    let mut reading = Reading { kind: Kind::Mouse, name: model_name(model)?.into(), percent: None, charging: false, charge_known: true };
    let Some((h, flen)) = feat else { return Some(reading) };

    let ins: Vec<(Dev, usize)> = all
        .iter()
        .filter(|i| i.in_len >= 2)
        .filter_map(|i| Dev::open(&i.path).or_else(|| open(&i.path, GENERIC_READ.0).map(Dev)).map(|d| (d, i.in_len)))
        .collect();
    for (d, len) in &ins {
        d.drain(*len);
    }
    if !ins.is_empty() {
        std::thread::sleep(Duration::from_millis(200));
        let mut got = SWEEP.iter().all(|&b| read_button(h, flen, b)).then(|| listen(&ins, 400)).flatten();
        // Fare bazen geç cevap verir: tuş 0 ile birkaç kez daha dürtülür.
        for _ in 0..4 {
            if got.is_some() || !read_button(h, flen, 0) {
                break;
            }
            got = listen(&ins, 550);
        }
        if let Some((p, c)) = got {
            // Alıcıda 0 ve şarjsız: fare kapalı / menzil dışı.
            if cable || p > 0 || c {
                reading.percent = Some(p);
                reading.charging = c || cable;
            }
        }
    }
    unsafe {
        let _ = CloseHandle(h);
    }
    Some(reading)
}

pub(super) fn read(seen: &mut Vec<String>) -> Vec<Reading> {
    let mut guard = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let models = guard.get_or_insert_with(HashMap::new);
    let mut out: Vec<Reading> = Vec::new();
    for (pid, cable) in PIDS {
        if let Some(r) = read_one(pid, cable, models) {
            seen.push(format!("{VID}&{pid}"));
            // Kablo takılıyken alıcı da görünebilir: aynı fare iki kez sayılmasın.
            if let Some(prev) = out.iter_mut().find(|x| x.name == r.name) {
                if prev.percent.is_none() {
                    *prev = r;
                }
            } else {
                out.push(r);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_yaniti() {
        assert_eq!(info_parse(&[0x09, 0x8F, 0x00, 18, 0, 0, 0, 0, 0]), Some(18));
        assert_eq!(info_parse(&[0x09, 0x86, 0x00, 18]), None);
        assert_eq!(model_name(20), Some("Wraith W1 Freestyle"));
        assert_eq!(model_name(0x18), None);
    }

    #[test]
    fn durum_raporu() {
        assert_eq!(status_parse(&[0x09, 0x80 | 64, 0x31]), Some((64, true)));
        assert_eq!(status_parse(&[0x09, 87]), Some((87, false)));
        assert_eq!(status_parse(&[0x09, 0xFF]), None);
        assert_eq!(status_parse(&[0x08, 50]), None);
    }
}
