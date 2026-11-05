//! Bu modül "seçili metni bilgisayara sesli nasıl okuturuz?" sorununu
//! çözüyor (sesli okuma). Kısayola (`shortcuts.speak`, varsayılan Ctrl+Alt+S)
//! basınca öndeki penceredeki seçim kopyalanır (seçim yoksa panodaki metin
//! kullanılır) ve yerel ses modeliyle okunur. Türkçe metin Türkçe sesle,
//! diğerleri Ayarlar → Yapay zekâ'da seçilen İngilizce sesle okunur. Hiçbir
//! şey buluta gitmez.
//!
//! Metin cümlelere bölünür: bir iş parçacığı sıradaki cümleyi üretirken öteki
//! öncekini çalar, böylece okuma ilk cümleden hemen sonra başlar. Okuma
//! sürerken kısayola yeniden basmak yeni bir süreç başlatır; o süreç adlı bir
//! olayla okumayı keser ve çıkar.

use crate::{ai, progress, translate};
use carlui_ai::{catalog, store, tts};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Media::Audio::*,
        System::{Com::*, DataExchange::*, Memory::*, Threading::*},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// Bir seferde okunacak en uzun metin (karakter).
const MAX_TEXT: usize = 20_000;

/// Okumayı başlatır; zaten okuyorsa susturur. `target`: seçimin alınacağı pencere.
pub fn run(owner: HWND, accent: Option<u32>, target: HWND) -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, false, w!("Local\\CarlUI.Speak"))?;
        let running = GetLastError() == ERROR_ALREADY_EXISTS;
        let stop = CreateEventW(None, true, false, w!("Local\\CarlUI.SpeakStop"))?;
        if running {
            carlui_core::info!("sesli okuma: susturuluyor");
            let _ = SetEvent(stop);
            return Ok(());
        }
        let _ = ResetEvent(stop);
        let text = selected_text(target).unwrap_or_default();
        let text: String = text.chars().take(MAX_TEXT).collect();
        if !text.chars().any(|c| c.is_alphanumeric()) {
            carlui_core::info!("sesli okuma: okunacak metin yok");
            return Ok(());
        }
        let cfg = carlui_core::config::load();
        let m = if translate::is_turkish(&text) { catalog::get(catalog::TURKISH_VOICE).unwrap_or_else(|| catalog::chosen(&cfg.ai, catalog::Feature::Speak)) } else { catalog::chosen(&cfg.ai, catalog::Feature::Speak) };
        let need = ai::missing_bytes(m);
        if need > 0 {
            if !crate::ask_model(owner, accent, m, need) {
                return Ok(());
            }
            // İndirme ilerleme kartında; bitince okumaya geçilir.
            let shared = progress::Shared::new();
            shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
            let worker = shared.clone();
            progress::run(shared.clone(), accent.unwrap_or(0x4CC2FF), move || {
                std::thread::spawn(move || {
                    if ai::ensure(&worker, m) {
                        worker.update(|s| {
                            s.title = t!(format!("{} hazır", m.name), format!("{} is ready", m.name));
                            s.frac = 1.0;
                            s.done = true;
                        });
                    }
                });
            })?;
            if ai::missing_bytes(m) > 0 {
                return Ok(());
            }
        }
        speak(m, &text, cfg.ai.speak_speed, stop);
        Ok(())
    }
}

/// Metni okur; `stop` olayı gelince hemen susar.
fn speak(m: &'static catalog::Model, text: &str, speed: f32, stop: HANDLE) {
    let halt = Arc::new(AtomicBool::new(false));
    // Susturma olayını bekleyen iş parçacığı: çalan sesi de keser.
    {
        let halt = halt.clone();
        let stop = stop.0 as isize;
        std::thread::spawn(move || unsafe {
            if WaitForSingleObject(HANDLE(stop as _), INFINITE) == WAIT_OBJECT_0 {
                halt.store(true, Ordering::SeqCst);
                let _ = PlaySoundW(PCWSTR::null(), None, SND_PURGE);
            }
        });
    }
    let parts = tts::sentences(text);
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(2);
    let producer = {
        let halt = halt.clone();
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let t0 = Instant::now();
            let mut voice = match tts::Voice::open(&store::dir(m)) {
                Ok(v) => v,
                Err(e) => return carlui_core::warn!("sesli okuma: {} açılamadı: {e}", m.id),
            };
            carlui_core::info!("sesli okuma: {} açıldı ({:?}), {} parça", m.id, t0.elapsed(), parts.len());
            for p in parts {
                if halt.load(Ordering::SeqCst) {
                    break;
                }
                match voice.synth(&p, speed) {
                    Ok(s) if !s.is_empty() => {
                        if tx.send(tts::wav(&s, voice.rate)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => carlui_core::warn!("sesli okuma: üretilemedi: {e}"),
                }
            }
        })
    };
    for w in rx {
        if halt.load(Ordering::SeqCst) {
            break;
        }
        unsafe {
            let _ = PlaySoundW(PCWSTR(w.as_ptr() as *const u16), None, SND_MEMORY | SND_SYNC | SND_NODEFAULT);
        }
    }
    // Üretimdeki cümle beklenmez; süreç çıkınca iş parçacığı da biter.
    halt.store(true, Ordering::SeqCst);
    drop(producer);
}

/// Öndeki penceredeki seçimi Ctrl+C ile alır; seçim yoksa panodaki metni döner.
fn selected_text(target: HWND) -> Option<String> {
    unsafe {
        // Kısayolun tuşları (Ctrl+Alt…) bırakılmadan Ctrl+C göndermek başka bir
        // kısayola dönüşür; bırakılmasını kısa süre bekle.
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(1500) && [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0) {
            std::thread::sleep(Duration::from_millis(15));
        }
        if !target.is_invalid() {
            let _ = SetForegroundWindow(target);
        }
        let before = GetClipboardSequenceNumber();
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
        };
        SendInput(&[key(VK_CONTROL, false), key(VK_C, false), key(VK_C, true), key(VK_CONTROL, true)], std::mem::size_of::<INPUT>() as i32);
        let t1 = Instant::now();
        while t1.elapsed() < Duration::from_millis(400) && GetClipboardSequenceNumber() == before {
            std::thread::sleep(Duration::from_millis(10));
        }
        clipboard_text()
    }
}

fn clipboard_text() -> Option<String> {
    unsafe {
        // Pano kısa süre başka süreçte açık olabilir.
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let out = (|| {
            let h: HANDLE = GetClipboardData(13 /* CF_UNICODETEXT */).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 && n < 4 * 1024 * 1024 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}
