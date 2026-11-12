//! Evcil hayvanların yüklenmesi. Şerit paketleri (yerleşik kedi, köpek,
//! kertenkele, örümcek ve kullanıcı paketleri) AYNI yoldan geçer: `pet.json` doğrulanır,
//! PNG şeritleri ayrı bir iş parçacığında WIC ile çözülür, sonuç dock
//! penceresine bir mesajla bildirilir. Yılan ve yunus kendi çizim
//! parçalarıyla (adlı sayfalar) gelir. Yüzeyler (GPU) UI iş parçacığında bir
//! kez oluşturulur; sonra her karede yalnızca hazır fırça değiştirilir.

use super::pack::{self, Anim, Manifest};
use carlui_core::live as cfg;
use std::{path::PathBuf, sync::mpsc};
use windows::Win32::{
    Foundation::*,
    Graphics::Imaging::*,
    System::Com::*,
    UI::WindowsAndMessaging::PostMessageW,
};

/// Çözme bitti (wParam/lParam kullanılmaz; sonuç kanaldan alınır).
pub const WM_APP_LIVE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 30;

pub type Files = &'static [(&'static str, &'static [u8])];

/// `assets/<klasör>/<ad>` dosyalarını exe'ye gömer: `[("ad", bayt), ...]`.
macro_rules! embed {
    ($dir:literal: $($f:literal),+ $(,)?) => {
        &[$(($f, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $dir, "/", $f)) as &[u8])),+]
    };
}
pub(crate) use embed;

/// Yerleşik şerit paketleri: yalnızca `pet.json`'un andığı dosyalar.
fn strip_pack(name: &str) -> Option<Files> {
    Some(match name {
        "cat" => embed!("pets/cat": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "lick.png"),
        // Golden retriever: "lick" soluma, "hop" kısa koşu, "stretch" oyun daveti.
        "cat_black" => embed!("pets/cat_black": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "lick.png"),
        "cat_gray" => embed!("pets/cat_gray": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "lick.png"),
        "cat_white" => embed!("pets/cat_white": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "lick.png"),
        "dog" => embed!("pets/dog": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "dog_black" => embed!("pets/dog_black": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "dog_brown" => embed!("pets/dog_brown": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "dog_white" => embed!("pets/dog_white": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "mouse" => embed!("pets/mouse": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "squirrel" => embed!("pets/squirrel": "pet.json", "idle.png", "walk.png", "sleep.png", "stretch.png", "sit_up.png", "alert.png", "pant.png", "run.png"),
        "lizard" => embed!("pets/lizard": "pet.json", "idle.png", "walk.png", "bask.png", "look_up.png"),
        "spider" => embed!("pets/spider": "pet.json", "idle.png", "walk.png", "sleep.png", "hop.png"),
        _ => return None,
    })
}

/// Yılanın çizim parçaları (bkz. `snake/art.rs`).
pub const SNAKE_FILES: Files = embed!("pets/snake":
    "bead_fill.png", "bead_band.png", "bead_line.png",
    "tail_fill_1.png", "tail_fill_2.png", "tail_fill_3.png", "tail_fill_4.png", "tail_fill_5.png",
    "head_m30_line.png", "head_m30_fill.png", "head_p00_line.png", "head_p00_fill.png",
    "head_p30_line.png", "head_p30_fill.png", "head_p60_line.png", "head_p60_fill.png",
    "head_p90_line.png", "head_p90_fill.png",
    "coil.png", "coil_up.png", "coil_extra.png",
);

/// Yunusun şeritleri (bkz. `dolphin/view.rs`).
pub const DOLPHIN_FILES: Files = embed!("pets/dolphin": "swim.png", "leap.png", "splash_out.png", "splash_in.png");

/// Hangi hayvan yüklenecek.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Gömülü şerit paketi (cat / dog / lizard / spider).
    Builtin(&'static str),
    Snake,
    Dolphin,
    User(String),
}

impl Source {
    /// Ayardaki `pet` değerinden; geçersiz ad `None`.
    pub fn from_config(pet: &str) -> Option<Self> {
        let pet = pet.trim();
        match pet {
            "" => None,
            "snake" => Some(Self::Snake),
            "dolphin" => Some(Self::Dolphin),
            _ => {
                if let Some(&b) = cfg::BUILTIN_PETS.iter().find(|b| **b == pet) {
                    strip_pack(b).map(|_| Self::Builtin(b))
                } else if cfg::valid_pack_name(pet) {
                    Some(Self::User(pet.to_string()))
                } else {
                    None
                }
            }
        }
    }
}

/// Çözülmüş bir resim: PBGRA (önceden çarpılmış) pikseller.
pub struct Sheet {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// Çözülmüş bir paket şeridi.
pub struct Strip {
    pub anim: Anim,
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

pub struct Decoded {
    pub manifest: Manifest,
    pub strips: Vec<Strip>,
}

/// Adlı sayfalar (yılan/yunus): `Files` sırasıyla.
pub struct Sheets(pub Vec<(&'static str, Sheet)>);

impl Sheets {
    pub fn get(&self, name: &str) -> Option<&Sheet> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, s)| s)
    }
}

/// Yüklenen hayvan.
pub enum Loaded {
    Strip(Decoded),
    Snake(Sheets),
    Dolphin(Sheets),
}

/// Dosya kaynağı: gömülü tablo ya da paket klasörü.
enum Store {
    Embedded(Files),
    Dir(PathBuf),
}

impl Store {
    fn len(&self, name: &str) -> Option<u64> {
        match self {
            Store::Embedded(f) => f.iter().find(|(n, _)| *n == name).map(|(_, b)| b.len() as u64),
            Store::Dir(d) => {
                // Bağlantı/klasör değil, düz dosya olmalı.
                let m = std::fs::symlink_metadata(d.join(name)).ok()?;
                m.file_type().is_file().then_some(m.len())
            }
        }
    }

    fn read(&self, name: &str, max: u64) -> Result<Vec<u8>, String> {
        let len = self.len(name).ok_or_else(|| format!("{name} yok"))?;
        if len > max {
            return Err(format!("{name} çok büyük"));
        }
        match self {
            Store::Embedded(f) => Ok(f.iter().find(|(n, _)| *n == name).map(|(_, b)| b.to_vec()).unwrap_or_default()),
            Store::Dir(d) => {
                let b = std::fs::read(d.join(name)).map_err(|e| format!("{name} okunamadı: {e}"))?;
                // Okurken büyümüş olabilir.
                if b.len() as u64 > max {
                    return Err(format!("{name} çok büyük"));
                }
                Ok(b)
            }
        }
    }
}

fn wic() -> Result<IWICImagingFactory, String> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.map_err(|e| format!("WIC: {e}"))
}

/// Şerit paketini doğrulayıp çözer (iş parçacığında; COM başlatılmış olmalı).
fn decode_pack(store: Store) -> Result<Decoded, String> {
    let json = store.read("pet.json", pack::MAX_MANIFEST as u64)?;
    let manifest = pack::parse(&json, |f| store.len(f))?;
    let wic = wic()?;
    let mut strips = Vec::new();
    for (anim, spec) in manifest.present() {
        let bytes = store.read(&spec.file, pack::MAX_PNG)?;
        let (w, h, px) = unsafe { decode_png(&wic, &bytes, |w, h| pack::check_strip(&manifest, spec, w, h)) }?;
        strips.push(Strip { anim, w, h, px });
    }
    Ok(Decoded { manifest, strips })
}

/// Gömülü adlı sayfaları çözer (boyutlar her biri 512×64'ü aşmaz).
pub fn decode_sheets(wic: &IWICImagingFactory, files: Files) -> Result<Sheets, String> {
    let mut out = Vec::with_capacity(files.len());
    for (name, bytes) in files {
        let (w, h, px) = unsafe { decode_png(wic, bytes, |w, h| if w <= 512 && h <= 64 { Ok(()) } else { Err(format!("{name}: {w}x{h} çok büyük")) }) }?;
        out.push((*name, Sheet { w, h, px }));
    }
    Ok(Sheets(out))
}

fn decode(src: &Source) -> Result<Loaded, String> {
    match src {
        Source::Builtin(b) => decode_pack(Store::Embedded(strip_pack(b).ok_or("bilinmeyen hayvan")?)).map(Loaded::Strip),
        Source::User(name) => decode_pack(Store::Dir(cfg::pets_dir().join(name))).map(Loaded::Strip),
        Source::Snake => decode_sheets(&wic()?, SNAKE_FILES).map(Loaded::Snake),
        Source::Dolphin => decode_sheets(&wic()?, DOLPHIN_FILES).map(Loaded::Dolphin),
    }
}

/// PNG → PBGRA. Boyut `check` ile piksel ayrılmadan önce denetlenir.
pub unsafe fn decode_png(wic: &IWICImagingFactory, bytes: &[u8], check: impl Fn(u32, u32) -> Result<(), String>) -> Result<(u32, u32, Vec<u8>), String> {
    let e = |w: &'static str| move |err: windows::core::Error| format!("{w}: {err}");
    let stream = wic.CreateStream().map_err(e("akış"))?;
    stream.InitializeFromMemory(bytes).map_err(e("akış"))?;
    let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).map_err(e("PNG bozuk"))?;
    let frame = dec.GetFrame(0).map_err(e("PNG bozuk"))?;
    let conv = wic.CreateFormatConverter().map_err(e("dönüştürücü"))?;
    conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).map_err(e("dönüştürücü"))?;
    let (mut w, mut h) = (0, 0);
    conv.GetSize(&mut w, &mut h).map_err(e("boyut"))?;
    check(w, h)?;
    let mut px = vec![0u8; (w * h * 4) as usize];
    conv.CopyPixels(std::ptr::null(), w * 4, &mut px).map_err(e("PNG bozuk"))?;
    Ok((w, h, px))
}

/// Hayvanı arka planda çözer; bitince `hwnd`'ye `WM_APP_LIVE` gönderir.
pub fn spawn(src: Source, hwnd: HWND) -> mpsc::Receiver<Result<Loaded, String>> {
    let (tx, rx) = mpsc::channel();
    let raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let r = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let r = decode(&src);
            CoUninitialize();
            r
        };
        if tx.send(r).is_ok() {
            unsafe {
                let _ = PostMessageW(HWND(raw as _), WM_APP_LIVE, WPARAM(0), LPARAM(0));
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn com() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    /// Şeridin en alttaki opak satırı (taban çizgisi denetimi için).
    fn lowest_row(s: &Strip) -> u32 {
        (0..s.h).rev().find(|&y| (0..s.w).any(|x| s.px[((y * s.w + x) * 4 + 3) as usize] > 0)).map_or(0, |y| y + 1)
    }

    #[test]
    fn yerlesik_paketler_gecerli() {
        com();
        let mut total = 0usize;
        for name in ["cat", "cat_black", "cat_gray", "cat_white", "dog", "dog_black", "dog_brown", "dog_white", "mouse", "squirrel", "lizard", "spider"] {
            total += strip_pack(name).unwrap().iter().map(|(_, b)| b.len()).sum::<usize>();
            let Ok(Loaded::Strip(d)) = decode(&Source::Builtin(name)) else { panic!("{name} çözülemedi") };
            assert!(d.strips.len() >= 4, "{name}");
            assert!(d.strips.iter().all(|s| s.px.len() == (s.w * s.h * 4) as usize && s.px.iter().skip(3).step_by(4).any(|&a| a > 0)));
            // Ayaklar taban satırına basar: oturuş şeridi taban çizgisinin altına taşmaz.
            let sit = d.strips.iter().find(|s| s.anim == Anim::Sit).unwrap();
            assert!(lowest_row(sit) <= d.manifest.baseline, "{name}: {} > {}", lowest_row(sit), d.manifest.baseline);
            // Gömülü dosya listesi pet.json'un andıklarıyla aynı (fazlası depoya girmesin).
            for (f, _) in strip_pack(name).unwrap().iter().filter(|(f, _)| *f != "pet.json") {
                assert!(d.manifest.present().any(|(_, s)| s.file == *f), "{name}: {f} kullanılmıyor");
            }
        }
        for files in [SNAKE_FILES, DOLPHIN_FILES] {
            total += files.iter().map(|(_, b)| b.len()).sum::<usize>();
        }
        assert!(matches!(decode(&Source::Snake), Ok(Loaded::Snake(_))));
        assert!(matches!(decode(&Source::Dolphin), Ok(Loaded::Dolphin(_))));
        // Gömülü varlıklar 1 MB'nin epey altında.
        assert!(total < 256 * 1024, "gömülü varlıklar {total} bayt");
    }

    #[test]
    fn kaynak_secimi() {
        assert_eq!(Source::from_config(""), None);
        assert_eq!(Source::from_config(" cat "), Some(Source::Builtin("cat")));
        assert_eq!(Source::from_config("spider"), Some(Source::Builtin("spider")));
        assert_eq!(Source::from_config("dog"), Some(Source::Builtin("dog")));
        assert_eq!(Source::from_config("snake"), Some(Source::Snake));
        assert_eq!(Source::from_config("dolphin"), Some(Source::Dolphin));
        assert_eq!(Source::from_config("tilki"), Some(Source::User("tilki".into())));
        assert_eq!(Source::from_config("..\\evil"), None);
    }

    #[test]
    fn bozuk_png_reddedilir() {
        com();
        unsafe {
            let wic = wic().unwrap();
            assert!(decode_png(&wic, b"\x89PNG\r\n\x1a\nbozuk", |_, _| Ok(())).is_err());
            // Gerçek PNG ama boyutu bildirilenden farklı: piksel ayrılmadan reddedilir.
            let png = strip_pack("cat").unwrap().iter().find(|(n, _)| *n == "idle.png").unwrap().1;
            assert!(decode_png(&wic, png, |w, _| if w == 999 { Ok(()) } else { Err("boyut".into()) }).is_err());
            assert!(decode_png(&wic, png, |_, _| Ok(())).is_ok());
        }
    }
}
