//! Kayıt saatimiz: görüntü kareleri ile WASAPI ses paketlerinin zamanını aynı
//! saatte (QPC, 100 ns) ölçmek için.

/// Şu anki zaman (QPC, 100 ns); WASAPI'nin paket zamanlarıyla aynı saat.
pub fn now() -> i64 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let f = *FREQ.get_or_init(|| {
        let mut f = 0;
        let _ = unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1)
    });
    let mut c = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut c) };
    (c as i128 * 10_000_000 / f as i128) as i64
}
