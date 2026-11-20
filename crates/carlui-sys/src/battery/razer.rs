//! Razer kablosuz fare ve kulaklıklar (VID 1532).
//!
//! Razer cihazları 90 baytlık özellik raporuyla konuşur (önünde rapor kimliği 0):
//! `[durum, işlem_kimliği, kalan×2, protokol, veri_boyu, sınıf, komut, arg×80, sağlama, 0]`.
//! Sağlama 2..88 baytlarının XOR'udur. Güç sınıfı 0x07: 0x80 pil (arg[1], 0..255),
//! 0x84 şarj durumu (arg[1] = 1 şarjda). Durum 0x02 başarı, 0x01 meşgul,
//! 0x04 cihaz uyuyor, 0x05 desteklenmiyor. İşlem kimliği modele göre değişir
//! (0x1F, 0x3F, 0xFF); doğru olan denenerek bulunur ve hatırlanır.

use super::*;
use std::{collections::HashMap, sync::Mutex};

const VID: &str = "vid_1532";
const FEATURE_LEN: usize = 91;
const TIDS: [u8; 3] = [0x1F, 0x3F, 0xFF];
const CLASS_POWER: u8 = 0x07;
const CMD_BATTERY: u8 = 0x80;
const CMD_CHARGING: u8 = 0x84;

const OK: u8 = 0x02;
const BUSY: u8 = 0x01;
const UNSUPPORTED: u8 = 0x05;

/// Yol başına: çalışan işlem kimliği ya da `None` (pil yok); zamanı.
static CACHE: Mutex<Option<HashMap<String, (Instant, Option<u8>)>>> = Mutex::new(None);
const RECHECK: Duration = Duration::from_secs(600);

fn packet(tid: u8, cmd: u8) -> [u8; FEATURE_LEN] {
    let mut b = [0u8; FEATURE_LEN];
    let p = &mut b[1..]; // b[0] rapor kimliği
    p[1] = tid;
    p[5] = 0x02; // veri boyu
    p[6] = CLASS_POWER;
    p[7] = cmd;
    p[88] = p[2..88].iter().fold(0, |a, x| a ^ x);
    b
}

/// Yanıt: (durum, arg[1]) — işlem kimliği ve komut eşleşirse.
fn parse(r: &[u8], tid: u8, cmd: u8) -> Option<(u8, u8)> {
    let p = r.get(1..)?;
    (p.len() >= 10 && p[1] == tid && p[6] == CLASS_POWER && p[7] == cmd).then(|| (p[0], p[9]))
}

fn query(h: HANDLE, tid: u8, cmd: u8) -> Option<(u8, u8)> {
    let mut req = packet(tid, cmd);
    for _ in 0..3 {
        unsafe {
            if !HidD_SetFeature(h, req.as_mut_ptr() as _, FEATURE_LEN as u32).as_bool() {
                return None;
            }
        }
        std::thread::sleep(Duration::from_millis(60));
        let mut resp = [0u8; FEATURE_LEN];
        let ok = unsafe { HidD_GetFeature(h, resp.as_mut_ptr() as _, FEATURE_LEN as u32).as_bool() };
        if !ok {
            return None;
        }
        // Eski bir cevap ya da meşgul: biraz bekleyip yeniden sor.
        match parse(&resp, tid, cmd) {
            Some((BUSY, _)) | None => std::thread::sleep(Duration::from_millis(40)),
            got => return got,
        }
    }
    None
}

/// Çalışan işlem kimliği: `Some(Some(tid))`, pil yok: `Some(None)`, cevap yok: `None`.
fn find_tid(h: HANDLE) -> Option<Option<u8>> {
    let mut answered = false;
    for tid in TIDS {
        match query(h, tid, CMD_BATTERY) {
            Some((OK, _)) => return Some(Some(tid)),
            Some((UNSUPPORTED, _)) => return Some(None),
            Some(_) => answered = true, // uyuyor vb.: kimlik doğru olabilir ama emin değiliz
            None => {}
        }
    }
    (!answered).then_some(None)
}

pub(super) fn read(seen: &mut Vec<String>) -> Vec<Reading> {
    let mut out: Vec<Reading> = Vec::new();
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    let mut done_pids: Vec<String> = Vec::new();
    for i in interfaces(VID).into_iter().filter(|i| i.feat_len == FEATURE_LEN) {
        let lower = i.path.to_lowercase();
        let Some(pid) = lower.find("pid_").map(|n| lower[n..n + 8].to_string()) else { continue };
        if done_pids.contains(&pid) {
            continue;
        }
        let Some(h) = open(&i.path, 0) else { continue };
        let tid = match cache.get(&i.path) {
            Some((at, t)) if at.elapsed() < RECHECK || t.is_some() => *t,
            _ => {
                let t = find_tid(h).flatten();
                cache.insert(i.path.clone(), (Instant::now(), t));
                t
            }
        };
        if let Some(tid) = tid {
            done_pids.push(pid.clone());
            let name = product_name(h);
            let percent = match query(h, tid, CMD_BATTERY) {
                Some((OK, v)) => Some(((v as u32 * 100 + 127) / 255) as u8),
                _ => None, // uyuyor
            };
            let charging = matches!(query(h, tid, CMD_CHARGING), Some((OK, 1)));
            seen.push(format!("{VID}&{pid}"));
            if !out.iter().any(|r| r.name == name) {
                let kind = match kind_of(&name) {
                    Kind::Other => Kind::Mouse, // Razer'ın pilli cihazlarının çoğu fare
                    k => k,
                };
                out.push(Reading { kind, name, percent, charging, charge_known: true });
            }
        }
        unsafe {
            let _ = CloseHandle(h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paket_saglamasi() {
        let b = packet(0x1F, CMD_BATTERY);
        assert_eq!(b[0], 0);
        assert_eq!(&b[1..9], &[0, 0x1F, 0, 0, 0, 0x02, 0x07, 0x80]);
        assert_eq!(b[89], 0x02 ^ 0x07 ^ 0x80);
    }

    #[test]
    fn yanit() {
        let mut r = [0u8; FEATURE_LEN];
        r[1..9].copy_from_slice(&[OK, 0x1F, 0, 0, 0, 0x02, 0x07, 0x80]);
        r[10] = 255;
        assert_eq!(parse(&r, 0x1F, CMD_BATTERY), Some((OK, 255)));
        assert_eq!(parse(&r, 0xFF, CMD_BATTERY), None);
    }
}
