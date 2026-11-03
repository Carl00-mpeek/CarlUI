//! CPU: basit görünümde toplam kullanım grafiği + hız/sıcaklık kartları;
//! ayrıntılı görünümde her mantıksal çekirdeğin kendi grafiği, türü (P/E),
//! frekansı ve fiziksel çekirdeğinin sıcaklığı.

use super::{stat, stat_row, title};
use crate::{
    draw::{Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    Act, App,
};
use carlui_sys::cpu::Kind;

/// Sıcaklığa göre renk.
pub fn temp_color(t: f32) -> u32 {
    if t >= 90.0 {
        RED
    } else if t >= 75.0 {
        ORANGE
    } else {
        GREEN
    }
}

fn uptime() -> String {
    let s = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() } / 1000;
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 { format!("{d} g {h} sa {m} dk") } else { format!("{h} sa {m} dk") }
}

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let sm = &app.sampler;
    let p_cores: std::collections::BTreeSet<u32> = sm.now.cores.iter().filter(|c| c.kind == Kind::Performance).map(|c| c.core).collect();
    let e_cores: std::collections::BTreeSet<u32> = sm.now.cores.iter().filter(|c| c.kind == Kind::Efficient).map(|c| c.core).collect();
    title(p, area, "CPU", &sm.name);
    let sw = R::new(area.right() - 240.0, area.y + 2.0, 240.0, 30.0);
    p.segments(sw, &[("Basit", Act::CpuDetail(false)), ("Ayrıntılı", Act::CpuDetail(true))], app.cpu_detail as usize);

    let temp = sm.temps.as_ref().map(|t| t.package).filter(|t| *t > 0.0);
    if !app.cpu_detail {
        let g = R::new(area.x, area.y + 50.0, area.w, (area.h - 50.0 - 190.0).max(150.0));
        p.text(&format!("Kullanım %{:.0}", sm.now.usage), R::new(g.x, g.y - 2.0, 200.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
        p.text("60 saniye", R::new(g.right() - 200.0, g.y - 2.0, 200.0, 22.0), 12.0, false, TEXT, 0.45, Align::Right);
        let g = R::new(g.x, g.y + 22.0, g.w, g.h - 22.0);
        p.graph(crate::sample::LEN, g, &sm.cpu_hist.0, p.accent);
        let cards = stat_row(area, g.bottom() + 14.0, 3, 82.0);
        stat(p, cards[0], "Kullanım", &format!("%{:.0}", sm.now.usage), "", None);
        stat(p, cards[1], "Hız", &format!("{:.2} GHz", sm.now.mhz / 1000.0), "", None);
        match temp {
            Some(t) => stat(p, cards[2], "Sıcaklık", &format!("{t:.0} °C"), "paket", Some(temp_color(t))),
            None => stat(p, cards[2], "Sıcaklık", "—", "sensör sürücüsü kurulu değil", None),
        }
        let cards = stat_row(area, g.bottom() + 106.0, 3, 82.0);
        let cores = if e_cores.is_empty() { format!("{}", p_cores.len()) } else { format!("{}P + {}E", p_cores.len(), e_cores.len()) };
        stat(p, cards[0], "Çekirdekler", &cores, &format!("{} mantıksal işlemci", sm.now.cores.len()), None);
        let threads: u32 = app.procs.rows.iter().map(|r| r.threads).sum();
        stat(p, cards[1], "İşlemler", &app.procs.rows.len().to_string(), &format!("{threads} iş parçacığı"), None);
        stat(p, cards[2], "Çalışma süresi", &uptime(), "", None);
        return 0.0;
    }

    // Ayrıntılı: çekirdek ızgarası.
    let n = sm.now.cores.len().max(1);
    let cols = if n > 16 { 6 } else { 4 };
    let rows = n.div_ceil(cols);
    let top = area.y + 50.0;
    let gap = 10.0;
    let cw = (area.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let ch = ((area.h - 50.0 - gap * (rows as f32 - 1.0)) / rows as f32).min(170.0);
    for (i, c) in sm.now.cores.iter().enumerate() {
        let r = R::new(area.x + (cw + gap) * (i % cols) as f32, top + (ch + gap) * (i / cols) as f32, cw, ch);
        p.card(r);
        let kind = if c.kind == Kind::Performance { "P" } else { "E" };
        let kc = if c.kind == Kind::Performance { p.accent } else { GREEN };
        let badge = R::new(r.x + 10.0, r.y + 9.0, 20.0, 18.0);
        p.round(badge, 4.0, kc, 0.25);
        p.text(kind, badge, 11.0, true, kc, 1.0, Align::Center);
        p.text(&format!("CPU {}", c.index), R::new(r.x + 36.0, r.y + 6.0, r.w - 46.0, 24.0), 12.5, true, TEXT, 0.9, Align::Left);
        p.text(&format!("%{:.0}", c.usage), R::new(r.x + 10.0, r.y + 6.0, r.w - 20.0, 24.0), 13.0, true, TEXT, 0.95, Align::Right);
        let g = R::new(r.x + 10.0, r.y + 32.0, r.w - 20.0, r.h - 62.0);
        if let Some(h) = sm.core_hist.get(i) {
            p.graph(crate::sample::LEN, g, &h.0, kc);
        }
        p.text(&format!("{:.2} GHz", c.mhz / 1000.0), R::new(r.x + 10.0, r.bottom() - 28.0, r.w - 20.0, 24.0), 12.0, false, TEXT, 0.6, Align::Left);
        match sm.core_temp(c.core) {
            Some(t) => p.text(&format!("{t:.0} °C"), R::new(r.x + 10.0, r.bottom() - 28.0, r.w - 20.0, 24.0), 12.0, true, temp_color(t), 0.95, Align::Right),
            None => p.text("— °C", R::new(r.x + 10.0, r.bottom() - 28.0, r.w - 20.0, 24.0), 12.0, false, TEXT, 0.4, Align::Right),
        }
    }
    0.0
}
