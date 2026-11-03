//! Bu modül, CarlUI imlecinin oturum boyunca yerinde kalması sorununu şu
//! yöntemle çözüyor: ayar değişince (ya da Windows imleç şemasını/boyutunu
//! değiştirip bizimkini sildiğinde) imleç seti bir iş parçacığında yeniden
//! üretilip uygulanır; kapanışta ve koruyucuda kullanıcının şeması geri yüklenir.

use carlui_core::config::CursorConfig;
use carlui_core::{cursor, info, warn};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Son istek; eski iş parçacıkları sırası gelince kendini atlar.
static GEN: AtomicU32 = AtomicU32::new(0);
static LOCK: Mutex<()> = Mutex::new(());

pub fn sync(cfg: &CursorConfig) {
    let cfg = cfg.clone();
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("cursor".into())
        .spawn(move || {
            let _g = LOCK.lock();
            if GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            if !cfg.enabled {
                cursor::restore();
                info!("imleç: Windows şeması");
                return;
            }
            let t = std::time::Instant::now();
            let n = cursor::apply(&cfg);
            let color = cfg.style_color();
            if n > 0 {
                info!("imleç uygulandı: {:?} {color} kenar={} ışıma={} {} px, {n}/17 rol, {} ms", cfg.style, cfg.edge, cfg.glow, cursor::size(&cfg), t.elapsed().as_millis());
            } else {
                warn!("imleç uygulanamadı");
            }
        })
        .ok();
}

/// Kapanış: bekleyen işleri iptal edip şemayı geri yükler.
pub fn restore() {
    GEN.fetch_add(1, Ordering::SeqCst);
    let _g = LOCK.lock();
    cursor::restore();
}
