//! Bluetooth pilleri ve mikrofon/kamera kullanan uygulamaları yazdırır (deneme).

fn main() {
    let t = std::time::Instant::now();
    println!("pil: {:?} ({} ms)", carlui_sys::bt::batteries(), t.elapsed().as_millis());
    let t = std::time::Instant::now();
    println!("mikrofon: {:?}", carlui_sys::privacy::users("microphone"));
    println!("kamera: {:?} ({} ms)", carlui_sys::privacy::users("webcam"), t.elapsed().as_millis());
}
