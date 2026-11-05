//! Bu modül "CarlUI'yi tek tıkla, yönetici izni istemeden ve çalışan sürümü
//! bozmadan kur" sorununu arka plan iş parçacığında sıralı adımlarla çözüyor:
//! çalışan CarlUI kibarca kapatılır (açıksa Carl Vault da kilitlenip kapanır,
//! sonda yeniden açılır), gömülü exe'ler kullanıcı klasörüne yazılır
//! (kilitli bir exe varsa yeniden adlandırılıp yenisi yanına konur), Programlar
//! listesine kayıt ve Başlat menüsü kısayolları eklenir. İsteğe bağlı adımlar:
//! masaüstü kısayolu, imzalı Firefox eklentisi, CarlUI duvar kâğıdı, Carl
//! Office (dosya türleriyle), CPU sıcaklığı görevi (UAC).
//! Başlangıç kaydını CarlUI'nin kendisi, açıldığında kendi konumuna göre kurar.

use crate::{office, payload, shell, WM_APP_PROGRESS};
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
    /// CarlUI duvar kâğıdını masaüstü arka planı yap.
    pub wallpaper: bool,
    /// Carl Office modülü (exe, Başlat kısayolu, dosya türü kayıtları).
    pub office: bool,
    /// Sesli asistanı aç (`ai.assistant`); model ilk kullanımda indirilir.
    pub assistant: bool,
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
    let total = (payload::FILES.len() + payload::EXTENSION.len() + 8) as f32;
    let mut n = 0.0f32;
    let mut tick = |rep: &Reporter, s: &str| {
        n += 1.0;
        rep.step(s, n / total);
    };

    if !o.dry {
        tick(rep, t!("Çalışan CarlUI kapatılıyor", "Closing running CarlUI"));
        if shell::quit_carlui(Duration::from_secs(6)) {
            // Modüller (dock, tepsi, ada…) ana süreçten sonra kapanır.
            std::thread::sleep(Duration::from_millis(1200));
        }
    }
    // Carl Vault açıksa kapatılır (kilitlenir) ve kurulumdan sonra yeni sürümü,
    // kilitli olarak, yeniden açılır.
    let vault_was_open = !o.dry && shell::quit_vault(Duration::from_secs(5));
    if !o.dry {
        let need = crate::space::needed(o, crate::space::installed_now(&o.dir));
        if let Some(free) = crate::space::free_bytes(&o.dir).filter(|f| *f < need) {
            return fail(vec![t!(format!("Diskte yeterli yer yok: {} gerekli, {} boş", payload::mb(need), payload::mb(free)), format!("Not enough disk space: {} needed, {} free", payload::mb(need), payload::mb(free)))]);
        }
    }
    if let Err(e) = std::fs::create_dir_all(&o.dir) {
        return fail(vec![t!(format!("Klasör oluşturulamadı: {} ({e})", o.dir.display()), format!("Couldn't create folder: {} ({e})", o.dir.display()))]);
    }
    clean_old(&o.dir);

    tick(rep, t!("Dosyalar açılıyor", "Unpacking files"));
    if payload::embedded() && payload::unpack().is_empty() {
        return fail(vec![t!("Kurucu bozuk: gömülü dosyalar açılamadı. Kurucuyu yeniden indirin.", "The installer is damaged: its files couldn't be unpacked. Download it again.").into()]);
    }

    let since = Instant::now();
    for e in payload::FILES {
        let (name, bytes) = (&e.name, e.bytes());
        if *name == office::EXE && !o.office {
            continue;
        }
        tick(rep,&format!("{} · {name}", t!("Kopyalanıyor", "Copying")));
        if let Err(e) = place(&o.dir.join(name), bytes, since) {
            if *name == "carlui.exe" {
                return fail(vec![t!(format!("{name} yazılamadı: {e}"), format!("Couldn't write {name}: {e}"))]);
            }
            notes.push(t!(format!("{name} güncellenemedi: {e}"), format!("Couldn't update {name}: {e}")));
        }
    }
    tick(rep, t!("Kaldırıcı ekleniyor", "Adding uninstaller"));
    let uninstaller = o.dir.join(shell::UNINSTALLER);
    // Gömülü küçük kaldırıcı; yoksa (geliştirme derlemesi) kurucunun kendisi.
    let small = payload::UNINSTALLER.as_ref().map(|u| u.bytes()).filter(|b| !b.is_empty());
    let bytes = match small {
        Some(b) => Some(Ok(std::borrow::Cow::Borrowed(b))),
        None => std::env::current_exe().ok().filter(|me| *me != uninstaller).map(|me| std::fs::read(me).map(std::borrow::Cow::Owned)),
    };
    if let Some(r) = bytes {
        if let Err(e) = r.and_then(|b| place(&uninstaller, &b, since)) {
            notes.push(t!(format!("Kaldırıcı yazılamadı: {e}"), format!("Couldn't write the uninstaller: {e}")));
        }
    }

    let ext = ext_dir(&o.dir);
    let _ = std::fs::create_dir_all(&ext);
    for e in payload::EXTENSION {
        let (name, bytes) = (e.name, e.bytes());
        tick(rep, &format!("{} · {name}", t!("Eklenti", "Extension")));
        let to = ext.join(name);
        if let Some(d) = to.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&to, bytes) {
            notes.push(t!(format!("Eklenti dosyası yazılamadı ({name}): {e}"), format!("Couldn't write extension file ({name}): {e}")));
        }
    }
    if let Some(x) = &payload::XPI {
        let _ = std::fs::write(xpi_path(&o.dir), x.bytes());
    }

    if o.dry {
        notes.push(t!("Deneme kurulumu: kayıt defteri, kısayol ve görev adımları atlandı", "Test install: registry, shortcut and task steps skipped").into());
        return Outcome { ok: true, notes };
    }

    tick(rep, t!("Programlar listesine ekleniyor", "Adding to installed apps"));
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
            ("EstimatedSize", shell::Val::Dword(payload::size_kb(o))),
            ("NoModify", shell::Val::Dword(1)),
            ("NoRepair", shell::Val::Dword(1)),
        ],
    );
    if !ok {
        notes.push(t!("Programlar listesine eklenemedi", "Couldn't add to installed apps").into());
    }

    tick(rep, t!("Kısayollar oluşturuluyor", "Creating shortcuts"));
    match shell::menu_dir() {
        Some(m) => {
            let a = shell::shortcut(&m.join("CarlUI.lnk"), &exe, "", t!("CarlUI kabuğunu başlatır", "Starts the CarlUI shell"));
            let b = shell::shortcut(&m.join(t!("CarlUI Ayarları.lnk", "CarlUI Settings.lnk")), &o.dir.join("carlui-settings.exe"), "", t!("CarlUI ayarları", "CarlUI settings"));
            let c = shell::shortcut(&m.join(t!("CarlUI'yi kaldır.lnk", "Uninstall CarlUI.lnk")), &uninstaller, "--uninstall", t!("CarlUI'yi kaldırır", "Uninstalls CarlUI"));
            let d = shell::shortcut(&m.join("Carl Vault.lnk"), &o.dir.join("carlui-vault.exe"), "", t!("Yerel şifreli parola kasası", "Local encrypted password vault"));
            if !(a && b && c && d) {
                notes.push(t!("Başlat menüsü kısayollarının bazıları oluşturulamadı", "Some Start menu shortcuts couldn't be created").into());
            }
        }
        None => notes.push(t!("Başlat menüsü klasörü bulunamadı", "Start menu folder not found").into()),
    }
    // Sesli asistan: yalnızca ayar; modeli ilk kullanımda (lisans onayıyla) indirilir.
    // Okunamayan ayar dosyasının üzerine varsayılanlar yazılmasın.
    let path = carlui_core::paths::config_file();
    let cfg = match std::fs::read_to_string(&path) {
        Ok(text) => carlui_core::config::from_text(&text).ok(),
        Err(_) => Some(carlui_core::config::Config::default()),
    };
    if let Some(mut cfg) = cfg.filter(|c| c.ai.assistant != o.assistant) {
        cfg.ai.assistant = o.assistant;
        carlui_core::config::save(&cfg);
    }
    office::remember(o.office);
    if o.office {
        tick(rep, t!("Carl Office dosya türleri kaydediliyor", "Registering Carl Office file types"));
        let link = shell::menu_dir().is_some_and(|m| shell::shortcut(&m.join(office::LINK), &o.dir.join(office::EXE), "", t!("Excel, Word, PowerPoint ve PDF dosyalarını açar", "Opens Excel, Word, PowerPoint and PDF files")));
        if !(office::register(&o.dir) && link) {
            notes.push(t!("Carl Office dosya türlerinin bir kısmı kaydedilemedi", "Some Carl Office file types couldn't be registered").into());
        }
    } else {
        office::remove(&o.dir);
    }
    if o.desktop && !shell::desktop_link().is_some_and(|l| shell::shortcut(&l, &exe, "", t!("CarlUI kabuğunu başlatır", "Starts the CarlUI shell"))) {
        notes.push(t!("Masaüstü kısayolu oluşturulamadı", "Couldn't create the desktop shortcut").into());
    }

    if o.firefox && payload::XPI.is_some() {
        let xpi = xpi_path(&o.dir).display().to_string();
        if shell::reg_set(shell::FIREFOX_KEY, &[(shell::EXT_ID, shell::Val::Str(&xpi))]) {
            notes.push(t!("Firefox bir sonraki açılışta eklentiyi etkinleştirmek için onay isteyecek", "Firefox will ask to enable the extension next time it starts").into());
        } else {
            notes.push(t!("Firefox eklentisi tanıtılamadı", "Couldn't register the Firefox extension").into());
        }
    }

    if o.wallpaper {
        tick(rep, t!("Duvar kâğıdı uygulanıyor", "Applying wallpaper"));
        let path = o.dir.join(r"wallpaper\carlui.png");
        let ok = path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && payload::WALLPAPER.as_ref().is_some_and(|w| std::fs::write(&path, w.bytes()).is_ok()) && shell::set_wallpaper(&path);
        if !ok {
            notes.push(t!("Duvar kâğıdı uygulanamadı", "Couldn't apply the wallpaper").into());
        }
    }

    if o.sensors {
        tick(rep, t!("CPU sıcaklığı görevi kuruluyor (yönetici izni)", "Setting up the CPU temperature task (admin)"));
        match shell::run_elevated(HWND(rep.hwnd as _), &o.dir.join("carlui-sensors.exe"), "--install") {
            shell::Elevated::Done => {}
            shell::Elevated::Declined => notes.push(t!("CPU sıcaklığı: yönetici izni verilmedi, görev kurulmadı", "CPU temperature: admin rights declined, task not set up").into()),
            shell::Elevated::Failed(e) => notes.push(t!(format!("CPU sıcaklığı görevi kurulamadı: {e}"), format!("Couldn't set up the CPU temperature task: {e}"))),
        }
    }

    if o.launch {
        tick(rep, t!("CarlUI başlatılıyor", "Starting CarlUI"));
        shell::open(&exe.display().to_string(), "");
    }
    // Bu kurulumda kilitli olduğu için kenara alınan eski exe'ler: modüller
    // yeni sürümle açıldıysa artık serbesttir.
    std::thread::sleep(Duration::from_millis(800));
    clean_old(&o.dir);
    if vault_was_open {
        shell::open(&o.dir.join("carlui-vault.exe").display().to_string(), "");
    }
    Outcome { ok: true, notes }
}
