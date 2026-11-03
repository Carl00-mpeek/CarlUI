//! CPU yönlendirme: Windows güç modu (pil tasarrufu / dengeli / performans),
//! P ve E çekirdeklerinin anlık yükü ve en çok CPU kullanan süreçlerin hangi
//! çekirdeklere atandığı. Seçili sürece P/E/tüm çekirdek ataması ve
//! verimlilik modu verilebilir.

use super::{cell, col, colored, dim, table, title};
use crate::{
    draw::{Align, Painter, GREEN, ORANGE, R, TEXT},
    power::{Mode, Route},
    Act, App, Tab,
};
use carlui_sys::cpu::Kind;

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let sm = &app.sampler;
    title(p, area, t!("CPU yönlendirme", "CPU routing"), t!("işlerin hangi çekirdeklerde çalışacağı", "which cores work runs on"));

    let modes = [(t!("Pil tasarrufu", "Battery saver"), Act::Mode(Mode::Saver)), (t!("Dengeli", "Balanced"), Act::Mode(Mode::Balanced)), (t!("Performans", "Performance"), Act::Mode(Mode::Performance))];
    let sel = match app.mode {
        Mode::Saver => 0,
        Mode::Balanced => 1,
        Mode::Performance => 2,
    };
    let seg = R::new(area.x, area.y + 46.0, 420.0, 34.0);
    p.segments(seg, &modes, sel);
    let desc = match app.mode {
        Mode::Saver => t!("İşler E çekirdeklerine, frekans düşük: en az ısı.", "Work goes to E-cores, low clocks: least heat."),
        Mode::Balanced => t!("Ön plan P çekirdeklerine, arka plan E çekirdeklerine.", "Foreground on P-cores, background on E-cores."),
        Mode::Performance => t!("P çekirdekleri öncelikli, yüksek frekans.", "P-cores first, high clocks."),
    };
    p.text(desc, R::new(seg.right() + 16.0, seg.y, area.right() - seg.right() - 16.0, seg.h), 12.5, false, TEXT, 0.6, Align::Left);

    // P / E ortalamaları ve çekirdek başına çubuklar.
    let avg = |k: Kind| {
        let v: Vec<f32> = sm.now.cores.iter().filter(|c| c.kind == k).map(|c| c.usage).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    let box_r = R::new(area.x, seg.bottom() + 16.0, area.w, 150.0);
    p.card(box_r);
    let mut y = box_r.y + 12.0;
    for (k, label, color) in [(Kind::Performance, t!("P çekirdekleri", "P-cores"), p.accent), (Kind::Efficient, t!("E çekirdekleri", "E-cores"), GREEN)] {
        let Some(a) = avg(k) else { continue };
        p.text(label, R::new(box_r.x + 14.0, y, 130.0, 22.0), 12.5, true, color, 0.95, Align::Left);
        p.bar(R::new(box_r.x + 150.0, y + 7.0, 220.0, 8.0), a / 100.0, color);
        p.text(&carlui_core::lang::pct(a), R::new(box_r.x + 380.0, y, 50.0, 22.0), 12.5, false, TEXT, 0.85, Align::Left);
        y += 26.0;
    }
    // Her mantıksal çekirdek dikey çubuk.
    let n = sm.now.cores.len().max(1);
    let bx = box_r.x + 450.0;
    let bw = ((box_r.right() - 14.0 - bx) / n as f32).min(34.0);
    for (i, c) in sm.now.cores.iter().enumerate() {
        let color = if c.kind == Kind::Performance { p.accent } else { GREEN };
        let r = R::new(bx + i as f32 * bw, box_r.y + 14.0, bw - 6.0, box_r.h - 48.0);
        p.round(r, 3.0, 0xFFFFFF, 0.06);
        let fh = r.h * (c.usage / 100.0).clamp(0.0, 1.0);
        if fh > 0.5 {
            p.round(R::new(r.x, r.bottom() - fh, r.w, fh), 3.0, color, 0.85);
        }
        p.text(&c.index.to_string(), R::new(r.x - 4.0, r.bottom() + 2.0, r.w + 8.0, 16.0), 10.5, false, TEXT, 0.5, Align::Center);
    }
    p.text(t!("mantıksal çekirdekler (anlık %)", "logical cores (live %)"), R::new(bx, box_r.bottom() - 16.0, box_r.right() - bx - 14.0, 14.0), 10.5, false, TEXT, 0.4, Align::Center);
    p.text(t!("Toplam", "Total"), R::new(box_r.x + 14.0, box_r.bottom() - 34.0, 130.0, 22.0), 12.5, true, TEXT, 0.8, Align::Left);
    p.bar(R::new(box_r.x + 150.0, box_r.bottom() - 27.0, 220.0, 8.0), sm.now.usage / 100.0, TEXT);
    p.text(&carlui_core::lang::pct(sm.now.usage), R::new(box_r.x + 380.0, box_r.bottom() - 34.0, 50.0, 22.0), 12.5, false, TEXT, 0.85, Align::Left);

    // Seçili süreç için atama düğmeleri.
    let selrow = app.selected.and_then(|pid| app.procs.rows.iter().find(|r| r.pid == pid));
    let by = box_r.bottom() + 14.0;
    let on = selrow.is_some();
    let cur = app.selected.and_then(|pid| app.routes.get(&pid)).copied();
    let mut x = area.x;
    for (label, r, g) in [(t!("Yalnızca P", "P only"), Route::Performance, '\u{E945}'), (t!("Yalnızca E", "E only"), Route::Efficient, '\u{EC0A}'), (t!("Tüm çekirdekler", "All cores"), Route::All, '\u{E950}')] {
        let w = if r == Route::All { 160.0 } else { 120.0 };
        let br = R::new(x, by, w, 30.0);
        if cur.map(|c| c.0) == Some(r) {
            p.round(br, 6.0, p.accent, 0.25);
        }
        p.button(br, Some(g), label, Act::Route(r), on, false);
        x += w + 10.0;
    }
    let eco = cur.and_then(|c| c.1).unwrap_or(false);
    p.button(R::new(x, by, 170.0, 30.0), Some('\u{EC0A}'), if eco { t!("Verimlilik: açık", "Efficiency: on") } else { t!("Verimlilik modu", "Efficiency mode") }, Act::Eco, on, false);
    x += 180.0;
    let info = match selrow {
        Some(r) => t!(format!("{} (PID {}) seçili", r.name, r.pid), format!("{} (PID {}) selected", r.name, r.pid)),
        None => t!("Atama için listeden bir süreç seç", "Pick a process from the list to assign").into(),
    };
    p.text(&info, R::new(x, by, area.right() - x, 30.0), 12.0, false, TEXT, 0.55, Align::Left);

    // En çok CPU kullananlar.
    let mut rows: Vec<&crate::procs::Row> = app.procs.rows.iter().filter(|r| !r.windows && app.routes.contains_key(&r.pid)).collect();
    rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let cols = [col(t!("Ad", "Name"), 0.0, Align::Left, None), col("PID", 70.0, Align::Right, None), col("CPU", 80.0, Align::Right, None), col(t!("Çekirdekler", "Cores"), 150.0, Align::Left, None), col(t!("Verimlilik", "Efficiency"), 110.0, Align::Left, None)];
    let selected = app.selected.and_then(|pid| rows.iter().position(|r| r.pid == pid));
    let ty = by + 44.0;
    let scroll = app.scroll.get(&Tab::Routing).copied().unwrap_or(0.0);
    table(
        p,
        R::new(area.x, ty, area.w, area.bottom() - ty),
        &cols,
        None,
        rows.len(),
        scroll,
        selected,
        |i, c| {
            let r = rows[i];
            let (route, eco) = app.routes.get(&r.pid).copied().unwrap_or((Route::Unknown, None));
            match c {
                0 => cell(r.name.clone()),
                1 => dim(r.pid.to_string()),
                2 => {
                    if r.cpu < 0.05 {
                        dim("0")
                    } else {
                        cell(t!(format!("%{:.1}", r.cpu), format!("{:.1}%", r.cpu)))
                    }
                }
                3 => match route {
                    Route::All | Route::Unknown => dim(route.label()),
                    _ => colored(route.label(), ORANGE),
                },
                _ => match eco {
                    Some(true) => colored(t!("Açık", "On"), GREEN),
                    Some(false) => dim(t!("Kapalı", "Off")),
                    None => dim("—"),
                },
            }
        },
        |i| Act::Proc(rows[i].pid),
        Act::Scroll,
    )
}
