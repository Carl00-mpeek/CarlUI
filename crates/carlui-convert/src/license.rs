//! Bu modül "ticari kullanımı yasaklayan bir modeli kullanıcıya nasıl dürüstçe
//! sunarız?" sorusunu çözüyor. Kısıtlı lisanslı (`restricted`) bir model ilk
//! kez indirilmeden önce lisans adı, ne anlama geldiği ve resmi kaynağı açıkça
//! yazılır; kullanıcı "kabul ediyorum" demeden hiçbir dosya inmez. Onay
//! `store::accept` ile kaydedilir ve bir daha sorulmaz.

use crate::{ai, cursor};
use carlui_ai::{catalog::Model, store};
use carlui_ui::glassmenu::{self, Icon, Row};
use windows::Win32::Foundation::HWND;

/// Lisans onaylı mı; değilse sorar. `need`: indirilecek bayt (biliniyorsa düğmede yazılır).
pub fn ask(owner: HWND, accent: Option<u32>, m: &Model, need: Option<u64>) -> bool {
    if store::accepted(m) {
        return true;
    }
    let warn = || Icon::Glyph('\u{E7BA}', Some(0xFFB454));
    let info = || Icon::Glyph('\u{E946}', None);
    loop {
        let accept = match need {
            Some(n) => t!(format!("Lisansı kabul ediyorum, indir ({})", ai::mb(n)), format!("I accept the license, download ({})", ai::mb(n))),
            None => t!("Lisansı kabul ediyorum", "I accept the license").into(),
        };
        let rows = vec![
            Row::Label(t!(format!("{}: yalnızca ticari olmayan kullanım", m.name), format!("{}: non-commercial use only", m.name)), warn()),
            Row::Label(t!(format!("Lisans: {}", m.license), format!("License: {}", m.license)), info()),
            Row::Label(t!("Kişisel kullanım serbest; para kazanılan işlerde (reklam, ücretli seslendirme, gelir getiren video) kullanılamaz.", "Personal use is fine; it can't be used for paid work (ads, paid voice-over, monetized videos).").into(), info()),
            Row::Label(t!(format!("Model resmi kaynağından iner: {}", m.source.trim_start_matches("https://")), format!("Downloaded from its official source: {}", m.source.trim_start_matches("https://"))), info()),
            Row::Item { id: 1, text: accept, icon: Icon::Glyph('\u{E73E}', None) },
            Row::Item { id: 3, text: t!("Lisansı ve kaynağı tarayıcıda aç", "Open the license and source in the browser").into(), icon: Icon::Glyph('\u{E774}', None) },
            Row::Item { id: 2, text: t!("Vazgeç", "Cancel").into(), icon: Icon::Glyph('\u{E711}', None) },
        ];
        match glassmenu::run(owner, rows, accent, cursor()) {
            Some(1) => {
                if let Err(e) = store::accept(m) {
                    carlui_core::warn!("lisans onayı yazılamadı: {e}");
                }
                carlui_core::info!("lisans onaylandı: {} ({})", m.id, m.license);
                return true;
            }
            Some(3) => crate::open(m.source),
            _ => return false,
        }
    }
}
