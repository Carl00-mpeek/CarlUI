// cargo run -p carlui-ai --release --example tts -- <model klasörü> "<metin>" <çıktı.wav> [hız]
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    carlui_ai::runtime::install(&AtomicBool::new(false), &|_| {}).unwrap();
    let t = std::time::Instant::now();
    let mut v = carlui_ai::tts::Voice::open(std::path::Path::new(&a[1])).unwrap();
    println!("açıldı {:?} ({:?}, {} Hz)", t.elapsed(), v.lang, v.rate);
    let speed: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut pcm = Vec::new();
    for s in carlui_ai::tts::sentences(&a[2]) {
        println!("{s}\n  → {}", v.phonemes(&s));
        let t = std::time::Instant::now();
        let out = v.synth(&s, speed).unwrap();
        println!("  {:.1} sn ses, {:?}", out.len() as f32 / v.rate as f32, t.elapsed());
        pcm.extend(out);
        pcm.extend(std::iter::repeat(0.0).take(v.rate as usize / 5));
    }
    std::fs::write(&a[3], carlui_ai::tts::wav(&pcm, v.rate)).unwrap();
}
