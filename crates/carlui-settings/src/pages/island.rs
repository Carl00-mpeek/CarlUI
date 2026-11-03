//! Ada ve bildirimler: saat, konum, medya rengi, sistem göstergesi, bildirimler.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::config::ClockMode;

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let cfg = &app.cfg;
    f.section(p, "Ada");
    let sel = match cfg.clock.mode {
        ClockMode::Always => 0,
        ClockMode::WhenAppActive => 1,
        ClockMode::Never => 2,
    };
    f.choice(
        p,
        "Saat",
        "Gizli: ada yalnızca bildirim ya da medya varken görünür.",
        &[
            ("Her zaman", edit(|c| c.clock.mode = ClockMode::Always)),
            ("Uygulamada", edit(|c| c.clock.mode = ClockMode::WhenAppActive)),
            ("Gizli", edit(|c| c.clock.mode = ClockMode::Never)),
        ],
        sel,
        104.0,
    );
    f.slider(p, "Üst kenardan uzaklık", "", cfg.island.top_margin, 0.0, 40.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.island.top_margin = v);
    f.toggle(p, "Tam ekran pencerede yukarı saklan", "Fare üst kenara gelince geri çıkar.", cfg.island.hide_over_maximized, |c| c.island.hide_over_maximized ^= true);
    f.toggle(p, "Kapak rengini al", "Şarkı/video çalarken ada kapağın rengine bürünür.", cfg.island.media_tint, |c| c.island.media_tint ^= true);

    f.section(p, "Sistem göstergesi");
    f.toggle(p, "CPU sıcaklığı", "", cfg.stats.cpu_temp, |c| c.stats.cpu_temp ^= true);
    f.toggle(p, "GPU sıcaklığı", "", cfg.stats.gpu_temp, |c| c.stats.gpu_temp ^= true);
    f.toggle(p, "RAM kullanımı", "", cfg.stats.ram, |c| c.stats.ram ^= true);
    f.slider(p, "Açılma gecikmesi", "Fare adanın üstünde bu kadar kalınca genişler.", cfg.stats.hover_ms as f32, 100.0, 1500.0, 50.0, |v| format!("{v:.0} ms"), |c, v| c.stats.hover_ms = v as u32);

    f.section(p, "Bildirimler");
    f.toggle(p, "Bildirimleri adada göster", "", cfg.notify.enabled, |c| c.notify.enabled ^= true);
    f.toggle(p, "Medya çubuğu", "Çalan şarkı ve oynatma düğmeleri.", cfg.notify.media_bar, |c| c.notify.media_bar ^= true);
    f.toggle(p, "Ses göstergesi", "Ses açılıp kısılınca adada görünür.", cfg.notify.volume_osd, |c| c.notify.volume_osd ^= true);
    f.slider(p, "Ekranda kalma süresi", "", cfg.notify.duration_ms as f32, 1000.0, 10000.0, 250.0, |v| format!("{:.1} sn", v / 1000.0).replace('.', ","), |c, v| c.notify.duration_ms = v as u32);
    f.toggle(p, "Tam ekran pencerede de göster", "", cfg.notify.over_maximized, |c| c.notify.over_maximized ^= true);
    f.toggle(p, "Windows bildirim balonlarını gizle", "Bildirimler yalnızca adada görünür; Bildirim Merkezi'ne yine düşer.", cfg.notify.hide_windows_banners, |c| c.notify.hide_windows_banners ^= true);
}
