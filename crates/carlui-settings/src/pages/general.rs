//! Genel: dil, Windows ile başlama, güncellemeler, görev çubuğu, adanın monitörü.

use crate::{
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_ui::paint::R;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.general;
    f.section(p, t!("Dil", "Language"));
    let langs: [(&str, crate::Act); 3] = [
        (t!("Otomatik", "Auto"), edit(|c| set_lang(c, "auto"))),
        ("Türkçe", edit(|c| set_lang(c, "tr"))),
        ("English", edit(|c| set_lang(c, "en"))),
    ];
    let cur = match c.language.as_str() {
        "tr" => 1,
        "en" => 2,
        _ => 0,
    };
    f.choice(p, t!("Arayüz dili", "Interface language"), t!("Otomatik: Windows'un diline göre seçilir.", "Auto: follows the Windows display language."), &langs, cur, 82.0);

    f.section(p, t!("Sistem", "System"));
    f.toggle(p, t!("Windows ile başlat", "Start with Windows"), t!("Oturum açılınca CarlUI hemen başlar.", "CarlUI starts as soon as you sign in."), c.start_with_windows, |c| c.general.start_with_windows ^= true);
    f.toggle(p, t!("Yeni sürümleri bildir", "Notify about new versions"), t!("Günde bir kez GitHub'a bakılır; yeni sürüm kendiliğinden kurulmaz.", "Checks GitHub once a day; nothing is installed automatically."), c.check_updates, |c| c.general.check_updates ^= true);
    f.toggle(p, t!("Windows görev çubuğunu gizle", "Hide the Windows taskbar"), t!("Dock çalışırken Windows'un kendi görev çubuğu görünmez.", "The Windows taskbar stays hidden while the dock runs."), c.hide_taskbar, |c| c.general.hide_taskbar ^= true);
    f.section(p, t!("Yedekleme", "Backup"));
    let r = f.row(p, t!("Ayarları dışa / içe aktar", "Export / import settings"), t!("Tüm ayarlar tek bir .toml dosyasına yazılır; başka bilgisayarda içe aktarılabilir.", "All settings go into one .toml file you can import on another PC."), 230.0);
    let by = r.y + (r.h - 32.0) / 2.0;
    p.button(R::new(r.right() - 16.0 - 104.0, by, 104.0, 32.0), Some('\u{E8B5}'), t!("İçe aktar", "Import"), Act::Backup(false), true, false);
    p.button(R::new(r.right() - 16.0 - 216.0, by, 104.0, 32.0), Some('\u{EDE1}'), t!("Dışa aktar", "Export"), Act::Backup(true), true, false);
    if let Some(n) = &app.notice {
        f.note(p, n);
    }
    f.section(p, t!("Tanıtım", "Tour"));
    let r = f.row(p, t!("Karşılamayı yeniden göster", "Show the welcome again"), t!("İlk kurulumdaki kısa tanıtım: görünüm, dock, ada ve modüller.", "The short first-run tour: look, dock, island and modules."), 150.0);
    let label = t!("Göster", "Show");
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E768}'), label, Act::Welcome, true, false);

    let n = unsafe { GetSystemMetrics(SM_CMONITORS) }.clamp(1, 4) as u32;
    if n > 1 {
        f.section(p, t!("Monitör", "Monitor"));
        let names: Vec<String> = (0..n).map(|i| if i == 0 { t!("Ana", "Main").to_string() } else { format!("{}.", i + 1) }).collect();
        let items: Vec<(&str, crate::Act)> = names.iter().enumerate().map(|(i, s)| (s.as_str(), edit(move |c| c.general.top_monitor = i as u32))).collect();
        f.choice(p, t!("Adanın monitörü", "Island monitor"), t!("Üstteki ada ve bildirimler bu monitörde görünür.", "The top island and notifications appear on this monitor."), &items, c.top_monitor.min(n - 1) as usize, 70.0);
    }
}

fn set_lang(c: &mut carlui_core::config::Config, lang: &str) {
    c.general.language = lang.into();
    carlui_core::lang::apply(lang);
}
