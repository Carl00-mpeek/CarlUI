// Fotoğraftan dock hayvanı üretir ve göz kontrolü için büyütülmüş önizleme yazar.
//
// cargo run -p carlui-ai --example petgen -- <foto> <çıkış klasörü> [ad] [--left] [--size N] [--crop W] [--smooth K]
//
// Fotoğrafta saydamlık varsa (zaten kesilmişse) model kullanılmaz; yoksa
// kurulu BiRefNet ile arka plan silinir. `--smooth K`: saydam resmi önce K
// kat yumuşak büyütür (küçük bir sprite'ı "fotoğraf" gibi denemek için).
use carlui_ai::{petgen, Image};
use std::{path::PathBuf, sync::atomic::AtomicBool};

/// İki doğrusal büyütme (yalnızca deneme için).
fn smooth(img: &Image, k: u32) -> Image {
    let (w, h) = (img.w * k, img.h * k);
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    let at = |x: i64, y: i64, c: usize| img.bgra[((y.clamp(0, img.h as i64 - 1) * img.w as i64 + x.clamp(0, img.w as i64 - 1)) * 4) as usize + c] as f32;
    for y in 0..h {
        for x in 0..w {
            let fx = (x as f32 + 0.5) / k as f32 - 0.5;
            let fy = (y as f32 + 0.5) / k as f32 - 0.5;
            let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            for c in 0..4 {
                let a = at(x0, y0, c) * (1.0 - tx) + at(x0 + 1, y0, c) * tx;
                let b = at(x0, y0 + 1, c) * (1.0 - tx) + at(x0 + 1, y0 + 1, c) * tx;
                bgra[((y * w + x) * 4) as usize + c] = (a * (1.0 - ty) + b * ty).round() as u8;
            }
        }
    }
    Image { w, h, bgra }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    // `--install <model>`: modeli uygulamanın kendi yoluyla (katalogdaki resmi
    // adres, boyut ve SHA-256 doğrulaması) kurar ve çıkar.
    if let Some(id) = a.iter().position(|x| x == "--install").and_then(|i| a.get(i + 1)) {
        let m = carlui_ai::catalog::get(id).expect("katalogda yok");
        let never = AtomicBool::new(false);
        let t = std::time::Instant::now();
        carlui_ai::store::install(m, &never, &|f| print!("\r{:3.0}%", f * 100.0)).unwrap();
        println!("\rkuruldu {:?}: {}", t.elapsed(), carlui_ai::store::dir(m).display());
        return;
    }
    // `--stop-after MS`: kesimi MS sonra durdurur; "Durdur"un ne kadar çabuk
    // tuttuğunu ölçer.
    if let Some(ms) = a.iter().position(|x| x == "--stop-after").and_then(|i| a.get(i + 1)).and_then(|v| v.parse::<u64>().ok()) {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let t = std::time::Instant::now();
        let r = petgen::cut_out(std::path::Path::new(&a[0]), &cancel, &|_| {});
        println!("durdurma: {:?} sonra döndü, sonuç: {:?}", t.elapsed(), r.map(|i| (i.w, i.h)));
        return;
    }
    let flag = |f: &str| a.iter().position(|x| x == f);
    let val = |f: &str| flag(f).and_then(|i| a.get(i + 1)).and_then(|v| v.parse::<u32>().ok());
    let pos: Vec<&String> = a.iter().enumerate().filter(|(i, x)| !x.starts_with("--") && !(*i > 0 && a[i - 1].starts_with("--s"))).map(|(_, x)| x).collect();
    let photo = PathBuf::from(pos[0]);
    let out = PathBuf::from(pos[1]);
    let name = pos.get(2).map_or("Deneme", |s| s.as_str());
    let opts = petgen::Opts { size: val("--size").unwrap_or(28), faces_left: flag("--left").is_some(), overwrite: true };
    std::fs::create_dir_all(&out).unwrap();
    let never = AtomicBool::new(false);
    let t = std::time::Instant::now();
    let mut img = petgen::wic::load(&photo, petgen::PHOTO_MAX_SIDE).unwrap();
    let cut = if img.bgra.chunks_exact(4).any(|p| p[3] < 250) {
        if let Some(cw) = val("--crop") {
            // Şeridin ilk karesi.
            let cw = cw.min(img.w);
            let rows = img.bgra.chunks_exact(img.w as usize * 4).flat_map(|r| r[..cw as usize * 4].to_vec()).collect();
            img = Image { w: cw, h: img.h, bgra: rows };
        }
        if let Some(k) = val("--smooth") {
            img = smooth(&img, k);
        }
        println!("saydam resim, model kullanılmadı ({}x{})", img.w, img.h);
        img
    } else {
        let c = petgen::cut_out(&photo, &never, &|f| print!("\r{:3.0}%", f * 100.0)).unwrap();
        println!("\rarka plan silindi {:?}", t.elapsed());
        c
    };
    let t = std::time::Instant::now();
    let pet = petgen::make(&cut, name, &opts).unwrap();
    println!("üretildi {:?}: kare {}x{}", t.elapsed(), pet.fw, pet.fh);
    let dir = petgen::write(&pet, &out, true).unwrap();
    println!("paket: {}", dir.display());
    let prev = out.join(format!("{}_preview.png", petgen::pack::folder_name(name)));
    petgen::wic::save_bgra(&prev, &petgen::preview(&pet, 8)).unwrap();
    println!("önizleme: {}", prev.display());
}
