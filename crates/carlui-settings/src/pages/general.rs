//! Genel: dil, Windows ile başlama, görev çubuğu, adanın monitörü.

use crate::{
    ui::{edit, Flow, P},
    App,
};
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

    f.section(p, t!("Başlangıç", "Startup"));
    f.toggle(p, t!("Windows ile başlat", "Start with Windows"), t!("Oturum açılınca CarlUI hemen başlar.", "CarlUI starts as soon as you sign in."), c.start_with_windows, |c| c.general.start_with_windows ^= true);
    f.section(p, t!("Görev çubuğu", "Taskbar"));
    f.toggle(p, t!("Windows görev çubuğunu gizle", "Hide the Windows taskbar"), t!("Dock çalışırken Windows'un kendi görev çubuğu görünmez.", "The Windows taskbar stays hidden while the dock runs."), c.hide_taskbar, |c| c.general.hide_taskbar ^= true);

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
