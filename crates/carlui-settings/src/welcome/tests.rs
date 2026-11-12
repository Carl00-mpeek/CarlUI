//! Karşılama akışı testleri: adım makinesi, modül anahtarları ve atlamanın
//! ayara dokunmaması.

use super::model::*;
use carlui_core::config::{Config, DockPosition, IslandStyle, TaskbarSize};

#[test]
fn ileri_geri_sinirda_durur() {
    assert_eq!(navigate(Step::Look, Nav::Back), Outcome::Stay(Step::Look));
    assert_eq!(navigate(Step::Look, Nav::Next), Outcome::Stay(Step::Dock));
    assert_eq!(navigate(Step::Dock, Nav::Next), Outcome::Stay(Step::Island));
    assert_eq!(navigate(Step::Island, Nav::Next), Outcome::Stay(Step::Modules));
    assert_eq!(navigate(Step::Modules, Nav::Next), Outcome::Stay(Step::Finish));
    assert_eq!(navigate(Step::Finish, Nav::Back), Outcome::Stay(Step::Modules));
    assert_eq!(navigate(Step::Dock, Nav::Back), Outcome::Stay(Step::Look));
}

#[test]
fn atla_ve_bitir_kapatir() {
    for s in Step::ALL {
        assert_eq!(navigate(s, Nav::Skip), Outcome::Close);
        assert_eq!(navigate(s, Nav::Finish), Outcome::Close);
        assert_eq!(navigate(s, Nav::Go(Step::Island)), Outcome::Stay(Step::Island));
    }
    assert_eq!(navigate(Step::Finish, Nav::Next), Outcome::Close);
}

/// Baştan sona "İleri" ile gidilince her adım bir kez görülür.
#[test]
fn bastan_sona_yuruyus() {
    let mut s = Step::Look;
    let mut seen = vec![s];
    while let Outcome::Stay(n) = navigate(s, Nav::Next) {
        s = n;
        seen.push(s);
    }
    assert_eq!(seen, Step::ALL.to_vec());
    assert_eq!(Step::ALL.len(), Step::COUNT + 1);
}

/// Atla yalnızca karşılama işaretini değiştirir.
#[test]
fn atla_yalnizca_isareti_yazar() {
    let mut c = Config::fresh();
    c.dock.icon_size = 41.0;
    c.launcher.enabled = false;
    let before = c.clone();
    mark(&mut c);
    assert!(c.general.welcomed);
    let mut expect = before;
    expect.general.welcomed = true;
    assert_eq!(c, expect);
}

/// Her modül anahtarı yalnızca kendi bayrağını değiştirir.
#[test]
fn modul_anahtari_yalnizca_kendi_bayragini_degistirir() {
    let base = Config::default();
    for m in Module::ALL {
        let mut c = base.clone();
        let was = m.get(&c);
        m.toggle(&mut c);
        assert_eq!(m.get(&c), !was, "{m:?}");
        for other in Module::ALL.iter().filter(|o| **o != m) {
            assert_eq!(other.get(&c), other.get(&base), "{m:?} → {other:?}");
        }
        // Geri çevirince ayar başlangıçla birebir aynı: başka alan değişmemiş.
        m.toggle(&mut c);
        assert_eq!(c, base, "{m:?}");
    }
}

#[test]
fn gorev_cubugu_alta_alir_yan_konum_kapatir() {
    let mut c = Config::default();
    set_dock(&mut c, DockChoice::Position(DockPosition::Left));
    assert_eq!(c.dock.position, DockPosition::Left);
    assert!(!c.dock.taskbar);
    set_dock(&mut c, DockChoice::Taskbar(TaskbarSize::Compact));
    assert!(c.dock.taskbar);
    assert_eq!(c.dock.taskbar_size, TaskbarSize::Compact);
    assert_eq!(c.dock.position, DockPosition::Bottom);
    assert_eq!(dock_selection(&c), (0, 1));
    set_dock(&mut c, DockChoice::Position(DockPosition::Right));
    assert_eq!(dock_selection(&c), (2, 0));
    set_dock(&mut c, DockChoice::Taskbar(TaskbarSize::Full));
    set_dock(&mut c, DockChoice::Minimal);
    assert_eq!(dock_selection(&c), (0, 0));
}

/// Özetteki kısayollar ayardan okunur; kapalı modül ve boş kısayol atlanır.
#[test]
fn ozet_kisayollari_ayardan_gelir() {
    let mut c = Config::default();
    c.shortcuts.search = "Alt+Space".into();
    let t = tips(&c);
    assert_eq!(t.len(), 3);
    assert_eq!(t[0].1, "Alt+Space");
    assert_eq!(t[1].1, c.shortcuts.clipboard);
    c.launcher.enabled = false;
    c.shortcuts.screenshot = " ".into();
    let t = tips(&c);
    assert!(t.iter().all(|(_, k)| k != "Alt+Space" && !k.trim().is_empty()));
    assert!(t.iter().any(|(_, k)| *k == c.shortcuts.vault));
}

/// Dock ve ada seçimleri yalnızca kendi alanlarına dokunur.
#[test]
fn dock_ve_ada_baska_alana_dokunmaz() {
    let base = Config::default();
    let mut c = base.clone();
    set_dock(&mut c, DockChoice::Taskbar(TaskbarSize::Full));
    set_island(&mut c, IslandStyle::Notch);
    assert_eq!(c.island.style, IslandStyle::Notch);
    let mut back = c.clone();
    back.dock.taskbar = base.dock.taskbar;
    back.dock.taskbar_size = base.dock.taskbar_size;
    back.dock.position = base.dock.position;
    back.island.style = base.island.style;
    assert_eq!(back, base);
}
