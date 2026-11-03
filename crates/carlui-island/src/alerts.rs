//! Bu modül "bilgisayar ısınınca ya da disk dolunca haberim olsun" sorununu
//! çözüyor: 10 sn'de bir CPU (CarlUI Sensors'ın paylaşılan belleği), GPU
//! (NVML) ve disk (StorageDeviceTemperatureProperty / NVMe sağlık kaydı)
//! sıcaklığı, dakikada bir de sürücülerin boş alanı okunur. Sınır ayarlardan
//! (`alerts`) gelir.
//!
//! Anlık sıçramalar bildirim getirmesin diye sıcaklık 30 sn üst üste sınırın
//! üstünde kalmalı. Bildirim bir kez gelir; değer sınırın 5 °C altına inene
//! kadar susar, inmezse 30 dk'da bir hatırlatır. Disk için de aynısı: boş
//! alan sınırın 2 puan üstüne çıkınca yeniden kurulur.

use crate::notify::Note;
use carlui_core::config::AlertsConfig;
use carlui_sys::{bytes, disk, gpu, temp};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

const TICK: Duration = Duration::from_secs(10);
/// Sıcaklık bu kadar ölçüm üst üste sınırı aşmalı (3 × 10 sn).
const CONFIRM: u8 = 3;
/// Sınırın altına bu kadar inince yeniden kurulur.
const COOL: f32 = 5.0;
const FREE_MARGIN: f32 = 2.0;
/// Sorun sürerse hatırlatma aralığı.
const REPEAT: Duration = Duration::from_secs(30 * 60);
/// Diskler bu kadar ölçümde bir okunur (sıcaklık 30 sn, boş alan 60 sn).
const DISK_EVERY: u32 = 3;
const SPACE_EVERY: u32 = 6;

/// Bir göstergenin durumu: kaç ölçümdür sınırın üstünde, en son ne zaman bildirildi.
#[derive(Default, Debug)]
struct Gate {
    streak: u8,
    fired: Option<Instant>,
}

impl Gate {
    /// `over`: sınır aşıldı; `clear`: yeterince düzeldi (susma biter).
    /// Bildirim gerekiyorsa `true`.
    fn step(&mut self, over: bool, clear: bool, need: u8, now: Instant) -> bool {
        if clear {
            *self = Self::default();
            return false;
        }
        if !over {
            self.streak = 0;
            return false;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak < need || self.fired.is_some_and(|t| now.duration_since(t) < REPEAT) {
            return false;
        }
        self.fired = Some(now);
        true
    }
}

#[derive(Default)]
struct Alerts {
    gates: HashMap<String, Gate>,
}

impl Alerts {
    /// Sıcaklık: sınırı aşıp aşmadığına bakar.
    fn hot(&mut self, key: &str, value: f32, limit: u32, now: Instant) -> bool {
        let limit = limit as f32;
        self.gates.entry(key.into()).or_default().step(value >= limit, value < limit - COOL, CONFIRM, now)
    }

    /// Boş alan yüzdesi sınırın altına indi mi (tek ölçüm yeter).
    fn low(&mut self, key: &str, free_pct: f32, limit: u32, now: Instant) -> bool {
        let limit = limit as f32;
        self.gates.entry(key.into()).or_default().step(free_pct < limit, free_pct >= limit + FREE_MARGIN, 1, now)
    }
}

fn deg(v: f32) -> String {
    format!("{v:.0} °C")
}

fn hot_note(what: &str, detail: &str, value: f32, limit: u32) -> Note {
    let body = t!(format!("{}{} · sınır {}", detail, deg(value), deg(limit as f32)), format!("{}{} · limit {}", detail, deg(value), deg(limit as f32)));
    Note { app: t!("Uyarılar", "Alerts").into(), title: t!(format!("{what} çok ısındı"), format!("{what} is running hot")), body, ..Default::default() }
}

/// Sabit sürücüler (C:, D: …): harf, toplam, boş.
fn drives() -> Vec<(String, f64, f64)> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let letter = (b'A' + i as u8) as char;
            let root = HSTRING::from(format!("{letter}:\\"));
            // 3 = DRIVE_FIXED (USB bellekler ve ağ sürücüleri dışarıda).
            if unsafe { GetDriveTypeW(&root) } != 3 {
                return None;
            }
            let (mut free, mut total) = (0u64, 0u64);
            unsafe { GetDiskFreeSpaceExW(&root, Some(&mut free), Some(&mut total), None) }.ok()?;
            (total > 0).then(|| (format!("{letter}:"), total as f64, free as f64))
        })
        .collect()
}

fn check(a: &mut Alerts, c: &AlertsConfig, n: u32, now: Instant) -> Vec<Note> {
    let mut out = Vec::new();
    if c.cpu {
        if let Some(t) = temp::read() {
            if a.hot("cpu", t.package, c.cpu_temp, now) {
                out.push(hot_note("CPU", "", t.package, c.cpu_temp));
            }
        }
    }
    if c.gpu {
        if let Some(g) = gpu::read().filter(|g| g.temp > 0.0) {
            if a.hot("gpu", g.temp, c.gpu_temp, now) {
                out.push(hot_note("GPU", &format!("{} · ", g.name), g.temp, c.gpu_temp));
            }
        }
    }
    if c.disk && n % DISK_EVERY == 0 {
        for d in disk::list() {
            let Some(t) = d.temp else { continue };
            // Disk ölçümü 30 sn'de bir: tek ölçüm 30 sn sayılır.
            let key = format!("disk{}", d.index);
            let limit = c.disk_temp as f32;
            if a.gates.entry(key).or_default().step(t >= limit, t < limit - COOL, 1, now) {
                let what = if d.ssd { "SSD" } else { "Disk" };
                let name = if d.volumes.is_empty() { d.model.clone() } else { format!("{} ({})", d.model, d.volumes.join(" ")) };
                out.push(hot_note(what, &format!("{name} · "), t, c.disk_temp));
            }
        }
    }
    if c.space && n % SPACE_EVERY == 0 {
        for (letter, total, free) in drives() {
            let pct = (free / total * 100.0) as f32;
            if a.low(&letter, pct, c.space_pct, now) {
                out.push(Note {
                    app: t!("Uyarılar", "Alerts").into(),
                    title: t!(format!("{letter} dolmak üzere"), format!("{letter} is almost full")),
                    body: t!(format!("{} boş ({})", bytes(free), carlui_core::lang::pct(pct)), format!("{} free ({})", bytes(free), carlui_core::lang::pct(pct))),
                    file: Some(format!("{letter}\\")),
                    ..Default::default()
                });
            }
        }
    }
    out
}

/// İzleyiciyi başlatır; uyarılar `notes`'a eklenip `msg` gönderilir.
pub fn start(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("alerts".into())
        .spawn(move || {
            let mut a = Alerts::default();
            // Açılışta ilk ölçüm hemen (dolu disk hemen haber versin), sonra 10 sn'de bir.
            for n in 0u32.. {
                let cfg = carlui_core::config::load().alerts;
                let fresh = check(&mut a, &cfg, n, Instant::now());
                if !fresh.is_empty() {
                    for f in &fresh {
                        carlui_core::info!("uyarı: {} — {}", f.title, f.body);
                    }
                    if let Ok(mut q) = notes.lock() {
                        q.extend(fresh);
                    }
                    unsafe {
                        let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(0), LPARAM(0));
                    }
                }
                std::thread::sleep(TICK);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_needs_three_and_rearms() {
        let mut a = Alerts::default();
        let t0 = Instant::now();
        assert!(!a.hot("cpu", 95.0, 90, t0));
        assert!(!a.hot("cpu", 95.0, 90, t0));
        assert!(a.hot("cpu", 95.0, 90, t0));
        // Susar: biraz inip yeniden çıkınca tekrar gelmez.
        assert!(!a.hot("cpu", 88.0, 90, t0));
        for _ in 0..5 {
            assert!(!a.hot("cpu", 95.0, 90, t0));
        }
        // 30 dk sonra hatırlatır.
        assert!(a.hot("cpu", 95.0, 90, t0 + REPEAT));
        // 5 °C soğuyunca yeniden kurulur.
        assert!(!a.hot("cpu", 84.0, 90, t0 + REPEAT));
        assert!(!a.hot("cpu", 95.0, 90, t0 + REPEAT));
        assert!(!a.hot("cpu", 95.0, 90, t0 + REPEAT));
        assert!(a.hot("cpu", 95.0, 90, t0 + REPEAT));
    }

    #[test]
    fn spike_is_ignored() {
        let mut a = Alerts::default();
        let t0 = Instant::now();
        assert!(!a.hot("gpu", 99.0, 85, t0));
        assert!(!a.hot("gpu", 70.0, 85, t0));
        assert!(!a.hot("gpu", 99.0, 85, t0));
        assert!(!a.hot("gpu", 99.0, 85, t0));
        assert!(a.hot("gpu", 99.0, 85, t0));
    }

    #[test]
    fn space() {
        let mut a = Alerts::default();
        let t0 = Instant::now();
        assert!(!a.low("C:", 15.0, 10, t0));
        assert!(a.low("C:", 8.0, 10, t0));
        assert!(!a.low("C:", 7.0, 10, t0));
        assert!(!a.low("C:", 11.0, 10, t0));
        assert!(!a.low("C:", 9.0, 10, t0));
        assert!(!a.low("C:", 12.5, 10, t0));
        assert!(a.low("C:", 9.0, 10, t0));
    }

    #[test]
    #[ignore = "gerçek donanımı okur"]
    fn live() {
        let c = AlertsConfig { cpu_temp: 1, gpu_temp: 1, disk_temp: 1, space_pct: 100, ..Default::default() };
        let mut a = Alerts::default();
        let t0 = Instant::now();
        let mut all = Vec::new();
        for n in 0..3 {
            all.extend(check(&mut a, &c, n * 6, t0));
        }
        for x in &all {
            println!("{} — {}", x.title, x.body);
        }
        assert!(!all.is_empty());
    }
}
