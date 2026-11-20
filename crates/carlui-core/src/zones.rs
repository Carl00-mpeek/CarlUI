//! Bu modül "pencereyi ekranın belli bir parçasına hızlıca oturtmak" sorununun
//! hesap kısmını çözüyor: hazır ya da kullanıcının tanımladığı bölge
//! düzeninden, bir monitörün çalışma alanı için bölge dikdörtgenlerini üretir,
//! imlecin hangi bölgede olduğunu bulur ve iki bölgeyi birleştirir.
//! Pencere sürükleme ve katman çizimi ana süreçte (`carlui/src/zones.rs`);
//! burada Windows çağrısı yoktur, hepsi test edilebilir.

use serde::{Deserialize, Serialize};

/// Bölge düzeni. `Auto` ekranın en/boy oranına göre seçer.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ZoneLayout {
    /// Geniş ekran (21:9 ve üstü) → odak (25/50/25); dikey ekran → iki satır;
    /// diğerleri → 2×2 ızgara (Ctrl ile yarımlar).
    #[default]
    Auto,
    /// İki eşit sütun.
    Halves,
    /// Üç eşit sütun.
    Thirds,
    /// Ortada geniş, iki yanda dar sütun (25/50/25).
    Focus,
    /// Solda geniş ana alan, sağda yan sütun (60/40).
    MainSide,
    /// 2×2 ızgara.
    Grid,
    /// 3 sütun × 2 satır.
    Grid6,
    /// `zones_custom` listesi (yüzde olarak x, y, genişlik, yükseklik).
    Custom,
}

impl ZoneLayout {
    /// Ayarlarda sırayla gösterilen düzenler.
    pub const ALL: [ZoneLayout; 8] = [ZoneLayout::Auto, ZoneLayout::Halves, ZoneLayout::Thirds, ZoneLayout::Focus, ZoneLayout::MainSide, ZoneLayout::Grid, ZoneLayout::Grid6, ZoneLayout::Custom];
}

/// Çalışma alanının kesirleri: `[x, y, w, h]`, hepsi 0..1.
pub type Frac = [f32; 4];

fn columns(ws: &[f32]) -> Vec<Frac> {
    let mut x = 0.0;
    ws.iter()
        .map(|w| {
            let r = [x, 0.0, *w, 1.0];
            x += w;
            r
        })
        .collect()
}

fn grid(cols: usize, rows: usize) -> Vec<Frac> {
    let (w, h) = (1.0 / cols as f32, 1.0 / rows as f32);
    (0..rows).flat_map(|r| (0..cols).map(move |c| [c as f32 * w, r as f32 * h, w, h])).collect()
}

/// Kullanıcının yüzde listesini kesirlere çevirir; ekran dışına taşanlar
/// kırpılır, kırpılınca %2'den küçük kalanlar atılır.
fn custom(list: &[[f32; 4]]) -> Vec<Frac> {
    list.iter()
        .filter(|z| z.iter().all(|v| v.is_finite()))
        .filter_map(|z| {
            let x0 = (z[0] / 100.0).clamp(0.0, 1.0);
            let y0 = (z[1] / 100.0).clamp(0.0, 1.0);
            let x1 = ((z[0] + z[2]) / 100.0).clamp(0.0, 1.0);
            let y1 = ((z[1] + z[3]) / 100.0).clamp(0.0, 1.0);
            (x1 - x0 >= 0.02 && y1 - y0 >= 0.02).then_some([x0, y0, x1 - x0, y1 - y0])
        })
        .collect()
}

/// Düzenin kesirleri; `aspect` = çalışma alanının genişlik / yükseklik oranı.
/// Özel liste boş ya da geçersizse `Auto` kullanılır.
pub fn fractions(layout: ZoneLayout, custom_list: &[[f32; 4]], aspect: f32) -> Vec<Frac> {
    match layout {
        ZoneLayout::Halves => columns(&[0.5, 0.5]),
        ZoneLayout::Thirds => columns(&[1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0]),
        ZoneLayout::Focus => columns(&[0.25, 0.5, 0.25]),
        ZoneLayout::MainSide => columns(&[0.6, 0.4]),
        ZoneLayout::Grid => grid(2, 2),
        ZoneLayout::Grid6 => grid(3, 2),
        ZoneLayout::Custom => {
            let v = custom(custom_list);
            if v.is_empty() {
                fractions(ZoneLayout::Auto, &[], aspect)
            } else {
                v
            }
        }
        ZoneLayout::Auto => {
            if aspect >= 2.0 {
                columns(&[0.25, 0.5, 0.25])
            } else if aspect < 1.0 {
                grid(1, 2)
            } else {
                grid(2, 2)
            }
        }
    }
}

/// Ekran dikdörtgeni: `[sol, üst, sağ, alt]` (fiziksel piksel).
pub type Rect = [i32; 4];

/// Kesirleri çalışma alanına yerleştirir (boşluksuz; komşu bölgeler bitişik).
pub fn rects(fr: &[Frac], work: Rect) -> Vec<Rect> {
    let (w, h) = ((work[2] - work[0]) as f32, (work[3] - work[1]) as f32);
    let at = |o: i32, size: f32, f: f32| o + (size * f).round() as i32;
    fr.iter().map(|f| [at(work[0], w, f[0]), at(work[1], h, f[1]), at(work[0], w, f[0] + f[2]), at(work[1], h, f[1] + f[3])]).collect()
}

fn area(r: &Rect) -> i64 {
    (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64
}

/// İmlecin bulunduğu bölge; üst üste binenlerde (özel düzen) en küçüğü.
pub fn hit(zones: &[Rect], x: i32, y: i32) -> Option<usize> {
    zones.iter().enumerate().filter(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]).min_by_key(|(_, r)| area(r)).map(|(i, _)| i)
}

/// İki bölgeyi kapsayan en küçük dikdörtgen (Ctrl ile birleştirme).
pub fn span(a: Rect, b: Rect) -> Rect {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// Bölgenin pencereye verilecek hali: çalışma alanının kenarına değen
/// kenarlar `gap`, iç kenarlar `gap / 2` içeri çekilir; böylece iki pencere
/// arasında da kenarla pencere arasında da aynı boşluk kalır.
pub fn inset(r: Rect, work: Rect, gap: i32) -> Rect {
    if gap <= 0 {
        return r;
    }
    let half = gap / 2;
    let d = |edge: i32, wall: i32| if edge == wall { gap } else { half };
    let out = [r[0] + d(r[0], work[0]), r[1] + d(r[1], work[1]), r[2] - d(r[2], work[2]), r[3] - d(r[3], work[3])];
    // Çok küçük bölgede boşluk pencereyi yok etmesin.
    if out[2] - out[0] < 40 || out[3] - out[1] < 40 {
        r
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = [0, 0, 1920, 1040];

    #[test]
    fn hazir_duzenler_alani_tam_kaplar() {
        for l in ZoneLayout::ALL {
            let fr = fractions(l, &[], 16.0 / 9.0);
            assert!(!fr.is_empty(), "{l:?}");
            let total: f32 = fr.iter().map(|f| f[2] * f[3]).sum();
            assert!((total - 1.0).abs() < 1e-4, "{l:?}: {total}");
        }
    }

    #[test]
    fn otomatik_ekrana_gore_secer() {
        assert_eq!(fractions(ZoneLayout::Auto, &[], 21.0 / 9.0).len(), 3);
        assert_eq!(fractions(ZoneLayout::Auto, &[], 16.0 / 9.0).len(), 4);
        assert_eq!(fractions(ZoneLayout::Auto, &[], 9.0 / 16.0).len(), 2);
    }

    #[test]
    fn ozel_liste_kirpilir_bozuk_olan_atilir() {
        let list = [[0.0, 0.0, 70.0, 100.0], [70.0, 0.0, 50.0, 100.0], [10.0, 10.0, 1.0, 50.0], [f32::NAN, 0.0, 10.0, 10.0]];
        let fr = fractions(ZoneLayout::Custom, &list, 1.7);
        assert_eq!(fr.len(), 2);
        assert!((fr[1][0] - 0.7).abs() < 1e-6 && (fr[1][2] - 0.3).abs() < 1e-6);
        // Boş özel liste otomatiğe düşer.
        assert_eq!(fractions(ZoneLayout::Custom, &[], 16.0 / 9.0), fractions(ZoneLayout::Auto, &[], 16.0 / 9.0));
    }

    #[test]
    fn dikdortgenler_bitisik_ve_kaymali_alanda_dogru() {
        let work = [1920, 0, 4480, 1400];
        let r = rects(&fractions(ZoneLayout::Thirds, &[], 1.8), work);
        assert_eq!(r[0][0], 1920);
        assert_eq!(r[0][2], r[1][0]);
        assert_eq!(r[1][2], r[2][0]);
        assert_eq!(r[2][2], 4480);
    }

    #[test]
    fn imlecin_bolgesi_ve_birlestirme() {
        let r = rects(&fractions(ZoneLayout::Grid, &[], 1.8), WORK);
        assert_eq!(hit(&r, 10, 10), Some(0));
        assert_eq!(hit(&r, 1900, 1000), Some(3));
        assert_eq!(hit(&r, 5000, 10), None);
        // Sol üst + sol alt = sol yarı.
        assert_eq!(span(r[0], r[2]), [0, 0, 960, 1040]);
    }

    #[test]
    fn ust_uste_binende_kucuk_bolge_kazanir() {
        let r = rects(&fractions(ZoneLayout::Custom, &[[0.0, 0.0, 100.0, 100.0], [40.0, 40.0, 20.0, 20.0]], 1.8), WORK);
        assert_eq!(hit(&r, 960, 520), Some(1));
        assert_eq!(hit(&r, 10, 10), Some(0));
    }

    #[test]
    fn bosluk_kenarda_tam_icte_yarim() {
        let left = [0, 0, 960, 1040];
        assert_eq!(inset(left, WORK, 8), [8, 8, 956, 1032]);
        let right = [960, 0, 1920, 1040];
        assert_eq!(inset(right, WORK, 8), [964, 8, 1912, 1032]);
        assert_eq!(inset(left, WORK, 0), left);
        // Küçücük bölge boşlukla yok olmaz.
        assert_eq!(inset([0, 0, 30, 30], WORK, 20), [0, 0, 30, 30]);
    }

    #[test]
    fn toml_adlari() {
        #[derive(Serialize, Deserialize)]
        struct T {
            l: ZoneLayout,
        }
        assert_eq!(toml::to_string(&T { l: ZoneLayout::MainSide }).unwrap().trim(), "l = \"main-side\"");
        assert_eq!(toml::from_str::<T>("l = \"grid6\"").unwrap().l, ZoneLayout::Grid6);
    }
}
