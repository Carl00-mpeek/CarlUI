//! Testler yalnızca HKCU altında geçici `Software\CarlUI-Test-…` anahtarlarına
//! yazar ve sonunda siler; gerçek başlangıç girdilerine dokunmaz, hiçbir
//! uygulamayı açmaz/kapatmaz.

use super::{elevate, packages, reg, set_moved, Toggle};
use windows::{core::*, Win32::System::Registry::*};

/// Geçici test anahtarı; düşerken ağacıyla silinir.
struct Temp(String);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = format!("Software\\CarlUI-Test-{}-{tag}", std::process::id());
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(p.as_str()));
        }
        Temp(p)
    }
    fn path(&self, sub: &str) -> String {
        format!("{}\\{sub}", self.0)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.0.as_str()));
        }
    }
}

const HK: HKEY = HKEY_CURRENT_USER;

fn wide(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect()
}

#[test]
fn large_value_does_not_hide_the_rest() {
    let t = Temp::new("values");
    let k = t.path("Run");
    let big = "x".repeat(10_000);
    reg::set_value(HK, &k, "A_big", REG_SZ.0, &wide(&big)).ok().unwrap();
    for n in ["B", "C", "D"] {
        reg::set_value(HK, &k, n, REG_SZ.0, &wide(n)).ok().unwrap();
    }
    let v = reg::values(HK, &k);
    assert_eq!(v.len(), 4, "8 KB'tan büyük değerden sonrakiler de okunmalı");
    let a = v.iter().find(|x| x.0 == "A_big").unwrap();
    assert_eq!(reg::reg_string(&a.1), big);
}

#[test]
fn approved_round_trip() {
    let t = Temp::new("approved");
    let base = format!("{}\\", t.0);
    // Kayıt yoksa etkin sayılır.
    assert!(reg::approved(HK, &base, "Run", "App"));
    reg::set_approved(HK, &base, "Run", "App", false).ok().unwrap();
    assert!(!reg::approved(HK, &base, "Run", "App"));
    let (ty, d) = reg::value(HK, &t.path("Run"), "App").unwrap();
    assert_eq!((ty, d.len(), d[0]), (REG_BINARY.0, 12, 3));
    reg::set_approved(HK, &base, "Run", "App", true).ok().unwrap();
    assert!(reg::approved(HK, &base, "Run", "App"));
    assert_eq!(reg::value(HK, &t.path("Run"), "App").unwrap().1[0], 2);
}

#[test]
fn moved_entry_restored_byte_exact() {
    let t = Temp::new("moved");
    let (live, backup) = (t.path("RunOnce"), t.path("Backup"));
    let data = wide("\"%ProgramFiles%\\App\\app.exe\" --tray");
    reg::set_value(HK, &live, "App", REG_EXPAND_SZ.0, &data).ok().unwrap();
    reg::set_value(HK, &live, "Other", REG_SZ.0, &wide("o")).ok().unwrap();

    set_moved(HK, &live, &backup, "App", false).ok().unwrap();
    assert!(reg::value(HK, &live, "App").is_none(), "kapatınca asıl anahtardan çıkmalı");
    assert_eq!(reg::value(HK, &backup, "App"), Some((REG_EXPAND_SZ.0, data.clone())));

    set_moved(HK, &live, &backup, "App", true).ok().unwrap();
    assert_eq!(reg::value(HK, &live, "App"), Some((REG_EXPAND_SZ.0, data.clone())), "tür ve veri birebir geri gelmeli");
    assert!(reg::value(HK, &backup, "App").is_none());
    assert!(reg::value(HK, &live, "Other").is_some(), "diğer girdilere dokunulmamalı");

    // Uygulama girdisini kendisi yeniden yazdıysa açarken eski yedek onu ezmez.
    reg::set_value(HK, &backup, "App", REG_SZ.0, &wide("eski")).ok().unwrap();
    set_moved(HK, &live, &backup, "App", true).ok().unwrap();
    assert_eq!(reg::value(HK, &live, "App").unwrap().1, data);
    // Kapatırken eski yedeğin üzerine güncel girdi yazılır.
    reg::set_value(HK, &backup, "App", REG_SZ.0, &wide("eski")).ok().unwrap();
    set_moved(HK, &live, &backup, "App", false).ok().unwrap();
    assert_eq!(reg::value(HK, &backup, "App").unwrap().1, data);
    // İkisinde de yoksa hata.
    assert!(set_moved(HK, &live, &backup, "Yok", true).is_err());
}

#[test]
fn manifest_startup_tasks() {
    let whatsapp = r#"<Package><Applications><Application Id="App" Executable="WhatsApp.exe">
        <Extensions><uap5:Extension Category="windows.startupTask" Executable="WhatsApp.Root.exe" EntryPoint="Windows.FullTrustApplication">
        <uap5:StartupTask TaskId="WhatsAppStartupTask" Enabled="false" DisplayName="WhatsApp" /></uap5:Extension></Extensions></Application></Applications></Package>"#;
    let t = packages::parse_manifest(whatsapp);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].id.as_str(), t[0].enabled, t[0].display.as_str(), t[0].exe.as_str()), ("WhatsAppStartupTask", false, "WhatsApp", "WhatsApp.Root.exe"));

    let terminal = r#"<Application Id="App" Executable="WindowsTerminal.exe"><Extensions>
        <uap5:Extension Category="windows.startupTask"><uap5:StartupTask TaskId="StartTerminalOnLoginTask" Enabled="true" DisplayName="ms-resource:Name"/></uap5:Extension>
        </Extensions></Application>"#;
    let t = packages::parse_manifest(terminal);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].exe.as_str(), t[0].enabled, t[0].display.as_str()), ("WindowsTerminal.exe", true, ""));

    assert_eq!(packages::family("5319275A.WhatsAppDesktop_2.2540.6.0_x64__cv1g1gvanyjgm").as_deref(), Some("5319275A.WhatsAppDesktop_cv1g1gvanyjgm"));
}

#[test]
fn package_state_round_trip() {
    let t = Temp::new("pkg");
    let (f, task) = ("Fake.App_abc", "FakeTask");
    assert!(packages::is_on(packages::state(HK, &t.0, f, task), true), "değer yoksa manifest varsayılanı");
    packages::set_state(HK, &t.0, f, task, false).ok().unwrap();
    assert_eq!(packages::state(HK, &t.0, f, task), Some(1));
    assert!(!packages::is_on(packages::state(HK, &t.0, f, task), true));
    packages::set_state(HK, &t.0, f, task, true).ok().unwrap();
    assert_eq!(packages::state(HK, &t.0, f, task), Some(2));
    // İlke yönetiyorsa dokunulmaz.
    reg::set_value(HK, &format!("{}\\{f}\\{task}", t.0), "State", REG_DWORD.0, &3u32.to_le_bytes()).ok().unwrap();
    assert!(packages::set_state(HK, &t.0, f, task, true).is_err());
    assert_eq!(packages::state(HK, &t.0, f, task), Some(3));
}

#[test]
fn quote_survives_command_line_parsing() {
    use windows::Win32::UI::Shell::CommandLineToArgvW;
    let args = ["a \"b\"", "c:\\dir\\", "x\\\\\"y", "", "düz ad", "son\\"];
    let line = std::iter::once("prog".to_string()).chain(args.iter().map(|a| elevate::quote(a))).collect::<Vec<_>>().join(" ");
    unsafe {
        let mut n = 0;
        let argv = CommandLineToArgvW(&HSTRING::from(line.as_str()), &mut n);
        assert!(!argv.is_null());
        let got: Vec<String> = (1..n as usize).map(|i| (*argv.add(i)).to_string().unwrap()).collect();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(argv as _));
        assert_eq!(got, args);
    }
}

#[test]
fn helper_args_round_trip() {
    let all = [
        Toggle::Approved { hive: HKEY_LOCAL_MACHINE.0 as isize, key: "Run32", name: "Riot Vanguard".into() },
        Toggle::Moved { hive: HKEY_CURRENT_USER.0 as isize, src: "Policy", name: "x\\".into() },
        Toggle::Task("\\Klasör\\Görev".into()),
        Toggle::Service("svc".into()),
        Toggle::Package { family: "A_b".into(), task: "T".into() },
    ];
    for t in all {
        for on in [true, false] {
            assert_eq!(elevate::parse(&elevate::args(&t, on)), Some((t.clone(), on)));
        }
    }
    // Bilinmeyen anahtara yazdırılamaz.
    let bad = ["approved", "1", "0", "Winlogon", "x"].map(String::from);
    assert!(elevate::parse(&bad).is_none());
    assert_eq!(elevate::run_helper(&bad), 2);
}

/// Elle: `cargo test -p carlui-taskmgr scan_lists -- --ignored --nocapture`.
/// Yalnızca okur (gerçek sistemin taraması).
#[test]
#[ignore]
fn scan_lists() {
    for i in super::scan() {
        if !matches!(i.toggle, Toggle::Task(_) | Toggle::Service(_)) {
            println!("{} | {} | {} | {:?}", i.enabled, i.source, i.name, i.toggle);
        }
    }
}
