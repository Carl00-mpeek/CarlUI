//! Bu modül "hiçbir sürücü ya da izleme aracı yokken yine de bir sıcaklık
//! gösterebilir miyiz?" sorununu çözüyor. Windows, anakartın ACPI ısı
//! bölgelerini (_TZ) "Thermal Zone Information" performans sayacıyla
//! yayımlar; bu sayaç yönetici izni istemez (WMI'daki
//! `MSAcpi_ThermalZoneTemperature` ise ister).
//!
//! Sınırları: ısı bölgesi çekirdeğin kendi algılayıcısı değildir. Dizüstülerde
//! çoğu zaman işlemciye yakın bir algılayıcıdır ve işe yarar; masaüstü
//! anakartların çoğu ise hiç değişmeyen sabit bir değer (sık görüleni
//! 27,85 °C = 301 K) yazar. Bu yüzden bir bölge, ilk değerinden en az bir kez
//! saptığı görülene kadar "canlı" sayılmaz ve gösterilmez.

use super::{Source, Temps};
use std::sync::Mutex;
use windows::{core::*, Win32::System::Performance::*, Win32::System::SystemInformation::GetTickCount64};

/// Sayaç yoksa (sanal makine, kapalı sayaçlar) bu aralıkla yeniden denenir.
const RETRY_MS: u64 = 60_000;
/// Bundan küçük oynamalar "değişmedi" sayılır.
const EPS: f32 = 0.05;

struct Query {
    q: isize,
    c: isize,
    /// Sayaç onda bir Kelvin mi (High Precision) yoksa tam Kelvin mi?
    tenths: bool,
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.q);
        }
    }
}

struct State {
    query: Option<Query>,
    retry_at: u64,
    watch: Watch,
}

static STATE: Mutex<State> = Mutex::new(State { query: None, retry_at: 0, watch: Watch { zones: Vec::new() } });

pub fn read() -> Option<Temps> {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = unsafe { GetTickCount64() };
    if st.query.is_none() {
        if now < st.retry_at {
            return None;
        }
        st.query = open();
        if st.query.is_none() {
            st.retry_at = now + RETRY_MS;
            return None;
        }
    }
    let zones = st.query.as_ref().and_then(sample);
    let Some(zones) = zones else {
        st.query = None;
        st.retry_at = now + RETRY_MS;
        return None;
    };
    let package = st.watch.feed(&zones)?;
    Some(Temps { package, cores: Vec::new(), source: Source::Acpi })
}

fn open() -> Option<Query> {
    unsafe {
        let mut q = 0;
        if PdhOpenQueryW(None, 0, &mut q) != 0 {
            return None;
        }
        // Dil bağımsız (İngilizce) adlarla: Türkçe Windows'ta da çalışır.
        for (path, tenths) in [(h!("\\Thermal Zone Information(*)\\High Precision Temperature"), true), (h!("\\Thermal Zone Information(*)\\Temperature"), false)] {
            let mut c = 0;
            if PdhAddEnglishCounterW(q, path, 0, &mut c) == 0 {
                let query = Query { q, c, tenths };
                return sample(&query).filter(|z| !z.is_empty()).map(|_| query);
            }
        }
        PdhCloseQuery(q);
        None
    }
}

/// Bölge adı ve °C değeri.
fn sample(query: &Query) -> Option<Vec<(String, f32)>> {
    unsafe {
        if PdhCollectQueryData(query.q) != 0 {
            return None;
        }
        let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000); // PDH_FMT_NOCAP100
        let (mut size, mut count) = (0u32, 0u32);
        let r = PdhGetFormattedCounterArrayW(query.c, fmt, &mut size, &mut count, None);
        if r != PDH_MORE_DATA || size == 0 {
            return (r == 0).then(Vec::new);
        }
        // u64 dizisi: öğeler 8 bayta hizalı olmalı.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(query.c, fmt, &mut size, &mut count, Some(items)) != 0 {
            return None;
        }
        let items = std::slice::from_raw_parts(items, count as usize);
        Some(
            items
                .iter()
                .filter(|i| i.FmtValue.CStatus == 0)
                .map(|i| {
                    let k = i.FmtValue.Anonymous.doubleValue as f32;
                    (i.szName.to_string().unwrap_or_default(), if query.tenths { k / 10.0 } else { k } - 273.15)
                })
                .collect(),
        )
    }
}

/// Sabit (hiç değişmeyen) bölgeleri ayıklar.
#[derive(Default)]
pub(super) struct Watch {
    /// Ad, ilk değer, değiştiği görüldü mü.
    zones: Vec<(String, f32, bool)>,
}

impl Watch {
    /// Yeni ölçümleri işler; değiştiği görülmüş, makul aralıktaki bölgelerin
    /// en sıcağını döndürür.
    pub fn feed(&mut self, now: &[(String, f32)]) -> Option<f32> {
        let mut best: Option<f32> = None;
        for (name, t) in now {
            if !(5.0..=115.0).contains(t) {
                continue;
            }
            let z = match self.zones.iter_mut().find(|z| z.0 == *name) {
                Some(z) => z,
                None => {
                    self.zones.push((name.clone(), *t, false));
                    self.zones.last_mut().unwrap()
                }
            };
            z.2 |= (t - z.1).abs() > EPS;
            if z.2 {
                best = Some(best.map_or(*t, |b| b.max(*t)));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(name: &str, t: f32) -> (String, f32) {
        (name.into(), t)
    }

    #[test]
    fn sabit_bolge_hic_gosterilmez() {
        let mut w = Watch::default();
        for _ in 0..100 {
            assert_eq!(w.feed(&[z("\\_TZ.TZ00", 27.85)]), None);
        }
    }

    #[test]
    fn degisen_bolge_canli_sayilir_ve_en_sicagi_secilir() {
        let mut w = Watch::default();
        assert_eq!(w.feed(&[z("TZ00", 27.85), z("CPUZ", 50.0)]), None);
        assert_eq!(w.feed(&[z("TZ00", 27.85), z("CPUZ", 52.0)]), Some(52.0));
        // Canlı bölge eski değerine dönse de canlı kalır.
        assert_eq!(w.feed(&[z("TZ00", 27.85), z("CPUZ", 50.0)]), Some(50.0));
        assert_eq!(w.feed(&[z("TZ00", 30.0), z("CPUZ", 29.0)]), Some(30.0));
    }

    /// Bu makinedeki gerçek sayaç: `cargo test -p carlui-sys -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bu_makinenin_bolgeleri() {
        let q = open().expect("Thermal Zone Information sayacı yok");
        println!("onda bir Kelvin: {} · bölgeler: {:?}", q.tenths, sample(&q));
    }

    #[test]
    fn mantiksiz_degerler_atlanir() {
        let mut w = Watch::default();
        w.feed(&[z("A", -273.15)]);
        assert_eq!(w.feed(&[z("A", 0.0)]), None);
        w.feed(&[z("B", 40.0)]);
        assert_eq!(w.feed(&[z("B", 200.0)]), None);
    }
}
