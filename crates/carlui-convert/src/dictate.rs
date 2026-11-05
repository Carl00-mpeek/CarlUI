//! Bu modül "klavye yerine konuşarak nasıl yazarız?" sorununu çözüyor (sesle
//! yazma / dikte). Kısayola (`shortcuts.dictate`, varsayılan Ctrl+Alt+D) bir kez
//! basınca mikrofon dinlenir, dock'un üstündeki kartta ses seviyesi görünür;
//! yeniden basınca kayıt biter, ses yerel Whisper modeliyle yazıya dökülür ve
//! metin imlecin olduğu yere klavye girdisi olarak yazılır. Hiçbir şey buluta
//! gitmez. Kart odak almaz; yazılan pencere öndeki pencere olarak kalır.
//!
//! İkinci basış yeni bir süreç başlatır: o süreç çalışan dikteyi adlı bir
//! olayla durdurup hemen çıkar.

use crate::{ai, progress::Shared};
use carlui_ai::{catalog, mic::Mic, whisper::{self, Whisper}};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Threading::*},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// En uzun kayıt; unutulan dikte sonsuza dek dinlemesin.
const MAX: Duration = Duration::from_secs(300);

/// Dikteyi başlatır; zaten çalışıyorsa durdurur. `target`: yazılacak pencere.
pub fn run(owner: HWND, accent: Option<u32>, target: HWND) -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, false, w!("Local\\CarlUI.Dictate"))?;
        let running = GetLastError() == ERROR_ALREADY_EXISTS;
        let stop = CreateEventW(None, false, false, w!("Local\\CarlUI.DictateStop"))?;
        if running {
            carlui_core::info!("dikte: çalışan kayıt durduruluyor");
            let _ = SetEvent(stop);
            return Ok(());
        }
        let _ = ResetEvent(stop);
        let cfg = carlui_core::config::load();
        let m = catalog::chosen(&cfg.ai, catalog::Feature::Speech);
        let need = ai::missing_bytes(m);
        if need > 0 && !crate::ask_model(owner, accent, m, need) {
            return Ok(());
        }
        let key = cfg.shortcuts.dictate.clone();
        let lang = cfg.ai.speech_lang.clone();
        let shared = Shared::new();
        shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
        let worker = shared.clone();
        let stop = stop.0 as isize;
        let target = target.0 as isize;
        crate::progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
            std::thread::spawn(move || {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                work(&worker, m, &key, &lang, HANDLE(stop as _), HWND(target as _));
            });
        })
    }
}

fn work(w: &Arc<Shared>, m: &'static catalog::Model, key: &str, lang: &str, stop: HANDLE, target: HWND) {
    // Model yoksa önce indir (onay alındı); dinlemeye ondan sonra başla.
    if !ai::ensure(w, m) {
        return;
    }
    // Model kayıt sürerken arka planda yüklenir.
    let loader = std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ai::speech_dir(m).ok_or_else(|| t!("Model dosyası bulunamadı", "Model file not found").to_string()).and_then(|d| Whisper::open(&d))
    });
    let mut mic = match Mic::open() {
        Ok(m) => m,
        Err(e) => {
            carlui_core::warn!("dikte: mikrofon açılamadı: {e}");
            return ai::fail(w, t!("Mikrofon açılamadı", "Couldn't open the microphone").into());
        }
    };
    w.update(|s| {
        s.title = t!("Dinliyor…", "Listening…").into();
        s.file = t!(format!("Bitirmek için yeniden {key}"), format!("Press {key} again to finish"));
        s.frac = 0.0;
    });
    let t0 = Instant::now();
    let mut pcm = Vec::new();
    let mut level = 0f32;
    loop {
        let stopped = unsafe { WaitForSingleObject(stop, 40) } == WAIT_OBJECT_0;
        match mic.read(&mut pcm) {
            Ok(peak) => {
                // Hızlı yükselen, yavaş düşen seviye göstergesi.
                level = if peak > level { peak } else { level * 0.85 };
                let shown = (level * 3.0).min(1.0);
                w.update(|s| s.frac = shown);
            }
            Err(e) => {
                carlui_core::warn!("dikte: mikrofon okunamadı: {e}");
                break;
            }
        }
        if stopped || w.cancel.load(Ordering::SeqCst) || t0.elapsed() > MAX {
            break;
        }
    }
    drop(mic);
    if w.cancel.load(Ordering::SeqCst) {
        return;
    }
    let loud = loudest(&pcm);
    carlui_core::info!("dikte: {:.1} sn ses, en yüksek seviye {loud:.4}", pcm.len() as f32 / whisper::SR as f32);
    // Sessizlikte Whisper uydurma metin üretebilir ("Teşekkürler." gibi); hiç yazma.
    if loud == 0.0 && pcm.len() > whisper::SR {
        // Tam sıfır: mikrofon sessize alınmış ya da Windows gizlilik ayarı erişimi kapatmış.
        return ai::fail(w, t!("Mikrofondan ses gelmiyor (sessiz ya da izin kapalı)", "No sound from the microphone (muted or access off)").into());
    }
    if loud < SILENCE {
        return ai::fail(w, t!("Konuşma duyulmadı", "No speech heard").into());
    }
    w.update(|s| {
        s.title = t!("Yazıya dökülüyor", "Transcribing").into();
        s.file = m.name.into();
        s.frac = 0.0;
    });
    let mut wh = match loader.join().unwrap_or_else(|_| Err("panik".into())) {
        Ok(wh) => wh,
        Err(e) => {
            carlui_core::warn!("dikte: model açılamadı: {e}");
            return ai::fail(w, t!("Model açılamadı", "Couldn't load the model").into());
        }
    };
    let lang = Some(lang).filter(|l| !l.is_empty());
    let text = match wh.transcribe(&pcm, lang, false, &w.cancel, &|f| w.update(|s| s.frac = f)) {
        Ok((segs, _)) => whisper::to_text(&segs).trim().to_string(),
        Err(_) if w.cancel.load(Ordering::SeqCst) => return,
        Err(e) => {
            carlui_core::warn!("dikte: yazıya dökülemedi: {e}");
            return ai::fail(w, t!("Yazıya dökülemedi", "Transcription failed").into());
        }
    };
    if text.is_empty() {
        return ai::fail(w, t!("Konuşma duyulmadı", "No speech heard").into());
    }
    unsafe {
        // Kart odak almaz; indirme sorusu sorulduysa hedefi geri öne al.
        if GetForegroundWindow() != target && IsWindow(target).as_bool() {
            let _ = SetForegroundWindow(target);
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    wait_modifiers_up();
    type_text(&text);
    carlui_core::info!("dikte: {} karakter yazıldı", text.chars().count());
    w.update(|s| {
        s.title = t!("Yazıldı", "Typed").into();
        s.file = short(&text, 60);
        s.done = true;
        s.frac = 1.0;
    });
}

/// 100 ms'lik pencerelerin en yüksek RMS'i bunun altındaysa kayıt sessiz sayılır.
pub(crate) const SILENCE: f32 = 0.006;

pub(crate) fn loudest(pcm: &[f32]) -> f32 {
    pcm.chunks(whisper::SR / 10).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).fold(0.0, f32::max)
}

pub(crate) fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Kısayolun Ctrl/Alt/Shift/Win'i hâlâ basılıysa harfler kısayol olarak gider;
/// bırakılana dek (en fazla 3 sn) bekle.
pub(crate) fn wait_modifiers_up() {
    let t0 = Instant::now();
    let held = || [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } as u16 & 0x8000 != 0);
    while held() && t0.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Metni öndeki pencereye klavye girdisi olarak yazar (klavye düzeninden bağımsız).
fn type_text(text: &str) {
    let mut ev = Vec::with_capacity(text.len() * 2);
    let mut buf = [0u16; 2];
    for c in text.chars().filter(|c| *c != '\r') {
        for &u in c.encode_utf16(&mut buf).iter() {
            for up in [false, true] {
                let f = if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE };
                ev.push(INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wScan: u, dwFlags: f, ..Default::default() } } });
            }
        }
    }
    for chunk in ev.chunks(400) {
        unsafe {
            SendInput(chunk, std::mem::size_of::<INPUT>() as i32);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gate() {
        assert!(loudest(&vec![0.0; 16_000]) < SILENCE);
        // Hafif oda gürültüsü sessiz sayılır, normal konuşma sayılmaz.
        let hiss: Vec<f32> = (0..16_000).map(|i| ((i * 7919) % 13) as f32 / 13.0 * 0.004 - 0.002).collect();
        assert!(loudest(&hiss) < SILENCE);
        let voice: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.05).sin() * 0.1).collect();
        assert!(loudest(&voice) > SILENCE);
        assert_eq!(short("abcdef", 3), "abc…");
    }
}
