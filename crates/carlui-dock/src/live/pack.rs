//! Evcil hayvan paketi: `pet.json` + her durum için yatay PNG şeridi.
//! Yerleşik kediler de, kullanıcı paketleri de bu tek biçimden okunur.
//! Burada yalnızca doğrulama var (saf, sınanabilir); PNG çözme `load.rs`'de.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Paketteki animasyon durumları. Sıra `Anim as usize` ile dizi indisidir.
/// İlk altısı ilk sürümün durumlarıdır; eski paketler yalnızca onları kullanır.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anim {
    Sit,
    Walk,
    Sleep,
    Stretch,
    Look,
    Alert,
    /// Yalanma/temizlenme: oturuşta ara sıra bir kez oynar.
    Lick,
    /// Güneşlenme: uzun, sakin dinlenme (kertenkele).
    Bask,
    /// Zıplama: ara karelerde ileri sıçrar (örümcek).
    Hop,
}

pub const N_ANIMS: usize = 9;

pub const ANIMS: [Anim; N_ANIMS] = [Anim::Sit, Anim::Walk, Anim::Sleep, Anim::Stretch, Anim::Look, Anim::Alert, Anim::Lick, Anim::Bask, Anim::Hop];

impl Anim {
    pub fn key(self) -> &'static str {
        match self {
            Anim::Sit => "sit",
            Anim::Walk => "walk",
            Anim::Sleep => "sleep",
            Anim::Stretch => "stretch",
            Anim::Look => "look",
            Anim::Alert => "alert",
            Anim::Lick => "lick",
            Anim::Bask => "bask",
            Anim::Hop => "hop",
        }
    }

    /// Anahtar → durum. Sanatçının şerit adları da kabul edilir:
    /// `idle` = sit, `sit_up` / `look_up` = look.
    fn from_key(k: &str) -> Option<Self> {
        match k {
            "idle" => Some(Anim::Sit),
            "sit_up" | "look_up" => Some(Anim::Look),
            _ => ANIMS.into_iter().find(|a| a.key() == k),
        }
    }
}

// ---- sınırlar --------------------------------------------------------------

pub const MAX_MANIFEST: usize = 16 * 1024;
pub const MAX_PNG: u64 = 512 * 1024;
pub const MAX_PACK: u64 = 2 * 1024 * 1024;
pub const FRAME_MIN: u32 = 8;
pub const FRAME_MAX: u32 = 128;
pub const FRAMES_MAX: u32 = 32;
pub const FPS_MAX: f32 = 24.0;
/// Bir şeridin en geniş hali (piksel).
pub const STRIP_MAX: u32 = 2048;
/// Çözülmüş karelerin toplam belleği (bayt): bellek bütçesi birkaç MB.
pub const DECODED_MAX: u64 = 3 * 1024 * 1024;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    /// Piksel sanatı: tam sayı katına en yakın komşu ile büyütülür.
    #[default]
    Nearest,
    /// Boyalı çizim (ör. 2x hazırlanmış): yumuşak ölçekleme.
    Linear,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Facing {
    #[default]
    Right,
    Left,
}

/// Bir durumun şeridi. Hız `fps` ya da kare başına `ms` ile verilir (biri
/// zorunlu; eski paketler `fps` kullanır).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StateSpec {
    pub file: String,
    pub frames: u32,
    #[serde(default)]
    pub fps: Option<f32>,
    #[serde(default)]
    pub ms: Option<u32>,
    /// Son karede kal ve çıkarken şeridi geriye oynat (ör. kedinin doğrulup
    /// geri oturması).
    #[serde(default)]
    pub reverse_out: bool,
}

impl StateSpec {
    /// Saniyedeki kare (doğrulanmış pakette her zaman 0..=FPS_MAX).
    pub fn rate(&self) -> f32 {
        match (self.fps, self.ms) {
            (Some(f), _) => f,
            (None, Some(ms)) if ms > 0 => 1000.0 / ms as f32,
            _ => 0.0,
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    name: String,
    frame_width: u32,
    frame_height: u32,
    #[serde(default = "one")]
    scale: f32,
    #[serde(default)]
    filter: Filter,
    #[serde(default)]
    facing: Facing,
    baseline: u32,
    #[serde(default = "walk_speed")]
    walk_speed: f32,
    states: BTreeMap<String, StateSpec>,
}

fn one() -> f32 {
    1.0
}

fn walk_speed() -> f32 {
    18.0
}

/// Doğrulanmış paket bilgisi.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub fw: u32,
    pub fh: u32,
    /// %100 DPI'da bir sanat pikseli kaç ekran pikseli.
    pub scale: f32,
    pub filter: Filter,
    pub facing: Facing,
    /// Patilerin bastığı satır (üstten, sanat pikseli): dock'un üst kenarına oturur.
    pub baseline: u32,
    /// Yürüme hızı (sanat pikseli / saniye).
    pub walk_speed: f32,
    /// `ANIMS` sırasıyla; olmayan durum `sit`'e düşer.
    pub states: [Option<StateSpec>; N_ANIMS],
}

impl Manifest {
    /// Durumun kendi şeridi yoksa oturma şeridi.
    pub fn spec(&self, a: Anim) -> &StateSpec {
        self.states[a as usize].as_ref().or(self.states[Anim::Sit as usize].as_ref()).expect("sit doğrulamada zorunlu")
    }

    /// Paketin gerçekten kullandığı durumlar (sit dahil).
    pub fn present(&self) -> impl Iterator<Item = (Anim, &StateSpec)> {
        ANIMS.into_iter().filter_map(|a| self.states[a as usize].as_ref().map(|s| (a, s)))
    }
}

/// Dosya adı yalnızca `[a-z0-9_-]+.png` olabilir (klasör dışına çıkılamasın).
fn valid_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else { return false };
    !stem.is_empty() && stem.len() <= 32 && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// `pet.json`'u okur ve doğrular. `file_len`: paket klasöründeki dosyanın
/// bayt boyu (yoksa `None`).
pub fn parse(json: &[u8], file_len: impl Fn(&str) -> Option<u64>) -> Result<Manifest, String> {
    if json.len() > MAX_MANIFEST {
        return Err("pet.json çok büyük".into());
    }
    let text = std::str::from_utf8(json).map_err(|_| "pet.json UTF-8 değil".to_string())?;
    let raw: RawManifest = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("pet.json bozuk: {e}"))?;
    let name = raw.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err("ad 1-64 karakter olmalı".into());
    }
    for (what, v) in [("frame_width", raw.frame_width), ("frame_height", raw.frame_height)] {
        if !(FRAME_MIN..=FRAME_MAX).contains(&v) {
            return Err(format!("{what} {FRAME_MIN}-{FRAME_MAX} arası olmalı"));
        }
    }
    if !raw.scale.is_finite() || !(0.25..=8.0).contains(&raw.scale) {
        return Err("scale 0.25-8 arası olmalı".into());
    }
    if raw.baseline == 0 || raw.baseline > raw.frame_height {
        return Err("baseline 1..frame_height arası olmalı".into());
    }
    if !raw.walk_speed.is_finite() || !(1.0..=200.0).contains(&raw.walk_speed) {
        return Err("walk_speed 1-200 arası olmalı".into());
    }
    let mut states: [Option<StateSpec>; N_ANIMS] = Default::default();
    let mut total = 0u64;
    let mut seen: Vec<&str> = Vec::new();
    let mut pixels = 0u64;
    for (k, s) in &raw.states {
        let a = Anim::from_key(k).ok_or_else(|| format!("bilinmeyen durum: {k}"))?;
        if states[a as usize].is_some() {
            // ör. hem "sit" hem "idle".
            return Err(format!("{k}: durum iki kez verilmiş"));
        }
        if !(1..=FRAMES_MAX).contains(&s.frames) {
            return Err(format!("{k}: frames 1-{FRAMES_MAX} arası olmalı"));
        }
        match (s.fps, s.ms) {
            (Some(_), Some(_)) => return Err(format!("{k}: fps ile ms birlikte verilmez")),
            (None, None) => return Err(format!("{k}: fps ya da ms gerekli")),
            (Some(f), None) if !f.is_finite() || !(0.0..=FPS_MAX).contains(&f) => return Err(format!("{k}: fps 0-{FPS_MAX} arası olmalı")),
            // 0: durağan; en hızlı 1000/FPS_MAX ms.
            (None, Some(ms)) if ms != 0 && (ms as f32) < 1000.0 / FPS_MAX || ms > 10_000 => {
                return Err(format!("{k}: ms 0 ya da {}-10000 arası olmalı", (1000.0 / FPS_MAX).ceil()))
            }
            _ => {}
        }
        if s.frames * raw.frame_width > STRIP_MAX {
            return Err(format!("{k}: şerit {STRIP_MAX} pikselden geniş"));
        }
        if !valid_file(&s.file) {
            return Err(format!("{k}: geçersiz dosya adı {}", s.file));
        }
        let len = file_len(&s.file).ok_or_else(|| format!("{k}: {} yok", s.file))?;
        if len > MAX_PNG {
            return Err(format!("{k}: {} çok büyük", s.file));
        }
        // Aynı dosyayı iki durum paylaşabilir; boyut bir kez sayılır.
        if !seen.contains(&s.file.as_str()) {
            seen.push(&s.file);
            total += len;
        }
        pixels += s.frames as u64 * raw.frame_width as u64 * raw.frame_height as u64 * 4;
        states[a as usize] = Some(s.clone());
    }
    if states[Anim::Sit as usize].is_none() {
        return Err("sit durumu zorunlu".into());
    }
    if total > MAX_PACK {
        return Err("paket 2 MB'den büyük".into());
    }
    if pixels > DECODED_MAX {
        return Err("kareler bellekte 3 MB'yi aşıyor".into());
    }
    Ok(Manifest {
        name: name.to_string(),
        fw: raw.frame_width,
        fh: raw.frame_height,
        scale: raw.scale,
        filter: raw.filter,
        facing: raw.facing,
        baseline: raw.baseline,
        walk_speed: raw.walk_speed,
        states,
    })
}

/// Çözülmüş şeridin boyutu bildirilenle aynı mı?
pub fn check_strip(m: &Manifest, s: &StateSpec, w: u32, h: u32) -> Result<(), String> {
    if w != s.frames * m.fw || h != m.fh {
        return Err(format!("{}: {w}x{h} bekleneni ({}x{}) tutmuyor", s.file, s.frames * m.fw, m.fh));
    }
    Ok(())
}

/// Ekrandaki büyütme: en yakın komşuda tam sayıya yuvarlanır (keskin pikseller),
/// yumuşakta olduğu gibi kalır. `dpi`: monitör ölçeği (1.0 = %100).
pub fn display_scale(m: &Manifest, dpi: f32) -> f32 {
    let k = m.scale * dpi.max(0.5);
    match m.filter {
        Filter::Nearest => k.round().max(1.0),
        Filter::Linear => k,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "name": "Test",
        "frame_width": 32, "frame_height": 24, "scale": 1, "baseline": 22,
        "states": {
            "sit": {"file": "sit.png", "frames": 4, "fps": 8},
            "walk": {"file": "walk.png", "frames": 6, "fps": 10}
        }
    }"#;

    fn files(name: &str) -> Option<u64> {
        matches!(name, "sit.png" | "walk.png").then_some(4000)
    }

    #[test]
    fn gecerli_paket() {
        let m = parse(GOOD.as_bytes(), files).unwrap();
        assert_eq!((m.fw, m.fh, m.baseline), (32, 24, 22));
        assert_eq!(m.filter, Filter::Nearest);
        assert_eq!(m.facing, Facing::Right);
        // Eksik durum sit'e düşer.
        assert_eq!(m.spec(Anim::Sleep).file, "sit.png");
        assert_eq!(m.spec(Anim::Walk).file, "walk.png");
        assert_eq!(m.present().count(), 2);
        assert!(check_strip(&m, m.spec(Anim::Sit), 128, 24).is_ok());
        assert!(check_strip(&m, m.spec(Anim::Sit), 127, 24).is_err());
        assert!(check_strip(&m, m.spec(Anim::Sit), 128, 25).is_err());
        assert_eq!(display_scale(&m, 1.25), 1.0);
        assert_eq!(display_scale(&m, 1.5), 2.0);
        assert_eq!(display_scale(&m, 2.0), 2.0);
    }

    fn bad(json: &str) -> String {
        parse(json.as_bytes(), files).unwrap_err()
    }

    #[test]
    fn hatali_paketler_reddedilir() {
        let g = |from: &str, to: &str| GOOD.replace(from, to);
        // Bozuk JSON, bilinmeyen alan, bilinmeyen durum.
        assert!(bad("{ bozuk").contains("bozuk"));
        bad(&g("\"scale\": 1", "\"scale\": 1, \"hack\": true"));
        assert!(bad(&g("\"walk\"", "\"dance\"")).contains("bilinmeyen"));
        // Aralık dışı değerler.
        bad(&g("\"frame_width\": 32", "\"frame_width\": 4"));
        bad(&g("\"frame_height\": 24", "\"frame_height\": 512"));
        bad(&g("\"frames\": 4", "\"frames\": 0"));
        bad(&g("\"frames\": 4", "\"frames\": 33"));
        bad(&g("\"fps\": 8", "\"fps\": 60"));
        bad(&g("\"fps\": 8", "\"fps\": -1"));
        bad(&g("\"scale\": 1", "\"scale\": 100"));
        bad(&g("\"baseline\": 22", "\"baseline\": 0"));
        bad(&g("\"baseline\": 22", "\"baseline\": 25"));
        bad(&g("\"name\": \"Test\"", "\"name\": \"  \""));
        // Klasör dışına çıkan ya da PNG olmayan dosya adları.
        bad(&g("\"sit.png\"", "\"../sit.png\""));
        bad(&g("\"sit.png\"", "\"C:/x/sit.png\""));
        bad(&g("\"sit.png\"", "\"sit.exe\""));
        bad(&g("\"sit.png\"", "\"Sit.png\""));
        // sit zorunlu; eksik dosya.
        assert!(bad(&g("\"sit\"", "\"look\"")).contains("sit"));
        assert!(bad(&g("\"walk.png\"", "\"yok.png\"")).contains("yok"));
        // Çok geniş şerit.
        bad(&g("\"frame_width\": 32", "\"frame_width\": 128").replace("\"frames\": 6", "\"frames\": 32"));
    }

    #[test]
    fn buyuk_dosyalar_reddedilir() {
        assert!(parse(GOOD.as_bytes(), |_| Some(MAX_PNG + 1)).unwrap_err().contains("büyük"));
        // Tek tek sınırın altında ama toplamda fazla: 6 durum × 500 KB > 2 MB.
        let six = r#"{"name":"x","frame_width":16,"frame_height":16,"baseline":16,"states":{
            "sit":{"file":"a.png","frames":1,"fps":0},"walk":{"file":"b.png","frames":1,"fps":0},
            "sleep":{"file":"c.png","frames":1,"fps":0},"stretch":{"file":"d.png","frames":1,"fps":0},
            "look":{"file":"e.png","frames":1,"fps":0}}}"#;
        assert!(parse(six.as_bytes(), |_| Some(500 * 1024)).unwrap_err().contains("2 MB"));
        let huge = format!("{{\"name\":\"{}\"}}", "a".repeat(MAX_MANIFEST));
        assert!(parse(huge.as_bytes(), files).unwrap_err().contains("büyük"));
        assert!(parse(&[0xFF, 0xFE, 0x00], files).is_err());
        // Dosyalar küçük ama çözülünce bellek bütçesini aşar: 4 × 32 kare × 64×128.
        let mem = r#"{"name":"x","frame_width":64,"frame_height":128,"baseline":128,"states":{
            "sit":{"file":"a.png","frames":32,"fps":0},"walk":{"file":"b.png","frames":32,"fps":0},
            "sleep":{"file":"c.png","frames":32,"fps":0},"stretch":{"file":"d.png","frames":32,"fps":0}}}"#;
        assert!(parse(mem.as_bytes(), |_| Some(1000)).unwrap_err().contains("bellek"));
    }

    /// İlk sürümün (6 durum, fps) paketi aynen okunmaya devam eder.
    #[test]
    fn eski_paket_bicimi_gecerli() {
        let old = r#"{
            "name": "Kedi (siyah, geçici çizim)", "frame_width": 32, "frame_height": 24, "scale": 1,
            "filter": "nearest", "facing": "right", "baseline": 24, "walk_speed": 18,
            "states": {
                "sit": { "file": "sit.png", "frames": 4, "fps": 8 },
                "walk": { "file": "walk.png", "frames": 6, "fps": 10 },
                "sleep": { "file": "sleep.png", "frames": 1, "fps": 0 },
                "stretch": { "file": "stretch.png", "frames": 4, "fps": 6 },
                "look": { "file": "look.png", "frames": 1, "fps": 0 },
                "alert": { "file": "alert.png", "frames": 2, "fps": 6 }
            }
        }"#;
        let m = parse(old.as_bytes(), |_| Some(1000)).unwrap();
        assert_eq!(m.present().count(), 6);
        assert_eq!(m.spec(Anim::Walk).rate(), 10.0);
        assert_eq!(m.spec(Anim::Sleep).rate(), 0.0);
        assert!(!m.spec(Anim::Look).reverse_out);
        // Yeni durumlar yoksa oturuşa düşer.
        assert_eq!(m.spec(Anim::Lick).file, "sit.png");
        assert_eq!(m.spec(Anim::Hop).file, "sit.png");
    }

    #[test]
    fn yeni_durumlar_ve_ms() {
        let new = r#"{"name":"x","frame_width":32,"frame_height":24,"baseline":24,"states":{
            "idle":{"file":"a.png","frames":8,"ms":150},
            "sit_up":{"file":"b.png","frames":4,"ms":110,"reverse_out":true},
            "lick":{"file":"c.png","frames":6,"ms":130},
            "bask":{"file":"d.png","frames":4,"ms":450},
            "hop":{"file":"e.png","frames":6,"ms":70}}}"#;
        let m = parse(new.as_bytes(), |_| Some(1000)).unwrap();
        assert_eq!(m.spec(Anim::Sit).file, "a.png");
        assert_eq!(m.spec(Anim::Look).file, "b.png");
        assert!(m.spec(Anim::Look).reverse_out);
        assert!((m.spec(Anim::Sit).rate() - 1000.0 / 150.0).abs() < 1e-4);
        assert_eq!(m.present().count(), 5);
        let p = |j: &str| parse(j.as_bytes(), |_| Some(1000));
        // İkisi birden ya da hiçbiri olmaz; aynı durum iki adla verilmez.
        assert!(p(&new.replace("\"ms\":150", "\"ms\":150,\"fps\":5")).is_err());
        assert!(p(&new.replace(",\"ms\":150", "")).is_err());
        assert!(p(&new.replace("\"ms\":150", "\"ms\":5")).is_err());
        assert!(p(&new.replace("\"ms\":150", "\"ms\":0")).is_ok());
        assert!(p(&new.replace("\"lick\"", "\"sit\"")).unwrap_err().contains("iki kez"));
        assert!(p(&new.replace("\"sit_up\"", "\"look_up\"")).is_ok());
    }
}
