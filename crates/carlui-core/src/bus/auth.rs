//! Olay yolu karşı uç doğrulaması. Sorun: aynı kullanıcının herhangi bir
//! süreci boruya bağlanıp `Hello` ve ardından süreç öldürme, indirme, çıkış
//! gibi hassas olaylar yollayabiliyordu. Çözüm iki katman:
//! 1. Boru yalnızca bu kullanıcıya ve SYSTEM'e açık bir DACL ile kurulur
//!    (başka kullanıcılar bağlanamaz, yayını da dinleyemez).
//! 2. Bağlanan (ya da bağlanılan) sürecin exe'si bu exe ile aynı klasörde
//!    olmalı, aynı oturumda ve aynı kullanıcı SID'iyle çalışmalı.
//!
//! Aynı süreç (ör. testler) her zaman kabul edilir. Yol kuralı `path_allowed`
//! saf bir işlevdir; izinli klasör dışarıdan verilerek test edilir.

use std::{path::Path, sync::OnceLock};
use windows::{
    core::{HSTRING, PWSTR},
    Win32::{
        Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
        Security::{
            Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
            GetLengthSid, GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        },
        System::{
            Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
};

/// Saf yol kuralı: `image` doğrudan `allowed_dir` klasöründe mi (alt klasör
/// sayılmaz). Büyük/küçük harf, `\\?\` öneki, `/` ve sondaki `\` farkı yok
/// sayılır; `.`/`..` içeren yol reddedilir. İkisi de önceden kanonik olmalı.
pub(crate) fn path_allowed(image: &str, allowed_dir: &str) -> bool {
    let img = norm(image);
    let dir = norm(allowed_dir);
    if img.is_empty() || dir.is_empty() || img.split('\\').any(|c| c == "." || c == "..") {
        return false;
    }
    match img.rsplit_once('\\') {
        Some((parent, file)) => !file.is_empty() && parent == dir,
        None => false,
    }
}

fn norm(p: &str) -> String {
    let p = p.trim().replace('/', "\\");
    let p = if let Some(r) = p.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{r}")
    } else if let Some(r) = p.strip_prefix(r"\\?\") {
        r.to_string()
    } else {
        p
    };
    p.trim_end_matches('\\').to_lowercase()
}

fn canonical(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().into_owned()
}

/// Bu exe'nin (kanonik) klasörü; bulunamazsa boş (o zaman hiçbir yabancı süreç geçmez).
fn allowed_dir() -> &'static str {
    static D: OnceLock<String> = OnceLock::new();
    D.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::canonicalize(e).ok())
            .and_then(|e| e.parent().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    })
}

/// Sürecin belirtecindeki kullanıcı SID'i (ikili).
fn process_sid(proc: HANDLE) -> Option<Vec<u8>> {
    unsafe {
        let mut tok = HANDLE::default();
        OpenProcessToken(proc, TOKEN_QUERY, &mut tok).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(tok, TokenUser, None, 0, &mut len);
        // u64 tampon: TOKEN_USER hizası için.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = len > 0 && GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr() as _), len, &mut len).is_ok();
        let _ = CloseHandle(tok);
        if !ok {
            return None;
        }
        let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        if sid.0.is_null() {
            return None;
        }
        let n = GetLengthSid(sid) as usize;
        Some(std::slice::from_raw_parts(sid.0 as *const u8, n).to_vec())
    }
}

fn own_sid() -> Option<&'static [u8]> {
    static S: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    S.get_or_init(|| process_sid(unsafe { GetCurrentProcess() })).as_deref()
}

fn session_of(pid: u32) -> Option<u32> {
    let mut s = 0;
    unsafe { ProcessIdToSessionId(pid, &mut s).ok().map(|_| s) }
}

/// Karşı uçtaki süreci doğrular. `Err` metni log içindir.
pub(crate) fn verify_pid(pid: u32) -> Result<(), String> {
    verify_pid_in(pid, allowed_dir())
}

/// `verify_pid`, izinli klasör dışarıdan verilerek (testler için).
fn verify_pid_in(pid: u32, dir: &str) -> Result<(), String> {
    if pid == 0 {
        return Err("süreç kimliği alınamadı".into());
    }
    let me = unsafe { GetCurrentProcessId() };
    if pid == me {
        return Ok(());
    }
    let exe = crate::win::process_exe(pid);
    if exe.is_empty() {
        return Err(format!("süreç {pid} yolu okunamadı"));
    }
    if !path_allowed(&canonical(Path::new(&exe)), dir) {
        return Err(format!("izinsiz konum: {exe}"));
    }
    match (session_of(pid), session_of(me)) {
        (Some(a), Some(b)) if a == b => {}
        _ => return Err(format!("başka oturum: {exe}")),
    }
    let sid = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|e| format!("süreç açılamadı ({exe}): {e}"))?;
        let s = process_sid(h);
        let _ = CloseHandle(h);
        s
    };
    match (sid.as_deref(), own_sid()) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err(format!("başka kullanıcı ya da belirteç okunamadı: {exe}")),
    }
}

/// Saf kısma kuralı: aynı neden en son `last` anında loglandıysa `now`'da
/// yeniden loglanır mı (ilk kez her zaman evet).
fn due(last: Option<std::time::Instant>, now: std::time::Instant, every: std::time::Duration) -> bool {
    last.map_or(true, |t| now.saturating_duration_since(t) >= every)
}

/// Ret uyarıları döngüde tekrarlanabilir (sahte sunucu, sürekli yeniden
/// bağlanan yabancı istemci): her neden için `every`'de en çok bir kez `true`.
pub(crate) fn should_log(reason: &str, every: std::time::Duration) -> bool {
    use std::{collections::HashMap, sync::Mutex, time::Instant};
    static LAST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let mut m = LAST.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if !due(m.get(reason).copied(), now, every) {
        return false;
    }
    // Sınırsız büyümesin: çok farklı neden birikirse eskiler unutulur.
    if m.len() >= 64 {
        m.retain(|_, t| now.saturating_duration_since(*t) < every);
    }
    m.insert(reason.to_string(), now);
    true
}

/// Sunucu tarafı: boruya bağlanan istemcinin süreç kimliği (0: alınamadı).
pub(crate) fn client_pid(h: HANDLE) -> u32 {
    let mut pid = 0;
    let _ = unsafe { GetNamedPipeClientProcessId(h, &mut pid) };
    pid
}

/// İstemci tarafı: bağlanılan borunun sunucu süreç kimliği (0: alınamadı).
pub(crate) fn server_pid(h: HANDLE) -> u32 {
    let mut pid = 0;
    let _ = unsafe { GetNamedPipeServerProcessId(h, &mut pid) };
    pid
}

/// Yalnızca bu kullanıcıya ve SYSTEM'e tam erişim veren güvenlik tanımı
/// (`D:P` = devralınmayan DACL). Bırakılınca bellek serbest kalır.
pub(crate) struct PipeSecurity {
    sd: PSECURITY_DESCRIPTOR,
}

impl PipeSecurity {
    pub(crate) fn new() -> Option<Self> {
        let sddl = owner_sddl()?;
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl.as_str()), SDDL_REVISION_1, &mut sd, None).ok()? };
        Some(Self { sd })
    }

    pub(crate) fn attrs(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.sd.0, bInheritHandle: false.into() }
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(HLOCAL(self.sd.0));
        }
    }
}

/// `D:P(A;;GA;;;<kullanıcı SID>)(A;;GA;;;SY)`; bir kez hesaplanır.
fn owner_sddl() -> Option<String> {
    static S: OnceLock<Option<String>> = OnceLock::new();
    S.get_or_init(|| {
        let sid = own_sid()?;
        let text = unsafe {
            let mut p = PWSTR::null();
            ConvertSidToStringSidW(windows::Win32::Security::PSID(sid.as_ptr() as *mut _), &mut p).ok()?;
            let s = p.to_string().ok();
            let _ = LocalFree(HLOCAL(p.0 as _));
            s?
        };
        Some(format!("D:P(A;;GA;;;{text})(A;;GA;;;SY)"))
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Program Files\CarlUI";

    #[test]
    fn ayni_klasordeki_exe_gecer() {
        assert!(path_allowed(r"C:\Program Files\CarlUI\carlui-dock.exe", DIR));
        assert!(path_allowed(r"c:\program files\carlui\CARLUI-ISLAND.EXE", DIR));
        assert!(path_allowed(r"\\?\C:\Program Files\CarlUI\carlui-tray.exe", DIR));
        assert!(path_allowed(r"C:/Program Files/CarlUI/carlui-tools.exe", r"\\?\C:\Program Files\CarlUI\"));
        assert!(path_allowed(r"\\?\UNC\srv\pay\carlui\x.exe", r"\\srv\pay\carlui"));
    }

    #[test]
    fn baska_klasor_reddedilir() {
        assert!(!path_allowed(r"C:\Program Files\CarlUI\sub\carlui-dock.exe", DIR));
        assert!(!path_allowed(r"C:\Program Files\CarlUI2\carlui-dock.exe", DIR));
        assert!(!path_allowed(r"C:\Program Files\carlui-dock.exe", DIR));
        assert!(!path_allowed(r"C:\Users\x\AppData\Local\Temp\carlui-dock.exe", DIR));
        assert!(!path_allowed(r"C:\Program Files\CarlUI\..\CarlUI\x.exe", DIR));
        assert!(!path_allowed(r"C:\Program Files\CarlUI\", DIR));
        assert!(!path_allowed("", DIR));
        assert!(!path_allowed(r"C:\Program Files\CarlUI\x.exe", ""));
        assert!(!path_allowed("carlui-dock.exe", DIR));
    }

    #[test]
    fn kendi_sureci_kabul_baskasi_ret() {
        assert!(verify_pid(unsafe { GetCurrentProcessId() }).is_ok());
        assert!(verify_pid(0).is_err());
        // System süreci (4): yolu okunamaz ya da klasör dışı.
        assert!(verify_pid(4).is_err());
    }

    /// Bu test sürecini başlatan süreç (cargo): aynı kullanıcı ve oturum.
    fn parent_pid() -> Option<u32> {
        use windows::Win32::System::Diagnostics::ToolHelp::*;
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let me = GetCurrentProcessId();
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut found = None;
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                if e.th32ProcessID == me {
                    found = Some(e.th32ParentProcessID);
                    break;
                }
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
            found
        }
    }

    #[test]
    fn baska_surec_klasorune_gore_dogrulanir() {
        // Hiçbir süreç başlatmadan, var olan üst süreçle tam yolu (oturum + SID) dener.
        let Some(pid) = parent_pid().filter(|&p| !crate::win::process_exe(p).is_empty()) else { return };
        let exe = canonical(Path::new(&crate::win::process_exe(pid)));
        let dir = Path::new(&exe).parent().unwrap().to_string_lossy().into_owned();
        assert_eq!(verify_pid_in(pid, &dir), Ok(()));
        assert!(verify_pid_in(pid, r"C:\olmayan\klasor").is_err());
    }

    #[test]
    fn log_kisma_kurali() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let dk = Duration::from_secs(60);
        assert!(due(None, t0, dk));
        assert!(!due(Some(t0), t0 + Duration::from_secs(59), dk));
        assert!(due(Some(t0), t0 + dk, dk));
        assert!(should_log("test-neden", dk));
        assert!(!should_log("test-neden", dk));
        assert!(should_log("test-baska", dk));
    }

    #[test]
    fn guvenlik_tanimi_kurulur() {
        assert!(own_sid().is_some_and(|s| !s.is_empty()));
        let sddl = owner_sddl().expect("sddl");
        assert!(sddl.starts_with("D:P(A;;GA;;;S-1-"), "{sddl}");
        assert!(PipeSecurity::new().is_some());
    }
}
