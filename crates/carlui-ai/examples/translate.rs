// cargo run -p carlui-ai --release --example translate -- <model klasörü> "metin"
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let never = AtomicBool::new(false);
    carlui_ai::runtime::install(&never, &|_| {}).unwrap();
    let t = std::time::Instant::now();
    let mut m = carlui_ai::marian::Marian::open(std::path::Path::new(&a[1])).unwrap();
    println!("açıldı {:?}", t.elapsed());
    for s in &a[2..] {
        let t = std::time::Instant::now();
        let out = m.translate(&s.replace("\n", "\n"), &never, &|_| {}).unwrap();
        println!("{:?}\n→ {out}\n", t.elapsed());
    }
}
