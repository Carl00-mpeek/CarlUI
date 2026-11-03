//! Başlangıçta çalışan her şey. Windows Görev Yöneticisi yalnızca Run kayıtlarını
//! ve Başlangıç klasörünü gösterir; burada şunların hepsi taranır:
//! - Kayıt defteri: HKCU/HKLM Run, RunOnce, 32 bit Run, Policies\Explorer\Run
//! - Başlangıç klasörleri (kullanıcı ve tüm kullanıcılar)
//! - Zamanlanmış görevler: oturum açılışında ya da sistem açılışında tetiklenenler
//! - Hizmetler: otomatik başlayanlar
//! - Winlogon: Shell ve Userinit (değiştirilmişse kötü amaçlı yazılım işaretidir)
//! Etkin/devre dışı durumu Gezgin'in "StartupApproved" kaydından okunur ve
//! oraya yazılır (Windows Görev Yöneticisi de aynı yeri kullanır).
//! Tarama birkaç yüz öğe olduğu için arka planda yapılır.

use crate::util::{exe_of, is_windows_path};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Registry::*, Services::*, TaskScheduler::*},
        UI::Shell::*,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub enum Toggle {
    /// StartupApproved altındaki anahtar ("Run", "Run32", "StartupFolder") ve değer adı.
    Approved { hive: isize, key: &'static str, name: String },
    /// Zamanlanmış görevin tam yolu.
    Task(String),
    None,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub source: &'static str,
    pub command: String,
    pub path: String,
    pub enabled: bool,
    pub windows: bool,
    pub toggle: Toggle,
}

fn hkey(h: isize) -> HKEY {
    HKEY(h as _)
}

fn values(root: HKEY, key: &str) -> Vec<(String, Vec<u8>, u32)> {
    let mut out = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(key), 0, KEY_READ, &mut k).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 512];
            let mut nlen = name.len() as u32;
            let mut data = vec![0u8; 8192];
            let mut dlen = data.len() as u32;
            let mut ty = 0u32;
            if RegEnumValueW(k, i, PWSTR(name.as_mut_ptr()), &mut nlen, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut dlen)).is_err() {
                break;
            }
            data.truncate(dlen as usize);
            out.push((String::from_utf16_lossy(&name[..nlen as usize]), data, ty));
        }
        let _ = RegCloseKey(k);
    }
    out
}

fn reg_string(data: &[u8]) -> String {
    let w: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&w)
}

const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\";

/// StartupApproved kaydı: yoksa etkin; ilk baytın en düşük biti 1 ise devre dışı.
fn approved(hive: HKEY, key: &str, name: &str) -> bool {
    values(hive, &format!("{APPROVED}{key}")).iter().find(|(n, ..)| n.eq_ignore_ascii_case(name)).map(|(_, d, _)| d.first().map(|b| b & 1 == 0).unwrap_or(true)).unwrap_or(true)
}

fn run_keys(out: &mut Vec<Item>) {
    let list: [(HKEY, &str, &'static str, Option<&'static str>); 8] = [
        (HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Kullanıcı Run", "Registry · User Run"), Some("Run")),
        (HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce", t!("Kayıt · Kullanıcı RunOnce", "Registry · User RunOnce"), None),
        (HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer\\Run", t!("Kayıt · Kullanıcı ilkesi", "Registry · User policy"), None),
        (HKEY_LOCAL_MACHINE, "Software\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Sistem Run", "Registry · System Run"), Some("Run")),
        (HKEY_LOCAL_MACHINE, "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce", t!("Kayıt · Sistem RunOnce", "Registry · System RunOnce"), None),
        (HKEY_LOCAL_MACHINE, "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run", t!("Kayıt · Sistem Run (32 bit)", "Registry · System Run (32-bit)"), Some("Run32")),
        (HKEY_LOCAL_MACHINE, "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer\\Run", t!("Kayıt · Sistem ilkesi", "Registry · System policy"), None),
        (HKEY_LOCAL_MACHINE, "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce", t!("Kayıt · Sistem RunOnce (32 bit)", "Registry · System RunOnce (32-bit)"), None),
    ];
    for (hive, key, source, appr) in list {
        for (name, data, ty) in values(hive, key) {
            if ty != REG_SZ.0 && ty != REG_EXPAND_SZ.0 {
                continue;
            }
            let command = reg_string(&data);
            let path = exe_of(&command);
            let (enabled, toggle) = match appr {
                Some(k) => (approved(hive, k, &name), Toggle::Approved { hive: hive.0 as isize, key: k, name: name.clone() }),
                None => (true, Toggle::None),
            };
            out.push(Item { windows: is_windows_path(&path), name, source, command, path, enabled, toggle });
        }
    }
}

fn startup_folders(out: &mut Vec<Item>) {
    for (id, hive, source) in [(FOLDERID_Startup, HKEY_CURRENT_USER, t!("Başlangıç klasörü", "Startup folder")), (FOLDERID_CommonStartup, HKEY_LOCAL_MACHINE, t!("Başlangıç klasörü (tümü)", "Startup folder (all users)"))] {
        let Ok(dir) = (unsafe { SHGetKnownFolderPath(&id, KNOWN_FOLDER_FLAG(0), None) }) else { continue };
        let dir = unsafe {
            let s = dir.to_string().unwrap_or_default();
            CoTaskMemFree(Some(dir.0 as _));
            s
        };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if file.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let full = e.path().to_string_lossy().to_string();
            let target = if file.to_lowercase().ends_with(".lnk") { link_target(&full) } else { full.clone() };
            let name = file.trim_end_matches(".lnk").to_string();
            out.push(Item {
                enabled: approved(hive, "StartupFolder", &file),
                toggle: Toggle::Approved { hive: hive.0 as isize, key: "StartupFolder", name: file },
                windows: is_windows_path(&target),
                name,
                source,
                command: target.clone(),
                path: target,
            });
        }
    }
}

fn link_target(lnk: &str) -> String {
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return String::new() };
        let Ok(pf) = link.cast::<IPersistFile>() else { return String::new() };
        if pf.Load(&HSTRING::from(lnk), STGM_READ).is_err() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
        String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
    }
}

fn winlogon(out: &mut Vec<Item>) {
    let key = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon";
    for (hive, label) in [(HKEY_LOCAL_MACHINE, t!("Winlogon · Sistem", "Winlogon · System")), (HKEY_CURRENT_USER, t!("Winlogon · Kullanıcı", "Winlogon · User"))] {
        for (name, data, _) in values(hive, key) {
            if !(name.eq_ignore_ascii_case("Shell") || name.eq_ignore_ascii_case("Userinit")) {
                continue;
            }
            let command = reg_string(&data);
            for part in command.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let path = exe_of(part);
                out.push(Item { windows: is_windows_path(&path), name: format!("{name}: {}", crate::util::file_name(&path)), source: label, command: part.to_string(), path, enabled: true, toggle: Toggle::None });
            }
        }
    }
}

fn services(out: &mut Vec<Item>) {
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_ENUMERATE_SERVICE) else { return };
        let mut need = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        let _ = EnumServicesStatusExW(scm, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL, None, &mut need, &mut count, Some(&mut resume), None);
        let mut buf = vec![0u8; need as usize + 4096];
        resume = 0;
        if EnumServicesStatusExW(scm, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL, Some(&mut buf), &mut need, &mut count, Some(&mut resume), None).is_ok() {
            let list = std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize);
            for s in list {
                let Ok(svc) = OpenServiceW(scm, s.lpServiceName, SERVICE_QUERY_CONFIG) else { continue };
                let mut need = 0u32;
                let _ = QueryServiceConfigW(svc, None, 0, &mut need);
                let mut cb = vec![0u8; need as usize + 64];
                let cfg = cb.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
                if QueryServiceConfigW(svc, Some(cfg), cb.len() as u32, &mut need).is_ok() && (*cfg).dwStartType == SERVICE_AUTO_START {
                    let command = (*cfg).lpBinaryPathName.to_string().unwrap_or_default();
                    let path = exe_of(&command);
                    out.push(Item {
                        name: s.lpDisplayName.to_string().unwrap_or_default(),
                        source: t!("Hizmet (otomatik)", "Service (automatic)"),
                        windows: is_windows_path(&path),
                        command,
                        path,
                        enabled: true,
                        toggle: Toggle::None,
                    });
                }
                let _ = CloseServiceHandle(svc);
            }
        }
        let _ = CloseServiceHandle(scm);
    }
}

fn tasks(out: &mut Vec<Item>) {
    unsafe {
        let Ok(svc) = CoCreateInstance::<_, ITaskService>(&TaskScheduler, None, CLSCTX_INPROC_SERVER) else { return };
        let v = VARIANT::default();
        if svc.Connect(&v, &v, &v, &v).is_err() {
            return;
        }
        let Ok(root) = svc.GetFolder(&BSTR::from("\\")) else { return };
        walk(&root, out);
    }
}

unsafe fn walk(folder: &ITaskFolder, out: &mut Vec<Item>) {
    if let Ok(list) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
        let n = list.Count().unwrap_or(0);
        for i in 1..=n {
            let Ok(task) = list.get_Item(&VARIANT::from(i)) else { continue };
            let Ok(def) = task.Definition() else { continue };
            let Ok(trigs) = def.Triggers() else { continue };
            let mut tn = 0i32;
            let _ = trigs.Count(&mut tn);
            let mut kind = None;
            for t in 1..=tn {
                if let Ok(tr) = trigs.get_Item(t) {
                    let mut ty = TASK_TRIGGER_TYPE2::default();
                    if tr.Type(&mut ty).is_ok() {
                        if ty == TASK_TRIGGER_LOGON {
                            kind = Some(t!("Görev · Oturum açılışı", "Task · At sign-in"));
                        } else if ty == TASK_TRIGGER_BOOT && kind.is_none() {
                            kind = Some(t!("Görev · Sistem açılışı", "Task · At startup"));
                        }
                    }
                }
            }
            let Some(source) = kind else { continue };
            let task_path = task.Path().map(|b| b.to_string()).unwrap_or_default();
            let mut command = String::new();
            if let Ok(acts) = def.Actions() {
                if let Ok(a) = acts.get_Item(1) {
                    if let Ok(ex) = a.cast::<IExecAction>() {
                        let mut p = BSTR::new();
                        let mut args = BSTR::new();
                        let _ = ex.Path(&mut p);
                        let _ = ex.Arguments(&mut args);
                        command = format!("{} {}", p, args).trim().to_string();
                        if !p.is_empty() && !p.to_string().starts_with('"') && p.to_string().contains(' ') {
                            command = format!("\"{}\" {}", p, args).trim().to_string();
                        }
                    }
                }
            }
            let path = exe_of(&command);
            let en = task.Enabled().unwrap_or_default();
            let name = task.Name().map(|b| b.to_string()).unwrap_or_default();
            out.push(Item {
                windows: task_path.to_lowercase().starts_with("\\microsoft\\"),
                name,
                source,
                command: if command.is_empty() { task_path.clone() } else { command },
                path,
                enabled: en.as_bool(),
                toggle: Toggle::Task(task_path),
            });
        }
    }
    if let Ok(subs) = folder.GetFolders(0) {
        let n = subs.Count().unwrap_or(0);
        for i in 1..=n {
            if let Ok(f) = subs.get_Item(&VARIANT::from(i)) {
                walk(&f, out);
            }
        }
    }
}

/// Bütün kaynakları tarar (yavaş: arka planda çağırın).
pub fn scan() -> Vec<Item> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut out = Vec::new();
    run_keys(&mut out);
    startup_folders(&mut out);
    winlogon(&mut out);
    tasks(&mut out);
    services(&mut out);
    out
}

/// Öğeyi etkinleştir/devre dışı bırak. Hata metni döner (ör. yönetici gerekir).
pub fn set_enabled(item: &Item, on: bool) -> std::result::Result<(), String> {
    match &item.toggle {
        Toggle::Approved { hive, key, name } => unsafe {
            let mut k = HKEY::default();
            let path = HSTRING::from(format!("{APPROVED}{key}"));
            if RegCreateKeyExW(hkey(*hive), &path, 0, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_err() {
                return Err(t!("Bu öğeyi değiştirmek için yönetici izni gerekiyor", "Changing this item needs admin rights").into());
            }
            // 02 = etkin, 03 = devre dışı; ardından devre dışı bırakılma zamanı (FILETIME).
            let mut data = [0u8; 12];
            data[0] = if on { 2 } else { 3 };
            if !on {
                let ft = windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime();
                data[4..8].copy_from_slice(&ft.dwLowDateTime.to_le_bytes());
                data[8..12].copy_from_slice(&ft.dwHighDateTime.to_le_bytes());
            }
            let r = RegSetValueExW(k, &HSTRING::from(name.as_str()), 0, REG_BINARY, Some(&data));
            let _ = RegCloseKey(k);
            r.ok().map_err(|_| t!("Bu öğeyi değiştirmek için yönetici izni gerekiyor", "Changing this item needs admin rights").to_string())
        },
        Toggle::Task(path) => unsafe {
            let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
            let v = VARIANT::default();
            svc.Connect(&v, &v, &v, &v).map_err(|e| e.message())?;
            let (dir, name) = path.rsplit_once('\\').unwrap_or(("", path));
            let folder = svc.GetFolder(&BSTR::from(if dir.is_empty() { "\\" } else { dir })).map_err(|e| e.message())?;
            let task = folder.GetTask(&BSTR::from(name)).map_err(|e| e.message())?;
            task.SetEnabled(VARIANT_BOOL::from(on)).map_err(|_| t!("Bu görevi değiştirmek için yönetici izni gerekiyor", "Changing this task needs admin rights").to_string())
        },
        Toggle::None => Err(t!("Bu öğe buradan kapatılamaz (hizmet/Winlogon)", "This item can't be turned off here (service/Winlogon)").into()),
    }
}
