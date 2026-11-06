//! Bir dosyayı kimin tuttuğunu Windows'un Yeniden Başlatma Yöneticisi'ne
//! sorar (Restart Manager; kurucuların "şu uygulamalar açık" listesi de buradan
//! gelir). Kapatma iki kademelidir: önce pencereye kibarca "kapan" denir,
//! böylece uygulama kaydedilmemiş iş için sorabilir; zorla kapatma ancak
//! kullanıcı onaylayınca ve süreç hâlâ aynı süreçse yapılır.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{RestartManager::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

/// Klasörlerde bakılacak en çok dosya: dev klasörlerde tarama sonsuza gitmesin.
pub const MAX_FILES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    /// Dosya Gezgini: masaüstü de bu süreçtir, süreç kapatılmaz, yalnızca klasör pencereleri.
    Explorer,
    Service,
    /// Windows'un kapanmasına izin vermediği süreç.
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    /// Sürecin başlangıç anı: PID yeniden kullanılmışsa yanlış süreç kapatılmasın.
    pub started: u64,
    pub name: String,
    pub exe: String,
    pub kind: Kind,
}

impl Holder {
    pub fn killable(&self) -> bool {
        self.kind == Kind::App
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillError {
    /// Süreç zaten kapanmış ya da PID başkasına geçmiş.
    Gone,
    /// Yönetici yetkisi gerekiyor.
    Denied,
    Failed,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, stop: &AtomicBool) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES || stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), out, stop);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

/// Seçilen öğeleri dosya listesine açar. İkinci değer: sınıra ulaşıldı mı.
pub fn expand(paths: &[PathBuf], stop: &AtomicBool) -> (Vec<PathBuf>, bool) {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            walk(p, &mut out, stop);
        } else {
            out.push(p.clone());
        }
    }
    let full = out.len() >= MAX_FILES;
    (out, full)
}

fn kind_of(app: RM_APP_TYPE, exe: &str) -> Kind {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or("");
    if app == RmExplorer || file.eq_ignore_ascii_case("explorer.exe") {
        Kind::Explorer
    } else if app == RmCritical {
        Kind::Critical
    } else if app == RmService {
        Kind::Service
    } else {
        Kind::App
    }
}

fn wide(a: &[u16]) -> String {
    String::from_utf16_lossy(&a[..a.iter().position(|c| *c == 0).unwrap_or(a.len())])
}

fn stamp(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

unsafe fn list(session: u32) -> std::result::Result<Vec<RM_PROCESS_INFO>, String> {
    let mut items = vec![RM_PROCESS_INFO::default(); 32];
    for _ in 0..4 {
        let (mut need, mut n, mut why) = (0u32, items.len() as u32, 0u32);
        let r = RmGetList(session, &mut need, &mut n, Some(items.as_mut_ptr()), &mut why);
        if r == ERROR_SUCCESS {
            items.truncate(n as usize);
            return Ok(items);
        }
        if r != ERROR_MORE_DATA {
            return Err(format!("RmGetList {}", r.0));
        }
        items.resize(need as usize + 8, RM_PROCESS_INFO::default());
    }
    Err("RmGetList".into())
}

/// Dosyaları tutan süreçleri bulur; bu aracın kendisi listelenmez.
pub fn find(files: &[PathBuf]) -> std::result::Result<Vec<Holder>, String> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    unsafe {
        let mut session = 0u32;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        let r = RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr()));
        if r != ERROR_SUCCESS {
            return Err(format!("RmStartSession {}", r.0));
        }
        let run = || -> std::result::Result<Vec<RM_PROCESS_INFO>, String> {
            for part in files.chunks(1000) {
                let names: Vec<HSTRING> = part.iter().map(|p| HSTRING::from(p.as_os_str())).collect();
                let ptrs: Vec<PCWSTR> = names.iter().map(|h| PCWSTR(h.as_ptr())).collect();
                let r = RmRegisterResources(session, Some(&ptrs), None, None);
                if r != ERROR_SUCCESS {
                    return Err(format!("RmRegisterResources {}", r.0));
                }
            }
            list(session)
        };
        let got = run();
        let _ = RmEndSession(session);
        let me = std::process::id();
        let mut out: Vec<Holder> = got?
            .iter()
            .filter(|i| i.Process.dwProcessId != me)
            .map(|i| {
                let exe = carlui_core::win::process_exe(i.Process.dwProcessId);
                let name = wide(&i.strAppName);
                let name = if name.is_empty() { exe.rsplit('\\').next().unwrap_or("?").to_string() } else { name };
                Holder { pid: i.Process.dwProcessId, started: stamp(i.Process.ProcessStartTime), kind: kind_of(i.ApplicationType, &exe), exe, name }
            })
            .collect();
        out.sort_by_key(|h| (h.kind != Kind::App, h.name.to_lowercase(), h.pid));
        out.dedup_by_key(|h| h.pid);
        Ok(out)
    }
}

struct Find {
    pid: u32,
    explorer: bool,
    hits: Vec<HWND>,
}

unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
    let f = &mut *(l.0 as *mut Find);
    let mut pid = 0;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let top = GetWindow(h, GW_OWNER).map_or(true, |o| o.is_invalid());
    // Masaüstü (Progman) de Gezgin'indir; ona WM_CLOSE "Windows'u kapat" demektir.
    if pid == f.pid && top && IsWindowVisible(h).as_bool() && (!f.explorer || carlui_core::win::class_name(h) == "CabinetWClass") {
        f.hits.push(h);
    }
    TRUE
}

/// Süreçteki pencerelere kapanmasını söyler. `false`: kapatılacak pencere yok.
pub fn close(h: &Holder) -> bool {
    let mut f = Find { pid: h.pid, explorer: h.kind == Kind::Explorer, hits: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Find as isize));
        for w in &f.hits {
            let _ = PostMessageW(*w, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    !f.hits.is_empty()
}

/// Kullanıcının onayladığı zorla kapatma.
pub fn kill(h: &Holder) -> std::result::Result<(), KillError> {
    if !h.killable() {
        return Err(KillError::Failed);
    }
    unsafe {
        let p = match OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, h.pid) {
            Ok(p) => p,
            Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => return Err(KillError::Denied),
            Err(_) => return Err(KillError::Gone),
        };
        let (mut a, mut b, mut c, mut d) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let same = GetProcessTimes(p, &mut a, &mut b, &mut c, &mut d).is_ok() && stamp(a) == h.started;
        let r = if !same {
            Err(KillError::Gone)
        } else {
            TerminateProcess(p, 1).map_err(|e| if e.code() == ERROR_ACCESS_DENIED.to_hresult() { KillError::Denied } else { KillError::Failed })
        };
        let _ = CloseHandle(p);
        carlui_core::info!("kilit açıcı: {} (PID {}) zorla kapatma: {r:?}", h.name, h.pid);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::{fs::OpenOptionsExt, process::CommandExt},
        time::Duration,
    };

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("carlui-unlock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("alt")).unwrap();
        d
    }

    #[test]
    fn folders_expand_to_files() {
        let d = dir("expand");
        std::fs::write(d.join("a.txt"), "a").unwrap();
        std::fs::write(d.join("alt").join("b.txt"), "b").unwrap();
        let (files, full) = expand(&[d.clone()], &AtomicBool::new(false));
        assert_eq!(files.len(), 2);
        assert!(!full);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn explorer_is_never_a_plain_app() {
        assert_eq!(kind_of(RmUnknownApp, r"C:\Windows\explorer.exe"), Kind::Explorer);
        assert_eq!(kind_of(RmMainWindow, r"C:\x\app.exe"), Kind::App);
        assert_eq!(kind_of(RmService, r"C:\x\svc.exe"), Kind::Service);
        assert_eq!(kind_of(RmCritical, r"C:\x\c.exe"), Kind::Critical);
        let h = Holder { pid: 1, started: 0, name: "x".into(), exe: "x".into(), kind: Kind::Explorer };
        assert!(!h.killable());
        assert_eq!(kill(&h), Err(KillError::Failed));
    }

    #[test]
    fn wrong_start_time_is_not_killed() {
        let h = Holder { pid: std::process::id(), started: 1, name: "ben".into(), exe: String::new(), kind: Kind::App };
        assert_eq!(kill(&h), Err(KillError::Gone));
    }

    /// Başka bir sürecin kilidi bulunur, zorla kapatılınca kilit kalkar.
    #[test]
    fn other_process_is_found_and_killed() {
        let d = dir("other");
        let f = d.join("kilitli dosya.txt");
        std::fs::write(&f, "x").unwrap();
        let mut child = std::process::Command::new("cmd")
            .raw_arg(format!("/c ping -n 20 127.0.0.1 >> \"{}\"", f.display()))
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut found = Vec::new();
        for _ in 0..100 {
            found = find(&[f.clone()]).unwrap();
            if !found.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(found.iter().any(|h| h.pid == child.id()), "{found:?}");
        for h in &found {
            assert_eq!(kill(h), Ok(()));
        }
        let _ = child.wait();
        let mut left = found;
        for _ in 0..100 {
            left = find(&[f.clone()]).unwrap();
            if left.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(left, Vec::new());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn own_lock_is_not_listed() {
        let d = dir("own");
        let f = d.join("k.bin");
        std::fs::write(&f, "x").unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&f).unwrap();
        assert_eq!(find(&[f.clone()]).unwrap(), Vec::new());
        drop(held);
        assert_eq!(find(&[f]).unwrap(), Vec::new());
        let _ = std::fs::remove_dir_all(&d);
    }
}
