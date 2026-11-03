//! Ada ve bildirimler: saat, konum, medya rengi, sistem göstergesi, bildirimler.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::config::ClockMode;

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let cfg = &app.cfg;
    f.section(p, t!("Ada", "Island"));
    let sel = match cfg.clock.mode {
        ClockMode::Always => 0,
        ClockMode::WhenAppActive => 1,
        ClockMode::Never => 2,
    };
    f.choice(
        p,
        t!("Saat", "Clock"),
        t!("Gizli: ada yalnızca bildirim ya da medya varken görünür.", "Hidden: the island only appears for notifications or media."),
        &[
            (t!("Her zaman", "Always"), edit(|c| c.clock.mode = ClockMode::Always)),
            (t!("Uygulamada", "In apps"), edit(|c| c.clock.mode = ClockMode::WhenAppActive)),
            (t!("Gizli", "Hidden"), edit(|c| c.clock.mode = ClockMode::Never)),
        ],
        sel,
        104.0,
    );
    f.slider(p, t!("Üst kenardan uzaklık", "Distance from top edge"), "", cfg.island.top_margin, 0.0, 40.0, 1.0, |v| format!("{v:.0} px"), |c, v| c.island.top_margin = v);
    f.toggle(p, t!("Tam ekran pencerede yukarı saklan", "Hide over maximized windows"), t!("Fare üst kenara gelince geri çıkar.", "Comes back when the mouse reaches the top edge."), cfg.island.hide_over_maximized, |c| c.island.hide_over_maximized ^= true);
    f.toggle(p, t!("Kapak rengini al", "Match cover color"), t!("Şarkı/video çalarken ada kapağın rengine bürünür.", "While music or video plays, the island takes the cover art's color."), cfg.island.media_tint, |c| c.island.media_tint ^= true);

    f.section(p, t!("Sistem göstergesi", "System stats"));
    f.toggle(p, t!("CPU sıcaklığı", "CPU temperature"), "", cfg.stats.cpu_temp, |c| c.stats.cpu_temp ^= true);
    f.toggle(p, t!("GPU sıcaklığı", "GPU temperature"), "", cfg.stats.gpu_temp, |c| c.stats.gpu_temp ^= true);
    f.toggle(p, t!("RAM kullanımı", "RAM usage"), "", cfg.stats.ram, |c| c.stats.ram ^= true);
    f.slider(p, t!("Açılma gecikmesi", "Open delay"), t!("Fare adanın üstünde bu kadar kalınca genişler.", "Expands after the mouse rests on the island this long."), cfg.stats.hover_ms as f32, 100.0, 1500.0, 50.0, |v| format!("{v:.0} ms"), |c, v| c.stats.hover_ms = v as u32);

    f.section(p, t!("İndirme göstergesi", "Download indicator"));
    f.toggle(p, t!("Dosya adı", "File name"), "", cfg.download.name, |c| c.download.name ^= true);
    f.toggle(p, t!("Yüzde", "Percentage"), "", cfg.download.percent, |c| c.download.percent ^= true);
    f.toggle(p, t!("İnen / toplam boyut", "Downloaded / total size"), "", cfg.download.size, |c| c.download.size ^= true);
    f.toggle(p, t!("Hız", "Speed"), "", cfg.download.speed, |c| c.download.speed ^= true);
    f.toggle(p, t!("Kalan süre", "Time left"), "", cfg.download.eta, |c| c.download.eta ^= true);
    f.toggle(
        p,
        t!("Diğer programların indirmeleri", "Downloads from other apps"),
        t!("Steam, Epic, torrent… İnternet ve disk kullanımından tahmin edilir; dosya adı ve yüzde görünmez.", "Steam, Epic, torrents… Estimated from network and disk use; no file name or percentage."),
        cfg.download.others,
        |c| c.download.others ^= true,
    );

    f.section(p, t!("Bildirimler", "Notifications"));
    f.toggle(p, t!("Bildirimleri adada göster", "Show notifications on the island"), "", cfg.notify.enabled, |c| c.notify.enabled ^= true);
    f.toggle(p, t!("Medya çubuğu", "Media bar"), t!("Çalan şarkı ve oynatma düğmeleri.", "Now playing and playback buttons."), cfg.notify.media_bar, |c| c.notify.media_bar ^= true);
    f.toggle(p, t!("Ses göstergesi", "Volume indicator"), t!("Ses açılıp kısılınca adada görünür.", "Shows on the island when the volume changes."), cfg.notify.volume_osd, |c| c.notify.volume_osd ^= true);
    f.slider(p, t!("Ekranda kalma süresi", "Time on screen"), "", cfg.notify.duration_ms as f32, 1000.0, 10000.0, 250.0, |v| t!(format!("{:.1} sn", v / 1000.0).replace('.', ","), format!("{:.1} s", v / 1000.0)), |c, v| c.notify.duration_ms = v as u32);
    f.toggle(p, t!("Tam ekran pencerede de göster", "Also show over maximized windows"), "", cfg.notify.over_maximized, |c| c.notify.over_maximized ^= true);
    f.toggle(p, t!("Windows bildirim balonlarını gizle", "Hide Windows notification banners"), t!("Bildirimler yalnızca adada görünür; Bildirim Merkezi'ne yine düşer.", "Notifications only show on the island; they still reach the Notification Center."), cfg.notify.hide_windows_banners, |c| c.notify.hide_windows_banners ^= true);
}
