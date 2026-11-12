//! Görünüm modülünün birim testleri.

use super::*;
use crate::config::{DockBehavior, Pinned};

/// Görünüm dışı alanları dolu, tanınabilir bir ayar.
fn busy() -> Config {
    let mut c = Config::default();
    c.dock.pinned = vec![Pinned { name: "Not".into(), path: "C:\\x\\not.exe".into(), args: String::new(), icon: String::new(), aumid: String::new(), exe: String::new() }];
    c.dock.stacks = vec!["documents".into()];
    c.dock.behavior = DockBehavior::AutoHide;
    c.dock.recycle_bin = false;
    c.shortcuts.search = "Ctrl+Alt+Q".into();
    c.theme.wallpaper_accent = "#123456".into();
    c.theme.reduce_motion = true;
    c.cursor.enabled = false;
    c.island.top_margin = 22.0;
    c.general.language = "en".into();
    c
}

#[test]
fn apply_touches_only_appearance() {
    let before = busy();
    for look in builtin::all() {
        let mut c = before.clone();
        apply(&look, &mut c);
        assert_eq!(c.dock.pinned, before.dock.pinned);
        assert_eq!(c.dock.stacks, before.dock.stacks);
        assert_eq!(c.dock.behavior, before.dock.behavior);
        assert_eq!(c.dock.recycle_bin, before.dock.recycle_bin);
        assert_eq!(c.shortcuts, before.shortcuts);
        assert_eq!(c.theme.wallpaper_accent, "#123456");
        assert!(c.theme.reduce_motion);
        assert!(!c.cursor.enabled);
        assert_eq!(c.island.top_margin, 22.0);
        assert_eq!(c.general, before.general);
        assert_eq!(c.clipboard, before.clipboard);
        assert!(matches(&look, &c), "{} uygulandıktan sonra denk gelmeli", look.name);
    }
}

#[test]
fn capture_apply_round_trip() {
    let mut a = busy();
    a.theme.palette = Palette::Emerald;
    a.theme.accent_source = AccentSource::Custom;
    a.theme.custom_accent = "#2A9D8F".into();
    a.theme.glass_density = 0.3;
    a.dock.icon_size = 44.0;
    a.dock.position = DockPosition::Left;
    a.island.style = IslandStyle::Card;
    a.cursor.style = CursorStyle::Ink;
    a.cursor.ink_color = "#2b4c9a".into();
    a.cursor.scale = 1.2;
    let look = capture(&a, "Benim");
    let mut b = Config::default();
    apply(&look, &mut b);
    assert_eq!(capture(&b, "Benim"), look);
    assert!(matches(&look, &b));
    assert_eq!(b.cursor.ink_color, "#2b4c9a");
}

#[test]
fn default_config_is_carlui_look() {
    let b = builtin::all();
    assert_eq!(current(&Config::default(), &b, &[]), Which::Builtin(0));
    // Saklı ama kullanılmayan özel renk eşleşmeyi bozmaz.
    let mut c = Config::default();
    c.theme.custom_accent = "#FF0000".into();
    assert_eq!(current(&c, &b, &[]), Which::Builtin(0));
}

#[test]
fn builtin_detected_and_tweak_is_custom() {
    let b = builtin::all();
    let mut c = busy();
    apply(&b[3], &mut c);
    assert_eq!(current(&c, &b, &[]), Which::Builtin(3));
    c.dock.icon_size += 4.0;
    assert_eq!(current(&c, &b, &[]), Which::Custom);
    // Kullanıcı görünümü olarak kaydedilince o seçili görünür.
    let mine = capture(&c, "Benim");
    assert_eq!(current(&c, &b, std::slice::from_ref(&mine)), Which::User(0));
}

#[test]
fn builtins_are_distinct_and_valid() {
    let b = builtin::all();
    assert_eq!(b.len(), builtin::IDS.len());
    for (i, x) in b.iter().enumerate() {
        assert_eq!(&sanitize::sanitized(x), x, "{} aralık dışında", x.name);
        let mut c = Config::default();
        apply(x, &mut c);
        for (j, y) in b.iter().enumerate() {
            assert_eq!(matches(y, &c), i == j, "{} / {}", x.name, y.name);
        }
    }
    assert_eq!(builtin::by_id("taskbar").unwrap().dock.taskbar, true);
    assert!(builtin::by_id("yok").is_none());
}

#[test]
fn export_import_round_trip() {
    let mut c = busy();
    apply(&builtin::all()[2], &mut c);
    c.theme.accent_source = AccentSource::Custom;
    c.theme.custom_accent = "#4F8BFF".into();
    let look = capture(&c, "Paylaşılan görünüm");
    let text = file::to_text(&look);
    assert!(text.contains("format = \"carllook\""));
    assert!(!text.contains("pinned") && !text.contains("not.exe"));
    let back = file::from_text(&text).unwrap();
    assert_eq!(back, look);
}

#[test]
fn import_rejects_bad_input() {
    use file::{from_text, ImportError};
    assert_eq!(from_text(&"#".repeat(file::MAX_BYTES + 1)), Err(ImportError::TooLarge));
    assert_eq!(from_text("bu toml değil = = ="), Err(ImportError::Corrupt));
    assert_eq!(from_text("[general]\nlanguage = \"tr\"\n"), Err(ImportError::NotALook));
    let good = file::to_text(&builtin::all()[1]);
    assert_eq!(from_text(&good.replace("version = 1", "version = 9")), Err(ImportError::Newer(9)));
    assert_eq!(from_text(&good.replace("version = 1", "version = 0")), Err(ImportError::Corrupt));
    // Yanlış tür ve tanınmayan seçenek reddedilir.
    assert_eq!(from_text(&good.replace("icon_size = 28.0", "icon_size = \"büyük\"")), Err(ImportError::Corrupt));
    assert_eq!(from_text(&good.replace("style = \"ink\"", "style = \"rm -rf\"")), Err(ImportError::Corrupt));
    // Eksik bölüm reddedilir.
    let no_dock: String = good.split("[dock]").next().unwrap().to_string() + &good[good.find("[island]").unwrap()..];
    assert_eq!(from_text(&no_dock), Err(ImportError::Corrupt));
    // Tanınmayan alanlar yok sayılır.
    assert!(from_text(&good.replace("[dock]", "[dock]\nrun = \"cmd.exe\"\npinned = []")).is_ok());
}

#[test]
fn import_clamps_values() {
    let text = r##"
format = "carllook"
version = 1
name = "  Çok\u0007 uzun   ad  <script>  ${"x".repeat(80)}  "
[theme]
mode = "light"
palette = "emerald"
accent_source = "custom"
custom_accent = "kırmızı"
glass_density = 7.5
corner_radius = -3.0
animation_speed = nan
[dock]
position = "left"
taskbar = true
icon_size = 900.0
bottom_margin = -40.0
menu_accent = "C:\\Windows\\cmd.exe"
[island]
style = "notch"
[cursor]
style = "zen"
color = "not-a-color"
edge = "#ABC"
scale = 99.0
"##
    .replace("${\"x\".repeat(80)}", &"x".repeat(80));
    let l = file::from_text(&text).unwrap();
    assert!(l.name.chars().count() <= sanitize::NAME_MAX);
    assert!(!l.name.contains('<') && !l.name.chars().any(char::is_control));
    assert!(l.name.starts_with("Çok uzun ad"));
    assert_eq!(l.theme.glass_density, 1.0);
    assert_eq!(l.theme.corner_radius, 0.5);
    assert_eq!(l.theme.animation_speed, 1.0);
    // Okunamayan özel renk palete düşer.
    assert_eq!(l.theme.accent_source, AccentSource::Palette);
    assert_eq!(l.theme.custom_accent, "");
    assert_eq!(l.dock.icon_size, 64.0);
    assert_eq!(l.dock.bottom_margin, 0.0);
    assert_eq!(l.dock.position, DockPosition::Bottom);
    assert_eq!(l.dock.menu_accent, "system");
    assert_eq!(l.cursor.color, "#bcd6ff");
    assert_eq!(l.cursor.edge, "#aabbcc");
    assert_eq!(l.cursor.scale, 1.6);
}

#[test]
fn undo_restores_previous_appearance() {
    let mut c = busy();
    c.theme.accent_source = AccentSource::Custom;
    c.theme.custom_accent = "#4F8BFF".into();
    c.cursor.color = "#36e27a".into();
    let before = c.clone();
    let snap = apply_undoable(&builtin::all()[1], &mut c);
    assert_ne!(c, before);
    // Arada görünüm dışı bir ayar değişirse geri almada korunur.
    c.theme.reduce_motion = false;
    snap.restore(&mut c);
    let mut expect = before.clone();
    expect.theme.reduce_motion = false;
    assert_eq!(c, expect);
}

#[test]
fn store_names() {
    let mut s = store::Store::default();
    assert!(s.put(capture(&Config::default(), "İş")));
    assert!(s.put(capture(&Config::default(), " oyun ")));
    assert!(!s.put(capture(&Config::default(), "   ")));
    assert_eq!(s.free_name("oyun"), "oyun (2)");
    assert_eq!(s.free_name("yeni"), "yeni");
    assert!(!s.rename(0, "OYUN"));
    assert!(s.rename(0, "Çalışma"));
    assert_eq!(s.looks[0].name, "Çalışma");
    assert!(s.remove(0).is_some());
    assert_eq!(s.looks.len(), 1);
    let back: store::Store = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}
