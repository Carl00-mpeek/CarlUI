//! Bu modül "ekranın bir bölümünü videoya almak istiyorum" sorununu çözüyor:
//! ekran görüntüsü aracında alan seçilip kayıt düğmesine basılınca
//! `carlui-replay.exe --area x,y,g,y` olarak açılır, kayıt bitince kapanır.
//! Arka planda sürekli çalışan bir şey yoktur.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod audio;
mod clock;
mod record;
mod writer;

use carlui_core::bus::{BusClient, Event};

/// Adada kısa bir bildirim göster.
fn note(title: &str, body: &str) {
    let data = serde_json::json!({ "app": t!("Alan kaydı", "Area recording"), "title": title, "body": body });
    BusClient::send_once(&Event::Custom { name: "island-note".into(), data });
}

fn main() -> windows::core::Result<()> {
    let mut args = std::env::args().skip_while(|a| a != "--area");
    if args.next().is_some() {
        return record::run(&args.next().unwrap_or_default());
    }
    // Eski sürümlerin başlattığı anlık tekrar artık yok: hiçbir şey yapmadan çık.
    Ok(())
}
