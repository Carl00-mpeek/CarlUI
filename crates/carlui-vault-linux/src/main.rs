//! Carl Vault, Linux sürümü: kasa penceresi ve tarayıcı eklentisinin
//! konuştuğu yerel köprü tek bir süreçte çalışır. Kasa biçimi Windows'takiyle
//! aynıdır; `~/.local/share/CarlUI/vault` klasörü iki sistem arasında taşınabilir.
//!
//! `--minimized`: oturum açılışında simge durumunda başlar (KDE otomatik başlatma).

mod bridge;
mod clipboard;
mod core;
mod pairing;
mod peer;
mod reply;
mod settings;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let settings = settings::Settings::load();
    carlui_vault::lang::apply(&settings.lang);
    let shared = core::Shared::new(carlui_vault::vault::default_dir(), settings);

    if let Err(e) = bridge::start(shared.clone()) {
        eprintln!("köprü başlatılamadı (port {} dolu olabilir): {e}", bridge::port());
    }
    {
        let s = shared.clone();
        let _ = std::thread::Builder::new().name("ticker".into()).spawn(move || s.ticker());
    }

    let minimized = std::env::args().any(|a| a == "--minimized");
    let viewport = egui::ViewportBuilder::default()
        .with_title("Carl Vault")
        .with_app_id("carl-vault")
        .with_inner_size([960.0, 640.0])
        .with_min_inner_size([640.0, 420.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Carl Vault", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, shared, minimized)))))
}
