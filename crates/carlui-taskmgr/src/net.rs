//! Uygulama başına ağ kullanımı. Windows süreç başına ağ baytını hazır
//! sayaçla vermez; çekirdeğin "Microsoft-Windows-Kernel-Network" ETW
//! sağlayıcısı ise her TCP/UDP gönderme ve alma olayında süreç kimliğini ve
//! bayt sayısını yazar. Gerçek zamanlı bir ETW oturumu açıp bu olayları süreç
//! başına toplarız; `procs` iki ölçüm arasındaki farktan hız hesaplar.
//!
//! ETW oturumu yalnızca yönetici yetkisiyle açılabilir; yetki yoksa hiç
//! denenmez (sütun "Yönetici olarak açın" der). Oturum adı sabittir: önceki
//! bir kopya çökerek açık bıraktıysa başlarken kapatılır. Yalnızca yerel
//! sayaçtır: hiçbir veri dışarı gönderilmez, paket içeriği okunmaz.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use windows::{
    core::{GUID, PCWSTR, PWSTR},
    Win32::{
        Foundation::ERROR_SUCCESS,
        System::Diagnostics::Etw::{
            CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
            PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
        },
    },
};

const SESSION: &str = "CarlUI-TaskMgr-Net";
/// Microsoft-Windows-Kernel-Network.
const PROVIDER: GUID = GUID::from_u128(0x7DD42A49_5329_4832_8DFD_43D979153A88);
/// KERNEL_NETWORK_KEYWORD_IPV4 | KERNEL_NETWORK_KEYWORD_IPV6.
const KEYWORDS: u64 = 0x10 | 0x20;
const LEVEL_INFO: u8 = 4;

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Süreç başına toplam bayt (oturum başından beri).
static BYTES: Mutex<Option<HashMap<u32, u64>>> = Mutex::new(None);
static HANDLE: Mutex<u64> = Mutex::new(0);

/// Olay kimliği bir veri gönderme/alma olayı mı (TCP/UDP, IPv4/IPv6).
/// Bu olayların hepsinde yük `PID: u32, size: u32` ile başlar.
pub fn is_transfer(id: u16) -> bool {
    matches!(id, 10 | 11 | 26 | 27 | 42 | 43 | 58 | 59)
}

/// Olay yükünden (pid, bayt).
pub fn parse(data: &[u8]) -> Option<(u32, u32)> {
    let pid = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?);
    let size = u32::from_le_bytes(data.get(4..8)?.try_into().ok()?);
    Some((pid, size))
}

pub fn add(map: &mut HashMap<u32, u64>, pid: u32, size: u32) {
    *map.entry(pid).or_default() += size as u64;
}

unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    let Some(r) = rec.as_ref() else { return };
    if r.EventHeader.ProviderId != PROVIDER || !is_transfer(r.EventHeader.EventDescriptor.Id) || r.UserData.is_null() {
        return;
    }
    let data = std::slice::from_raw_parts(r.UserData as *const u8, r.UserDataLength as usize);
    if let Some((pid, size)) = parse(data) {
        if let Ok(mut m) = BYTES.lock() {
            add(m.get_or_insert_with(HashMap::new), pid, size);
        }
    }
}

/// EVENT_TRACE_PROPERTIES ve ardındaki oturum adı için hizalı tampon.
fn props() -> Vec<u64> {
    let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + 2 * (SESSION.len() + 1) + 64;
    let mut buf = vec![0u64; size.div_ceil(8)];
    let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
    unsafe {
        (*p).Wnode.BufferSize = (buf.len() * 8) as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).Wnode.ClientContext = 1; // QPC zaman damgası
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).FlushTimer = 1;
        (*p).LoggerNameOffset = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    }
    buf
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Oturumu açar ve olay işleme iş parçacığını başlatır. Yalnızca yönetici
/// yetkisiyle çağrılmalı; başarısızsa false (sütun boş kalır).
pub fn start() -> bool {
    if ACTIVE.load(Ordering::Relaxed) {
        return true;
    }
    let name = wide(SESSION);
    unsafe {
        // Önceki bir kopyadan kalan aynı adlı oturum.
        let mut stale = props();
        let _ = ControlTraceW(CONTROLTRACE_HANDLE::default(), PCWSTR(name.as_ptr()), stale.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
        let mut buf = props();
        let mut h = CONTROLTRACE_HANDLE::default();
        let e = StartTraceW(&mut h, PCWSTR(name.as_ptr()), buf.as_mut_ptr() as *mut _);
        if e != ERROR_SUCCESS {
            carlui_core::warn!("ağ izleme oturumu açılamadı: {}", e.0);
            return false;
        }
        let e = EnableTraceEx2(h, &PROVIDER, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0, LEVEL_INFO, KEYWORDS, 0, 0, None);
        if e != ERROR_SUCCESS {
            carlui_core::warn!("ağ sağlayıcısı etkinleştirilemedi: {}", e.0);
            let _ = ControlTraceW(h, PCWSTR::null(), buf.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
            return false;
        }
        if let Ok(mut s) = HANDLE.lock() {
            *s = h.Value;
        }
    }
    ACTIVE.store(true, Ordering::Relaxed);
    std::thread::spawn(move || unsafe {
        let mut name = wide(SESSION);
        let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name.as_mut_ptr()), ..Default::default() };
        log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        log.Anonymous2.EventRecordCallback = Some(on_event);
        let t = OpenTraceW(&mut log);
        if t.Value == u64::MAX {
            carlui_core::warn!("ağ izleme oturumu okunamadı");
            ACTIVE.store(false, Ordering::Relaxed);
            stop();
            return;
        }
        // Oturum durdurulana kadar burada bekler.
        let _ = ProcessTrace(&[t], None, None);
        let _ = CloseTrace(t);
        ACTIVE.store(false, Ordering::Relaxed);
    });
    true
}

/// Oturumu kapatır (pencere kapanırken).
pub fn stop() {
    let h = HANDLE.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or(0);
    if h == 0 {
        return;
    }
    let mut buf = props();
    unsafe {
        let _ = ControlTraceW(CONTROLTRACE_HANDLE { Value: h }, PCWSTR::null(), buf.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
    }
}

/// Ölçüm var mı (yönetici + oturum açık).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Süreç başına toplam bayt; `procs` farkı alır.
pub fn totals() -> HashMap<u32, u64> {
    BYTES.lock().ok().and_then(|m| m.clone()).unwrap_or_default()
}

/// Kapanan süreçlerin sayaçlarını at (pid yeniden kullanılınca yanlış fark çıkmasın).
pub fn retain(live: impl Fn(u32) -> bool) {
    if let Ok(mut m) = BYTES.lock() {
        if let Some(m) = m.as_mut() {
            m.retain(|pid, _| live(*pid));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalnizca_veri_olaylari() {
        for id in [10, 11, 26, 27, 42, 43, 58, 59] {
            assert!(is_transfer(id), "{id}");
        }
        for id in [12, 13, 15, 16, 0] {
            assert!(!is_transfer(id), "{id}");
        }
    }

    #[test]
    fn yuk_okunur_ve_toplanir() {
        let mut d = 1234u32.to_le_bytes().to_vec();
        d.extend(1500u32.to_le_bytes());
        d.extend([1, 2, 3, 4]);
        assert_eq!(parse(&d), Some((1234, 1500)));
        assert_eq!(parse(&d[..6]), None);
        let mut m = HashMap::new();
        add(&mut m, 1234, 1500);
        add(&mut m, 1234, 500);
        assert_eq!(m[&1234], 2000);
    }
}
