//! Hayvan üretecinin sınamaları (modelsiz): yapay bir "kesilmiş fotoğraf"
//! üzerinden piksel sanatı, şeritler, ad güvenliği ve paketin dock'un
//! kurallarıyla (`carlui-dock/src/live/pack.rs`) okunabilmesi.

use super::sprite::{rgb, Sprite, CLEAR};
use super::*;

/// Yan duran turuncu bir "kedi" taslağı: gövde, baş, kulaklar, dört bacak,
/// kuyruk; yumuşak kenarlı (fotoğraftan kesilmiş gibi) ve gölgeli.
pub fn critter(w: u32, h: u32) -> Image {
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    let (fw, fh) = (w as f32, h as f32);
    let inside = |x: f32, y: f32| -> f32 {
        let (u, v) = (x / fw, y / fh);
        let ell = |cx: f32, cy: f32, rx: f32, ry: f32| 1.0 - (((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2)).sqrt();
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| ((u - x0).min(x1 - u).min(v - y0).min(y1 - v)) * 8.0;
        let parts = [
            ell(0.45, 0.55, 0.30, 0.20),
            ell(0.78, 0.35, 0.14, 0.17),
            rect(0.68, 0.12, 0.74, 0.25),
            rect(0.82, 0.12, 0.88, 0.25),
            rect(0.22, 0.65, 0.28, 0.95),
            rect(0.32, 0.65, 0.38, 0.95),
            rect(0.55, 0.65, 0.61, 0.95),
            rect(0.65, 0.65, 0.71, 0.95),
            rect(0.06, 0.30, 0.18, 0.40),
        ];
        parts.iter().fold(f32::MIN, |a, &b| a.max(b))
    };
    for y in 0..h {
        for x in 0..w {
            let d = inside(x as f32, y as f32);
            let a = (d * 40.0 + 0.5).clamp(0.0, 1.0);
            let shade = 1.0 - 0.35 * (y as f32 / fh);
            let belly = y as f32 / fh > 0.6 && (x as f32 / fw - 0.45).abs() < 0.2;
            let eye = ((x as f32 / fw - 0.84).powi(2) + (y as f32 / fh - 0.33).powi(2)).sqrt() < 0.02;
            let (r, g, b) = if eye {
                (20.0, 20.0, 20.0)
            } else if belly {
                (240.0 * shade, 230.0 * shade, 220.0 * shade)
            } else {
                (230.0 * shade, 130.0 * shade, 40.0 * shade)
            };
            let i = ((y * w + x) * 4) as usize;
            bgra[i..i + 4].copy_from_slice(&[b as u8, g as u8, r as u8, (a * 255.0) as u8]);
        }
    }
    Image { w, h, bgra }
}

/// Testin geçici klasörü (sonunda silinir).
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("carlui-petgen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn renk_indirgeme() {
    let mut s = Sprite::new(20, 10);
    for (i, p) in s.px.iter_mut().enumerate() {
        *p = rgb((i * 7) as u8, (i * 13) as u8, (i * 3) as u8);
    }
    s.px[0] = CLEAR;
    palette::quantise(&mut s, 13);
    let c = palette::colours(&s);
    assert!(c.len() <= 13 && c.len() >= 8, "{} renk", c.len());
    assert_eq!(s.px[0], CLEAR, "saydam piksel boyanmamalı");
    // Az renkli sprite olduğu gibi kalır.
    let mut t = Sprite::new(3, 1);
    t.px = vec![rgb(255, 0, 0), rgb(0, 255, 0), rgb(0, 0, 255)];
    let before = t.clone();
    palette::quantise(&mut t, 13);
    assert_eq!(t, before);
}

#[test]
fn kontur() {
    let mut s = Sprite::new(1, 1);
    s.px[0] = rgb(200, 100, 0);
    let o = s.outline(OUTLINE);
    assert_eq!((o.w, o.h), (3, 3));
    // Artı biçimi: dört yan konturlu, köşeler boş, orta korunur.
    assert_eq!(o.get(1, 1), rgb(200, 100, 0));
    for (x, y) in [(0, 1), (2, 1), (1, 0), (1, 2)] {
        assert_eq!(o.get(x, y), OUTLINE);
    }
    for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
        assert_eq!(o.get(x, y), CLEAR);
    }
}

/// `#` opak, `.` saydam satırlardan sprite.
fn sketch(rows: &[&str]) -> Sprite {
    let mut s = Sprite::new(rows[0].len(), rows.len());
    for (y, r) in rows.iter().enumerate() {
        for (x, ch) in r.bytes().enumerate() {
            if ch == b'#' {
                s.set(x as i32, y as i32, rgb(200, 120, 40));
            }
        }
    }
    s
}

#[test]
fn dal_boslugu_koprulenir() {
    // Önündeki dal kesilince iki parçaya bölünmüş gövde: 2 piksellik çapraz yarık kapanır.
    let mut s = sketch(&["###..###", "###..###", "###..###"]);
    s.bridge_gaps();
    assert_eq!(s.opaque_count(), 24, "yarık dolmalı");
    // Aynı parçanın iki bacağı arasındaki boşluk korunur.
    let mut legs = sketch(&["#####", "#####", "#...#", "#...#"]);
    let before = legs.clone();
    legs.bridge_gaps();
    assert_eq!(legs, before, "bacak arası doldurulmamalı");
    // Tek piksellik kırıntı köprülenmez.
    let mut speck = sketch(&["###.#", "###..", "###.."]);
    speck.bridge_gaps();
    assert_eq!(speck.get(3, 0), CLEAR);
}

#[test]
fn uykuda_kopan_kirinti_atilir() {
    // Sol üstte kuyruk ucu gövdeye yalnızca 2. satırdan bağlı; uykuda o satır
    // silinince kopar ve atılmalı (gövde 2. sütundan başlar).
    let s = sketch(&[
        "##..................",
        ".#..................",
        ".##.................",
        "..##################",
        "..##################",
        "..##################",
        "..#.#..........#.#..",
        "..#.#..........#.#..",
    ]);
    for f in anim::sleep(&s) {
        assert_eq!(f.x, 2, "kopan kuyruk ucu kalmamalı");
    }
}

#[test]
fn kesimden_piksel_sanati() {
    let img = critter(600, 400);
    let s = sprite::from_cutout(&img, 28).unwrap();
    // Yumuşak kenarın eşik altı pikselleri kırpıldığından en fazla 28.
    assert!((22..=28).contains(&s.w.max(s.h)), "{}x{}", s.w, s.h);
    assert!(s.opaque_count() > 150);
    // Boş resim anlaşılır hatayla döner.
    let empty = Image { w: 50, h: 50, bgra: vec![0; 50 * 50 * 4] };
    assert!(sprite::from_cutout(&empty, 28).is_err());
    // Küçük resim büyütülmez.
    let small = sprite::from_cutout(&critter(20, 14), 28).unwrap();
    assert!(small.w <= 20 && small.h <= 14);
}

#[test]
fn seritler_sinirlarda() {
    let pet = make(&critter(900, 600), "Tekir", &Opts::default()).unwrap();
    assert!((8..=128).contains(&pet.fw) && (8..=128).contains(&pet.fh));
    let keys: Vec<&str> = pet.strips.iter().map(|s| s.key).collect();
    assert_eq!(keys, ["idle", "walk", "sleep", "hop"]);
    let mut all = Vec::new();
    for s in &pet.strips {
        assert!((1..=32).contains(&s.frames));
        assert!(s.frames * pet.fw <= 2048);
        assert_eq!(s.rgba.len() as u32, s.frames * pet.fw * pet.fh * 4);
        assert!(s.ms >= 42 && s.ms <= 10_000);
        for p in s.rgba.chunks_exact(4) {
            assert!(p[3] == 0 || p[3] == 255, "yumuşatma olmamalı");
            if p[3] == 255 {
                all.push(u32::from_be_bytes([0, p[0], p[1], p[2]]));
            }
        }
    }
    all.sort_unstable();
    all.dedup();
    assert!(all.len() <= 16, "{} renk", all.len());
    let n = |k: &str| pet.strip(k).unwrap().frames;
    assert_eq!((n("idle"), n("walk"), n("sleep"), n("hop")), (8, 8, 4, 6));
    let (fw, fh) = (pet.fw as usize, pet.fh as usize);
    let grounded = |k: &str, i: u32| pet.frame_alpha(k, i).unwrap()[(fh - 1) * fw..].iter().any(|&a| a);
    // Yerdeki kareler en alt satıra basar; zıplamanın havadaki kareleri basmaz.
    for k in ["idle", "walk", "sleep"] {
        for i in 0..n(k) {
            assert!(grounded(k, i), "{k} {i}");
        }
    }
    assert!(grounded("hop", 0) && !grounded("hop", 2));
    // Nefes gerçekten kıpırdar; uyku oturuştan alçaktır.
    assert_ne!(pet.frame_alpha("idle", 0), pet.frame_alpha("idle", 4));
    // (Uyku karelerinde "z" de var, o yüzden üst kenar yerine dolu piksel sayısı.)
    let count = |k: &str, i| pet.frame_alpha(k, i).unwrap().iter().filter(|&&a| a).count();
    assert!(count("sleep", 0) + 40 < count("idle", 0), "{} {}", count("sleep", 0), count("idle", 0));
    // Yürüme silüete delik açmaz.
    let base = holes(&pet.frame_alpha("idle", 0).unwrap(), fw, fh);
    for i in 0..n("walk") {
        assert!(holes(&pet.frame_alpha("walk", i).unwrap(), fw, fh) <= base, "walk {i}");
    }
}

/// Kenardan ulaşılamayan (silüetin içine kapalı) saydam piksel sayısı.
fn holes(m: &[bool], w: usize, h: usize) -> usize {
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = (0..w * h).filter(|&i| !m[i] && (i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1)).collect();
    while let Some(i) = stack.pop() {
        if seen[i] || m[i] {
            continue;
        }
        seen[i] = true;
        let (x, y) = (i % w, i / w);
        if x > 0 { stack.push(i - 1) }
        if x + 1 < w { stack.push(i + 1) }
        if y > 0 { stack.push(i - w) }
        if y + 1 < h { stack.push(i + w) }
    }
    (0..w * h).filter(|&i| !m[i] && !seen[i]).count()
}

#[test]
fn sola_bakan_aynalanir() {
    let img = critter(600, 400);
    let r = make(&img, "a", &Opts::default()).unwrap();
    let l = make(&img, "a", &Opts { faces_left: true, ..Opts::default() }).unwrap();
    let (fw, fh) = (r.fw as usize, r.fh as usize);
    let a = r.frame_alpha("idle", 0).unwrap();
    let b = l.frame_alpha("idle", 0).unwrap();
    // Opak piksel sayısı aynı, biçim aynalı (kare dolgusu farklı olabilir).
    assert_eq!(a.iter().filter(|&&v| v).count(), b.iter().filter(|&&v| v).count());
    let rows = |m: &[bool]| (0..fh).map(|y| m[y * fw..(y + 1) * fw].iter().position(|&v| v)).collect::<Vec<_>>();
    assert_ne!(rows(&a), rows(&b));
}

#[test]
fn klasor_adlari_guvenli() {
    assert_eq!(pack::folder_name("Tekir Kedim"), "Tekir_Kedim");
    assert_eq!(pack::folder_name("Şirin Çiçek Ğ İı Öö Üü"), "Sirin_Cicek_G_Ii_Oo_Uu");
    assert_eq!(pack::folder_name("..\\..\\Windows"), "Windows");
    assert_eq!(pack::folder_name("a/b:c*?\"<>|"), "abc");
    assert_eq!(pack::folder_name("   "), "pet");
    assert_eq!(pack::folder_name("🐱"), "pet");
    assert_eq!(pack::folder_name("cat"), "cat_pet");
    assert_eq!(pack::folder_name("Snake"), "Snake_pet");
    assert_eq!(pack::folder_name("con"), "con_pet");
    assert_eq!(pack::folder_name("LPT1"), "LPT1_pet");
    let long = pack::folder_name(&"x".repeat(100));
    assert_eq!(long.len(), 32);
    for n in ["Tekir Kedim", "..", "cat", "con", "ğ", &"y".repeat(80)] {
        assert!(carlui_core::live::valid_pack_name(&pack::folder_name(n)), "{n}");
    }
    assert_eq!(pack::display_name("  Tekir\u{7}  ", "x"), "Tekir");
    assert_eq!(pack::display_name(" ", "x"), "x");
}

#[test]
fn paket_yazilir_ve_dock_kurallarina_uyar() {
    let tmp = TempDir::new("write");
    let pet = make(&critter(800, 500), "Pamuk Tekir", &Opts::default()).unwrap();
    let dir = write(&pet, &tmp.0, false).unwrap();
    assert_eq!(dir, tmp.0.join("Pamuk_Tekir"));
    let json = std::fs::read(dir.join("pet.json")).unwrap();
    let m = dock_rules::parse(&json, |f| std::fs::metadata(dir.join(f)).ok().map(|m| m.len())).unwrap();
    assert_eq!(m.name, "Pamuk Tekir");
    assert_eq!((m.fw, m.fh, m.baseline), (pet.fw, pet.fh, pet.fh));
    // PNG'ler bildirilen boyutta çözülür.
    for s in &pet.strips {
        let img = wic::load(&dir.join(s.file), 4096).unwrap();
        assert_eq!((img.w, img.h), (s.frames * pet.fw, pet.fh), "{}", s.file);
    }
    // Üzerine yazma istenmeden ikinci kez yazılmaz; istenince yazılır.
    assert!(write(&pet, &tmp.0, false).unwrap_err().contains("Pamuk_Tekir"));
    assert!(write(&pet, &tmp.0, true).is_ok());
    // Geçici klasör kalmaz; geçersiz klasör adı reddedilir.
    let names: Vec<String> = std::fs::read_dir(&tmp.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Pamuk_Tekir"]);
    assert!(pack::write(&pet, &tmp.0, "../kacis", false).is_err());
    assert!(pack::write(&pet, &tmp.0, "cat", false).is_err());
    assert!(pack::write(&pet, &tmp.0, "con", false).is_err());
}

#[test]
fn dock_kurallari_hatayi_yakalar() {
    // Kopya kuralların kendisi de sınansın: bozuk paketler geçmemeli.
    let pet = make(&critter(300, 200), "x", &Opts::default()).unwrap();
    let good = pack::manifest_json(&pet);
    let ok = |j: &str| dock_rules::parse(j.as_bytes(), |_| Some(1000)).is_ok();
    assert!(ok(&good));
    assert!(!ok(&good.replace("\"idle\"", "\"dans\"")));
    assert!(!ok(&good.replace("\"idle.png\"", "\"../idle.png\"")));
    assert!(!ok(&good.replace("\"scale\": 1", "\"scale\": 1, \"hack\": 1")));
    assert!(!ok(&good.replace("\"ms\": 100", "\"ms\": 5")));
    assert!(dock_rules::parse(good.as_bytes(), |_| Some(600 * 1024)).is_err());
}

/// Dock'un `live/pack.rs` doğrulamasının kopyası (dock bir uygulama
/// crate'i; bağımlılık eklemeden aynı kuralları burada sınarız). Dock'ta
/// kural değişirse burası da güncellenmeli.
mod dock_rules {
    use serde_json::Value;

    pub struct Manifest {
        pub name: String,
        pub fw: u32,
        pub fh: u32,
        pub baseline: u32,
    }

    const TOP: [&str; 9] = ["name", "frame_width", "frame_height", "scale", "filter", "facing", "baseline", "walk_speed", "states"];
    const STATE: [&str; 5] = ["file", "frames", "fps", "ms", "reverse_out"];

    fn anim(k: &str) -> Option<&'static str> {
        Some(match k {
            "idle" | "sit" => "sit",
            "sit_up" | "look_up" | "look" => "look",
            "walk" => "walk",
            "sleep" => "sleep",
            "stretch" => "stretch",
            "alert" => "alert",
            "lick" => "lick",
            "bask" => "bask",
            "hop" => "hop",
            _ => return None,
        })
    }

    fn valid_file(name: &str) -> bool {
        let Some(stem) = name.strip_suffix(".png") else { return false };
        !stem.is_empty() && stem.len() <= 32 && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    }

    pub fn parse(json: &[u8], file_len: impl Fn(&str) -> Option<u64>) -> Result<Manifest, String> {
        if json.len() > 16 * 1024 {
            return Err("büyük".into());
        }
        let v: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        let o = v.as_object().ok_or("nesne değil")?;
        if let Some(k) = o.keys().find(|k| !TOP.contains(&k.as_str())) {
            return Err(format!("bilinmeyen alan {k}"));
        }
        let u = |k: &str| o.get(k).and_then(Value::as_u64).map(|n| n as u32).ok_or(format!("{k} yok"));
        let f = |k: &str, d: f64| o.get(k).map_or(Some(d), Value::as_f64).ok_or(format!("{k} sayı değil"));
        let name = o.get("name").and_then(Value::as_str).ok_or("ad yok")?.trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("ad".into());
        }
        let (fw, fh, baseline) = (u("frame_width")?, u("frame_height")?, u("baseline")?);
        if !(8..=128).contains(&fw) || !(8..=128).contains(&fh) || baseline == 0 || baseline > fh {
            return Err("boyut".into());
        }
        if !(0.25..=8.0).contains(&f("scale", 1.0)?) || !(1.0..=200.0).contains(&f("walk_speed", 18.0)?) {
            return Err("ölçek/hız".into());
        }
        if !matches!(o.get("filter").map(|v| v.as_str()), None | Some(Some("nearest" | "linear"))) || !matches!(o.get("facing").map(|v| v.as_str()), None | Some(Some("left" | "right"))) {
            return Err("filtre/yön".into());
        }
        let states = o.get("states").and_then(Value::as_object).ok_or("states yok")?;
        let (mut seen_anim, mut seen_file, mut total, mut pixels) = (Vec::new(), Vec::new(), 0u64, 0u64);
        for (k, s) in states {
            let a = anim(k).ok_or(format!("bilinmeyen durum {k}"))?;
            if seen_anim.contains(&a) {
                return Err(format!("{k} iki kez"));
            }
            seen_anim.push(a);
            let s = s.as_object().ok_or("durum nesne değil")?;
            if s.keys().any(|k| !STATE.contains(&k.as_str())) {
                return Err(format!("{k}: bilinmeyen alan"));
            }
            let frames = s.get("frames").and_then(Value::as_u64).ok_or("frames")? as u32;
            if !(1..=32).contains(&frames) || frames * fw > 2048 {
                return Err(format!("{k}: frames"));
            }
            match (s.get("fps").map(Value::as_f64), s.get("ms").map(Value::as_u64)) {
                (Some(Some(fps)), None) if (0.0..=24.0).contains(&fps) => {}
                (None, Some(Some(ms))) if ms == 0 || (42..=10_000).contains(&ms) => {}
                _ => return Err(format!("{k}: hız")),
            }
            let file = s.get("file").and_then(Value::as_str).ok_or("file")?;
            if !valid_file(file) {
                return Err(format!("{k}: dosya adı"));
            }
            let len = file_len(file).ok_or(format!("{file} yok"))?;
            if len > 512 * 1024 {
                return Err(format!("{file} büyük"));
            }
            if !seen_file.contains(&file) {
                seen_file.push(file);
                total += len;
            }
            pixels += frames as u64 * fw as u64 * fh as u64 * 4;
        }
        if !seen_anim.contains(&"sit") || total > 2 * 1024 * 1024 || pixels > 3 * 1024 * 1024 {
            return Err("sit/boyut".into());
        }
        Ok(Manifest { name, fw, fh, baseline })
    }
}
