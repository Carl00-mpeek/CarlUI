//! Bu modül "CarlUI'yi tek tıkla, yönetici izni istemeden ve çalışan sürümü
//! bozmadan kur" sorununu arka plan iş parçacığında sıralı adımlarla çözüyor:
//! çalışan CarlUI kibarca kapatılır, gömülü exe'ler kullanıcı klasörüne yazılır
//! (kilitli bir exe varsa yeniden adlandırılıp yenisi yanına konur), Programlar
//! listesine kayıt ve Başlat menüsü kısayolları eklenir. İsteğe bağlı adımlar:
//! masaüstü kısayolu, imzalı Firefox eklentisi, CPU sıcaklığı görevi (UAC).
//! Başlangıç kaydını CarlUI'nin kendisi, açıldığında kendi konumuna göre kurar.

use crate::{payload, shell, WM_APP_PROGRESS};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    System::Com::*,
    UI::WindowsAndMessaging::PostMessageW,
};

#[derive(Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub desktop: bool,
    pub launch: bool,
    pub sensors: bool,
    pub firefox: bool,
    /// Deneme: yalnızca dosyalar yazılır; kayıt defteri, kısayol, görev yok.
    pub dry: bool,
}

#[derive(Clone)]
pub struct Outcome {
    pub ok: bool,
    pub notes: Vec<String>,
}

#[derive(Default)]
pub struct Progress {
    pub step: String,
    pub frac: f32,
    pub done: Option<Outcome>,
}

pub type Shared = Arc<Mutex<Progress>>;

pub struct Reporter {
    pub hwnd: isize,
    pub shared: Shared,
}

impl Reporter {
    pub fn step(&self, step: &str, frac: f32) {
        if let Ok(mut p) = self.shared.lock() {
            p.step = step.to_string();
            p.frac = frac;
        }
        self.post();
    }

    pub fn finish(&self, o: Outcome) {
        if let Ok(mut p) = self.shared.lock() {
            p.frac = 1.0;
            p.done = Some(o);
        }
        self.post();
    }

    fn post(&self) {
        if self.hwnd != 0 {
            unsafe {
                let _ = PostMessageW(HWND(self.hwnd as _), WM_APP_PROGRESS, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn start(opts: Options, rep: Reporter) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let o = run(&opts, &rep);
        rep.finish(o);
    });
}

/// Dosyayı yazar. Aynısı zaten varsa dokunmaz; exe hâlâ çalışıyorsa (kilitli)
/// birkaç saniye bekler, olmazsa eskisini `.old` adıyla kenara alır.
fn place(path: &Path, bytes: &[u8], since: Instant) -> std::io::Result<()> {
    if std::fs::read(path).is_ok_and(|b| b == bytes) {
        return Ok(());
    }
    loop {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(_) if since.elapsed() < Duration::from_secs(4) => std::thread::sleep(Duration::from_millis(250)),
            Err(_) => break,
        }
    }
    let old = path.with_file_name(format!("{}.old", path.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_file(&old);
    std::fs::rename(path, &old)?;
    std::fs::write(path, bytes)
}

/// Önceki kurulumların kenara aldığı `.old` dosyaları (artık kilitli değillerse).
fn clean_old(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.ends_with(".exe.old") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

pub fn ext_dir(dir: &Path) -> PathBuf {
    dir.join(r"extension\carlui-downloads")
}

pub fn xpi_path(dir: &Path) -> PathBuf {
    dir.join(r"extension\carlui-downloads.xpi")
}

fn run(o: &Options, rep: &Reporter) -> Outcome {
    let mut notes = Vec::new();
    let fail = |notes: Vec<String>| Outcome { ok: false, notes };
    let total = (payload::FILES.len() + payload::EXTENSION.len() + 6) as f32;
    let mut n = 0.0f32;
    let mut tick = |rep: &Reporter, s: &str| {
        n += 1.0;
        rep.step(s, n / total);
    };

    if !o.dry {
        tick(rep, "Çalışan CarlUI kapatılıyor");
        if shell::quit_carlui(Duration::from_secs(6)) {
            // Modüller (dock, tepsi, ada…) ana süreçten sonra kapanır.
            std::thread::sleep(Duration::from_millis(1200));
        }
    }
    if let Err(e) = std::fs::create_dir_all(&o.dir) {
        return fail(vec![format!("Klasör oluşturulamadı: {} ({e})", o.dir.display())]);
    }
    clean_old(&o.dir);

    let since = Instant::now();
    for (name, bytes) in payload::FILES {
        tick(rep, &format!("Kopyalanıyor · {name}"));
        if let Err(e) = place(&o.dir.join(name), bytes, since) {
            if *name == "carlui.exe" {
                return fail(vec![format!("{name} yazılamadı: {e}")]);
            }
            notes.push(format!("{name} güncellenemedi: {e}"));
        }
    }
    tick(rep, "Kaldırıcı ekleniyor");
    let uninstaller = o.dir.join(shell::UNINSTALLER);
    match std::env::current_exe() {
        Ok(me) if me != uninstaller => match std::fs::read(&me) {
            Ok(b) => {
                if let Err(e) = place(&uninstaller, &b, since) {
                    notes.push(format!("Kaldırıcı yazılamadı: {e}"));
                }
            }
            Err(e) => notes.push(format!("Kaldırıcı yazılamadı: {e}")),
        },
        _ => {}
    }

    let ext = ext_dir(&o.dir);
    let _ = std::fs::create_dir_all(&ext);
    for (name, bytes) in payload::EXTENSION {
        tick(rep, &format!("Eklenti · {name}"));
        if let Err(e) = std::fs::write(ext.join(name), bytes) {
            notes.push(format!("Eklenti dosyası yazılamadı ({name}): {e}"));
        }
    }
    if let Some(x) = payload::XPI {
        let _ = std::fs::write(xpi_path(&o.dir), x);
    }

    if o.dry {
        notes.push("Deneme kurulumu: kayıt defteri, kısayol ve görev adımları atlandı".into());
        return Outcome { ok: true, notes };
    }

    tick(rep, "Programlar listesine ekleniyor");
    let dir = o.dir.display().to_string();
    let exe = o.dir.join("carlui.exe");
    let uninstall = format!("\"{}\" --uninstall", uninstaller.display());
    let icon = format!("{},0", exe.display());
    let ok = shell::reg_set(
        shell::UNINSTALL_KEY,
        &[
            ("DisplayName", shell::Val::Str("CarlUI")),
            ("DisplayVersion", shell::Val::Str(env!("CARGO_PKG_VERSION"))),
            ("Publisher", shell::Val::Str("CarlUI")),
            ("DisplayIcon", shell::Val::Str(&icon)),
            ("InstallLocation", shell::Val::Str(&dir)),
            ("UninstallString", shell::Val::Str(&uninstall)),
            ("EstimatedSize", shell::Val::Dword(payload::size_kb())),
            ("NoModify", shell::Val::Dword(1)),
            ("NoRepair", shell::Val::Dword(1)),
        ],
    );
    if !ok {
        notes.push("Programlar listesine eklenemedi".into());
    }

    tick(rep, "Kısayollar oluşturuluyor");
    match shell::menu_dir() {
        Some(m) => {
            let a = shell::shortcut(&m.join("CarlUI.lnk"), &exe, "", "CarlUI kabuğunu başlatır");
            let b = shell::shortcut(&m.join("CarlUI Ayarları.lnk"), &o.dir.join("carlui-settings.exe"), "", "CarlUI ayarları");
            let c = shell::shortcut(&m.join("CarlUI'yi kaldır.lnk"), &uninstaller, "--uninstall", "CarlUI'yi kaldırır");
            if !(a && b && c) {
                notes.push("Başlat menüsü kısayollarının bazıları oluşturulamadı".into());
            }
        }
        None => notes.push("Başlat menüsü klasörü bulunamadı".into()),
    }
    if o.desktop && !shell::desktop_link().is_some_and(|l| shell::shortcut(&l, &exe, "", "CarlUI kabuğunu başlatır")) {
        notes.push("Masaüstü kısayolu oluşturulamadı".into());
    }

    if o.firefox && payload::XPI.is_some() {
        let xpi = xpi_path(&o.dir).display().to_string();
        if shell::reg_set(shell::FIREFOX_KEY, &[(shell::EXT_ID, shell::Val::Str(&xpi))]) {
            notes.push("Firefox bir sonraki açılışta eklentiyi etkinleştirmek için onay isteyecek".into());
        } else {
            notes.push("Firefox eklentisi tanıtılamadı".into());
        }
    }

    if o.sensors {
        tick(rep, "CPU sıcaklığı görevi kuruluyor (yönetici izni)");
        match shell::run_elevated(HWND(rep.hwnd as _), &o.dir.join("carlui-sensors.exe"), "--install") {
            shell::Elevated::Done => {}
            shell::Elevated::Declined => notes.push("CPU sıcaklığı: yönetici izni verilmedi, görev kurulmadı".into()),
            shell::Elevated::Failed(e) => notes.push(format!("CPU sıcaklığı görevi kurulamadı: {e}")),
        }
    }

    if o.launch {
        tick(rep, "CarlUI başlatılıyor");
        shell::open(&exe.display().to_string(), "");
    }
    Outcome { ok: true, notes }
}
