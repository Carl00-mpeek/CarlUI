//! Bu modül "elimi klavyeye götürmeden 'YouTube'u aç', 'sesi 30 yap',
//! 'YouTube'da lofi ara' diyebilir miyim?" sorununu çözüyor (sesli asistan).
//! Kısayol (`shortcuts.assist`, varsayılan Ctrl+Alt+A) basılı tutulurken
//! mikrofon dinlenir; bırakınca ses yerel Whisper ile
//! yazıya dökülür, `carlui_launcher::voice` cümleyi anlar ve aramanın kendi
//! eylemiyle uygular. Kısayola basıp hemen bırakılırsa susunca biter.
//! Ada açıksa asistan aramanın içinde görünür (bkz. `assist_panel`): söylenenler
//! konuşurken arama kutusuna düşer, ayar açıksa uygulamadan önce onay bekler.
//! Whisper tuşa basınca açılır, yazıya dökünce kapanır. Akıllı asistan açıksa
//! dil modeli konuşurken açılır, yanıt verince kapanır; bellekte model beklemez.
//! Sürekli dinleme yoktur; mikrofon yalnızca kısayoldan sonra açılır. Ses buluta gitmez.

use crate::{
    ai,
    assist_panel::Panel,
    assist_warm,
    dictate::{loudest, short, wait_modifiers_up, SILENCE},
    progress::Shared,
};
use carlui_ai::{brain::Brain, catalog, mic::Mic, store, whisper::{self, Whisper}};
use carlui_launcher::{index, voice, voice_lines};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Threading::*},
    },
};

/// Basılı tutmada en uzun kayıt.
const HOLD_MAX: Duration = Duration::from_secs(15);
/// Bu süreden kısa basış "dokun-konuş"tur: susunca biter.
const TAP: Duration = Duration::from_millis(350);
/// Dokun-konuşta: konuşma bitti sayılan sessizlik, hiç konuşulmazsa bekleme, en uzun kayıt.
const QUIET_END: Duration = Duration::from_millis(800);
const NO_SPEECH: Duration = Duration::from_secs(5);
const TAP_MAX: Duration = Duration::from_secs(10);
/// Canlı yazı: bu kadar yeni ses birikince konuşma yeniden yazıya dökülür.
const LIVE_STEP: usize = whisper::SR * 6 / 10;

/// Komut için model: sesli komutta seçili Whisper (eski ayarlarda kalan Vosk seçimi önerilene döner).
pub(crate) fn model(cfg: &carlui_core::config::Config) -> &'static catalog::Model {
    catalog::chosen(&cfg.ai, catalog::Feature::Command)
}

/// Konuşma sürerken açık duran kilit; ikinci basış bunu görüp konuşmayı bitirir.
pub(crate) fn busy_mutex() -> Result<HANDLE> {
    unsafe { CreateMutexW(None, false, w!("Local\\CarlUI.Assist")) }
}

pub(crate) fn stop_event() -> Result<HANDLE> {
    unsafe { CreateEventW(None, false, false, w!("Local\\CarlUI.AssistStop")) }
}

pub(crate) fn opts(cfg: &carlui_core::config::Config, m: &'static catalog::Model, asked: bool) -> Opts {
    Opts {
        m,
        vk: if asked { 0 } else { cfg.shortcuts.assist_combo().map(|c| c.vk).unwrap_or(0) },
        lang: Some(cfg.ai.speech_lang.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| t!("tr", "en").into()),
        web: cfg.launcher.web_search.clone(),
        key: cfg.shortcuts.assist.clone(),
        confirm: cfg.ai.assist_confirm,
        smart: smart(cfg),
    }
}

pub(crate) type Loader = JoinHandle<std::result::Result<Whisper, String>>;

pub(crate) fn open(m: &'static catalog::Model) -> std::result::Result<Whisper, String> {
    ai::speech_dir(m).ok_or_else(|| t!("Model dosyası bulunamadı", "Model file not found").to_string()).and_then(|d| Whisper::open(&d))
}

pub(crate) fn load(m: &'static catalog::Model) -> Loader {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        open(m)
    })
}

/// `cold`: bekleyen sürece sormadan bu süreçte çalış.
pub fn run(owner: HWND, accent: Option<u32>, cold: bool) -> Result<()> {
    unsafe {
        let _mutex = busy_mutex()?;
        let running = GetLastError() == ERROR_ALREADY_EXISTS;
        let stop = stop_event()?;
        if running {
            // Dokun-konuş sürerken yeniden basış: hemen bitir.
            let _ = SetEvent(stop);
            return Ok(());
        }
        if !cold && assist_warm::hand_over() {
            return Ok(());
        }
        let _ = ResetEvent(stop);
        let cfg = carlui_core::config::load();
        let m = model(&cfg);
        let need = ai::missing_bytes(m);
        // İndirme sorulduysa tuş çoktan bırakılmıştır: dokun-konuş.
        let asked = need > 0;
        if asked && !crate::ask_model(owner, accent, m, need) {
            return Ok(());
        }
        let opts = opts(&cfg, m, asked);
        let stop_h = stop.0 as isize;
        // Model inecekse kartta ilerleme gösterilir; yoksa asistan aramada görünür.
        if let Some(p) = (!asked).then(Panel::open).flatten() {
            let worker = Shared::new();
            let _ = std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                work(&worker, &opts, HANDLE(stop_h as _), Some(&p), false);
            })
            .join();
            return Ok(());
        }
        let shared = Shared::new();
        shared.update(|s| s.title = t!("Dinliyor…", "Listening…").into());
        let worker = shared.clone();
        crate::progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
            std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                work(&worker, &opts, HANDLE(stop_h as _), None, false);
            });
        })
    }
}

pub(crate) struct Opts {
    m: &'static catalog::Model,
    /// Basılı tutulan tuş (0: dokun-konuş).
    vk: u32,
    pub lang: String,
    web: String,
    key: String,
    /// Uygulamadan önce onay beklensin (yalnızca aramada görünürken).
    confirm: bool,
    /// Akıllı asistan açık ve model kurulu: `Some(ekran kartında mı)`.
    smart: Option<bool>,
}

/// Akıllı asistan bu komutta kullanılacak mı, nerede (true = ekran kartı).
/// Oyunlar işlemciyi de ekran kartını da kullandığı için (CS2 gibi) oyunda
/// aygıt değiştirmek işe yaramaz: oyun öndeyken model, kullanıcı ayrıca izin
/// vermedikçe hiç açılmaz; yalnızca Whisper ve kurallar çalışır.
fn smart(cfg: &carlui_core::config::Config) -> Option<bool> {
    let ai = &cfg.ai;
    let on = ai.assistant && ai.smart && store::installed(catalog::recommended(catalog::Feature::Brain)) && (ai.smart_in_game || !carlui_core::win::game_in_front());
    on.then_some(ai.smart_gpu)
}

type Starting = JoinHandle<std::result::Result<Brain, String>>;

/// Dil modelini bu komut için açar; konuşma sürerken hazırlanır.
fn wake(gpu: bool) -> Starting {
    std::thread::spawn(move || Brain::start(catalog::recommended(catalog::Feature::Brain), gpu))
}

/// Gerekmediyse ya da iş bittiyse dil modeli beklemeden kapatılır.
fn sleep(b: Option<Starting>) {
    if let Some(h) = b {
        std::thread::spawn(move || drop(h.join()));
    }
}

/// Açılması bekleyip cümleyi sorar; açılamazsa kurallar çözer.
fn ask(b: &mut Option<Starting>, text: &str) -> Option<String> {
    match b.take()?.join().unwrap_or_else(|_| Err("panik".into())) {
        Ok(brain) => brain.ask(text),
        Err(e) => {
            carlui_core::warn!("asistan: dil modeli açılamadı: {e}");
            None
        }
    }
}

fn held(vk: u32) -> bool {
    carlui_core::hotkey::held(vk)
}

/// Model hazır olunca konuşma sürerken biriken sesi yazıya döküp adaya yollar.
/// Durdurulunca modeli son geçiş için geri verir.
fn live(loader: Loader, p: Arc<Panel>, heard: Arc<Mutex<Vec<f32>>>, halt: Arc<AtomicBool>, lang: String) -> Loader {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut wh = loader.join().unwrap_or_else(|_| Err("panik".into()))?;
        let (mut seen, mut last) = (0, String::new());
        while !halt.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(60));
            let pcm = heard.lock().map(|h| h.clone()).unwrap_or_default();
            if pcm.len() < seen + LIVE_STEP || loudest(&pcm) < SILENCE {
                continue;
            }
            seen = pcm.len();
            if let Ok((segs, _)) = wh.transcribe(&pcm, Some(&lang), false, &halt, &|_| {}) {
                let text = whisper::to_text(&segs).trim().to_string();
                if !text.is_empty() && text != last && !halt.load(Ordering::SeqCst) {
                    p.text(&text);
                    last = text;
                }
            }
        }
        Ok(wh)
    })
}

/// `lasting`: süreç iş bitince kapanmaz; uzun eylem (uygulama açılışı) beklenmez.
pub(crate) fn work(w: &Arc<Shared>, o: &Opts, stop: HANDLE, panel: Option<&Arc<Panel>>, lasting: bool) {
    if !ai::ensure(w, o.m) {
        return;
    }
    // Dil modeli konuşma sürerken açılır; nasıl biterse bitsin kapanır.
    struct Asleep(Option<Starting>);
    impl Drop for Asleep {
        fn drop(&mut self) {
            sleep(self.0.take());
        }
    }
    let mut brain = Asleep(o.smart.map(wake));
    let m = o.m;
    let fail = |e: String| {
        if let Some(p) = panel {
            p.done("", &e, true);
        }
        ai::fail(w, e);
    };
    // Kart açıkken konuşma duyulmadıysa söylenecek bir şey yok: kart sessizce kapanır.
    let unheard = || match panel {
        Some(_) => fail(t!("Konuşma duyulmadı", "No speech heard").into()),
        None => w.update(|s| {
            s.dismiss = true;
            s.done = true;
        }),
    };
    // Whisper ve uygulama listesi konuşurken arka planda hazırlanır.
    let heard = Arc::new(Mutex::new(Vec::new()));
    let halt = Arc::new(AtomicBool::new(false));
    // Konuşma nasıl biterse bitsin canlı yazı durur.
    struct Halt(Arc<AtomicBool>);
    impl Drop for Halt {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let _halt = Halt(halt.clone());
    let loader = match panel {
        Some(p) => live(load(m), p.clone(), heard.clone(), halt.clone(), o.lang.clone()),
        None => load(m),
    };
    let apps = std::thread::spawn(|| {
        let c = index::cached();
        if !c.is_empty() {
            return c;
        }
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        index::enumerate_apps()
    });
    let mut mic = match Mic::open() {
        Ok(m) => m,
        Err(e) => {
            carlui_core::warn!("asistan: mikrofon açılamadı: {e}");
            return fail(t!("Mikrofon açılamadı", "Couldn't open the microphone").into());
        }
    };
    let t0 = Instant::now();
    let mut hold = held(o.vk);
    let hint = |hold: bool| {
        if hold { t!("Bırakınca uygulanır", "Release to run").to_string() } else { t!(format!("Susunca uygulanır · durdurmak için {}", o.key), format!("Runs when you stop talking · {} to stop", o.key)) }
    };
    w.update(|s| {
        s.title = t!("Dinliyor…", "Listening…").into();
        s.file = hint(hold);
        s.frac = 0.0;
    });
    let mut pcm = Vec::new();
    let mut level = 0f32;
    let mut released: Option<Instant> = None;
    let mut voiced: Option<Instant> = None;
    let mut last_voice = t0;
    let mut floor = f32::MAX;
    loop {
        let stopped = unsafe { WaitForSingleObject(stop, 40) } == WAIT_OBJECT_0;
        let before = pcm.len();
        match mic.read(&mut pcm) {
            Ok(peak) => {
                level = if peak > level { peak } else { level * 0.85 };
                let shown = (level * 3.0).min(1.0);
                w.update(|s| s.frac = shown);
                if let Some(p) = panel {
                    p.level(shown);
                    if let Ok(mut h) = heard.lock() {
                        h.extend_from_slice(&pcm[before..]);
                    }
                }
            }
            Err(e) => {
                carlui_core::warn!("asistan: mikrofon okunamadı: {e}");
                break;
            }
        }
        if stopped || w.cancel.load(Ordering::SeqCst) {
            break;
        }
        let now = Instant::now();
        if hold {
            if !held(o.vk) {
                if now - t0 < TAP {
                    // Kısa basış: dokun-konuş'a geç.
                    hold = false;
                    w.update(|s| s.file = hint(false));
                } else if released.get_or_insert(now).elapsed() > Duration::from_millis(150) {
                    // Son hecenin kesilmemesi için bıraktıktan sonra kısa bir kuyruk.
                    break;
                }
            }
            if now - t0 > HOLD_MAX {
                break;
            }
            continue;
        }
        // Dokun-konuş: konuşma başlayıp sessizlik gelince bitir.
        let chunk = &pcm[before..];
        if !chunk.is_empty() {
            let rms = (chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32).sqrt();
            floor = floor.min(rms.max(0.0005));
            if rms > (floor * 4.0).max(SILENCE * 2.0) {
                voiced.get_or_insert(now);
                last_voice = now;
            }
        }
        let done = match voiced {
            Some(_) => now - last_voice > QUIET_END,
            None => now - t0 > NO_SPEECH,
        };
        if done || now - t0 > TAP_MAX {
            break;
        }
    }
    drop(mic);
    halt.store(true, Ordering::SeqCst);
    // Kısayolla durdurulduysa sinyal açık kalmasın; yoksa onay ekranı anında kapanır.
    let _ = unsafe { ResetEvent(stop) };
    if w.cancel.load(Ordering::SeqCst) {
        return;
    }
    let loud = loudest(&pcm);
    carlui_core::info!("asistan: {:.1} sn ses, en yüksek seviye {loud:.4}", pcm.len() as f32 / whisper::SR as f32);
    if loud == 0.0 && pcm.len() > whisper::SR / 2 {
        return fail(t!("Mikrofondan ses gelmiyor (sessiz ya da izin kapalı)", "No sound from the microphone (muted or access off)").into());
    }
    if loud < SILENCE || pcm.len() < whisper::SR / 4 {
        return unheard();
    }
    if let Some(p) = panel {
        p.thinking();
    }
    w.update(|s| {
        s.title = t!("Anlaşılıyor", "Understanding").into();
        s.file = o.m.name.into();
        s.frac = 0.0;
    });
    let mut wh = match loader.join().unwrap_or_else(|_| Err("panik".into())) {
        Ok(wh) => wh,
        Err(e) => {
            carlui_core::warn!("asistan: model açılamadı: {e}");
            return fail(t!("Model açılamadı", "Couldn't load the model").into());
        }
    };
    let t1 = Instant::now();
    let written = wh.transcribe(&pcm, Some(&o.lang), false, &w.cancel, &|f| w.update(|s| s.frac = f)).map(|(segs, _)| whisper::to_text(&segs).trim().to_string());
    // Whisper'ın işi bitti: bellek hemen geri verilir.
    drop(wh);
    let mut text = match written {
        Ok(text) => text,
        Err(_) if w.cancel.load(Ordering::SeqCst) => return,
        Err(e) => {
            carlui_core::warn!("asistan: yazıya dökülemedi: {e}");
            return fail(t!("Anlaşılamadı", "Didn't catch that").into());
        }
    };
    if panel.is_some_and(|p| p.cancelled()) {
        return;
    }
    if text.is_empty() {
        return unheard();
    }
    let reviewed = panel.filter(|_| o.confirm);
    if let Some(p) = reviewed {
        match p.review(&text, stop) {
            Some(t) => text = t,
            None => return,
        }
    }
    let apps = apps.join().unwrap_or_default();
    let intents = voice_lines::understand(&text, |t| ask(&mut brain.0, t));
    // Kurallar yettiyse ya da yanıt geldiyse dil modeli kapanır.
    drop(brain);
    let plan = voice_lines::check(voice_lines::plan_all(&intents, &apps, &index::running(), &o.web));
    carlui_core::info!("asistan: \"{text}\" → {intents:?} → {} ({} ms)", plan.title, t1.elapsed().as_millis());
    let ok = plan.ok();
    match panel {
        // Onaydan sonra arama kapanmıştır; olmayan iş bildirimle söylenir.
        Some(_) if reviewed.is_some() => {
            // Kur gibi yanıtlar da kapanan panelde görünmez; bildirimle söylenir.
            if !ok || matches!(plan.how, voice::How::Say(_)) {
                let data = serde_json::json!({ "app": "CarlUI", "title": plan.title, "body": text });
                carlui_core::bus::BusClient::send_once(&carlui_core::bus::Event::Custom { name: "island-note".into(), data });
            }
        }
        Some(p) => p.done(&text, &plan.title, !ok),
        None => {}
    }
    w.update(|s| {
        s.title = format!("“{}”", short(&text, 48));
        s.file = plan.title.clone();
        s.failed = !ok;
        s.done = true;
        s.frac = 1.0;
    });
    if let voice::How::Say(answer) = &plan.how {
        // Yalnızca yanıt: yapılacak iş yok, yanıt sesli okunur.
        return crate::speak::say(answer);
    }
    if ok {
        wait_modifiers_up();
        // Kabuk eylemleri (uygulama başlatma) tek iş parçacıklı COM ister.
        let how = plan.how.clone();
        let job = std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            voice::execute(&how);
        });
        if !lasting {
            let _ = job.join();
        }
    }
}
