//! Bu modül "bir fotoğraftan dock'ta yaşayan kendi piksel hayvanımızı nasıl
//! yaparız?" sorusunu çözüyor. Her şey bilgisayarda olur:
//!
//! 1. Fotoğraf okunur (büyükse küçültülür, EXIF yönü uygulanır).
//! 2. BiRefNet arka planı siler (`rmbg`; Ayarlar → Yapay zekâ'dan kurulan model).
//! 3. Nesne kırpılır, ~28 piksellik ızgaraya alan ortalamasıyla indirilir,
//!    renkler ≤16'ya düşürülür, 1 piksellik koyu kontur eklenir (yumuşatma yok).
//! 4. Tek duruştan kural tabanlı şeritler çıkar: oturma (nefes), yürüme,
//!    uyku, zıplama (`anim`).
//! 5. `%LOCALAPPDATA%\CarlUI\pets\<ad>\` altına `pet.json` + PNG yazılır.
//!
//! 3-5. adımlar saf ve modelsizdir (`make`, `write`): ayar sayfası kesilmiş
//! resmi bir kez alıp farklı boyutlarla hızlıca yeniden üretebilir.

pub mod anim;
pub mod pack;
pub mod palette;
pub mod sheet;
pub mod sprite;
pub mod wic;

#[cfg(test)]
mod tests;

use crate::{catalog, rmbg::Remover, runtime, store, Image};
pub use sheet::{preview, Pet};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Kontur rengi (yerleşik kediyle aynı koyu mürdüm).
pub const OUTLINE: u32 = sprite::rgb(0x2b, 0x1d, 0x26);
/// Gövde için en fazla renk: + kontur + uyku "z"si = 16'yı aşmaz.
pub const BODY_COLOURS: usize = 13;
/// Fotoğraf çıkarımdan önce en fazla bu kenara küçültülür (bellek).
pub const PHOTO_MAX_SIDE: u32 = 1536;

#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    /// Gövdenin en uzun kenarı (sanat pikseli, kontur hariç); 12-64.
    pub size: u32,
    /// Fotoğraftaki hayvan sola bakıyor: aynalanır (paket hep sağa bakar).
    pub faces_left: bool,
    /// Aynı adlı paket varsa üzerine yaz.
    pub overwrite: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self { size: 28, faces_left: false, overwrite: false }
    }
}

/// Kurulu bir arka plan silme modeli (önce ayarlarda seçili olan).
pub fn model() -> Result<PathBuf, String> {
    if !runtime::installed() {
        return Err(t!("Yapay zekâ motoru kurulu değil. Ayarlar → Yapay zekâ'dan bir arka plan silme modeli indirin.", "The AI engine isn't installed. Download a background removal model in Settings → AI.").into());
    }
    let chosen = catalog::chosen(&carlui_core::config::load().ai, catalog::Feature::Background);
    std::iter::once(chosen)
        .chain(catalog::of(catalog::Feature::Background))
        .filter(|m| store::installed(m))
        .find_map(|m| store::find(&store::dir(m), "model.onnx"))
        .ok_or_else(|| t!("Arka plan silme modeli kurulu değil. Ayarlar → Yapay zekâ'dan bir BiRefNet modeli indirin.", "No background removal model is installed. Download a BiRefNet model in Settings → AI.").into())
}

fn stopped() -> String {
    t!("Durduruldu", "Stopped").into()
}

/// Fotoğrafı okur ve arka planını siler (yavaş adım). `progress(0..1)`.
pub fn cut_out(photo: &Path, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<Image, String> {
    let onnx = model()?;
    let img = wic::load(photo, PHOTO_MAX_SIDE)?;
    progress(0.05);
    if cancel.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    let mut r = Remover::open(&onnx)?;
    progress(0.15);
    if cancel.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    r.remove(&img, cancel, &|f| progress(0.15 + 0.85 * f))
}

/// Kesilmiş resimden (saydam arka plan) hayvanı üretir. Saf, modelsiz, hızlı.
pub fn make(cutout: &Image, name: &str, opts: &Opts) -> Result<Pet, String> {
    let size = opts.size.clamp(12, 64) as usize;
    let mut base = sprite::from_cutout(cutout, size)?;
    if base.opaque_count() < 12 || base.w < 3 || base.h < 3 {
        return Err(t!("Nesne çok küçük ya da çok ince", "The subject is too small or too thin").into());
    }
    if opts.faces_left {
        base = base.mirror();
    }
    palette::enhance(&mut base, 1.25, 1.08);
    palette::quantise(&mut base, BODY_COLOURS);
    let folder = pack::folder_name(name);
    let shown = pack::display_name(name, &folder);
    // Büyük hayvan biraz daha hızlı yürüsün (adım boyu bedenle büyür).
    let walk_speed = (18.0 * base.h.max(base.w) as f32 / 28.0).clamp(10.0, 40.0).round();
    let states = vec![
        sheet::State { key: "idle", file: "idle.png", ms: 160, frames: anim::idle(&base) },
        sheet::State { key: "walk", file: "walk.png", ms: 100, frames: anim::walk(&base) },
        sheet::State { key: "sleep", file: "sleep.png", ms: 450, frames: anim::sleep(&base) },
        sheet::State { key: "hop", file: "hop.png", ms: 70, frames: anim::hop(&base) },
    ];
    sheet::compose(&shown, walk_speed, states, OUTLINE)
}

/// Paketi `root` altına yazar (`root` genelde `carlui_core::live::pets_dir()`).
/// Klasör adı `pack::folder_name(&pet.name)`.
pub fn write(pet: &Pet, root: &Path, overwrite: bool) -> Result<PathBuf, String> {
    pack::write(pet, root, &pack::folder_name(&pet.name), overwrite)
}

/// Uçtan uca: fotoğraf → `root\<ad>\` paketi. Döndürülen klasörün adı
/// ayardaki `dock.live.pet` değeridir.
pub fn generate_into(photo: &Path, name: &str, opts: &Opts, root: &Path, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    // Model yüklenmeden önce: var olan paketin üzerine yazılmayacaksa boşuna bekletme.
    let folder = pack::folder_name(name);
    if !opts.overwrite && std::fs::symlink_metadata(root.join(&folder)).is_ok() {
        return Err(t!(format!("\"{folder}\" adında bir hayvan zaten var"), format!("A pet named \"{folder}\" already exists")));
    }
    let cut = cut_out(photo, cancel, &|f| progress(0.9 * f))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    let pet = make(&cut, name, opts)?;
    let dir = pack::write(&pet, root, &folder, opts.overwrite)?;
    progress(1.0);
    Ok(dir)
}

/// `generate_into` kullanıcının paket klasörüne.
pub fn generate(photo: &Path, name: &str, opts: &Opts, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    generate_into(photo, name, opts, &carlui_core::live::pets_dir(), cancel, progress)
}
