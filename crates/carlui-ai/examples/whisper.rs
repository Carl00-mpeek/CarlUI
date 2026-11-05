// cargo run -p carlui-ai --release --example whisper -- <model klasörü> <ses.wav> [dil] [--ts]
// WREP=n: sesi n kez (aralarda 1 sn sessizlik) uzatır; pencere kaydırma denemesi.
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let never = AtomicBool::new(false);
    carlui_ai::runtime::install(&never, &|_| {}).unwrap();
    let one = carlui_ai::audio::read_wav(&std::fs::read(&a[2]).unwrap()).unwrap();
    let rep: usize = std::env::var("WREP").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut pcm = Vec::new();
    for _ in 0..rep {
        pcm.extend_from_slice(&one);
        pcm.extend(std::iter::repeat(0.0).take(16000));
    }
    let lang = a.get(3).filter(|s| !s.starts_with("--")).map(|s| s.as_str());
    let ts = a.iter().any(|s| s == "--ts");
    let t = std::time::Instant::now();
    let mut w = carlui_ai::whisper::Whisper::open(std::path::Path::new(&a[1])).unwrap();
    println!("açıldı {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let (segs, l) = w.transcribe(&pcm, lang, ts, &never, &|_| {}).unwrap();
    println!("{:.1} sn ses, {:?}, dil {l}", pcm.len() as f32 / 16000.0, t.elapsed());
    if ts {
        print!("{}", carlui_ai::whisper::to_srt(&segs));
    } else {
        println!("{}", carlui_ai::whisper::to_text(&segs));
    }
}
