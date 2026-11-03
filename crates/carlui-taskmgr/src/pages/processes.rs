//! İşlemler: süreçler uygulamaya göre gruplu (aynı exe tek satır, toplam
//! kullanım). Birden çok süreci olan uygulamanın solundaki ok tıklanınca
//! aşağı döner ve süreçler kısa bir animasyonla altında açılır; her biri kendi
//! kullanımını gösterir (CPU, GPU, bellek, G/Ç). İmzasızları öne alma ve Windows süreçlerini gizleme
//! anahtarları, klavyeyle filtre, seçili uygulama/süreç için sonlandır,
//! durdur/sürdür ve dosya konumu.

use super::{cell, col, colored, dim, table_ex, title, Cell};
use crate::{
    draw::{bytes, speed, Align, Painter, ORANGE, R, RED, TEXT},
    groups::{self, Line},
    sign::Sign,
    Act, App, SortCol, Tab,
};

fn cpu_cell(v: f32) -> Cell {
    let t = t!(format!("%{v:.1}"), format!("{v:.1}%"));
    if v >= 50.0 {
        colored(t, RED)
    } else if v >= 10.0 {
        colored(t, ORANGE)
    } else if v < 0.05 {
        dim("0")
    } else {
        cell(t)
    }
}

fn io_cell(v: f64) -> Cell {
    if v < 1024.0 { dim("0") } else { cell(speed(v)) }
}

pub fn paint(app: &App, p: &mut Painter, area: R) -> f32 {
    let gs = groups::build(app);
    let lines = groups::lines(app, &gs);
    let total = app.procs.rows.len();
    title(p, area, t!("İşlemler", "Processes"), &t!(format!("{} uygulama · {} süreç", gs.len(), total), format!("{} apps · {} processes", gs.len(), total)));

    // Anahtarlar + filtre.
    let y = area.y + 46.0;
    let mut x = area.x;
    x += p.toggle(x, y, t!("İmzasızlar önce", "Unsigned first"), app.unsigned_first, Act::UnsignedFirst) + 24.0;
    x += p.toggle(x, y, t!("Windows süreçlerini gizle", "Hide Windows processes"), app.hide_windows, Act::HideWindows) + 24.0;
    let fr = R::new(x, y, (area.right() - x).clamp(120.0, 260.0), 26.0);
    p.round(fr, 6.0, 0x000000, 0.2);
    p.outline(fr, 6.0, 0xFFFFFF, 0.1);
    p.glyph('\u{E721}', R::new(fr.x + 4.0, fr.y, 22.0, fr.h), 12.0, TEXT, 0.6);
    if app.filter.is_empty() {
        p.text(t!("Ara (yazmaya başla)", "Search (start typing)"), R::new(fr.x + 28.0, fr.y, fr.w - 34.0, fr.h), 12.5, false, TEXT, 0.4, Align::Left);
    } else {
        p.text(&app.filter, R::new(fr.x + 28.0, fr.y, fr.w - 56.0, fr.h), 12.5, false, TEXT, 0.95, Align::Left);
        let xr = R::new(fr.right() - 26.0, fr.y, 26.0, fr.h);
        p.glyph('\u{E711}', xr, 10.0, TEXT, if p.hover(xr) { 0.9 } else { 0.5 });
        p.hit(xr, Act::ClearFilter);
    }

    // Seçili uygulama/süreç düğmeleri.
    let (name, targets) = app.targets();
    let by = y + 40.0;
    let on = !targets.is_empty();
    p.button(R::new(area.x, by, 150.0, 30.0), Some('\u{E711}'), t!("Görevi sonlandır", "End task"), Act::Kill, on, true);
    if on && targets.iter().all(|r| r.suspended) {
        p.button(R::new(area.x + 160.0, by, 110.0, 30.0), Some('\u{E768}'), t!("Sürdür", "Resume"), Act::Resume, on, false);
    } else {
        p.button(R::new(area.x + 160.0, by, 110.0, 30.0), Some('\u{E733}'), t!("Durdur", "Suspend"), Act::Suspend, on, false);
    }
    let path = targets.iter().find(|r| !r.path.is_empty()).map(|r| r.path.clone());
    p.button(R::new(area.x + 280.0, by, 150.0, 30.0), Some('\u{E838}'), t!("Dosya konumu", "File location"), Act::Reveal, path.is_some(), false);
    if on {
        let threads: u32 = targets.iter().map(|r| r.threads).sum();
        let pid = if targets.len() == 1 { format!("PID {}", targets[0].pid) } else { t!(format!("{} süreç", targets.len()), format!("{} processes", targets.len())) };
        let info = format!("{name} · {pid} · {threads} {} · {}", t!("iş parçacığı", "threads"), path.as_deref().unwrap_or(t!("yol okunamadı", "path unavailable")));
        p.text(&info, R::new(area.x + 444.0, by, area.right() - area.x - 444.0, 30.0), 12.0, false, TEXT, 0.55, Align::Left);
    }

    let cols = [
        col(t!("Ad", "Name"), 0.0, Align::Left, Some(Act::Sort(SortCol::Name))),
        col("PID", 70.0, Align::Right, Some(Act::Sort(SortCol::Pid))),
        col(t!("Durum", "Status"), 110.0, Align::Left, None),
        col("CPU", 70.0, Align::Right, Some(Act::Sort(SortCol::Cpu))),
        col("GPU", 70.0, Align::Right, Some(Act::Sort(SortCol::Gpu))),
        col(t!("Bellek", "Memory"), 100.0, Align::Right, Some(Act::Sort(SortCol::Mem))),
        col(t!("G/Ç", "I/O"), 100.0, Align::Right, Some(Act::Sort(SortCol::Io))),
        col(t!("İmza", "Signature"), 90.0, Align::Left, Some(Act::Sort(SortCol::Sign))),
    ];
    let sorted = Some((
        match app.sort.0 {
            SortCol::Name => 0,
            SortCol::Pid => 1,
            SortCol::Cpu => 3,
            SortCol::Gpu => 4,
            SortCol::Mem => 5,
            SortCol::Io => 6,
            SortCol::Sign => 7,
        },
        app.sort.1,
    ));
    let selected = lines.iter().position(|(l, _)| match *l {
        Line::Group(g) => match (&app.selected_group, app.selected) {
            (Some(k), _) => gs[g].key == *k,
            (None, Some(pid)) => gs[g].rows.len() == 1 && gs[g].rows[0].pid == pid,
            _ => false,
        },
        Line::Child(g, c) => app.selected_group.is_none() && app.selected == Some(gs[g].rows[c].pid),
    });
    let sign_cell = |path: &str| {
        let s = app.signs.get(path);
        match s {
            Sign::Unsigned => colored(s.label(), ORANGE),
            _ => Cell { text: s.label().into(), color: TEXT, alpha: 0.5 },
        }
    };
    let ty = by + 44.0;
    let scroll = app.scroll.get(&Tab::Processes).copied().unwrap_or(0.0);
    table_ex(
        p,
        R::new(area.x, ty, area.w, area.bottom() - ty),
        &cols,
        sorted,
        lines.len(),
        scroll,
        selected,
        |i, c| match lines[i].0 {
            Line::Group(g) => {
                let g = &gs[g];
                let one = g.rows.len() == 1;
                match c {
                    0 => cell(""),
                    1 => {
                        if one {
                            dim(g.rows[0].pid.to_string())
                        } else {
                            dim("")
                        }
                    }
                    2 => {
                        let n = g.rows.iter().filter(|r| r.suspended).count();
                        if g.suspended {
                            colored(t!("Durduruldu", "Suspended"), ORANGE)
                        } else if n > 0 {
                            colored(t!(format!("{n} durduruldu"), format!("{n} suspended")), ORANGE)
                        } else {
                            dim(t!("Çalışıyor", "Running"))
                        }
                    }
                    3 => cpu_cell(g.cpu),
                    4 => cpu_cell(g.gpu.min(100.0)),
                    5 => cell(bytes(g.mem as f64)),
                    6 => io_cell(g.io),
                    _ => sign_cell(&g.path),
                }
            }
            Line::Child(g, ci) => {
                let r = gs[g].rows[ci];
                match c {
                    0 => cell(""),
                    1 => dim(r.pid.to_string()),
                    2 => {
                        if r.suspended {
                            colored(t!("Durduruldu", "Suspended"), ORANGE)
                        } else {
                            dim(t!("Çalışıyor", "Running"))
                        }
                    }
                    3 => cpu_cell(r.cpu),
                    4 => cpu_cell(r.gpu),
                    5 => cell(bytes(r.mem as f64)),
                    6 => io_cell(r.io),
                    _ => dim(""),
                }
            }
        },
        |i| match lines[i].0 {
            Line::Group(g) if gs[g].rows.len() == 1 => Act::Proc(gs[g].rows[0].pid),
            Line::Group(g) => Act::Group(gs[g].key.clone()),
            Line::Child(g, c) => Act::Proc(gs[g].rows[c].pid),
        },
        Act::Scroll,
        |i| lines[i].1,
        |p, i, r| {
            let k = lines[i].1;
            match lines[i].0 {
                Line::Group(g) => {
                    let g = &gs[g];
                    let multi = g.rows.len() > 1;
                    if multi {
                        let open = groups::openness(app, &g.key);
                        let ar = R::new(r.x, r.y + 4.0, 22.0, r.h - 8.0);
                        if p.hover(ar) {
                            p.round(ar, 4.0, 0xFFFFFF, 0.10);
                        }
                        p.glyph_rot('\u{E76C}', ar, 10.0, TEXT, 0.8, 90.0 * open);
                        p.hit(ar, Act::Expand(g.key.clone()));
                    }
                    let nx = r.x + 28.0;
                    let nw = p.fonts.width(&g.name, 13.0, false);
                    p.text(&g.name, R::new(nx, r.y, (r.w - 34.0).min(nw + 4.0), r.h), 13.0, false, TEXT, 0.92, Align::Left);
                    if multi {
                        p.text(&format!("({})", g.rows.len()), R::new(nx + nw + 6.0, r.y, 60.0, r.h), 12.0, false, TEXT, 0.45, Align::Left);
                    }
                }
                Line::Child(g, c) => {
                    let row = gs[g].rows[c];
                    // Ağaç çizgisi: gruptan alt satırlara.
                    p.fill(R::new(r.x + 10.0, r.y, 1.0, r.h), 0xFFFFFF, 0.12 * k);
                    p.fill(R::new(r.x + 10.0, r.y + r.h / 2.0, 12.0, 1.0), 0xFFFFFF, 0.12 * k);
                    p.text(&row.name, R::new(r.x + 28.0, r.y, r.w - 34.0, r.h), 12.5, false, TEXT, 0.72 * k, Align::Left);
                }
            }
        },
    )
}
