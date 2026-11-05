//! Bu modül "anlık tekrarı nereden açıp ayarlarız?" sorununu çözüyor: Anlık
//! tekrar sayfası. Kaydı açma, kaç dakika geriye tutulacağı (1–30), kare
//! hızı, kalite, ses ve kayıt klasörü burada. Kaydetme kısayolu Kısayollar
//! sayfasında da değiştirilebilir.

use crate::{
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::config::{Quality, ReplayConfig};
use carlui_ui::paint::R;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// Seçili ayarlarla geçici kaydın diskte kaplayacağı yaklaşık alan (GB).
fn disk_gb(c: &ReplayConfig) -> f64 {
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN).max(640) as u32, GetSystemMetrics(SM_CYSCREEN).max(480) as u32) };
    let bits = c.bitrate(w, h) as f64 + if c.audio { 192_000.0 } else { 0.0 };
    bits / 8.0 * (c.minutes() as f64 * 60.0 + 10.0) / 1e9
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.replay;
    let key = &app.cfg.shortcuts.replay;
    f.section(p, t!("Anlık tekrar", "Instant replay"));
    f.toggle(
        p,
        t!("Arka planda kaydet", "Record in the background"),
        &if key.trim().is_empty() {
            t!("Kaydetme kısayolunu Kısayollar sayfasından atayın.", "Assign the save shortcut on the Shortcuts page.").to_string()
        } else {
            t!(format!("{key} ile son dakikalar video olarak kaydedilir; oyun içinde de çalışır."), format!("{key} saves the last minutes as a video; works in games too."))
        },
        c.enabled,
        |c| c.replay.enabled ^= true,
    );
    f.slider(
        p,
        t!("Kaydedilecek süre", "Length to save"),
        &t!(format!("Diskte yaklaşık {:.1} GB geçici alan kullanır.", disk_gb(c)).replace('.', ","), format!("Uses about {:.1} GB of temporary disk space.", disk_gb(c))),
        c.minutes() as f32,
        1.0,
        ReplayConfig::MAX_MINUTES as f32,
        1.0,
        |v| t!(format!("{v:.0} dk"), format!("{v:.0} min")),
        |c, v| c.replay.minutes = v as u32,
    );

    f.section(p, t!("Kalite", "Quality"));
    let fps = if c.fps() == 30 { 0 } else { 1 };
    f.choice(p, t!("Kare hızı", "Frame rate"), "", &[("30 fps", edit(|c| c.replay.fps = 30)), ("60 fps", edit(|c| c.replay.fps = 60))], fps, 80.0);
    let q = match c.quality {
        Quality::Low => 0,
        Quality::Medium => 1,
        Quality::High => 2,
    };
    f.choice(
        p,
        t!("Görüntü kalitesi", "Video quality"),
        t!("Yüksek kalite daha çok disk kullanır; ekran kartı kodladığı için oyunu yavaşlatmaz.", "Higher quality uses more disk; the graphics card encodes, so games don't slow down."),
        &[
            (t!("Düşük", "Low"), edit(|c| c.replay.quality = Quality::Low)),
            (t!("Orta", "Medium"), edit(|c| c.replay.quality = Quality::Medium)),
            (t!("Yüksek", "High"), edit(|c| c.replay.quality = Quality::High)),
        ],
        q,
        80.0,
    );
    f.toggle(p, t!("Bilgisayarın sesini kaydet", "Record computer audio"), t!("Hoparlör ya da kulaklıktan çıkan ses.", "What plays through the speakers or headphones."), c.audio, |c| c.replay.audio ^= true);

    f.section(p, t!("Kayıtlar", "Recordings"));
    let r = f.row(p, t!("Kayıt klasörü", "Recordings folder"), &c.folder().display().to_string(), 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E838}'), t!("Aç", "Open"), Act::ReplayFolder, true, false);
    f.y += 6.0;
    f.note(p, t!("Birincil monitör kaydedilir; fare imleci videoda görünmez. Oyunun odağını almaz, özel tam ekranda da çalışır.", "The primary monitor is recorded; the mouse cursor isn't shown. It never takes focus from the game and works in exclusive fullscreen."));
}
