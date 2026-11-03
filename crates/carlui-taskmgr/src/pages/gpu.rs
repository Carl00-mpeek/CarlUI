//! GPU: yük ve VRAM grafikleri, sıcaklık, saat hızları, güç ve fan.

use super::{stat, stat_row, title};
use crate::{
    draw::{bytes, Align, Painter, GREEN, R, TEXT},
    pages::cpu::temp_color,
    App,
};

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let sm = &app.sampler;
    let Some(g) = &sm.gpu else {
        title(p, area, "GPU", "");
        p.text(t!("Ekran kartı bilgisi okunamadı.", "Couldn't read graphics card info."), R::new(area.x, area.y + 60.0, area.w, 30.0), 13.0, false, TEXT, 0.6, Align::Left);
        return 0.0;
    };
    title(p, area, "GPU", &g.name);
    let gh = ((area.h - 50.0 - 200.0) / 2.0).max(110.0);
    let vram_pct = if g.vram_total > 0.0 { g.vram_used / g.vram_total * 100.0 } else { 0.0 };
    let mut y = area.y + 50.0;
    for (label, value, hist, color) in [(t!("Etkinlik", "Activity"), carlui_core::lang::pct(g.load), &sm.gpu_hist, p.accent), (t!("Video belleği (VRAM)", "Video memory (VRAM)"), format!("{} / {} · {}", bytes(g.vram_used), bytes(g.vram_total), carlui_core::lang::pct(vram_pct)), &sm.vram_hist, GREEN)] {
        p.text(label, R::new(area.x, y, 300.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
        p.text(&value, R::new(area.right() - 300.0, y, 300.0, 22.0), 12.0, false, TEXT, 0.8, Align::Right);
        p.graph(crate::sample::LEN, R::new(area.x, y + 22.0, area.w, gh - 22.0), &hist.0, color);
        y += gh + 10.0;
    }
    let cards = stat_row(area, y + 4.0, 4, 82.0);
    stat(p, cards[0], t!("Etkinlik", "Activity"), &carlui_core::lang::pct(g.load), "", None);
    stat(p, cards[1], "VRAM", &bytes(g.vram_used), &format!("{} {} · {}", bytes(g.vram_total), t!("toplam", "total"), carlui_core::lang::pct(vram_pct)), None);
    if g.temp > 0.0 {
        stat(p, cards[2], t!("Sıcaklık", "Temperature"), &format!("{:.0} °C", g.temp), "", Some(temp_color(g.temp)));
    } else {
        stat(p, cards[2], t!("Sıcaklık", "Temperature"), "—", "", None);
    }
    let fan = g.fan.map(|f| format!(" · fan {}", carlui_core::lang::pct(f as f32))).unwrap_or_default();
    stat(p, cards[3], t!("Güç", "Power"), &if g.power_w > 0.0 { format!("{:.0} W", g.power_w) } else { "—".into() }, &fan.trim_start_matches(" · ").to_string(), None);
    let cards = stat_row(area, y + 96.0, 4, 82.0);
    stat(p, cards[0], t!("Çekirdek saati", "Core clock"), &if g.clock_mhz > 0 { format!("{} MHz", g.clock_mhz) } else { "—".into() }, "", None);
    stat(p, cards[1], t!("Bellek saati", "Memory clock"), &if g.mem_clock_mhz > 0 { format!("{} MHz", g.mem_clock_mhz) } else { "—".into() }, "", None);
    stat(p, cards[2], t!("Boş VRAM", "Free VRAM"), &bytes((g.vram_total - g.vram_used).max(0.0)), "", None);
    stat(p, cards[3], "Fan", &g.fan.map(|f| carlui_core::lang::pct(f as f32)).unwrap_or("—".into()), "", None);
    0.0
}
