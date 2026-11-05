// cargo run -p carlui-ai --release --example voice -- <model kimliği> [dil] <ses.wav>...
// Model yoksa indirir; her dosyayı 50 ms'lik parçalarla (öncesinde ve
// sonrasında 1 sn sessizlik) dinleyiciye verir, tanınan metni ve komutu yazar.
use std::sync::atomic::AtomicBool;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let never = AtomicBool::new(false);
    let m = carlui_ai::catalog::get(&a[0]).expect("model kimliği");
    if !carlui_ai::store::installed(m) {
        carlui_ai::store::install(m, &never, &|_| {}).unwrap();
    }
    if m.id.contains("whisper") {
        carlui_ai::runtime::install(&never, &|_| {}).unwrap();
    }
    let (lang, files) = if a[1].ends_with(".wav") { ("", &a[1..]) } else { (a[1].as_str(), &a[2..]) };
    let t = std::time::Instant::now();
    let mut e = carlui_ai::listen::Engine::open(m, lang).unwrap();
    println!("açıldı {:?}", t.elapsed());
    for f in files {
        let mut pcm = vec![0.0f32; 16000];
        pcm.extend(carlui_ai::audio::read_wav(&std::fs::read(f).unwrap()).unwrap());
        pcm.extend(std::iter::repeat(0.0).take(16000));
        let t = std::time::Instant::now();
        let mut got = Vec::new();
        for c in pcm.chunks(800) {
            if let Some(s) = e.feed(c, &never) {
                got.push(s);
            }
        }
        for s in got {
            println!("{f}: {s:?} → {:?} ({:?})", carlui_ai::command::parse(&s), t.elapsed());
        }
    }
}
