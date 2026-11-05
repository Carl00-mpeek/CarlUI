// cargo run -p carlui-ai --example upscale -- model.pth in.png out.png
#[path = "common/wic.rs"]
mod wic;
use std::sync::atomic::AtomicBool;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let never = AtomicBool::new(false);
    carlui_ai::runtime::install(&never, &|_| {}).unwrap();
    let t = std::time::Instant::now();
    let mut u = carlui_ai::esrgan::Upscaler::open(std::path::Path::new(&a[1])).unwrap();
    println!("açıldı {:?} ×{}", t.elapsed(), u.scale);
    let big = wic::load(&a[2]);
    // Ortadan 4 kat küçültülmüş 240x160 parça: düşük çözünürlüklü resim gibi.
    let (w, h) = (240usize, 160usize);
    let (cx, cy) = (big.w as usize / 2 - w * 2, big.h as usize / 2 - h * 2);
    let mut d = vec![0u8; w * h * 4];
    for y in 0..h { for x in 0..w { for c in 0..4 { let mut s = 0u32; for j in 0..4 { for i in 0..4 { s += big.bgra[((cy + y * 4 + j) * big.w as usize + cx + x * 4 + i) * 4 + c] as u32; } } d[(y * w + x) * 4 + c] = (s / 16) as u8; } } }
    let img = carlui_ai::Image { w: w as u32, h: h as u32, bgra: d };
    wic::save(&format!("{}.in.png", a[3]), &img);
    let t = std::time::Instant::now();
    let out = u.upscale(&img, &never, &|p| eprint!("\r{:.0}%", p * 100.0)).unwrap();
    println!("\n{}x{} → {}x{} {:?}", img.w, img.h, out.w, out.h, t.elapsed());
    wic::save(&a[3], &out);
}
