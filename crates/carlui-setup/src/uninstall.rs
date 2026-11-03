//! Bu modül "CarlUI iz bırakmadan ama kullanıcının dosyalarına dokunmadan
//! kaldırılsın" sorununu yalnızca kurucunun koyduğu şeyleri geri alarak
//! çözüyor: başlangıç görevi ve Run kaydı (bu konumu gösteriyorsa), sensör
//! görevi (UAC ile), kısayollar, kayıt defteri girişleri. Kurulan dosyalar tek
//! tek Geri Dönüşüm Kutusu'na gider; klasör bütünüyle silinmez, çünkü kullanıcı
//! içine kendi dosyalarını koymuş olabilir. Ayarlar ve veriler yalnızca
//! istenirse (yine Geri Dönüşüm Kutusu'na) kaldırılır.

use crate::{
    install::{ext_dir, xpi_path, Outcome, Reporter},
    payload, shell,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use windows::Win32::{Foundation::HWND, System::Com::*};

pub fn start(dir: PathBuf, purge: bool, dry: bool, rep: Reporter) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let o = run(&dir, purge, dry, &rep);
        rep.finish(o);
    });
}

fn points_into(s: &str, dir: &Path) -> bool {
    s.to_lowercase().contains(&dir.display().to_string().to_lowercase())
}

/// Kurulumun bu klasöre koyduğu dosyalar.
pub fn installed_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = payload::FILES.iter().map(|(n, _)| dir.join(n)).collect();
    v.push(dir.join(shell::UNINSTALLER));
    v.push(ext_dir(dir));
    v.push(xpi_path(dir));
    if let Ok(rd) = std::fs::read_dir(dir) {
        v.extend(rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().to_lowercase().ends_with(".exe.old")));
    }
    v.retain(|p| p.exists());
    v
}

fn run(dir: &Path, purge: bool, dry: bool, rep: &Reporter) -> Outcome {
    let mut notes = Vec::new();
    let files = installed_files(dir);
    if dry {
        notes.push(format!("Deneme: {} öge Geri Dönüşüm Kutusu'na gidecekti", files.len()));
        return Outcome { ok: true, notes };
    }

    rep.step("Çalışan CarlUI kapatılıyor", 0.1);
    if shell::quit_carlui(Duration::from_secs(6)) {
        std::thread::sleep(Duration::from_millis(1200));
    }

    rep.step("Başlangıç kaydı kaldırılıyor", 0.25);
    // Görev başka bir kopyayı (ör. geliştirme klasörü) gösteriyorsa ona dokunulmaz.
    if shell::run_hidden("schtasks", &["/Query", "/TN", "CarlUI", "/XML"]).is_some_and(|x| points_into(&x, dir)) {
        let _ = shell::run_hidden("schtasks", &["/Delete", "/TN", "CarlUI", "/F"]);
    }
    let run_key = r"Software\Microsoft\Windows\CurrentVersion\Run";
    if shell::reg_get(windows::Win32::System::Registry::HKEY_CURRENT_USER, run_key, "CarlUI").is_some_and(|v| points_into(&v, dir)) {
        shell::reg_delete_value(run_key, "CarlUI");
    }

    rep.step("CPU sıcaklığı görevi kaldırılıyor", 0.4);
    let sensors = dir.join("carlui-sensors.exe");
    if sensors.exists() && shell::run_hidden("schtasks", &["/Query", "/TN", "CarlUI Sensors", "/XML"]).is_some_and(|x| points_into(&x, dir)) {
        match shell::run_elevated(HWND(rep.hwnd as _), &sensors, "--uninstall") {
            shell::Elevated::Done => {}
            shell::Elevated::Declined => notes.push("CPU sıcaklığı görevi kaldırılmadı (yönetici izni verilmedi)".into()),
            shell::Elevated::Failed(e) => notes.push(format!("CPU sıcaklığı görevi kaldırılamadı: {e}")),
        }
    }

    rep.step("Kısayollar ve kayıtlar kaldırılıyor", 0.55);
    if let Some(m) = shell::menu_dir() {
        for n in ["CarlUI.lnk", "CarlUI Ayarları.lnk", "CarlUI'yi kaldır.lnk"] {
            let _ = std::fs::remove_file(m.join(n));
        }
        let _ = std::fs::remove_dir(&m);
    }
    if let Some(l) = shell::desktop_link() {
        if l.exists() && points_into(&shell::link_target(&l), dir) {
            let _ = std::fs::remove_file(&l);
        }
    }
    if shell::reg_get(windows::Win32::System::Registry::HKEY_CURRENT_USER, shell::FIREFOX_KEY, shell::EXT_ID).is_some_and(|v| points_into(&v, dir)) {
        shell::reg_delete_value(shell::FIREFOX_KEY, shell::EXT_ID);
    }
    shell::reg_delete_key(shell::UNINSTALL_KEY);

    rep.step("Dosyalar Geri Dönüşüm Kutusu'na taşınıyor", 0.75);
    if !shell::recycle(&files) {
        notes.push("Bazı dosyalar taşınamadı (hâlâ açık olabilirler)".into());
    }
    let _ = std::fs::remove_dir(dir.join("extension"));
    if std::fs::remove_dir(dir).is_err() && dir.exists() {
        notes.push(format!("Klasörde başka dosyalar olduğu için klasör yerinde bırakıldı: {}", dir.display()));
    }

    if purge {
        rep.step("Ayarlar ve veriler kaldırılıyor", 0.9);
        let data: Vec<PathBuf> = ["APPDATA", "LOCALAPPDATA"].iter().filter_map(|v| std::env::var_os(v)).map(|p| PathBuf::from(p).join("CarlUI")).collect();
        if !shell::recycle(&data) {
            notes.push("Ayar/veri klasörlerinin bir kısmı taşınamadı".into());
        }
    }
    notes.insert(0, "Kaldırılan dosyalar Geri Dönüşüm Kutusu'nda".into());
    Outcome { ok: true, notes }
}
