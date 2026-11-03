//! Tanı aracı: tray ses bölümünün göreceği çıkış aygıtlarını ve uygulama
//! oturumlarını yazdırır (hiçbir şeyi değiştirmez).
#[path = "../src/audio.rs"]
#[allow(dead_code)]
mod audio;

mod host {
    pub fn file_description(exe: &str) -> Option<String> {
        std::path::Path::new(exe).file_stem().map(|s| s.to_string_lossy().to_string())
    }
}

fn main() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    for d in audio::devices() {
        println!("aygıt {} {}", if d.default { "*" } else { " " }, d.name);
    }
    for a in audio::apps(8) {
        println!("uygulama {:<24} ses {:.2} sessiz {} ({})", a.name, a.volume, a.muted, a.exe);
    }
}
