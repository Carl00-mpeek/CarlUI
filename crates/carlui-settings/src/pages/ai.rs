//! Bu modül "hangi yapay zekâ modelini indirip kullanacağım?" sorusunu
//! çözüyor: Yapay zekâ sayfası. Kurucuda model yok; her özellik için küçük,
//! orta, büyük seçenekleri boyut ve lisansıyla listelenir. İndirme ayrı bir
//! süreçte (`carlui-convert --fetch`) resmi kaynaktan yapılır, ilerleme
//! kartında görünür; bitince sayfa kendini yeniler.

use crate::{
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_ai::{
    catalog::{self, Feature},
    store,
};
use carlui_ui::paint::{ORANGE, R};
use windows::Win32::{Foundation::HWND, Graphics::Gdi::InvalidateRect};

fn mb(bytes: u64) -> String {
    let m = bytes as f64 / 1_048_576.0;
    if m >= 1024.0 {
        t!(format!("{:.1} GB", m / 1024.0).replace('.', ","), format!("{:.1} GB", m / 1024.0))
    } else {
        format!("{:.0} MB", m.max(1.0))
    }
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.ai;
    f.section(p, t!("Genel", "General"));
    f.toggle(
        p,
        t!("Ekran kartını kullan", "Use the graphics card"),
        t!("DirectML ile çok daha hızlı; sorun çıkarsa kapatın, işlemci kullanılır.", "Much faster with DirectML; turn off if it causes trouble and the CPU is used."),
        c.gpu,
        |c| c.ai.gpu ^= true,
    );
    let lang = match c.speech_lang.as_str() {
        "tr" => 1,
        "en" => 2,
        _ => 0,
    };
    f.choice(
        p,
        t!("Konuşma dili", "Speech language"),
        t!("Dikte ve yazıya dökmede; otomatik her kayıtta dili kendisi bulur.", "For dictation and transcripts; automatic detects it for each recording."),
        &[
            (t!("Otomatik", "Automatic"), edit(|c| c.ai.speech_lang.clear())),
            ("Türkçe", edit(|c| c.ai.speech_lang = "tr".into())),
            ("English", edit(|c| c.ai.speech_lang = "en".into())),
        ],
        lang,
        86.0,
    );
    f.toggle(
        p,
        t!("Sesli komutla klip kaydet", "Save clips by voice"),
        t!("\"Son iki dakikayı kaydet\" deyin; anlık tekrar açık olmalı. Ses bilgisayardan çıkmaz.", "Say \"save the last two minutes\"; instant replay must be on. Audio never leaves the PC."),
        c.voice_command,
        |c| c.ai.voice_command ^= true,
    );
    if let Some(e) = &app.ai_error {
        f.row_c(p, t!("Model silinemedi", "Couldn't remove the model"), e, 0.0, Some(ORANGE));
    }

    let busy = app.fetching.lock().map(|v| v.clone()).unwrap_or_default();
    for feat in Feature::ALL {
        f.section(p, feat.title());
        let rec = catalog::recommended(feat).id;
        let sel = catalog::chosen(c, feat).id;
        if feat == Feature::Speak {
            speak_options(c, p, f);
        }
        // Ses ve yazıya dökme modelleri Ses stüdyosunda seçilip indirilir.
        if matches!(feat, Feature::Speak | Feature::Speech) {
            let m = catalog::chosen(c, feat);
            let label = t!(format!("Model: {}", m.name), format!("Model: {}", m.name));
            let desc = t!(
                "Küçük, orta, büyük modeller Ses stüdyosunda seçilip indirilir; kayıt da oradan yapılır.",
                "Small, medium and large models are picked and downloaded in the Voice studio, where you can also record."
            );
            let r = f.row(p, &label, desc, 160.0);
            p.button(R::new(r.right() - 16.0 - 150.0, r.y + (r.h - 32.0) / 2.0, 150.0, 32.0), Some('\u{E8D6}'), t!("Ses stüdyosu", "Voice studio"), Act::Studio, true, false);
            continue;
        }
        for m in catalog::of(feat) {
            // Çeviride iki yön de kullanılır; Türkçe ses Türkçe metinde kendiliğinden seçilir.
            let pick = catalog::selectable(m);
            let label = if m.id == rec {
                format!("{} · {}  {}", m.tier.label(), m.name, t!("(önerilen)", "(recommended)"))
            } else {
                format!("{} · {}", m.tier.label(), m.name)
            };
            let ctrl = if pick { 232.0 } else { 120.0 };
            let (desc, color) = if m.restricted {
                let d = t!(
                    format!("{} · {} · Yalnızca ticari olmayan kullanım · {}", mb(m.size()), m.license, m.about()),
                    format!("{} · {} · Non-commercial use only · {}", mb(m.size()), m.license, m.about())
                );
                (d, Some(ORANGE))
            } else {
                (format!("{} · {} · {}", mb(m.size()), m.license, m.about()), None)
            };
            // Önerilen sistem; bu bilgisayar altındaysa turuncu "uzun sürebilir" uyarısı.
            let (extra, extra_color) = match carlui_ai::hw::warning(m) {
                Some(w) => (format!("{} · {w}", carlui_ai::hw::summary(m)), Some(ORANGE)),
                None => (carlui_ai::hw::summary(m), None),
            };
            let r = f.row3(p, &label, &desc, color, &extra, extra_color, ctrl);
            let y = r.y + (r.h - 32.0) / 2.0;
            let x = r.right() - 16.0 - 112.0;
            let bx = R::new(x, y, 112.0, 32.0);
            if busy.contains(&m.id) {
                p.button(bx, Some('\u{E896}'), t!("İndiriliyor…", "Downloading…"), Act::Scroll, false, false);
            } else if store::installed(m) {
                p.button(bx, Some('\u{E74D}'), &t!(format!("Sil ({})", mb(store::used(m))), format!("Delete ({})", mb(store::used(m)))), Act::Model(m.id, false), true, true);
            } else {
                p.button(bx, Some('\u{E896}'), t!("İndir", "Download"), Act::Model(m.id, true), true, false);
            }
            if pick {
                let key = feat.key();
                let id = m.id;
                let sx = R::new(x - 8.0 - 104.0, y, 104.0, 32.0);
                if sel == id {
                    p.button(sx, Some('\u{E73E}'), t!("Seçili", "Selected"), Act::Scroll, false, false);
                } else {
                    p.button(sx, None, t!("Seç", "Select"), edit(move |c| c.ai.set(key, id)), true, false);
                }
            }
        }
    }
    f.note(p, t!("Modeller kurulumla gelmez; yalnızca resmi kaynaklarından indirilir ve doğrulanır.", "Models don't ship with the installer; they're downloaded only from their official sources and verified."));
    f.note(p, t!("Turuncu lisanslar ticari kullanımı yasaklar; bunlar ilk indirmede ayrıca onayınızı ister.", "Orange licenses forbid commercial use; those ask for your consent before the first download."));
    f.note(p, &format!("{} {}", t!("Klasör:", "Folder:"), store::root().display()));
}

fn speak_options(c: &carlui_core::ai::AiConfig, p: &mut P, f: &mut Flow) {
    const SPEEDS: [f32; 4] = [0.8, 1.0, 1.25, 1.5];
    let cur = SPEEDS.iter().position(|s| (s - c.speak_speed).abs() < 0.01).unwrap_or(1);
    f.choice(
        p,
        t!("Okuma hızı", "Reading speed"),
        t!("Kısayol: seçili metni (yoksa panodakini) okur; yeniden basınca susar. Türkçe metin Türkçe sesle okunur.", "Shortcut reads the selected text (or the clipboard); press again to stop. Turkish text uses the Turkish voice."),
        &[
            (t!("0,8×", "0.8×"), edit(|c| c.ai.speak_speed = 0.8)),
            ("1×", edit(|c| c.ai.speak_speed = 1.0)),
            (t!("1,25×", "1.25×"), edit(|c| c.ai.speak_speed = 1.25)),
            (t!("1,5×", "1.5×"), edit(|c| c.ai.speak_speed = 1.5)),
        ],
        cur,
        64.0,
    );
}

/// Modeli ayrı süreçte indirir; bitince sayfayı yeniden çizdirir.
pub fn fetch(app: &mut App, id: &'static str) {
    {
        let Ok(mut v) = app.fetching.lock() else { return };
        if v.contains(&id) {
            return;
        }
        v.push(id);
    }
    let exe = carlui_core::paths::sibling_exe("carlui-convert.exe");
    let child = std::process::Command::new(exe).arg("--fetch").arg(id).spawn();
    let list = app.fetching.clone();
    let hwnd = app.hwnd.0 as isize;
    let done = move || {
        if let Ok(mut v) = list.lock() {
            v.retain(|x| *x != id);
        }
        unsafe {
            let _ = InvalidateRect(HWND(hwnd as _), None, false);
        }
    };
    match child {
        Ok(mut ch) => {
            std::thread::spawn(move || {
                let _ = ch.wait();
                done();
            });
        }
        Err(e) => {
            done();
            app.ai_error = Some(e.to_string());
        }
    }
}

pub fn remove(id: &str) -> Result<(), String> {
    let m = catalog::get(id).ok_or_else(|| id.to_string())?;
    store::remove(m)
}
