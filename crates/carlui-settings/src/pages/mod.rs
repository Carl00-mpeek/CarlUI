//! Ayar sayfaları. Her sayfa `paint(app, painter, flow)` ile satırlarını dizer;
//! değişiklik `Act::Edit` kapanışıyla `Config`'e uygulanır ve hemen kaydedilir.

mod clipboard;
mod contacts;
mod cursor;
mod dock;
pub mod extension;
mod game;
mod general;
mod island;
mod look;
mod search;
pub mod shortcuts;

use crate::{ui::Flow, ui::P, App};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Page {
    General,
    Look,
    Dock,
    Island,
    Search,
    Clipboard,
    Game,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 8] = [Page::General, Page::Look, Page::Dock, Page::Island, Page::Search, Page::Clipboard, Page::Game, Page::Shortcuts];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "Genel",
            Page::Look => "Görünüm",
            Page::Dock => "Dock",
            Page::Island => "Ada ve bildirimler",
            Page::Search => "Arama",
            Page::Clipboard => "Pano",
            Page::Game => "Oyun modu",
            Page::Shortcuts => "Kısayollar",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Page::General => '\u{E713}',
            Page::Look => '\u{E790}',
            Page::Dock => '\u{E75B}',
            Page::Island => '\u{EA8F}',
            Page::Search => '\u{E721}',
            Page::Clipboard => '\u{E77F}',
            Page::Game => '\u{E7FC}',
            Page::Shortcuts => '\u{E765}',
        }
    }

    /// `--page` argümanındaki ad.
    pub fn from_arg(s: &str) -> Option<Page> {
        Some(match s.to_lowercase().as_str() {
            "general" | "genel" => Page::General,
            "look" | "theme" | "gorunum" | "görünüm" => Page::Look,
            "dock" => Page::Dock,
            "island" | "ada" => Page::Island,
            "search" | "arama" => Page::Search,
            "clipboard" | "pano" => Page::Clipboard,
            "game" | "oyun" => Page::Game,
            "shortcuts" | "kisayollar" | "kısayollar" => Page::Shortcuts,
            _ => return None,
        })
    }
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    match app.page {
        Page::General => general::paint(app, p, f),
        Page::Look => look::paint(app, p, f),
        Page::Dock => dock::paint(app, p, f),
        Page::Island => island::paint(app, p, f),
        Page::Search => search::paint(app, p, f),
        Page::Clipboard => clipboard::paint(app, p, f),
        Page::Game => game::paint(app, p, f),
        Page::Shortcuts => shortcuts::paint(app, p, f),
    }
}
