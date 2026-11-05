// cargo run -p carlui-ai --example rmbg -- model.onnx in.jpg out.png
#[path = "common/wic.rs"]
mod wic;
use std::sync::atomic::AtomicBool;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let never = AtomicBool::new(false);
    carlui_ai::runtime::install(&never, &|_| {}).unwrap();
    let t = std::time::Instant::now();
    let mut r = carlui_ai::rmbg::Remover::open(std::path::Path::new(&a[1])).unwrap();
    println!("açıldı {:?} {}x{}", t.elapsed(), r.w, r.h);
    let img = wic::load(&a[2]);
    let t = std::time::Instant::now();
    let out = r.remove(&img, &never, &|_| {}).unwrap();
    println!("{}x{} {:?}", img.w, img.h, t.elapsed());
    wic::save(&a[3], &out);
    // Göz kontrolü için pembe zemin üzerine.
    let mut v = out.bgra.clone();
    for p in v.chunks_exact_mut(4) { let a = p[3] as u32; for (c, bg) in [(0usize, 200u32), (1, 0), (2, 255)] { p[c] = ((p[c] as u32 * a + bg * (255 - a)) / 255) as u8; } p[3] = 255; }
    wic::save(&format!("{}.view.png", a[3]), &carlui_ai::Image { w: out.w, h: out.h, bgra: v });
}
