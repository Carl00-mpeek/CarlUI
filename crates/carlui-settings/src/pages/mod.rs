//! Ayar sayfaları. Her sayfa `paint(app, painter, flow)` ile satırlarını dizer;
//! değişiklik `Act::Edit` kapanışıyla `Config`'e uygulanır ve hemen kaydedilir.

pub mod ai;
pub mod backup;
mod applist;
mod clipboard;
mod contacts;
mod cursor;
mod dock;
pub mod extension;
mod game;
mod form;
mod general;
mod island;
mod look;
mod search;
pub mod sensors;
pub mod shortcuts;
mod tools;
mod vault;

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
    Vault,
    Tools,
    Ai,
}

impl Page {
    pub const ALL: [Page; 11] = [Page::General, Page::Look, Page::Dock, Page::Island, Page::Search, Page::Clipboard, Page::Tools, Page::Ai, Page::Game, Page::Vault, Page::Shortcuts];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => carlui_core::t!("Genel", "General"),
            Page::Look => carlui_core::t!("Görünüm", "Appearance"),
            Page::Dock => "Dock",
            Page::Island => carlui_core::t!("Ada ve bildirimler", "Island and notifications"),
            Page::Search => carlui_core::t!("Arama", "Search"),
            Page::Clipboard => carlui_core::t!("Pano", "Clipboard"),
            Page::Game => carlui_core::t!("Oyun modu", "Game mode"),
            Page::Shortcuts => carlui_core::t!("Kısayollar", "Shortcuts"),
            Page::Vault => "Carl Vault",
            Page::Tools => carlui_core::t!("Araçlar", "Tools"),
            Page::Ai => carlui_core::t!("Yapay zekâ", "AI"),
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
            Page::Vault => '\u{E72E}',
            Page::Tools => '\u{E90F}',
            Page::Ai => '\u{E945}',
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
            "vault" | "kasa" => Page::Vault,
            "tools" | "araclar" | "araçlar" => Page::Tools,
            "replay" | "kayit" | "kayıt" | "tekrar" => Page::Tools,
            "ai" | "yapay-zeka" | "yz" => Page::Ai,
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
        Page::Vault => vault::paint(app, p, f),
        Page::Tools => tools::paint(app, p, f),
        Page::Ai => ai::paint(app, p, f),
    }
    f.flush(p);
}
