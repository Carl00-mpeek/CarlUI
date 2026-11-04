//! İçe aktarma önizlemesi: kaynak, kategori başına kaç kayıt geleceği, kaçının
//! kasada zaten olduğu ve kayıt adları (yalnızca adlar; değerler gösterilmez).
//! "İçe aktar" yenileri tek seferde şifreleyip yazar, "Vazgeç" hiçbir şey yazmaz.

use super::{clipped, list::column, para, primary, P};
use crate::app::{importing::Preview, Act, App};
use carlui_core::t;
use carlui_ui::paint::{Align, ORANGE, R, TEXT};

/// Listede en çok bu kadar ad gösterilir.
const SHOW: usize = 300;

pub fn paint(app: &App, pv: &Preview, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, cw) = column(w);
    p.text(t!("İçe aktar", "Import"), R::new(x, 18.0, cw - 340.0, 38.0), 22.0, true, TEXT, 0.95, Align::Left);
    let n = pv.count();
    let label = t!(format!("{n} kaydı içe aktar"), format!("Import {n} items"));
    primary(p, R::new(x + cw - 190.0, 20.0, 190.0, 34.0), &label, Act::ImportConfirm, n > 0);
    p.button(R::new(x + cw - 296.0, 20.0, 98.0, 34.0), None, t!("Vazgeç", "Cancel"), Act::Cancel, true, false);

    let clip = R::new(super::SIDEBAR, 70.0, w - super::SIDEBAR, h - 70.0);
    let mut y = clip.y + 6.0 - app.scroll;
    clipped(p, clip, |p| {
        let src = if pv.file.is_empty() { pv.batch.source.clone() } else { format!("{} · {}", pv.batch.source, pv.file) };
        p.text(&src, R::new(x + 2.0, y, cw, 22.0), 13.0, true, p.accent, 0.95, Align::Left);
        y += 30.0;

        // Kategori özetleri: küçük kartlar.
        let per = 4;
        let chip_w = (cw - 8.0 * (per as f32 - 1.0)) / per as f32;
        for (i, (c, k)) in pv.by_category().into_iter().enumerate() {
            let r = R::new(x + (i % per) as f32 * (chip_w + 8.0), y + (i / per) as f32 * 58.0, chip_w, 50.0);
            p.card(r);
            p.glyph(c.glyph(), R::new(r.x + 10.0, r.y, 24.0, r.h), 14.0, p.accent, 1.0);
            p.text(&k.to_string(), R::new(r.x + 40.0, r.y + 6.0, r.w - 46.0, 22.0), 16.0, true, TEXT, 0.95, Align::Left);
            p.text(c.title(), R::new(r.x + 40.0, r.y + 26.0, r.w - 46.0, 18.0), 12.0, false, TEXT, 0.6, Align::Left);
        }
        let rows = pv.by_category().len().div_ceil(per);
        y += rows as f32 * 58.0 + 6.0;

        let mut notes = Vec::new();
        if pv.dups > 0 {
            notes.push(t!(format!("{} kayıt kasada zaten var; tekrar eklenmeyecek.", pv.dups), format!("{} items are already in the vault and won't be added again.", pv.dups)));
        }
        if pv.batch.skipped > 0 {
            notes.push(t!(
                format!("{} kayıt desteklenmediği için atlandı (ör. sayaçlı HOTP, Steam).", pv.batch.skipped),
                format!("{} items were skipped as unsupported (e.g. counter-based HOTP, Steam).", pv.batch.skipped)
            ));
        }
        for m in notes {
            y += para(p, &m, x, y, cw, 12.5, TEXT, 0.7) + 2.0;
        }
        if !pv.file.is_empty() {
            y += para(
                p,
                t!(
                    "Dışa aktarım dosyası şifresizdir: içe aktardıktan sonra onu sil ve Geri Dönüşüm Kutusu'nu boşalt.",
                    "The export file is unencrypted: after importing, delete it and empty the Recycle Bin."
                ),
                x,
                y,
                cw,
                12.5,
                ORANGE,
                0.95,
            ) + 6.0;
        }
        y += 8.0;

        for (it, fresh) in pv.batch.items.iter().zip(&pv.fresh).take(SHOW) {
            let r = R::new(x, y, cw, 40.0);
            p.round(r, 7.0, 0xFFFFFF, if *fresh { 0.05 } else { 0.02 });
            let a = if *fresh { 0.92 } else { 0.4 };
            p.glyph(it.meta.category.glyph(), R::new(r.x + 10.0, r.y, 24.0, r.h), 13.0, p.accent, a);
            p.text(&it.meta.name, R::new(r.x + 42.0, r.y, r.w - 200.0, r.h), 13.0, false, TEXT, a, Align::Left);
            let tag = if *fresh { it.meta.category.title() } else { t!("Kasada var", "Already in vault") };
            p.text(tag, R::new(r.right() - 160.0, r.y, 150.0, r.h), 12.0, false, TEXT, 0.45, Align::Right);
            y += 44.0;
        }
        if pv.batch.items.len() > SHOW {
            let more = pv.batch.items.len() - SHOW;
            p.text(&t!(format!("+{more} kayıt daha"), format!("+{more} more")), R::new(x, y, cw, 30.0), 12.5, false, TEXT, 0.5, Align::Center);
            y += 34.0;
        }
        y += 20.0;
    });
    (y + app.scroll - clip.y + 20.0 - clip.h).max(0.0)
}
