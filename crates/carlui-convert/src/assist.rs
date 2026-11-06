//! Bu modül "elimi klavyeye götürmeden 'YouTube'u aç', 'sesi 30 yap',
//! 'YouTube'da lofi ara' diyebilir miyim?" sorununu çözüyor (sesli asistan).
//! Kısayol (`shortcuts.assist`, varsayılan Ctrl+Alt+A) basılı tutulurken
//! mikrofon dinlenir; bırakınca ses yerel Whisper ile
//! yazıya dökülür, `carlui_launcher::voice` cümleyi anlar ve aramanın kendi
//! eylemiyle uygular. Kısayola basıp hemen bırakılırsa susunca biter.
//! Model ancak konuşma bitince çalışır; sürekli dinleme yoktur. Ses buluta gitmez.

use crate::{
    ai,
    dictate::{loudest, short, wait_modifiers_up, SILENCE},
    progress::Shared,
};
use carlui_ai::{catalog, mic::Mic, whisper::{self, Whisper}};
use carlui_launcher::{index, voice};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Threading::*},
        UI::Input::KeyboardAndMouse::GetAsyncKeyState,
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

/// Komut için model: sesli komutta seçili Whisper; Vosk seçiliyse (yalnızca
/// anlık tekrar cümlelerini tanır) önerilen Whisper.
fn model(cfg: &carlui_core::config::Config) -> &'static catalog::Model {
    let m = catalog::chosen(&cfg.ai, catalog::Feature::Command);
    if m.id.starts_with("vosk") { catalog::recommended(catalog::Feature::Command) } else { m }
}

pub fn run(owner: HWND, accent: Option<u32>) -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, false, w!("Local\\CarlUI.Assist"))?;
        let running = GetLastError() == ERROR_ALREADY_EXISTS;
        let stop = CreateEventW(None, false, false, w!("Local\\CarlUI.AssistStop"))?;
        if running {
            // Dokun-konuş sürerken yeniden basış: hemen bitir.
            let _ = SetEvent(stop);
            return Ok(());
        }
        let _ = ResetEvent(stop);
        let cfg = carlui_core::config::load();
        let m = model(&cfg);
        let vk = cfg.shortcuts.assist_combo().map(|c| c.vk).unwrap_or(0);
        let need = ai::missing_bytes(m);
        // İndirme sorulduysa tuş çoktan bırakılmıştır: dokun-konuş.
        let asked = need > 0;
        if asked && !crate::ask_model(owner, accent, m, need) {
            return Ok(());
        }
        let opts = Opts {
            m,
            vk: if asked { 0 } else { vk },
            lang: Some(cfg.ai.speech_lang.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| t!("tr", "en").into()),
            web: cfg.launcher.web_search.clone(),
            key: cfg.shortcuts.assist.clone(),
        };
        let shared = Shared::new();
        shared.update(|s| s.title = t!("Dinliyor…", "Listening…").into());
        let worker = shared.clone();
        let stop = stop.0 as isize;
        crate::progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
            std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                work(&worker, &opts, HANDLE(stop as _));
            });
        })
    }
}

struct Opts {
    m: &'static catalog::Model,
    /// Basılı tutulan tuş (0: dokun-konuş).
    vk: u32,
    lang: String,
    web: String,
    key: String,
}

fn held(vk: u32) -> bool {
    vk != 0 && unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0
}

fn work(w: &Arc<Shared>, o: &Opts, stop: HANDLE) {
    if !ai::ensure(w, o.m) {
        return;
    }
    let m = o.m;
    // Model ve uygulama listesi konuşurken arka planda hazırlanır.
    let loader = std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ai::speech_dir(m).ok_or_else(|| t!("Model dosyası bulunamadı", "Model file not found").to_string()).and_then(|d| Whisper::open(&d))
    });
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
            return ai::fail(w, t!("Mikrofon açılamadı", "Couldn't open the microphone").into());
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
    if w.cancel.load(Ordering::SeqCst) {
        return;
    }
    let loud = loudest(&pcm);
    carlui_core::info!("asistan: {:.1} sn ses, en yüksek seviye {loud:.4}", pcm.len() as f32 / whisper::SR as f32);
    if loud == 0.0 && pcm.len() > whisper::SR / 2 {
        return ai::fail(w, t!("Mikrofondan ses gelmiyor (sessiz ya da izin kapalı)", "No sound from the microphone (muted or access off)").into());
    }
    if loud < SILENCE || pcm.len() < whisper::SR / 4 {
        return ai::fail(w, t!("Konuşma duyulmadı", "No speech heard").into());
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
            return ai::fail(w, t!("Model açılamadı", "Couldn't load the model").into());
        }
    };
    let t1 = Instant::now();
    let text = match wh.transcribe(&pcm, Some(&o.lang), false, &w.cancel, &|f| w.update(|s| s.frac = f)) {
        Ok((segs, _)) => whisper::to_text(&segs).trim().to_string(),
        Err(_) if w.cancel.load(Ordering::SeqCst) => return,
        Err(e) => {
            carlui_core::warn!("asistan: yazıya dökülemedi: {e}");
            return ai::fail(w, t!("Anlaşılamadı", "Didn't catch that").into());
        }
    };
    drop(wh);
    let apps = apps.join().unwrap_or_default();
    let intent = voice::parse(&text);
    let plan = voice::plan(&intent, &apps, &index::running(), &o.web);
    carlui_core::info!("asistan: \"{text}\" → {intent:?} → {} ({} ms)", plan.title, t1.elapsed().as_millis());
    let ok = plan.ok();
    w.update(|s| {
        s.title = if text.is_empty() { t!("Konuşma duyulmadı", "No speech heard").into() } else { format!("“{}”", short(&text, 48)) };
        s.file = plan.title.clone();
        s.failed = !ok;
        s.done = true;
        s.frac = 1.0;
    });
    if ok {
        wait_modifiers_up();
        // Kabuk eylemleri (uygulama başlatma) tek iş parçacıklı COM ister.
        let how = plan.how.clone();
        let _ = std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            voice::execute(&how);
        })
        .join();
    }
}
