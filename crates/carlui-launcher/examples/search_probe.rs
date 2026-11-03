//! Tanı: uygulama listesi ve arama sonuçları. Kullanım: search_probe "sorgu" ["sorgu2" …]
use carlui_launcher::{currency::Rates, files::Files, index, search, usage::Usage};
use std::sync::Arc;

fn main() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let t = std::time::Instant::now();
    let apps = index::enumerate_apps();
    println!("{} uygulama, {} ms", apps.len(), t.elapsed().as_millis());
    for a in apps.iter().take(8) {
        println!("  {} | {} | {} | {}", a.name, a.parsing, a.target, a.aumid);
    }
    let run = index::running();
    println!("{} açık pencere", run.len());
    let mut usage = Usage::load();
    usage.import_legacy(&apps);
    let files = Files::default();
    files.ensure(|| {});
    std::thread::sleep(std::time::Duration::from_secs(4));
    let rates = Rates::default();
    let cfg = carlui_core::config::LauncherConfig::default();
    for q in std::env::args().skip(1) {
        let cx = search::Ctx { apps: &apps, run: &run, usage: &usage.counts, files: &files, rates: &rates, cfg: &cfg, wake: Arc::new(|| {}) };
        let mut r = search::build(&q, &cx);
        if r.iter().any(|i| i.act == search::Act::Pending) {
            std::thread::sleep(std::time::Duration::from_secs(3));
            r = search::build(&q, &cx);
        }
        println!("\n> {q}");
        for i in r.iter().take(12) {
            println!("  [{}] {} — {}", i.tag, i.title, i.sub);
        }
    }
    let (tiles, chips) = search::home(&apps, &run, &usage.counts);
    println!("\nana ekran: {:?}", tiles.iter().map(|t| &t.title).collect::<Vec<_>>());
    println!("çipler: {:?}", chips.iter().map(|t| &t.title).collect::<Vec<_>>());
}
