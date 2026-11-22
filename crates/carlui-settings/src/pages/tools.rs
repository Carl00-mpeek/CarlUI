//! Bu modül "küçük araçları nereden açıp kapatırız?" sorununu çözüyor:
//! Araçlar sayfası. Renk seçicinin biçimi, toplu yeniden adlandırma ve disk
//! analizörü modülleri, metin genişletici ve kısaltma listesi burada. Araçların
//! kısayolları Kısayollar sayfasında.

use crate::{
    input::Form,
    ui::{edit, Flow, LeadRow, P},
    Act, App,
};
use carlui_core::config::{ColorFormat, ExpandTrigger, ShotAction, ShotFormat};
use carlui_core::{tools::ToolsConfig, zones::ZoneLayout};
use carlui_ui::paint::{Align, R, TEXT};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.tools;
    let sc = &app.cfg.shortcuts;
    let key_hint = |v: &str| if v.trim().is_empty() { t!("Kısayolu Kısayollar sayfasından atayın.", "Assign a shortcut on the Shortcuts page.").to_string() } else { t!(format!("Kısayol: {v}"), format!("Shortcut: {v}")) };

    f.section(p, t!("Renk seçici", "Color picker"));
    let sel = match c.color_format {
        ColorFormat::Hex => 0,
        ColorFormat::Rgb => 1,
        ColorFormat::Hsl => 2,
    };
    // Atanmış kısayol (ya da nereden atanacağı) kartta görünür kalır.
    f.choice_sub(
        p,
        t!("Panoya kopyalanan biçim", "Format copied to the clipboard"),
        "",
        &key_hint(&sc.color_picker),
        &[("HEX", edit(|c| c.tools.color_format = ColorFormat::Hex)), ("RGB", edit(|c| c.tools.color_format = ColorFormat::Rgb)), ("HSL", edit(|c| c.tools.color_format = ColorFormat::Hsl))],
        sel,
        72.0,
    );

    f.section(p, t!("Dosyalar", "Files"));
    f.toggle_sub(
        p,
        t!("Toplu yeniden adlandırma", "Batch rename"),
        &t!(format!("Gezgin'de dosyaları seçip kısayola basın ({}).", sc.rename), format!("Select files in Explorer and press the shortcut ({}).", sc.rename)),
        &key_hint(&sc.rename),
        None,
        c.rename_enabled,
        |c| c.tools.rename_enabled ^= true,
    );
    f.toggle_sub(p, t!("Disk analizörü", "Disk analyzer"), "", &key_hint(&sc.disk), None, c.disk_enabled, |c| c.tools.disk_enabled ^= true);
    if c.disk_enabled {
        let r = f.row(p, t!("Disk analizörünü aç", "Open the disk analyzer"), t!("Hangi klasörün ne kadar yer kapladığını gösterir.", "Shows how much space each folder takes."), 120.0);
        p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{EDA2}'), t!("Aç", "Open"), Act::Tool("--disk"), true, false);
    }

    f.toggle_sub(
        p,
        t!("Dosya kilidi açıcı", "File unlocker"),
        &t!(format!("Gezgin'de öğeyi seçip kısayola basın ({}).", sc.unlock), format!("Select an item in Explorer and press the shortcut ({}).", sc.unlock)),
        &key_hint(&sc.unlock),
        None,
        c.unlock_enabled,
        |c| c.tools.unlock_enabled ^= true,
    );
    if c.unlock_enabled {
        let r = f.row(p, t!("Kilit açıcıyı aç", "Open the unlocker"), t!("Bir dosyayı hangi uygulamanın tuttuğunu gösterir, tek tıkla bıraktırır.", "Shows which app holds a file and makes it let go in one click."), 120.0);
        p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E785}'), t!("Aç", "Open"), Act::Tool("--unlock"), true, false);
    }

    f.section(p, t!("Pencereler", "Windows"));
    f.toggle(
        p,
        t!("Pencere yerlerini hatırla", "Remember window positions"),
        t!("Ekran çıkarılıp takılınca ya da uykudan sonra pencereler eski yerlerine döner.", "When a screen comes back or the PC wakes, windows return to their places."),
        c.window_memory,
        |c| c.tools.window_memory ^= true,
    );
    f.toggle(
        p,
        t!("Pencere yakalama bölgeleri", "Window snap zones"),
        t!(
            "Pencereyi başlığından sürüklerken Shift'e basılı tutun: bölgeler belirir, bırakınca pencere imlecin altındaki bölgeye oturur. Ctrl+Shift ile Ctrl'e bastığınız bölgeden imlecin bölgesine kadar birleşir.",
            "Hold Shift while dragging a window by its title bar: zones appear and the window fills the one under the cursor when you let go. With Ctrl+Shift, zones merge from where you pressed Ctrl to where the cursor is."
        ),
        c.snap_zones,
        |c| c.tools.snap_zones ^= true,
    );
    if c.snap_zones {
        zone_layouts(p, f, c);
        let sel = match c.zone_gap {
            0 => 0,
            1..=11 => 1,
            _ => 2,
        };
        f.choice(
            p,
            t!("Bölge boşluğu", "Zone spacing"),
            t!("Pencereler arasında ve ekran kenarında bırakılan boşluk.", "Space left between windows and along the screen edge."),
            &[(t!("Yok", "None"), edit(|c| c.tools.zone_gap = 0)), (t!("Dar", "Narrow"), edit(|c| c.tools.zone_gap = 8)), (t!("Geniş", "Wide"), edit(|c| c.tools.zone_gap = 16))],
            sel,
            84.0,
        );
    }
    let r = f.row_sub(p, t!("Pencere düzenleri", "Window layouts"), "", &key_hint(&sc.layouts), None, 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E737}'), t!("Aç", "Open"), Act::Tool("--layouts"), true, false);

    f.section(p, t!("Ekran görüntüsü", "Screenshot"));
    let s = &app.cfg.shots;
    let sel = match s.action {
        ShotAction::Copy => 0,
        ShotAction::Save => 1,
        ShotAction::Both => 2,
    };
    f.choice(
        p,
        t!("Enter'a basınca", "When you press Enter"),
        t!("Özel tam ekran oyunlarda da bu yapılır. Ctrl+C kopyalar, Ctrl+S kaydeder.", "Also used in exclusive fullscreen games. Ctrl+C copies, Ctrl+S saves."),
        &[
            (t!("Kopyala", "Copy"), edit(|c| c.shots.action = ShotAction::Copy)),
            (t!("Kaydet", "Save"), edit(|c| c.shots.action = ShotAction::Save)),
            (t!("İkisi", "Both"), edit(|c| c.shots.action = ShotAction::Both)),
        ],
        sel,
        84.0,
    );
    let sel = if s.format == ShotFormat::Jpg { 1 } else { 0 };
    f.choice(
        p,
        t!("Dosya biçimi", "File format"),
        t!("JPG daha küçüktür; PNG kayıpsızdır. Panoya her zaman PNG gider.", "JPG is smaller; PNG is lossless. The clipboard always gets PNG."),
        &[("PNG", edit(|c| c.shots.format = ShotFormat::Png)), ("JPG", edit(|c| c.shots.format = ShotFormat::Jpg))],
        sel,
        72.0,
    );
    f.toggle(
        p,
        t!("Tam ekranda da alan seç (deneysel)", "Select an area in fullscreen too (experimental)"),
        t!(
            "Kapalıyken tam ekran oyunda ekranın tamamı alınır. Açıkken alan seçme ekranı açılır; çoğu oyunda çalışır ama bazılarında oyun küçülebilir ya da seçim ekranı görünmeyebilir.",
            "When off, fullscreen games capture the whole screen. When on, the area picker opens; works in most games, but some may minimize or hide the picker."
        ),
        s.select_in_fullscreen,
        |c| c.shots.select_in_fullscreen ^= true,
    );
    let ext = if s.format == ShotFormat::Jpg { "jpg" } else { "png" };
    let sample = format!("{}.{ext}", s.file_name("2026-11-05", "14.30.00"));
    // Örnek dosya adı ve kayıt klasörü canlı değer: kartta görünür.
    let r = f.row_sub(p, t!("Dosya adı şablonu", "File name template"), "", &t!(format!("Örnek: {sample}"), format!("Example: {sample}")), None, 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E70F}'), t!("Değiştir", "Change"), Act::Input(9), app.input.is_none(), false);
    super::form::draw(app, p, f, Form::ShotName, &[(t!("Şablon — {tarih} ve {saat} yerine konur; boş bırakılırsa varsayılan", "Template — {date} and {time} are filled in; leave empty for the default"), t!("Ekran görüntüsü {tarih} {saat}", "Screenshot {date} {time}"))]);
    let r = f.row_sub(p, t!("Alan kaydı klasörü", "Area recording folder"), "", &app.cfg.record.folder().display().to_string(), None, 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E838}'), t!("Aç", "Open"), Act::RecordFolder, true, false);

    f.section(p, t!("Görev yöneticisi", "Task manager"));
    let sel = if app.cfg.taskmgr.history_secs >= 120 { 1 } else { 0 };
    f.choice(
        p,
        t!("Grafik geçmişi", "Graph history"),
        t!("CPU, bellek, disk ve GPU grafiklerinin gösterdiği süre.", "How far back the CPU, memory, disk and GPU graphs go."),
        &[(t!("60 sn", "60 s"), edit(|c| c.taskmgr.history_secs = 60)), (t!("5 dk", "5 min"), edit(|c| c.taskmgr.history_secs = 300))],
        sel,
        80.0,
    );

    f.section(p, t!("Metin genişletici", "Text expander"));
    f.toggle(
        p,
        t!("Kısaltmaları aç", "Expand abbreviations"),
        t!("Bir kısaltma yazınca yerine metni yazılır. {tarih}, {saat}, {pano} kullanılabilir.", "Typing an abbreviation replaces it with its text. {date}, {time}, {clipboard} can be used."),
        c.expander,
        |c| c.tools.expander ^= true,
    );
    if !c.expander {
        return;
    }
    let sel = if c.expand_on == ExpandTrigger::Delimiter { 1 } else { 0 };
    f.choice(
        p,
        t!("Ne zaman açılsın", "When to expand"),
        t!("Boşluk/Tab seçilirse kısaltma ancak ardından o tuşa basınca açılır; kelime içinde yanlışlıkla tetiklenmez.", "With Space/Tab, an abbreviation expands only when you press that key after it, so it won't fire by accident mid-word."),
        &[(t!("Hemen", "Immediately"), edit(|c| c.tools.expand_on = ExpandTrigger::Immediate)), (t!("Boşluk/Tab", "Space/Tab"), edit(|c| c.tools.expand_on = ExpandTrigger::Delimiter))],
        sel,
        96.0,
    );
    super::applist::draw(
        app,
        p,
        f,
        &c.expand_excluded,
        |c| &mut c.tools.expand_excluded,
        Form::ExpandApp,
        8,
        t!("Çalışmayacağı uygulamalar", "Apps where it's off"),
        t!("Bu uygulamalar öndeyken kısaltmalar açılmaz (ör. terminal, oyun).", "Abbreviations don't expand while these apps are in front (e.g. terminals, games)."),
        t!("exe adı, ör. cs2.exe", "exe name, e.g. cs2.exe"),
    );
    for (i, s) in c.snippets.iter().enumerate() {
        // Kısaltmanın metni kayıtlı veri: kartta görünür.
        let r = f.row_sub(p, &s.key, "", &s.text.replace('\n', " ⏎ "), None, 60.0);
        let del = R::new(r.right() - 12.0 - 32.0, r.y + (r.h - 32.0) / 2.0, 32.0, 32.0);
        if p.hover(del) {
            p.round(del, 6.0, 0xFFFFFF, 0.10);
        }
        p.glyph('\u{E74D}', del, 13.0, TEXT, 0.75);
        p.hit(
            del,
            edit(move |c| {
                if i < c.tools.snippets.len() {
                    c.tools.snippets.remove(i);
                }
            }),
        );
    }
    f.flush(p);
    if c.snippets.is_empty() {
        p.text(t!("Henüz kısaltma yok.", "No abbreviations yet."), R::new(f.x + 4.0, f.y, f.w, 24.0), 12.5, false, TEXT, 0.5, Align::Left);
        f.y += 28.0;
    }
    let r = f.row(p, t!("Kısaltma ekle", "Add abbreviation"), t!("Aynı kısaltma yeniden eklenirse metni güncellenir.", "Adding the same abbreviation again updates its text."), 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E710}'), t!("Ekle", "Add"), Act::Input(5), app.input.is_none(), false);
    super::form::draw(app, p, f, Form::Snippet, &[(t!("Kısaltma", "Abbreviation"), t!("ör. ;adres", "e.g. ;addr")), (t!("Metin", "Text"), t!("ör. Atatürk Cad. No 1, İstanbul — Ctrl+V çok satırlı metni de yapıştırır", "e.g. 1 Main St, Springfield — Ctrl+V also pastes multi-line text"))]);
}

/// Bölge düzeni seçimi: her düğmede düzenin küçük bir önizlemesi, dörder dörder iki satır.
fn zone_layouts(p: &mut P, f: &mut Flow, c: &ToolsConfig) {
    const PER_ROW: usize = 4;
    const LEAD: f32 = 22.0;
    let name = |l: ZoneLayout| match l {
        ZoneLayout::Auto => t!("Otomatik", "Auto"),
        ZoneLayout::Halves => t!("Yarım", "Halves"),
        ZoneLayout::Thirds => t!("Üçte bir", "Thirds"),
        ZoneLayout::Focus => t!("Odak", "Focus"),
        ZoneLayout::MainSide => t!("Ana + yan", "Main + side"),
        ZoneLayout::Grid => t!("Izgara", "Grid"),
        ZoneLayout::Grid6 => t!("Altılı", "Six"),
        ZoneLayout::Custom => t!("Özel", "Custom"),
    };
    let items: Vec<(&str, Act)> = ZoneLayout::ALL
        .iter()
        .map(|l| {
            let l = *l;
            (name(l), edit(move |c| c.tools.zone_layout = l))
        })
        .collect();
    let selected = ZoneLayout::ALL.iter().position(|l| *l == c.zone_layout).unwrap_or(0);
    let leads = [LEAD; PER_ROW];
    let each = items.iter().map(|(n, _)| p.fonts.width(n, 13.0, true) + LEAD + 6.0 + 20.0).fold(96.0_f32, f32::max).ceil();
    let rows: Vec<LeadRow> = items
        .chunks(PER_ROW)
        .enumerate()
        .map(|(i, it)| LeadRow { items: it, selected: selected.checked_sub(i * PER_ROW).filter(|s| *s < it.len()).unwrap_or(usize::MAX), each, lead_w: &leads[..it.len()] })
        .collect();
    let custom = c.zones_custom.clone();
    f.choice_lead_rows(
        p,
        t!("Bölge düzeni", "Zone layout"),
        t!(
            "Otomatik: geniş ekranda odak, dikey ekranda iki satır, diğerlerinde 2×2 ızgara. Özel düzen config.toml'daki zones_custom listesinden gelir (yüzde olarak x, y, genişlik, yükseklik).",
            "Auto: focus on ultrawide, two rows on portrait, a 2×2 grid otherwise. Custom comes from zones_custom in config.toml (percent x, y, width, height)."
        ),
        &rows,
        &|p, row, i, r| {
            let Some(l) = ZoneLayout::ALL.get(row * PER_ROW + i) else { return };
            let (w, h) = (LEAD, 14.0);
            let b = R::new(r.x, (r.y + (r.h - h) / 2.0).round(), w, h);
            for z in carlui_core::zones::fractions(*l, &custom, 16.0 / 9.0) {
                let zr = R::new(b.x + z[0] * w, b.y + z[1] * h, z[2] * w, z[3] * h).inset(0.75);
                p.round(zr, 1.5, TEXT, 0.55);
            }
        },
    );
}
