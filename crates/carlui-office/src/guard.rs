//! Bu modül "kötü niyetli ya da bozuk bir dosya Carl Office'i çökertmesin,
//! belleği tüketmesin" sorununu çözüyor. İki savunma burada toplanır:
//! sınırlar (zip girdisi, XML derinliği, formül uzunluğu, resim boyutu…
//! hepsi tek yerde) ve `deep`: dosya ayrıştırma ve hesaplama, yığını geniş
//! ayrı bir iş parçacığında çalışır. Windows'ta ana iş parçacığının yığını
//! 1 MB'tır; iç içe binlerce öğe ya da uzun bir formül zinciri onu taşırıp
//! süreci düşürebilirdi. Geniş yığın yalnızca ayrılır (rezerve), kullanıldıkça
//! belleğe girer; iş bitince iş parçacığı da biter.

use std::sync::Mutex;

/// Zip içindeki tek bir parçanın açılmış en büyük boyutu.
pub const ZIP_ENTRY: usize = 512 << 20;
/// XML'de iç içe öğe sınırı (Office belgeleri birkaç düzinede kalır).
pub const XML_DEPTH: usize = 1000;
/// Formül uzunluğu (Excel'in sınırı 8192 karakter).
pub const FORMULA_LEN: usize = 8192;
/// Formülde iç içe parantez/işlev sınırı (Excel 64 işlev düzeyine izin verir).
pub const FORMULA_NEST: usize = 128;
/// Hücre bağımlılık zincirinin en derin hali; aşılırsa hücre hata verir.
pub const CALC_DEPTH: usize = 100_000;
/// Arama işlevlerinin (DÜŞEYARA, ÇOKEĞERSAY…) bir seferde açacağı hücre sayısı.
pub const GRID_CELLS: u64 = 4_000_000;
/// Çizilen PDF sayfasının en çok piksel sayısı (yakınlaştırmada keskin kalsın).
pub const PIXELS: u64 = 40_000_000;
/// Belgedeki bir resmin en çok piksel sayısı (4K × 4K'dan büyük; ekrana yeter).
pub const IMAGE_PIXELS: u64 = 16_777_216;
/// Bitmap'in bir kenarı en çok (Direct2D'nin doku sınırı).
pub const SIDE: u32 = 16384;

const STACK: usize = 512 << 20;

/// `f`'yi geniş yığınlı bir iş parçacığında çalıştırıp sonucunu döndürür.
pub fn deep<T: Send, F: FnOnce() -> T + Send>(f: F) -> T {
    let slot = Mutex::new(Some(f));
    let take = || slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    let out = std::thread::scope(|s| {
        let h = std::thread::Builder::new().name("ayrıştır".into()).stack_size(STACK).spawn_scoped(s, || take().map(|f| f()));
        match h {
            Ok(h) => h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)),
            Err(_) => None,
        }
    });
    // İş parçacığı açılamadıysa (kaynak kalmadıysa) burada çalıştır.
    match out {
        Some(v) => v,
        None => take().expect("iş zaten çalıştı")(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deep_runs_and_returns() {
        let v = [1, 2, 3];
        assert_eq!(super::deep(|| v.iter().sum::<i32>()), 6);
    }
}
