//! Onaylanan artıkları kaldırır. Dosyalar Geri Dönüşüm Kutusu'na gider, kayıt
//! anahtarı yedeği alınamazsa silinmez. Normal kullanıcının silemedikleri için
//! bir kez yönetici izni istenir.

use crate::{
    leftovers::{Kind, Left},
    reg,
    safety::{self, Guard},
};
use carlui_core::paths;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Threading::{WaitForSingleObject, CREATE_NO_WINDOW, INFINITE},
        UI::{
            Shell::{ShellExecuteExW, SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FO_DELETE, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHFILEOPSTRUCTW},
            WindowsAndMessaging::SW_HIDE,
        },
    },
};

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub removed: usize,
    pub freed: u64,
    pub failed: Vec<String>,
    /// Kayıt anahtarı silindiyse yedeklerin klasörü.
    pub backup: Option<String>,
}

pub fn backup_dir() -> PathBuf {
    paths::data_dir().join("uninstall-backups")
}

fn allowed(g: &Guard, it: &Left) -> bool {
    match it.kind {
        Kind::Key => safety::key_ok(&it.path),
        _ => g.path_ok(&it.path),
    }
}

fn gone(it: &Left) -> bool {
    match it.kind {
        Kind::Key => !reg::exists(&it.path),
        _ => !Path::new(&it.path).exists(),
    }
}

/// Geri dönüşümü olmayan bir sürücüde Windows kalıcı silmeden önce uyarır.
fn recycle(path: &str) -> bool {
    let from: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let flags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING;
    let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: flags.0 as u16, ..Default::default() };
    unsafe { SHFileOperationW(&mut op) == 0 && !op.fAnyOperationsAborted.as_bool() }
}

fn backup(key: &str) -> bool {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = backup_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let leaf: String = key.rsplit('\\').next().unwrap_or("key").chars().filter(|c| c.is_alphanumeric()).take(40).collect();
    let file = dir.join(format!("{stamp}-{}-{}-{leaf}.reg", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let r = Command::new("reg.exe").args(["export", key]).arg(&file).args(["/y", "/reg:64"]).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW.0).status();
    r.is_ok_and(|s| s.success()) && file.exists()
}

fn remove(it: &Left) -> bool {
    match it.kind {
        Kind::Key => backup(&it.path) && reg::delete_tree(&it.path),
        _ => recycle(&it.path),
    }
}

/// Kendini yönetici olarak `--clean` ile açar ve bitmesini bekler. UAC
/// reddedilirse öğeler yerinde kalır, rapor bunları başarısız sayar.
fn elevate(items: &[Left]) {
    let Ok(json) = serde_json::to_vec(items) else { return };
    let file = std::env::temp_dir().join(format!("carlui-remover-{}.json", std::process::id()));
    if std::fs::write(&file, json).is_err() {
        return;
    }
    let Ok(me) = std::env::current_exe() else { return };
    let (exe, args) = (HSTRING::from(me.as_os_str()), HSTRING::from(format!("--clean \"{}\"", file.display())));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_ok() && !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
    let _ = std::fs::remove_file(&file);
}

/// Arka plan iş parçacığında çağrılır.
pub fn clean(items: &[Left]) -> Report {
    let guard = Guard::system();
    let mut retry = Vec::new();
    let mut rep = Report::default();
    for it in items {
        if !allowed(&guard, it) {
            carlui_core::warn!("kaldırıcı: korunan öğe atlandı: {}", it.path);
            rep.failed.push(it.path.clone());
        } else if !remove(it) || !gone(it) {
            retry.push(it.clone());
        }
    }
    if !retry.is_empty() {
        carlui_core::info!("kaldırıcı: {} öğe için yönetici izni isteniyor", retry.len());
        elevate(&retry);
    }
    for it in items {
        if rep.failed.contains(&it.path) {
            continue;
        }
        if gone(it) {
            rep.removed += 1;
            rep.freed += it.size;
            if it.kind == Kind::Key {
                rep.backup = Some(backup_dir().to_string_lossy().into_owned());
            }
        } else {
            rep.failed.push(it.path.clone());
        }
    }
    carlui_core::info!("kaldırıcı: {} öğe silindi, {} kaldı", rep.removed, rep.failed.len());
    rep
}

/// `--clean`: yönetici geçişi. Dosyadaki her öğe aynı denetimden yeniden geçer.
pub fn elevated(file: &str) {
    let Some(items) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<Vec<Left>>(&b).ok()) else { return };
    let guard = Guard::system();
    for it in items.iter().filter(|it| allowed(&guard, it)) {
        remove(it);
    }
}
