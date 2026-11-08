//! Bu modül "küçük araçları nereden açıp kapatırız?" sorununu çözüyor:
//! Araçlar sayfası. Renk seçicinin biçimi, toplu yeniden adlandırma ve disk
//! analizörü modülleri, metin genişletici ve kısaltma listesi burada. Araçların
//! kısayolları Kısayollar sayfasında.

use crate::{
    input::Form,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::config::{ColorFormat, ExpandTrigger, ShotAction, ShotFormat};
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
    f.choice(
        p,
        t!("Panoya kopyalanan biçim", "Format copied to the clipboard"),
        &key_hint(&sc.color_picker),
        &[("HEX", edit(|c| c.tools.color_format = ColorFormat::Hex)), ("RGB", edit(|c| c.tools.color_format = ColorFormat::Rgb)), ("HSL", edit(|c| c.tools.color_format = ColorFormat::Hsl))],
        sel,
        72.0,
    );

    f.section(p, t!("Dosyalar", "Files"));
    f.toggle(
        p,
        t!("Toplu yeniden adlandırma", "Batch rename"),
        &t!(format!("Gezgin'de dosyaları seçip kısayola basın ({}).", sc.rename), format!("Select files in Explorer and press the shortcut ({}).", sc.rename)),
        c.rename_enabled,
        |c| c.tools.rename_enabled ^= true,
    );
    f.toggle(p, t!("Disk analizörü", "Disk analyzer"), &key_hint(&sc.disk), c.disk_enabled, |c| c.tools.disk_enabled ^= true);
    if c.disk_enabled {
        let r = f.row(p, t!("Disk analizörünü aç", "Open the disk analyzer"), t!("Hangi klasörün ne kadar yer kapladığını gösterir.", "Shows how much space each folder takes."), 120.0);
        p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{EDA2}'), t!("Aç", "Open"), Act::Tool("--disk"), true, false);
    }

    f.toggle(
        p,
        t!("Dosya kilidi açıcı", "File unlocker"),
        &t!(format!("Gezgin'de öğeyi seçip kısayola basın ({}).", sc.unlock), format!("Select an item in Explorer and press the shortcut ({}).", sc.unlock)),
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
    let r = f.row(p, t!("Pencere düzenleri", "Window layouts"), &key_hint(&sc.layouts), 120.0);
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
    let ext = if s.format == ShotFormat::Jpg { "jpg" } else { "png" };
    let sample = format!("{}.{ext}", s.file_name("2026-11-05", "14.30.00"));
    let r = f.row(p, t!("Dosya adı şablonu", "File name template"), &t!(format!("Örnek: {sample}"), format!("Example: {sample}")), 120.0);
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E70F}'), t!("Değiştir", "Change"), Act::Input(9), app.input.is_none(), false);
    super::form::draw(app, p, f, Form::ShotName, &[(t!("Şablon — {tarih} ve {saat} yerine konur; boş bırakılırsa varsayılan", "Template — {date} and {time} are filled in; leave empty for the default"), t!("Ekran görüntüsü {tarih} {saat}", "Screenshot {date} {time}"))]);
    let r = f.row(p, t!("Alan kaydı klasörü", "Area recording folder"), &app.cfg.record.folder().display().to_string(), 120.0);
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
        let r = f.row(p, &s.key, &s.text.replace('\n', " ⏎ "), 60.0);
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
