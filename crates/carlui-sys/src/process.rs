//! Süreçler: listesi tek çağrıyla çekirdekten okunur (NtQuerySystemInformation,
//! sınıf 5); her sürecin CPU zamanı, belleği, G/Ç sayaçları ve iş parçacıkları
//! birlikte gelir. Bir sürecin "durdurulmuş" olduğu da buradan anlaşılır: bütün
//! iş parçacıkları "askıya alındı" nedeniyle bekliyorsa süreç durmuştur. Böylece
//! dock'un durdurduğunu görev yöneticisi, görev yöneticisinin durdurduğunu dock
//! görür; ayrıca bir liste tutmaya gerek kalmaz.

use std::mem::size_of;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Threading::*,
        },
    },
};

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub session: u32,
    pub threads: u32,
    pub handles: u32,
    /// Toplam CPU zamanı (100 ns birimi).
    pub cpu_time: i64,
    pub working_set: u64,
    pub private: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub create_time: i64,
    pub suspended: bool,
}

type NtQsi = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
type NtProc = unsafe extern "system" fn(HANDLE) -> i32;

fn ntdll(name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe { GetProcAddress(GetModuleHandleW(w!("ntdll.dll")).ok()?, name) }
}

fn rd<T: Copy>(b: &[u8], off: usize) -> T {
    unsafe { std::ptr::read_unaligned(b.as_ptr().add(off) as *const T) }
}

/// Çalışan bütün süreçler.
pub fn list() -> Vec<Proc> {
    let Some(f) = ntdll(s!("NtQuerySystemInformation")) else { return Vec::new() };
    let f: NtQsi = unsafe { std::mem::transmute(f) };
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let mut ret = 0u32;
        let st = unsafe { f(5, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut ret) };
        // STATUS_INFO_LENGTH_MISMATCH: büyüt ve yeniden dene.
        if st == 0xC0000004u32 as i32 {
            buf.resize((ret as usize).max(buf.len()) + (256 << 10), 0);
            continue;
        }
        if st < 0 {
            return Vec::new();
        }
        break;
    }
    let mut out = Vec::with_capacity(400);
    let mut off = 0usize;
    loop {
        let b = &buf[off..];
        let next = rd::<u32>(b, 0) as usize;
        let threads = rd::<u32>(b, 4);
        let name_len = rd::<u16>(b, 56) as usize / 2;
        let name_ptr = rd::<usize>(b, 64) as *const u16;
        let pid = rd::<usize>(b, 80) as u32;
        let name = if name_ptr.is_null() {
            if pid == 0 { "Sistem boşta".into() } else { "System".into() }
        } else {
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name_ptr, name_len) })
        };
        // Durdurulmuş mu: her iş parçacığı Waiting(5) + Suspended(5).
        let mut suspended = threads > 0;
        for t in 0..threads as usize {
            let tb = 256 + t * 80;
            if tb + 80 > b.len() {
                suspended = false;
                break;
            }
            if rd::<u32>(b, tb + 68) != 5 || rd::<u32>(b, tb + 72) != 5 {
                suspended = false;
                break;
            }
        }
        out.push(Proc {
            pid,
            parent: rd::<usize>(b, 88) as u32,
            name,
            session: rd::<u32>(b, 100),
            threads,
            handles: rd::<u32>(b, 96),
            cpu_time: rd::<i64>(b, 40) + rd::<i64>(b, 48),
            working_set: rd::<usize>(b, 144) as u64,
            private: rd::<usize>(b, 200) as u64,
            read_bytes: rd::<i64>(b, 232) as u64,
            write_bytes: rd::<i64>(b, 240) as u64,
            create_time: rd::<i64>(b, 32),
            suspended: suspended && pid != 0,
        });
        if next == 0 {
            break;
        }
        off += next;
    }
    out
}

pub fn is_suspended(pid: u32) -> bool {
    list().iter().any(|p| p.pid == pid && p.suspended)
}

fn with_handle(pid: u32, access: PROCESS_ACCESS_RIGHTS, f: impl FnOnce(HANDLE) -> bool) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(access, false, pid) else { return false };
        let ok = f(h);
        let _ = CloseHandle(h);
        ok
    }
}

fn nt_call(pid: u32, name: PCSTR) -> bool {
    let Some(f) = ntdll(name) else { return false };
    let f: NtProc = unsafe { std::mem::transmute(f) };
    with_handle(pid, PROCESS_SUSPEND_RESUME, |h| unsafe { f(h) } >= 0)
}

/// Sürecin bütün iş parçacıklarını dondurur (CPU kullanmaz, bellekte kalır).
pub fn suspend(pid: u32) -> bool {
    nt_call(pid, s!("NtSuspendProcess"))
}

pub fn resume(pid: u32) -> bool {
    nt_call(pid, s!("NtResumeProcess"))
}

/// Süreci hemen sonlandırır (kaydedilmemiş veri kaybolur).
pub fn terminate(pid: u32) -> bool {
    if pid == std::process::id() {
        return false;
    }
    with_handle(pid, PROCESS_TERMINATE, |h| unsafe { TerminateProcess(h, 1) }.is_ok())
}

/// Sürecin tam exe yolu (erişilemezse boş).
pub fn exe_path(pid: u32) -> String {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = with_handle(pid, PROCESS_QUERY_LIMITED_INFORMATION, |h| unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok());
    if ok { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
}

const _: () = assert!(size_of::<usize>() == 8, "yalnızca 64 bit");
