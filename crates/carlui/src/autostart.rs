//! Bu modül "kabuk Windows açılınca her şeyden önce başlasın" sorununu
//! Görev Zamanlayıcı ile çözüyor: oturum açılır açılmaz çalışan, gecikmesiz,
//! yönetici izni istemeyen bir görev kurar. Başlangıç klasörü ve Run anahtarı
//! Explorer'ın "başlangıç gecikmesi"ne takılır; zamanlanmış görev takılmaz.
//! Görev kurulamazsa HKCU Run anahtarına düşer. Ayar kapatılınca ikisi de silinir.

use carlui_core::{info, warn};
use std::{os::windows::process::CommandExt, process::Command};

const TASK: &str = "CarlUI";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const NO_WINDOW: u32 = 0x0800_0000;

fn cmd(exe: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(exe).args(args).creation_flags(NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>CarlUI kabuğu: oturum açılınca hemen başlar.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
    <StartWhenAvailable>false</StartWhenAvailable>
  </Settings>
  <Actions Context="Author"><Exec><Command>"{exe}"</Command></Exec></Actions>
</Task>"#,
        user = xml_escape(user),
        exe = xml_escape(exe)
    )
}

fn task_points_to(exe: &str) -> bool {
    cmd("schtasks", &["/Query", "/TN", TASK, "/XML"]).is_some_and(|x| x.to_lowercase().contains(&xml_escape(exe).to_lowercase()))
}

/// Ayara göre başlangıç kaydını kurar/kaldırır. Arka planda çağrılır.
pub fn apply(on: bool) {
    let Ok(exe) = std::env::current_exe().map(|p| p.display().to_string()) else { return };
    if !on {
        if cmd("schtasks", &["/Query", "/TN", TASK]).is_some() {
            let _ = cmd("schtasks", &["/Delete", "/TN", TASK, "/F"]);
            info!("başlangıç görevi kaldırıldı");
        }
        let _ = cmd("reg", &["delete", RUN_KEY, "/v", TASK, "/f"]);
        return;
    }
    if task_points_to(&exe) {
        let _ = cmd("reg", &["delete", RUN_KEY, "/v", TASK, "/f"]);
        return;
    }
    let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
    let file = std::env::temp_dir().join("carlui-task.xml");
    // schtasks XML'i UTF-16 bekler.
    let mut bytes = vec![0xFF, 0xFE];
    for u in task_xml(&exe, &user).encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let ok = std::fs::write(&file, bytes).is_ok() && cmd("schtasks", &["/Create", "/TN", TASK, "/XML", &file.display().to_string(), "/F"]).is_some();
    let _ = std::fs::remove_file(&file);
    if ok {
        info!("başlangıç görevi kuruldu (oturum açılınca hemen)");
        let _ = cmd("reg", &["delete", RUN_KEY, "/v", TASK, "/f"]);
    } else {
        warn!("zamanlanmış görev kurulamadı; Run anahtarı kullanılıyor");
        let _ = cmd("reg", &["add", RUN_KEY, "/v", TASK, "/t", "REG_SZ", "/d", &format!("\"{exe}\""), "/f"]);
    }
}
