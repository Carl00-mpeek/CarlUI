//! CarlUI ana süreci (`carlui.exe`).
//!
//! Bu süreç arayüz çizmez. Görevleri: görev çubuğunu yönetmek, modülleri
//! başlatıp çöktüklerinde yeniden başlatmak, olay yolunu barındırmak ve tam
//! ekran durumunu tüm modüllere duyurmak.
//!
//! Komut satırı:
//!   carlui.exe                    normal başlangıç
//!   carlui.exe --restore-taskbar  çalışanı kapat ve görev çubuğunu geri getir
//!   carlui.exe --quit             çalışanı düzgünce kapat
//!   carlui.exe --repair           CarlUI çalışmıyorsa görev çubuğunu ve imleci geri getir
//!   carlui.exe --guard <pid>      (iç kullanım) koruyucu süreç

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod fullscreen;
mod hotkey;
mod layouts;
mod pointer;
mod supervisor;
mod taskbar;

use carlui_core::{
    bus::{BusServer, Event},
    config::{self, Config},
    info, log, warn,
};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
    time::Duration,
};
use supervisor::Supervisor;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, Threading::*},
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};

const HOST_CLASS: PCWSTR = w!("CarlUI.Host");
const WM_APP_CONFIG: u32 = WM_APP + 1;
const TIMER_FULLSCREEN: usize = 1;
const TIMER_REAPPLY: usize = 2;
/// Öndeki pencere boyut değiştirdi (ör. video tam ekrana geçti): kısa gecikmeyle bak.
const TIMER_FS_SOON: usize = 3;
/// Windows imleç şemasını/boyutunu değiştirdi: bizimkini yeniden uygula.
const TIMER_CURSOR: usize = 4;
/// Pencere yerlerinin düzenli kaydı.
const TIMER_WINDOWS: usize = 5;
/// Ekran değişimi ya da uyanma yatıştı: pencereleri geri koy.
const TIMER_WINDOWS_BACK: usize = 6;

struct Host {
    hwnd: HWND,
    cfg: Config,
    bus: BusServer,
    sup: Supervisor,
    fullscreen: fullscreen::State,
    taskbar_created: u32,
    explorer_hook: HWINEVENTHOOK,
    explorer_pid: u32,
    pending_cfg: Arc<Mutex<Option<Config>>>,
    /// Öndeki uygulamanın pencere konumu kancası ve süreci (tam ekranda sökülür).
    follow_hook: HWINEVENTHOOK,
    follow_pid: u32,
    /// Ekran dizilimine göre hatırlanan pencere yerleri.
    windows: layouts::Memory,
}

fn explorer_pid() -> u32 {
    let mut pid = 0;
    if let Some(h) = carlui_core::win::explorer_tray() {
        unsafe {
            GetWindowThreadProcessId(h, Some(&mut pid));
        }
    }
    pid
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Ana iş parçacığındaki durumu kullanır. `SystemParametersInfo` gibi çağrılar
/// pencereye eşzamanlı mesaj gönderip yeniden girişe yol açabildiği için
/// meşgulse sessizce atlar.
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut h| h.as_mut().map(f)))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--guard") => guard(args.get(2).and_then(|p| p.parse().ok()).unwrap_or(0)),
        Some("--restore-taskbar") => {
            log::init("carlui");
            quit_running(Duration::from_secs(3));
            taskbar::restore();
        }
        Some("--quit") => {
            log::init("carlui");
            quit_running(Duration::from_secs(3));
        }
        Some("--repair") => repair(),
        _ => {
            if let Err(e) = run() {
                carlui_core::error!("başlatma hatası: {e}");
                if taskbar::hidden_by_us() {
                    taskbar::restore();
                }
            }
        }
    }
}

/// Güncelleme sırasında kilitli olduğu için kenara alınan eski exe'ler
/// (`*.exe.old`; örn. yönetici görevi olarak çalışan sensör). Açılışta artık
/// serbest olanlar silinir; hâlâ kullanılan sessizce kalır.
fn sweep_old() {
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return };
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().to_lowercase().ends_with(".exe.old") && std::fs::remove_file(e.path()).is_ok() {
            info!("eski dosya silindi: {}", e.file_name().to_string_lossy());
        }
    }
}

const HOST_MUTEX: PCWSTR = w!("Local\\CarlUI.Host");
const QUIT_EVENT: PCWSTR = w!("Local\\CarlUI.Quit");

/// Çalışan bir ana süreç var mı? (Pencere aramak yerine tekil-örnek mutex'ine bakılır;
/// pencere sınıfı araması bazı oturum/korumalı alan durumlarında başarısız olabiliyor.)
fn host_running() -> bool {
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, HOST_MUTEX) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

fn quit_running(timeout: Duration) {
    unsafe {
        let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, QUIT_EVENT) else { return };
        let _ = SetEvent(ev);
        let _ = CloseHandle(ev);
    }
    let start = std::time::Instant::now();
    while host_running() && start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// CarlUI ana süreci koruyucusuyla birlikte sonlandırıldıysa (Görev Yöneticisi'nde
/// "carlui.exe (2)" grubunu kapatmak) geri getirecek kimse kalmaz. Çalışan bir
/// CarlUI yoksa görev çubuğunu ve imleci geri getirir; varsa hiçbir şeye dokunmaz.
/// Anlık tekrar ve sesli klip kaldırıldı (0.9.3). Eski sürümün bıraktığı
/// geçici video parçalarını (birkaç GB olabilir) ve artık kullanılmayan Vosk
/// modelini sil. Kullanıcının kaydettiği klipler (Videolar\CarlUI) kalır.
fn remove_old_replay() {
    let data = carlui_core::paths::data_dir();
    for dir in [data.join("replay"), data.join("models").join("vosk-tr")] {
        if dir.is_dir() {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => info!("eski anlık tekrar dosyaları silindi: {}", dir.display()),
                Err(e) => warn!("{} silinemedi: {e}", dir.display()),
            }
        }
    }
}

fn repair() {
    log::init("carlui");
    // Koruyucu hâlâ yaşıyorsa önce o davransın.
    std::thread::sleep(Duration::from_millis(800));
    if host_running() {
        return;
    }
    if taskbar::hidden_by_us() {
        warn!("CarlUI sonlandırılmış; görev çubuğu geri getiriliyor");
        taskbar::restore();
    }
    carlui_core::cursor::restore();
}

/// Koruyucu: ana süreç bitene kadar bekler. Bittiğinde sistem hâlâ değiştirilmiş
/// durumdaysa (çökme, Görev Yöneticisi'nden sonlandırma) görev çubuğunu geri getirir.
fn guard(pid: u32) {
    log::init("guard");
    let mut code = 0u32;
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                WaitForSingleObject(h, INFINITE);
                let _ = GetExitCodeProcess(h, &mut code);
                let _ = CloseHandle(h);
            }
            Err(e) => warn!("ana süreç açılamadı ({pid}): {e}"),
        }
    }
    // Bu arada yeni bir CarlUI başladıysa çubuk onun sorumluluğunda.
    std::thread::sleep(Duration::from_millis(300));
    if host_running() {
        return;
    }
    // 0: düzgün kapanış (--quit, oturum kapanışı); 1: Görev Yöneticisi'nden
    // bilerek sonlandırma. Bunların dışı (panik, erişim ihlali) çöküştür.
    if code != 0 && code != 1 && crash_restart_allowed() {
        warn!("ana süreç çöktü (çıkış kodu {code:#x}); CarlUI yeniden başlatılıyor");
        if let Ok(me) = std::env::current_exe() {
            if std::process::Command::new(me).spawn().is_ok() {
                return;
            }
        }
    }
    if taskbar::hidden_by_us() {
        warn!("ana süreç beklenmedik şekilde kapandı; görev çubuğu geri getiriliyor");
        taskbar::restore();
    }
    carlui_core::cursor::restore();
}

/// Çöküş döngüsüne girmesin: 3 dakikada en çok 3 kez kendiliğinden yeniden başlat.
fn crash_restart_allowed() -> bool {
    let file = carlui_core::paths::data_dir().join("crash-restarts.txt");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut times: Vec<u64> = std::fs::read_to_string(&file).unwrap_or_default().lines().filter_map(|l| l.trim().parse().ok()).filter(|&t| now.saturating_sub(t) < 180).collect();
    if times.len() >= 3 {
        carlui_core::error!("ana süreç 3 dakikada 3 kez çöktü; otomatik yeniden başlatma durduruldu");
        return false;
    }
    times.push(now);
    let _ = std::fs::write(&file, times.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("\n"));
    true
}

fn hiding(cfg: &Config) -> bool {
    cfg.general.hide_taskbar
}

fn hook_explorer(old: HWINEVENTHOOK) -> HWINEVENTHOOK {
    unsafe {
        if !old.is_invalid() {
            let _ = UnhookWinEvent(old);
        }
        let mut pid = 0u32;
        if let Some(&tray) = taskbar::windows().first() {
            GetWindowThreadProcessId(tray, Some(&mut pid));
        }
        if pid == 0 {
            return HWINEVENTHOOK::default();
        }
        // Yalnızca explorer'ın "pencere göründü" olaylarını dinle: çubuğu kendisi
        // tekrar gösterirse anında geri gizleriz.
        SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(on_explorer_show), pid, 0, WINEVENT_OUTOFCONTEXT)
    }
}

unsafe extern "system" fn on_explorer_show(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW.0 || !taskbar::is_taskbar(hwnd) {
        return;
    }
    if with_host(|h| hiding(&h.cfg)).unwrap_or(false) {
        taskbar::rehide();
    }
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    with_host(|h| {
        check_fullscreen(h);
        hotkey::sync_rename(h.hwnd, &h.cfg);
        if fullscreen::desktop_is_foreground() {
            h.bus.publish(&Event::ShowDesktop);
        }
    });
}

unsafe extern "system" fn on_location(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW.0 || id_child != 0 || hwnd != GetForegroundWindow() {
        return;
    }
    // Boyutlandırma olay yağmurunu tek kontrolde birleştir.
    if let Some(host) = with_host(|h| h.hwnd) {
        SetTimer(host, TIMER_FS_SOON, 120, None);
    }
}

/// Video/F11 tam ekranı ön plan değişmeden olur: öndeki uygulamanın pencere
/// konumunu dinle ki dock beklemeden gizlensin. Tam ekrandayken kanca sökülür
/// (oyunun imleç kıpırtıları süreci uyandırmasın); çıkışı yoklama yakalar.
fn follow_foreground(h: &mut Host) {
    let mut pid = 0;
    if !h.fullscreen.active {
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        }
        if pid == std::process::id() {
            pid = 0;
        }
    }
    if pid == h.follow_pid {
        return;
    }
    unsafe {
        if !h.follow_hook.is_invalid() {
            let _ = UnhookWinEvent(h.follow_hook);
        }
        h.follow_hook = if pid == 0 {
            HWINEVENTHOOK::default()
        } else {
            SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_location), pid, 0, WINEVENT_OUTOFCONTEXT)
        };
    }
    h.follow_pid = pid;
}

fn check_fullscreen(h: &mut Host) {
    let s = fullscreen::query();
    if s != h.fullscreen {
        let class = if s.active { carlui_core::win::class_name(unsafe { GetForegroundWindow() }) } else { String::new() };
        info!("tam ekran: {} (özel: {}, monitör: {}, pencere: {class})", s.active, s.exclusive, s.monitor);
        h.bus.publish(&Event::Fullscreen { active: s.active, exclusive: s.exclusive, monitor: s.monitor.clone() });
        h.fullscreen = s;
    }
    follow_foreground(h);
}

fn shutdown(h: &mut Host) {
    info!("kapanıyor");
    h.bus.publish(&Event::Shutdown);
    h.sup.shutdown(Duration::from_millis(1500));
    // Durum dosyası olmasa da göster: başka bir örnek dosyayı silmiş olabilir.
    taskbar::restore();
    pointer::restore();
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER if wp.0 == TIMER_FULLSCREEN => {
                with_host(check_fullscreen);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_FS_SOON => {
                let _ = KillTimer(hwnd, TIMER_FS_SOON);
                with_host(check_fullscreen);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_REAPPLY => {
                let _ = KillTimer(hwnd, TIMER_REAPPLY);
                with_host(|h| {
                    if hiding(&h.cfg) {
                        taskbar::hide();
                    }
                    h.explorer_hook = hook_explorer(h.explorer_hook);
                });
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_WINDOWS => {
                with_host(|h| {
                    if h.cfg.tools.window_memory && !h.fullscreen.active {
                        h.windows.snapshot();
                    }
                });
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_WINDOWS_BACK => {
                let _ = KillTimer(hwnd, TIMER_WINDOWS_BACK);
                with_host(|h| {
                    if h.cfg.tools.window_memory {
                        h.windows.restore(h.fullscreen.active);
                    }
                });
                LRESULT(0)
            }
            WM_POWERBROADCAST if wp.0 == PBT_APMSUSPEND as usize => {
                with_host(|h| {
                    if h.cfg.tools.window_memory {
                        h.windows.snapshot();
                        h.windows.hold();
                    }
                });
                LRESULT(1)
            }
            WM_POWERBROADCAST if wp.0 == PBT_APMRESUMEAUTOMATIC as usize => {
                SetTimer(hwnd, TIMER_WINDOWS_BACK, 3000, None);
                LRESULT(1)
            }
            WM_TIMER if wp.0 == TIMER_CURSOR => {
                let _ = KillTimer(hwnd, TIMER_CURSOR);
                with_host(|h| {
                    if h.cfg.cursor.enabled {
                        pointer::sync(&h.cfg.cursor);
                    }
                });
                LRESULT(0)
            }
            WM_SETTINGCHANGE if wp.0 == SPI_SETCURSORS.0 as usize => {
                SetTimer(hwnd, TIMER_CURSOR, 400, None);
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                SetTimer(hwnd, TIMER_REAPPLY, 800, None);
                with_host(|h| {
                    if h.cfg.tools.window_memory {
                        h.windows.hold();
                    }
                });
                SetTimer(hwnd, TIMER_WINDOWS_BACK, 1500, None);
                LRESULT(0)
            }
            WM_APP_CONFIG => {
                with_host(|h| {
                    let Some(new) = h.pending_cfg.lock().ok().and_then(|mut p| p.take()) else { return };
                    match (hiding(&h.cfg), hiding(&new)) {
                        (false, true) => taskbar::hide(),
                        (true, false) => taskbar::restore(),
                        _ => {}
                    }
                    if h.cfg.cursor != new.cursor {
                        pointer::sync(&new.cursor);
                    }
                    if h.cfg.shortcuts != new.shortcuts || h.cfg.tools.rename_enabled != new.tools.rename_enabled || h.cfg.tools.disk_enabled != new.tools.disk_enabled || h.cfg.tools.unlock_enabled != new.tools.unlock_enabled {
                        hotkey::register(h.hwnd, &new);
                    }
                    if !new.tools.window_memory {
                        h.windows.clear();
                    }
                    h.cfg = new;
                });
                LRESULT(0)
            }
            WM_HOTKEY if wp.0 == hotkey::VAULT as usize => {
                hotkey::open_vault();
                LRESULT(0)
            }
            WM_HOTKEY => {
                hotkey::tool(wp.0 as i32);
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION if wp.0 != 0 => {
                with_host(shutdown);
                LRESULT(0)
            }
            WM_CLOSE => {
                with_host(shutdown);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                let created = with_host(|h| h.taskbar_created).unwrap_or(0);
                if created != 0 && msg == created {
                    // Tray modülü de ikonları yeniden istemek için bu yayını yapar;
                    // yalnızca explorer'ın süreci değiştiyse gerçek bir yeniden başlamadır.
                    let pid = explorer_pid();
                    let same = with_host(|h| std::mem::replace(&mut h.explorer_pid, pid) == pid).unwrap_or(false);
                    if same && pid != 0 {
                        return LRESULT(0);
                    }
                    info!("explorer yeniden başladı");
                    with_host(|h| h.bus.publish(&Event::ExplorerRestarted));
                    // Explorer çubuğu oluşturup bir süre sonra gösteriyor; biraz bekleyip gizle.
                    SetTimer(hwnd, TIMER_REAPPLY, 600, None);
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }
}

fn run() -> Result<()> {
    log::init("carlui");
    unsafe {
        let _mutex = CreateMutexW(None, true, HOST_MUTEX)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            info!("zaten çalışıyor, çıkılıyor");
            return Ok(());
        }

        if taskbar::hidden_by_us() {
            warn!("önceki oturum düzgün kapanmamış; önce sistem eski haline getiriliyor");
            taskbar::restore();
        }

        let cfg = config::load();
        let auto = cfg.general.start_with_windows;
        std::thread::Builder::new()
            .name("autostart".into())
            .spawn(move || {
                autostart::apply(auto);
                sweep_old();
            })
            .ok();
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: HOST_CLASS,
            ..Default::default()
        });
        // Yayın mesajlarını ("TaskbarCreated", WM_SETTINGCHANGE) alabilmek için
        // gizli ama üst düzey bir pencere (message-only pencereler yayın almaz).
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            HOST_CLASS,
            w!("CarlUI"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )?;
        let host_raw = hwnd.0 as isize;

        // `carlui.exe --quit` ve kaldırıcı bu olayı tetikleyerek düzgün kapanış ister.
        let quit_ev = CreateEventW(None, false, false, QUIT_EVENT)?.0 as isize;
        std::thread::Builder::new()
            .name("quit-wait".into())
            .spawn(move || {
                if WaitForSingleObject(HANDLE(quit_ev as _), INFINITE) == WAIT_OBJECT_0 {
                    info!("kapatma isteği alındı");
                    let _ = PostMessageW(HWND(host_raw as _), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            })
            .ok();

        let bus = BusServer::start(move |ev| match ev {
            Event::Hello { module } => info!("modül bağlandı: {module}"),
            Event::Custom { name, .. } if name == "quit" => {
                let _ = PostMessageW(HWND(host_raw as _), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            _ => {}
        });

        let sup = Supervisor::new();
        sup.run_guard();
        if hiding(&cfg) {
            taskbar::hide();
        }
        if cfg.cursor.enabled {
            pointer::sync(&cfg.cursor);
        }
        sup.run_module("dock", "carlui-dock.exe");
        sup.run_module("tray", "carlui-tray.exe");
        sup.run_module("island", "carlui-island.exe");
        sup.run_module("start", "carlui-start.exe");
        sup.run_module("expand", "carlui-expand.exe");
        sup.run_module_with("assist", "carlui-convert.exe", &["--assist-warm"]);
        std::thread::spawn(remove_old_replay);

        let pending_cfg: Arc<Mutex<Option<Config>>> = Arc::default();
        let pending = pending_cfg.clone();
        config::watch(move |c| {
            if let Ok(mut p) = pending.lock() {
                *p = Some(c);
            }
            let _ = PostMessageW(HWND(host_raw as _), WM_APP_CONFIG, WPARAM(0), LPARAM(0));
        });

        let fg_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        // Ön plan değişmeden tam ekrana geçen oyunlar için seyrek bir yedek kontrol.
        SetTimer(hwnd, TIMER_FULLSCREEN, 1500, None);
        SetTimer(hwnd, TIMER_WINDOWS, 5000, None);

        HOST.with(|h| {
            *h.borrow_mut() = Some(Host {
                hwnd,
                cfg,
                bus,
                sup,
                fullscreen: fullscreen::State::default(),
                taskbar_created: carlui_core::win::registered_message("TaskbarCreated"),
                explorer_hook: hook_explorer(HWINEVENTHOOK::default()),
                explorer_pid: explorer_pid(),
                pending_cfg,
                follow_hook: HWINEVENTHOOK::default(),
                follow_pid: 0,
                windows: layouts::Memory::default(),
            })
        });
        with_host(follow_foreground);
        with_host(|h| hotkey::register(h.hwnd, &h.cfg));
        info!("hazır");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWinEvent(fg_hook);
        if let Some(h) = HOST.with(|h| h.borrow_mut().take()) {
            let _ = UnhookWinEvent(h.explorer_hook);
            let _ = h.hwnd;
        }
    }
    Ok(())
}
