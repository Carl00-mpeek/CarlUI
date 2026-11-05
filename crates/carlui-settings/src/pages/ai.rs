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
        // Çeviride iki yön de kullanılır; seçim yok, yalnızca indirme.
        let pick = feat != Feature::Translate;
        for m in catalog::of(feat) {
            let label = if m.id == rec {
                format!("{} · {}  {}", m.tier.label(), m.name, t!("(önerilen)", "(recommended)"))
            } else {
                format!("{} · {}", m.tier.label(), m.name)
            };
            let desc = format!("{} · {} · {}", mb(m.size()), m.license, m.about());
            let r = f.row(p, &label, &desc, if pick { 232.0 } else { 120.0 });
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
    f.note(p, t!("Modeller yalnızca resmi kaynaklarından indirilir ve doğrulanır; hepsi ticari kullanıma açık lisanslıdır.", "Models are downloaded only from their official sources and verified; all have licenses that allow commercial use."));
    f.note(p, &format!("{} {}", t!("Klasör:", "Folder:"), store::root().display()));
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
