//! RAM: kullanım grafiği, kullanılan/boş, ayrılmış (commit) ve önbellek.

use super::{stat, stat_row, title};
use crate::{
    draw::{bytes, Align, Painter, ORANGE, R, RED, TEXT},
    App,
};

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let sm = &app.sampler;
    let r = &sm.ram;
    title(p, area, "RAM", &format!("{} toplam", bytes(r.total)));
    let pct = r.percent();
    let g = R::new(area.x, area.y + 50.0, area.w, (area.h - 50.0 - 200.0).max(150.0));
    p.text("Bellek kullanımı", R::new(g.x, g.y, 300.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
    p.text(&format!("{} / {} · %{pct:.0}", bytes(r.used), bytes(r.total)), R::new(g.right() - 300.0, g.y, 300.0, 22.0), 12.0, false, TEXT, 0.8, Align::Right);
    let color = if pct >= 90.0 { RED } else if pct >= 75.0 { ORANGE } else { 0xB98CFF };
    let gr = R::new(g.x, g.y + 22.0, g.w, g.h - 22.0);
    p.graph(crate::sample::LEN, gr, &sm.ram_hist.0, color);

    // Bileşim çubuğu: kullanılan | önbellek | boş.
    let br = R::new(area.x, gr.bottom() + 12.0, area.w, 14.0);
    p.round(br, 4.0, 0xFFFFFF, 0.08);
    if r.total > 0.0 {
        let uw = (br.w * (r.used / r.total) as f32).max(0.0);
        let cw = (br.w * (r.cached.min(r.total - r.used) / r.total) as f32).max(0.0);
        p.round(R::new(br.x, br.y, uw, br.h), 4.0, color, 0.9);
        p.fill(R::new(br.x + uw, br.y, cw, br.h), color, 0.35);
    }
    let cards = stat_row(area, br.bottom() + 14.0, 4, 82.0);
    stat(p, cards[0], "Kullanılan", &bytes(r.used), &format!("%{pct:.0}"), Some(color));
    stat(p, cards[1], "Boş", &bytes((r.total - r.used).max(0.0)), "", None);
    stat(p, cards[2], "Önbellek", &bytes(r.cached), "gerektiğinde boşaltılır", None);
    let cp = if r.commit_limit > 0.0 { r.commit / r.commit_limit * 100.0 } else { 0.0 };
    stat(p, cards[3], "Ayrılmış", &format!("{} / {}", bytes(r.commit), bytes(r.commit_limit)), &format!("%{cp:.0} (sayfa dosyası dahil)"), None);
    let cards = stat_row(area, br.bottom() + 106.0, 2, 82.0);
    let mut top: Vec<&crate::procs::Row> = app.procs.rows.iter().collect();
    top.sort_by(|a, b| b.mem.cmp(&a.mem));
    let names: Vec<String> = top.iter().take(3).map(|r| format!("{} {}", r.name, bytes(r.mem as f64))).collect();
    stat(p, cards[0], "En çok kullanan", top.first().map(|r| r.name.as_str()).unwrap_or("—"), &names.join(" · "), None);
    let all: u64 = app.procs.rows.iter().map(|r| r.mem).sum();
    stat(p, cards[1], "Süreçlerin çalışma kümesi", &bytes(all as f64), &format!("{} süreç", app.procs.rows.len()), None);
    0.0
}
