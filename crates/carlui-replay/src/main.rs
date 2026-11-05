//! Bu modül "az önce olanı (oyundaki anı) kaydetmeyi unuttum" sorununu
//! çözüyor: anlık tekrar. Açıkken ekran ve bilgisayarın sesi arka planda
//! sürekli kaydedilir, yalnızca son N dakika (1–30, varsayılan 5) diskte
//! tutulur; kısayola (varsayılan Alt+F10) basınca o süre tek bir MP4 olarak
//! Videolar\CarlUI'ya kaydedilir ve adada bildirim çıkar. Panel kısayolu
//! (varsayılan Alt+F8) ya da `--panel` küçük denetim panelini açar/kapatır.
//!
//! Hafif kalması için: görüntü DXGI masaüstü çoğaltma ile GPU'da alınır,
//! ekran kartının donanım kodlayıcısında H.264/H.265'e çevrilir (CPU'ya
//! inmez); kaydederken yeniden kodlama yapılmaz. Odak hiç alınmaz (panel de
//! etkinleşmez); özel tam ekran oyunlarda da çalışır. Fare imleci kayda girmez.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod audio;
mod capture;
mod mixer;
mod panel;
mod remux;
mod ring;
mod writer;

use carlui_core::{
    bus::{BusClient, Event},
    config::{self, Codec, Config, ReplayConfig, Resolution},
    log,
};
use mixer::Flags;
use ring::Ring;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::JoinHandle,
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Direct3D11::ID3D11Texture2D, Dxgi::IDXGIDevice},
        Media::MediaFoundation::*,
        System::{Com::*, SystemInformation::GetLocalTime, Threading::*},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_APP_RELOAD: u32 = WM_APP + 1;
/// Panelden: son süreyi kaydet.
const WM_APP_SAVE: u32 = WM_APP + 2;
/// İkinci örnekten ya da kısayoldan: paneli aç/kapat.
const WM_APP_PANEL: u32 = WM_APP + 3;
const HOTKEY: i32 = 1;
const HOTKEY_PANEL: i32 = 2;
const MSG_CLASS: PCWSTR = w!("CarlUI.Replay.Msg");

static PENDING: Mutex<Option<Config>> = Mutex::new(None);
pub static SAVING: AtomicBool = AtomicBool::new(false);
/// Komutların gönderildiği gizli mesaj penceresi.
static MSG_HWND: AtomicIsize = AtomicIsize::new(0);

/// Panelin gösterdiği durum (ana iş parçacığı günceller).
#[derive(Clone, Default)]
pub struct Status {
    pub cfg: ReplayConfig,
    pub save_key: String,
    pub panel_key: String,
    /// Kaydın başladığı an (kayıt yoksa None).
    pub started: Option<Instant>,
    /// Kayıt boyutu ve sıkıştırma (ör. "1920×1080 · H.265").
    pub info: String,
}

pub static STATUS: Mutex<Option<Status>> = Mutex::new(None);

/// Ana iş parçacığına komut gönderir (panelden).
fn post(msg: u32) {
    let h = MSG_HWND.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(HWND(h as _), msg, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn post_save() {
    post(WM_APP_SAVE);
}

type Current = Arc<Mutex<Option<Arc<Ring>>>>;

/// Açık bir kayıt: görüntü ve ses iş parçacıkları.
struct Session {
    cfg: ReplayConfig,
    stop: Arc<AtomicBool>,
    ring: Current,
    flags: Arc<Flags>,
    started: Instant,
    threads: Vec<JoinHandle<()>>,
}

/// Bu ekran kartı H.265'i donanımda kodlayabiliyor mu (bir kez denenir).
unsafe fn hevc_ok(fmt: &writer::Format, dm: &IMFDXGIDeviceManager) -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let p = temp_dir().join(format!("probe-{}.mp4", std::process::id()));
        let r = writer::Writer::open(&p, fmt, dm);
        let ok = r.is_ok();
        if let Ok(w) = r {
            let _ = w.finish();
        }
        let _ = std::fs::remove_file(&p);
        if !ok {
            warn!("H.265 donanım kodlayıcısı yok; H.264 kullanılıyor");
        }
        ok
    })
}

fn temp_dir() -> PathBuf {
    carlui_core::paths::data_dir().join("replay")
}

fn note(title: &str, body: &str) {
    let data = serde_json::json!({ "app": t!("Anlık tekrar", "Instant replay"), "title": title, "body": body });
    BusClient::send_once(&Event::Custom { name: "island-note".into(), data });
}

unsafe fn video_loop(cfg: ReplayConfig, stop: Arc<AtomicBool>, cur: Current) {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    let mut screen = match capture::open() {
        Ok(s) => s,
        Err(e) => {
            warn!("ekran yakalama açılamadı: {e}");
            return;
        }
    };
    // Oyun GPU'yu doldurduğunda kopyalarımız geride kalmasın.
    if let Ok(d) = screen.dev.cast::<IDXGIDevice>() {
        let _ = d.SetGPUThreadPriority(7);
    }
    let mut token = 0u32;
    let mut dm = None;
    if let Err(e) = MFCreateDXGIDeviceManager(&mut token, &mut dm) {
        warn!("MF aygıt yöneticisi açılamadı: {e}");
        return;
    }
    let Some(dm) = dm else { return };
    if let Err(e) = dm.ResetDevice(&screen.dev, token) {
        warn!("MF aygıt yöneticisi kurulamadı: {e}");
        return;
    }
    let fps = cfg.fps();
    let period = 10_000_000 / fps as i64;
    // Kodlayıcı kareyi eşzamansız okur: dokular sırayla yeniden kullanılır.
    const POOL: usize = 6;
    'outer: while !stop.load(Ordering::SeqCst) {
        let (w, h) = (screen.w & !1, screen.h & !1);
        let (ow, oh) = cfg.out_size(w, h);
        let mut fmt = writer::Format { w, h, ow, oh, hevc: cfg.codec == config::Codec::Hevc, fps, bitrate: cfg.bitrate(ow, oh), audio: true };
        if fmt.hevc && !hevc_ok(&fmt, &dm) {
            fmt.hevc = false;
            fmt.bitrate = ReplayConfig { codec: config::Codec::H264, ..cfg.clone() }.bitrate(ow, oh);
        }
        set_info(&fmt);
        let make = || -> Option<(ID3D11Texture2D, Vec<ID3D11Texture2D>)> {
            let p = screen.texture().ok()?;
            let pool = (0..POOL).map(|_| screen.texture().ok()).collect::<Option<Vec<_>>>()?;
            Some((p, pool))
        };
        let Some((pending, pool)) = make() else {
            warn!("kayıt dokuları oluşturulamadı");
            return;
        };
        let r = Ring::new(fmt, dm.clone(), temp_dir(), cfg.minutes());
        *cur.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.clone());
        info!("anlık tekrar kaydı başladı: {w}x{h} → {ow}x{oh} {} {fps} fps, {} Mbps, {} dk", if fmt.hevc { "H.265" } else { "H.264" }, fmt.bitrate / 1_000_000, cfg.minutes());
        let mut next = ring::now();
        let mut have = false;
        let mut fresh = false;
        let mut last = 0i64;
        let mut slot = 0;
        while !stop.load(Ordering::SeqCst) {
            let wait = ((next - ring::now()) / 10_000).clamp(0, 100) as u32;
            match screen.next(wait, &pending) {
                capture::Got::Frame => {
                    have = true;
                    fresh = true;
                }
                capture::Got::Idle => {}
                capture::Got::Lost => {
                    if (screen.w & !1, screen.h & !1) != (w, h) {
                        info!("ekran boyutu değişti ({w}x{h} → {}x{}), kayıt yeniden kuruluyor", screen.w, screen.h);
                        *cur.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        r.stop();
                        continue 'outer;
                    }
                }
            }
            let t = ring::now();
            if t < next {
                continue;
            }
            // Ekran değişmese de yarım saniyede bir kare: video donmuş görünmesin.
            if have && (fresh || t - last >= 5_000_000) {
                let tex = &pool[slot];
                slot = (slot + 1) % POOL;
                screen.ctx.CopyResource(tex, &pending);
                r.video(tex, t, period);
                last = t;
                fresh = false;
            }
            next += period;
            if next < t {
                next = t + period;
            }
        }
        *cur.lock().unwrap_or_else(|e| e.into_inner()) = None;
        r.stop();
    }
}

fn set_info(f: &writer::Format) {
    if let Some(s) = STATUS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        s.info = format!("{}×{} · {} fps · {}", f.ow, f.oh, f.fps, if f.hevc { "H.265" } else { "H.264" });
    }
}

impl Session {
    fn start(cfg: &ReplayConfig) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let ring: Current = Arc::new(Mutex::new(None));
        let mut threads = Vec::new();
        let (c, s, r) = (cfg.clone(), stop.clone(), ring.clone());
        if let Ok(h) = std::thread::Builder::new().name("replay-video".into()).spawn(move || unsafe { video_loop(c, s, r) }) {
            threads.push(h);
        }
        let flags = Arc::new(Flags::default());
        flags.audio.store(cfg.audio, Ordering::SeqCst);
        flags.mic.store(cfg.mic, Ordering::SeqCst);
        // Ses izi hep vardır: ses/mikrofon kayıt sürerken açılıp kapatılabilir.
        let (s, r, f) = (stop.clone(), ring.clone(), flags.clone());
        if let Ok(h) = std::thread::Builder::new().name("replay-audio".into()).spawn(move || unsafe { mixer::run(s, r, f) }) {
            threads.push(h);
        }
        Self { cfg: cfg.clone(), stop, ring, flags, started: Instant::now(), threads }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads {
            let _ = t.join();
        }
        info!("anlık tekrar kaydı durdu");
    }

    /// Ayar değişince baştan kurmak gerekiyor mu (süre ve ses değişimi kurmaz).
    fn same(&self, c: &ReplayConfig) -> bool {
        self.cfg.fps() == c.fps() && self.cfg.quality == c.quality && self.cfg.resolution == c.resolution && self.cfg.codec == c.codec
    }
}

fn file_name() -> String {
    let t = unsafe { GetLocalTime() };
    format!("Replay_{}-{:02}-{:02}_{:02}-{:02}-{:02}.mp4", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Son süreyi `folder`'a yazar (çağıran iş parçacığında).
unsafe fn save_now(ring: &Arc<Ring>, folder: &std::path::Path) -> (PathBuf, Result<f64>) {
    let segs = ring.cut(ring::now());
    let _ = std::fs::create_dir_all(folder);
    let out = folder.join(file_name());
    let started = std::time::Instant::now();
    let res = remux::join(&segs, &out);
    ring.release();
    if let Ok(secs) = &res {
        info!("tekrar kaydedildi: {} ({secs:.0} sn, {} ms)", out.display(), started.elapsed().as_millis());
    }
    (out, res)
}

/// Yalnızca debug derlemede: CARLUI_REPLAY_TEST=<sn> verilirse ayarlara
/// dokunmadan o kadar kaydedip CARLUI_REPLAY_OUT klasörüne yazar ve çıkar.
unsafe fn self_test() -> bool {
    let Some(secs) = std::env::var("CARLUI_REPLAY_TEST").ok().filter(|_| cfg!(debug_assertions)).and_then(|s| s.parse::<u64>().ok()) else {
        return false;
    };
    let folder = PathBuf::from(std::env::var("CARLUI_REPLAY_OUT").unwrap_or_else(|_| ".".into()));
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let resolution = match env("CARLUI_REPLAY_RES").as_str() {
        "720" => Resolution::P720,
        "1080" => Resolution::P1080,
        "1440" => Resolution::P1440,
        _ => Resolution::Native,
    };
    let codec = if env("CARLUI_REPLAY_CODEC") == "hevc" { Codec::Hevc } else { Codec::H264 };
    let rc = ReplayConfig { enabled: true, minutes: 1, resolution, codec, mic: env("CARLUI_REPLAY_MIC") == "1", ..Default::default() };
    let s = Session::start(&rc);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let ring = s.ring.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match ring {
        Some(r) => {
            let (out, res) = save_now(&r, &folder);
            println!("{} -> {res:?}", out.display());
        }
        None => println!("kayıt başlamadı"),
    }
    s.stop();
    true
}

fn save(ring: Arc<Ring>, folder: PathBuf) {
    if SAVING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("replay-save".into()).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let (out, res) = save_now(&ring, &folder);
        match res {
            Ok(secs) => {
                let len = if secs >= 60.0 { format!("{}:{:02}", secs as u32 / 60, secs as u32 % 60) } else { format!("{secs:.0} sn") };
                note(t!("Tekrar kaydedildi", "Replay saved"), &format!("{len} · {}", out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            Err(e) => {
                warn!("tekrar kaydedilemedi: {e}");
                let _ = std::fs::remove_file(&out);
                note(t!("Tekrar kaydedilemedi", "Replay not saved"), &e.message());
            }
        }
        SAVING.store(false, Ordering::SeqCst);
    });
}

unsafe fn hotkey(id: i32, c: Option<carlui_core::keys::Combo>, name: &str) {
    let _ = UnregisterHotKey(None, id);
    let Some(c) = c else { return };
    let mut mods = MOD_NOREPEAT;
    for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
        if on {
            mods |= m;
        }
    }
    if let Err(e) = RegisterHotKey(None, id, mods, c.vk) {
        warn!("{name} kısayolu kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}");
    }
}

unsafe fn register(cfg: &Config) {
    hotkey(HOTKEY, cfg.shortcuts.replay_combo().filter(|_| cfg.replay.enabled), "anlık tekrar");
    // Panel kısayolu kayıt kapalıyken de çalışır (paneli açıp kaydı başlatmak için).
    hotkey(HOTKEY_PANEL, cfg.shortcuts.replay_panel_combo(), "anlık tekrar paneli");
}

/// Ayarlara göre kaydı başlatır, durdurur ya da süresini günceller.
fn apply(sess: &mut Option<Session>, cfg: &Config) {
    let rc = &cfg.replay;
    match sess {
        Some(s) if !rc.enabled || !s.same(rc) => {
            sess.take().unwrap().stop();
        }
        Some(s) => {
            if s.cfg.minutes() != rc.minutes() {
                if let Some(r) = s.ring.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    r.set_keep(rc.minutes());
                }
            }
            s.flags.audio.store(rc.audio, Ordering::SeqCst);
            s.flags.mic.store(rc.mic, Ordering::SeqCst);
            s.cfg = rc.clone();
        }
        None => {}
    }
    mixer::GAIN_SYS.store(rc.audio_volume.min(ReplayConfig::MAX_VOLUME), Ordering::SeqCst);
    mixer::GAIN_MIC.store(rc.mic_volume.min(ReplayConfig::MAX_VOLUME), Ordering::SeqCst);
    if rc.enabled && sess.is_none() {
        *sess = Some(Session::start(rc));
    }
    {
        let mut st = STATUS.lock().unwrap_or_else(|e| e.into_inner());
        let info = st.as_ref().map(|s| s.info.clone()).unwrap_or_default();
        *st = Some(Status { cfg: rc.clone(), save_key: cfg.shortcuts.replay.clone(), panel_key: cfg.shortcuts.replay_panel.clone(), started: sess.as_ref().map(|s| s.started), info });
    }
    panel::refresh();
    unsafe { register(cfg) };
}

fn main() -> Result<()> {
    log::init("replay");
    unsafe {
        let want_panel = std::env::args().any(|a| a == "--panel");
        let _mutex = CreateMutexW(None, true, w!("Local\\CarlUI.Replay"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Zaten çalışıyor: `--panel` ise ona paneli açtır.
            if want_panel {
                if let Ok(h) = FindWindowExW(HWND_MESSAGE, None, MSG_CLASS, None) {
                    let _ = PostMessageW(h, WM_APP_PANEL, WPARAM(0), LPARAM(0));
                }
            }
            return Ok(());
        }
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        // Önceki çalıştırmadan kalan geçici segmentler.
        let dir = temp_dir();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        if self_test() {
            let _ = MFShutdown();
            return Ok(());
        }

        let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let wc = WNDCLASSW { lpfnWndProc: Some(msg_proc), hInstance: inst.into(), lpszClassName: MSG_CLASS, ..Default::default() };
        RegisterClassW(&wc);
        let mw = CreateWindowExW(WINDOW_EX_STYLE(0), MSG_CLASS, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, HWND_MESSAGE, None, inst, None)?;
        MSG_HWND.store(mw.0 as isize, Ordering::SeqCst);

        let mut cfg = config::load();
        let mut sess = None;
        apply(&mut sess, &cfg);

        let tid = GetCurrentThreadId();
        config::watch(move |c| {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
            let _ = PostThreadMessageW(tid, WM_APP_RELOAD, WPARAM(0), LPARAM(0));
        });

        if want_panel {
            panel::toggle();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_APP_PANEL => panel::toggle(),
                WM_HOTKEY if msg.wParam.0 as i32 == HOTKEY_PANEL => panel::toggle(),
                WM_APP_RELOAD => {
                    if let Some(c) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        apply(&mut sess, &c);
                        cfg = c;
                    }
                }
                WM_APP_SAVE => {
                    let ring = sess.as_ref().and_then(|s| s.ring.lock().unwrap_or_else(|e| e.into_inner()).clone());
                    if let Some(r) = ring {
                        save(r, cfg.replay.folder());
                    }
                }
                WM_HOTKEY if msg.wParam.0 as i32 == HOTKEY => {
                    let ring = sess.as_ref().and_then(|s| s.ring.lock().unwrap_or_else(|e| e.into_inner()).clone());
                    match ring {
                        Some(r) => save(r, cfg.replay.folder()),
                        None => note(t!("Anlık tekrar", "Instant replay"), t!("Kayıt henüz başlamadı.", "Recording has not started yet.")),
                    }
                }
                _ => {
                    DispatchMessageW(&msg);
                }
            }
        }
        panel::close();
        if let Some(s) = sess.take() {
            s.stop();
        }
        let _ = MFShutdown();
    }
    Ok(())
}

unsafe extern "system" fn msg_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}
