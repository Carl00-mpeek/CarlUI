//! Diğer ekranlardaki dock'lar (ayarlardan açılır, varsayılan kapalı).
//!
//! Ana dock her ek monitör için kendini `--screen N` ile bir kez daha başlatır.
//! Her kopya kendi monitörünün kenarında durur ve yalnızca o monitördeki
//! pencereleri (ve sabitlenmişleri) gösterir. Win tuşu, Win+1…9, pano ve
//! dönüştürücü kısayolları yalnızca ana dock'tadır; tray düğmesi de onda.
//! Seçenek kapanınca ya da monitör çıkarılınca kopya kendiliğinden kapanır.

use carlui_core::win::{self, Monitor};
use std::sync::OnceLock;
use windows::{
    core::HSTRING,
    Win32::{Foundation::RECT, Graphics::Gdi::HMONITOR},
};

static CHILD: OnceLock<Option<usize>> = OnceLock::new();

/// Bu süreç ek ekran dock'u mu; öyleyse kaçıncı ek monitör.
pub fn child() -> Option<usize> {
    *CHILD.get_or_init(|| {
        let mut args = std::env::args();
        while let Some(a) = args.next() {
            if a == "--screen" {
                return args.next().and_then(|n| n.parse().ok());
            }
        }
        None
    })
}

/// Ana monitör dışındakiler, soldan sağa (sonra yukarıdan aşağı) sıralı:
/// numaralar monitör eklenip çıkarılmadıkça sabit kalır.
pub fn others() -> Vec<Monitor> {
    let mut v: Vec<Monitor> = win::monitors().into_iter().filter(|m| !m.primary).collect();
    v.sort_by_key(|m| (m.rect.left, m.rect.top));
    v
}

/// Bu dock'un monitörü (ek ekran yoksa `None`: kopya kapanmalı). Kum
/// havuzunda hep ilk ek monitör.
pub fn monitor() -> Option<Monitor> {
    if crate::sandbox::on() {
        return others().first().copied();
    }
    match child() {
        Some(n) => others().get(n).copied(),
        None => win::primary_monitor(),
    }
}

pub fn rect() -> RECT {
    monitor().map(|m| m.rect).unwrap_or_default()
}

/// Pencere filtresi: seçenek açık ve ek ekran varken her dock yalnızca
/// kendi monitöründeki pencereleri gösterir (pencere iki dock'ta birden olmasın).
pub fn filter(enabled: bool) -> Option<HMONITOR> {
    if !enabled || others().is_empty() || crate::sandbox::on() {
        return None;
    }
    monitor().map(|m| m.handle)
}

/// Ana dock: seçenek açıksa her ek monitör için bir kopya başlatır.
/// Zaten çalışan kopya kendi kilidini görüp hemen çıkar; bu yüzden tekrar
/// çağırmak (ayar değişimi, monitör takılması) güvenli.
pub fn spawn(enabled: bool) {
    if !enabled || child().is_some() || crate::sandbox::on() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let n = others().len();
    for i in 0..n {
        let r = std::process::Command::new(&exe).args(["--screen", &i.to_string()]).spawn();
        match r {
            Ok(_) => carlui_core::info!("ek ekran dock'u başlatıldı ({})", i + 1),
            Err(e) => carlui_core::warn!("ek ekran dock'u başlatılamadı: {e}"),
        }
    }
}

/// Tek kopya kilidinin adı (ana dock ve her ek ekran ayrı).
pub fn mutex_name() -> HSTRING {
    if crate::sandbox::on() {
        return HSTRING::from("Local\\CarlUI.DockSandbox");
    }
    match child() {
        Some(n) => HSTRING::from(format!("Local\\CarlUI.Dock2.{n}")),
        None => HSTRING::from("Local\\CarlUI.Dock"),
    }
}
