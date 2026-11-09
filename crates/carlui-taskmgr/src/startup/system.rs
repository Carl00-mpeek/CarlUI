//! Zamanlanmış görevler (oturum/sistem açılışında tetiklenenler) ve otomatik
//! hizmetler. Açıp kapatmak yalnızca "etkin" bayrağını ya da başlangıç türünü
//! değiştirir: çalışan görev örneği ya da hizmet durdurulmaz, başlatılmaz.

use super::{reg, Fail, Item, Toggle};
use crate::util::{exe_of, is_windows_path};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Registry::*, Services::*, TaskScheduler::*},
    },
};

fn hr_fail(e: Error) -> Fail {
    if e.code() == E_ACCESSDENIED || e.code() == ERROR_ACCESS_DENIED.to_hresult() {
        Fail::Denied
    } else {
        Fail::Other(e.message())
    }
}

fn service() -> std::result::Result<ITaskService, Fail> {
    unsafe {
        let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(hr_fail)?;
        let v = VARIANT::default();
        svc.Connect(&v, &v, &v, &v).map_err(hr_fail)?;
        Ok(svc)
    }
}

fn task(path: &str) -> std::result::Result<IRegisteredTask, Fail> {
    let svc = service()?;
    let (dir, name) = path.rsplit_once('\\').unwrap_or(("", path));
    unsafe {
        let folder = svc.GetFolder(&BSTR::from(if dir.is_empty() { "\\" } else { dir })).map_err(hr_fail)?;
        folder.GetTask(&BSTR::from(name)).map_err(hr_fail)
    }
}

/// Görevin etkin bayrağını değiştirir (çalışan örneğe dokunmaz).
pub fn set_task(path: &str, on: bool) -> std::result::Result<(), Fail> {
    let t = task(path)?;
    unsafe { t.SetEnabled(VARIANT_BOOL::from(on)).map_err(hr_fail) }
}

pub fn task_enabled(path: &str) -> Option<bool> {
    let t = task(path).ok()?;
    unsafe { t.Enabled().ok().map(|b| b.as_bool()) }
}

const SERVICES: &str = "SYSTEM\\CurrentControlSet\\Services\\";

/// Hizmetin başlangıç türü otomatik mi (kayıttan; okuma yetkisi her zaman var).
pub fn service_auto(name: &str) -> Option<bool> {
    reg::value(HKEY_LOCAL_MACHINE, &format!("{SERVICES}{name}"), "Start").and_then(|(_, d)| reg::dword(&d)).map(|s| s == SERVICE_AUTO_START.0)
}

pub fn set_service(name: &str, on: bool) -> std::result::Result<(), Fail> {
    let start = if on { SERVICE_AUTO_START } else { SERVICE_DEMAND_START };
    let r = unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT).map_err(hr_fail)?;
        let r = match OpenServiceW(scm, &HSTRING::from(name), SERVICE_CHANGE_CONFIG) {
            Ok(svc) => {
                let r = ChangeServiceConfigW(svc, ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE), start, SERVICE_ERROR(SERVICE_NO_CHANGE), None, None, None, None, None, None, None).map_err(hr_fail);
                let _ = CloseServiceHandle(svc);
                r
            }
            Err(e) => Err(hr_fail(e)),
        };
        let _ = CloseServiceHandle(scm);
        r
    };
    match r {
        // Bazı hizmetler (koruma altındakiler) hizmet yöneticisinden değiştirilmeye
        // izin vermez; yöneticiyken kayıttaki başlangıç türü doğrudan yazılır,
        // bir sonraki açılışta geçerli olur.
        Err(Fail::Denied) if crate::admin::is_elevated() => reg::set_value(HKEY_LOCAL_MACHINE, &format!("{SERVICES}{name}"), "Start", REG_DWORD.0, &start.0.to_le_bytes()),
        r => r,
    }
}

pub fn services(out: &mut Vec<Item>) {
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
            let marked = super::marked_services();
            for s in list {
                let Ok(svc) = OpenServiceW(scm, s.lpServiceName, SERVICE_QUERY_CONFIG) else { continue };
                let mut need = 0u32;
                let _ = QueryServiceConfigW(svc, None, 0, &mut need);
                let mut cb = vec![0u8; need as usize + 64];
                let cfg = cb.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
                let id = s.lpServiceName.to_string().unwrap_or_default();
                if QueryServiceConfigW(svc, Some(cfg), cb.len() as u32, &mut need).is_ok() {
                    let auto = (*cfg).dwStartType == SERVICE_AUTO_START;
                    // Bizim kapattıklarımız da listede kalır ki geri açılabilsin.
                    let ours = (*cfg).dwStartType == SERVICE_DEMAND_START && marked.iter().any(|m| m.eq_ignore_ascii_case(&id));
                    if auto || ours {
                        let command = (*cfg).lpBinaryPathName.to_string().unwrap_or_default();
                        let path = exe_of(&command);
                        let windows = is_windows_path(&path);
                        out.push(Item {
                            name: s.lpDisplayName.to_string().unwrap_or_default(),
                            source: t!("Hizmet (otomatik)", "Service (automatic)"),
                            windows,
                            command,
                            path,
                            enabled: auto,
                            // Windows'un kendi hizmetleri buradan kapatılmaz (sistemi bozabilir).
                            toggle: if windows { Toggle::None } else { Toggle::Service(id) },
                        });
                    }
                }
                let _ = CloseServiceHandle(svc);
            }
        }
        let _ = CloseServiceHandle(scm);
    }
}

pub fn tasks(out: &mut Vec<Item>) {
    let Ok(svc) = service() else { return };
    unsafe {
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
