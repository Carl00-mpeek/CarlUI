//! Bu modül "oyundayken klavyeye uzanmadan son dakikaları nasıl kaydederiz?"
//! sorununu çözüyor: Ayarlar → Yapay zekâ'da sesli komut açıksa ve anlık
//! tekrar kaydı sürüyorsa mikrofon yerel bir modelle (Vosk ya da Whisper)
//! dinlenir; "son iki dakikayı kaydet" duyulunca ana döngüye istenen süreyle
//! kaydetme iletisi gider. Ses hiçbir yere kaydedilmez ya da gönderilmez.
//! Model yükleme ana döngüyü bekletmesin diye ayrı iş parçacığında yapılır.

use carlui_ai::{catalog, listen::Listener, store};
use carlui_core::config::Config;
use std::sync::Mutex;
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP},
};

/// Kaydet iletisi; `wParam`: istenen saniye (0 = tutulan sürenin tamamı).
pub const WM_APP_VOICE: u32 = WM_APP + 4;

/// Son kurulan ayarın anahtarı (model + dil) ve dinleyici (açılamadıysa yok).
static CUR: Mutex<Option<(String, Option<Listener>)>> = Mutex::new(None);

/// Ayara göre dinleyiciyi başlatır, değiştirir ya da durdurur.
pub fn sync(cfg: &Config, tid: u32) {
    let want = (cfg.ai.voice_command && cfg.replay.enabled).then(|| {
        let m = catalog::chosen(&cfg.ai, catalog::Feature::Command);
        (format!("{}|{}", m.id, cfg.ai.speech_lang), m, cfg.ai.speech_lang.clone())
    });
    let _ = std::thread::Builder::new().name("voice-sync".into()).spawn(move || {
        // Kilit, üst üste gelen ayar değişikliklerini sıraya koyar.
        let mut cur = CUR.lock().unwrap_or_else(|e| e.into_inner());
        match (cur.as_ref(), want.as_ref()) {
            (None, None) => return,
            // Aynı ayar: çalışıyorsa dokunma; açılamamışsa yalnızca model sonradan indiyse yeniden dene.
            (Some((k, l)), Some((wk, m, _))) if k == wk && (l.is_some() || !store::installed(m)) => return,
            _ => {}
        }
        // Önce eskisi durur (mikrofon ve model serbest kalır).
        *cur = None;
        let Some((key, m, lang)) = want else {
            return;
        };
        let started = Listener::start(m, &lang, move |c| unsafe {
            let _ = PostThreadMessageW(tid, WM_APP_VOICE, WPARAM(c.secs.unwrap_or(0) as usize), LPARAM(0));
        });
        match started {
            Ok(l) => *cur = Some((key, Some(l))),
            Err(e) => {
                *cur = Some((key, None));
                carlui_core::warn!("sesli komut başlatılamadı ({}): {e}", m.id);
                let body = if store::installed(m) {
                    t!("Mikrofon ya da model açılamadı.", "Couldn't open the microphone or the model.")
                } else {
                    t!("Önce Ayarlar → Yapay zekâ'dan komut modelini indirin.", "Download the command model in Settings → AI first.")
                };
                crate::note(t!("Sesli komut", "Voice command"), body);
            }
        }
    });
}

/// Çıkışta dinleyiciyi durdurur.
pub fn stop() {
    *CUR.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
