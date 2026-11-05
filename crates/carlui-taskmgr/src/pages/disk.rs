//! Disk: her fiziksel disk için etkinlik grafiği, okuma/yazma hızı,
//! sıcaklık, sağlık ve birimlerin doluluğu. Sayfa kaydırılabilir.

use super::title;
use crate::{
    draw::{bytes, speed, Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    pages::cpu::temp_color,
    Act, App, Tab,
};

const SECTION: f32 = 230.0;

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let sm = &app.sampler;
    title(p, area, "Disk", &t!(format!("{} disk", sm.disks.len()), format!("{} disks", sm.disks.len())));
    let body = R::new(area.x, area.y + 46.0, area.w, area.h - 46.0);
    let total = sm.disks.len() as f32 * (SECTION + 12.0);
    let max = (total - body.h).max(0.0);
    let scroll = app.scroll.get(&Tab::Disk).copied().unwrap_or(0.0).clamp(0.0, max);
    p.hit(body, Act::Scroll);
    unsafe { p.rt.PushAxisAlignedClip(&body.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED) };
    for (i, d) in sm.disks.iter().enumerate() {
        let r = R::new(body.x, body.y + i as f32 * (SECTION + 12.0) - scroll, body.w, SECTION);
        if r.bottom() < body.y || r.y > body.bottom() {
            continue;
        }
        p.card(r);
        let kind = if d.ssd { "SSD" } else { "HDD" };
        p.text(&format!("Disk {} · {}", d.index, d.model), R::new(r.x + 16.0, r.y + 10.0, r.w - 300.0, 24.0), 14.0, true, TEXT, 0.95, Align::Left);
        p.text(&format!("{kind} · {} · {}", d.bus, d.volumes.join(" ")), R::new(r.right() - 300.0, r.y + 10.0, 284.0, 24.0), 12.0, false, TEXT, 0.55, Align::Right);
        // Sol: grafik.
        let gw = r.w * 0.55;
        let g = R::new(r.x + 16.0, r.y + 58.0, gw, r.h - 74.0);
        p.text(&format!("{} {}", t!("Etkinlik", "Activity"), carlui_core::lang::pct(d.active)), R::new(g.x, g.y - 22.0, 200.0, 20.0), 12.0, false, TEXT, 0.6, Align::Left);
        p.text(&t!(format!("Okuma {} · Yazma {}", speed(d.read_bps), speed(d.write_bps)), format!("Read {} · Write {}", speed(d.read_bps), speed(d.write_bps))), R::new(g.right() - 300.0, g.y - 22.0, 300.0, 20.0), 12.0, false, TEXT, 0.7, Align::Right);
        if let Some(h) = sm.disk_hist.get(i) {
            p.graph(crate::sample::len(), g, &h.0, 0x5FD3C6);
        }
        // Sağ: bilgiler.
        let x = g.right() + 24.0;
        let w = r.right() - 16.0 - x;
        let line = |p: &mut Painter, y: f32, label: &str, value: &str, color: u32| {
            p.text(label, R::new(x, y, w, 22.0), 12.5, false, TEXT, 0.6, Align::Left);
            p.text(value, R::new(x, y, w, 22.0), 13.0, true, color, 0.95, Align::Right);
        };
        let mut y = r.y + 44.0;
        let (verdict, why) = d.verdict();
        let vc = match verdict {
            "Kötü" => RED,
            "Dikkat" => ORANGE,
            _ => GREEN,
        };
        let shown = match (verdict, carlui_core::lang::is_tr()) {
            (v, true) => v,
            ("Kötü", false) => "Bad",
            ("Dikkat", false) => "Caution",
            _ => "Good",
        };
        line(p, y, t!("Sağlık", "Health"), shown, vc);
        y += 20.0;
        p.text(&why, R::new(x, y, w, 18.0), 11.5, false, TEXT, 0.5, Align::Right);
        y += 26.0;
        match d.temp {
            Some(t) => line(p, y, t!("Sıcaklık", "Temperature"), &format!("{t:.0} °C"), temp_color(t - 15.0)),
            None => line(p, y, t!("Sıcaklık", "Temperature"), "—", TEXT),
        }
        y += 26.0;
        if let Some(h) = &d.health {
            if h.hours > 0 {
                line(p, y, t!("Çalışma süresi", "Power-on time"), &t!(format!("{} saat", h.hours), format!("{} hours", h.hours)), TEXT);
                y += 26.0;
            }
            if h.written_bytes > 0.0 {
                line(p, y, t!("Toplam yazılan", "Total written"), &bytes(h.written_bytes), TEXT);
                y += 26.0;
            }
        }
        let used = d.used_pct();
        line(p, y, t!("Doluluk", "Used"), &format!("{} / {} · {}", bytes(d.total - d.free), bytes(d.total), carlui_core::lang::pct(used)), TEXT);
        y += 26.0;
        let uc = if used >= 90.0 { RED } else if used >= 75.0 { ORANGE } else { p.accent };
        p.bar(R::new(x, y + 2.0, w, 8.0), used / 100.0, uc);
    }
    unsafe { p.rt.PopAxisAlignedClip() };
    max
}
