//! Kaldırma adımlarının ekranları: kaldırıcı çalışıyor, bekleme, artık listesi
//! ve sonuç.

use super::{human, list::shorten, Flow, Ui, A};
use crate::leftovers::{Kind, Left};
use carlui_ui::paint::{wrap, Align, Painter, GREEN, ORANGE, R, TEXT};

const ROW: f32 = 38.0;

fn check(p: &mut Painter<A>, r: R, on: bool) {
    if on {
        p.round(r, 4.0, p.accent, 0.95);
        p.glyph('\u{E73E}', r, 11.0, 0x000000, 0.85);
    } else {
        p.outline(r, 4.0, TEXT, 0.45);
    }
}

fn lines(p: &mut Painter<A>, text: &str, r: R, color: u32) -> f32 {
    let list = wrap(text, r.w, |s| p.fonts.width(s, 13.0, false));
    for (i, l) in list.iter().enumerate() {
        p.text(l, R::new(r.x, r.y + i as f32 * 20.0, r.w, 20.0), 13.0, false, color, 0.9, Align::Center);
    }
    list.len() as f32 * 20.0
}

impl Ui {
    pub(super) fn paint_flow(&mut self, p: &mut Painter<A>, body: R) {
        match &self.flow {
            Flow::Running { exited, error } => {
                let (exited, error) = (*exited, error.clone());
                self.paint_running(p, body, exited, error);
            }
            Flow::Busy => self.paint_busy(p, body),
            Flow::Review { .. } => self.paint_review(p, body),
            Flow::Done(_) => self.paint_done(p, body),
            _ => {}
        }
    }

    fn target_name(&self) -> String {
        self.apps.get(self.target).map(|a| a.name.clone()).unwrap_or_default()
    }

    fn paint_running(&mut self, p: &mut Painter<A>, body: R, exited: bool, error: Option<String>) {
        let name = self.target_name();
        let cx = body.x + body.w / 2.0;
        let mut y = body.y + body.h / 2.0 - 90.0;
        let col = R::new(cx - 260.0, y, 520.0, 30.0);
        p.text(&t!(format!("{name} kaldırılıyor"), format!("Removing {name}")), col, 18.0, true, TEXT, 0.95, Align::Center);
        y += 42.0;
        let text = match (&error, exited) {
            (Some(e), _) => t!(format!("Kaldırıcı başlatılamadı: {e}"), format!("The uninstaller could not start: {e}")),
            (None, true) => t!("Kaldırıcı kapandı ama uygulamanın kaydı duruyor. Vazgeçtiysen uygulama hâlâ kurulu olabilir.", "The uninstaller closed but the app is still registered. If you cancelled, the app may still be installed.").to_string(),
            (None, false) => t!("Kaldırıcının penceresindeki adımları tamamla. Bitince artık taraması kendiliğinden başlar.", "Finish the steps in the uninstaller window. The leftover scan starts by itself afterwards.").to_string(),
        };
        y += lines(p, &text, R::new(col.x, y, col.w, 0.0), if exited { ORANGE } else { TEXT }) + 20.0;
        p.button(R::new(cx - 190.0, y, 220.0, 38.0), Some('\u{E721}'), t!("Bitti, artıkları tara", "Done, scan leftovers"), A::Finished, true, false);
        p.button(R::new(cx + 40.0, y, 150.0, 38.0), None, t!("Vazgeç", "Cancel"), A::Back, true, false);
    }

    fn paint_busy(&mut self, p: &mut Painter<A>, body: R) {
        let cx = body.x + body.w / 2.0;
        let y = body.y + body.h / 2.0 - 40.0;
        p.text(&self.busy, R::new(body.x, y, body.w, 30.0), 18.0, true, TEXT, 0.95, Align::Center);
        let bar = R::new(cx - 220.0, y + 44.0, 440.0, 8.0);
        let x = (self.started.elapsed().as_secs_f32() * 0.6).fract();
        p.round(bar, 4.0, TEXT, 0.1);
        p.round(R::new(bar.x + bar.w * x * 0.75, bar.y, bar.w * 0.25, bar.h), 4.0, p.accent, 0.9);
    }

    fn paint_review(&mut self, p: &mut Painter<A>, body: R) {
        let name = self.target_name();
        let Flow::Review { items, gone, scroll } = &mut self.flow else { return };
        let gone = *gone;
        if items.is_empty() {
            let y = body.y + body.h / 2.0 - 50.0;
            p.text(&t!(format!("{name} için artık bulunamadı"), format!("No leftovers found for {name}")), R::new(body.x, y, body.w, 30.0), 18.0, true, TEXT, 0.95, Align::Center);
            p.button(R::new(body.x + body.w / 2.0 - 80.0, y + 48.0, 160.0, 38.0), None, t!("Listeye dön", "Back to list"), A::Close, true, false);
            return;
        }
        p.text(&t!(format!("{name}: {} artık bulundu", items.len()), format!("{name}: {} leftovers found", items.len())), R::new(body.x, body.y, body.w - 150.0, 30.0), 17.0, true, TEXT, 0.95, Align::Left);
        let all = items.iter().all(|i| i.on);
        p.button(R::new(body.right() - 140.0, body.y, 140.0, 32.0), None, if all { t!("Hiçbirini seçme", "Select none") } else { t!("Tümünü seç", "Select all") }, A::All, true, false);
        let mut top = body.y + 38.0;
        if !gone {
            p.text(t!("Uygulama hâlâ kurulu görünüyor. Bu yüzden hiçbir öğe seçili gelmedi; seçtiklerin uygulamayı bozabilir.", "The app still looks installed, so nothing is preselected; removing these may break it."), R::new(body.x, top, body.w, 22.0), 12.5, false, ORANGE, 0.95, Align::Left);
            top += 26.0;
        }
        let list = R::new(body.x, top, body.w, body.bottom() - top - 62.0);
        p.card(list);
        let rows = ((list.h - 8.0) / ROW).floor().max(1.0) as usize;
        *scroll = (*scroll).min(items.len().saturating_sub(rows));
        for (k, it) in items.iter().enumerate().skip(*scroll).take(rows) {
            let r = R::new(list.x + 4.0, list.y + 4.0 + (k - *scroll) as f32 * ROW, list.w - 16.0, ROW - 2.0);
            if p.hover(r) {
                p.round(r, 5.0, 0xFFFFFF, 0.07);
            }
            p.hit(r, A::Toggle(k));
            check(p, R::new(r.x + 8.0, r.y + 9.0, 18.0, 18.0), it.on);
            let glyph = match it.kind {
                Kind::Folder => '\u{E8B7}',
                Kind::File => '\u{E8A5}',
                Kind::Key => '\u{E74C}',
            };
            p.glyph(glyph, R::new(r.x + 34.0, r.y, 22.0, r.h), 13.0, TEXT, 0.7);
            let tw = r.w - 62.0 - 130.0;
            let s = shorten(p, &it.path, 12.5, tw);
            p.text(&s, R::new(r.x + 62.0, r.y, tw, r.h), 12.5, false, TEXT, 0.9, Align::Left);
            if it.kind != Kind::Key {
                p.text(&human(it.size), R::new(r.right() - 124.0, r.y, 80.0, r.h), 12.0, false, TEXT, 0.65, Align::Right);
                p.button(R::new(r.right() - 36.0, r.y + 4.0, 30.0, 28.0), Some('\u{E8DA}'), "", A::Reveal(k), true, false);
            }
        }
        if items.len() > rows {
            let track = list.h - 12.0;
            let th = (track * rows as f32 / items.len() as f32).max(24.0);
            let ty = list.y + 6.0 + (track - th) * *scroll as f32 / (items.len() - rows) as f32;
            p.round(R::new(list.right() - 8.0, ty, 4.0, th), 2.0, TEXT, 0.25);
        }
        let chosen: Vec<&Left> = items.iter().filter(|i| i.on).collect();
        let size: u64 = chosen.iter().map(|i| i.size).sum();
        let y = body.bottom() - 50.0;
        p.text(&t!(format!("Seçilen: {} öğe · {}", chosen.len(), human(size)), format!("Selected: {} items · {}", chosen.len(), human(size))), R::new(body.x, y, body.w - 340.0, 24.0), 13.0, true, TEXT, 0.9, Align::Left);
        p.text(t!("Dosyalar Geri Dönüşüm Kutusu'na gider, kayıt anahtarları önce yedeklenir.", "Files go to the Recycle Bin; registry keys are backed up first."), R::new(body.x, y + 24.0, body.w - 340.0, 22.0), 12.0, false, TEXT, 0.6, Align::Left);
        p.button(R::new(body.right() - 320.0, y + 6.0, 190.0, 38.0), Some('\u{E74D}'), t!("Seçilenleri sil", "Remove selected"), A::Delete, !chosen.is_empty(), true);
        p.button(R::new(body.right() - 120.0, y + 6.0, 120.0, 38.0), None, t!("Bitir", "Finish"), A::Close, true, false);
    }

    fn paint_done(&mut self, p: &mut Painter<A>, body: R) {
        let Flow::Done(rep) = &self.flow else { return };
        let cx = body.x + body.w / 2.0;
        let col = R::new(cx - 300.0, 0.0, 600.0, 0.0);
        let mut y = body.y + body.h / 2.0 - 110.0;
        let ok = rep.failed.is_empty();
        p.glyph(if ok { '\u{E73E}' } else { '\u{E7BA}' }, R::new(col.x, y, col.w, 36.0), 26.0, if ok { GREEN } else { ORANGE }, 1.0);
        y += 44.0;
        let title = if ok { t!("Temizlik bitti", "All clean") } else { t!("Bazı öğeler kaldırılamadı", "Some items could not be removed") };
        p.text(title, R::new(col.x, y, col.w, 30.0), 18.0, true, TEXT, 0.95, Align::Center);
        y += 36.0;
        let sum = if rep.freed > 0 { t!(format!("{} öğe kaldırıldı · {} boşaldı", rep.removed, human(rep.freed)), format!("{} items removed · {} freed", rep.removed, human(rep.freed))) } else { t!(format!("{} öğe kaldırıldı", rep.removed), format!("{} items removed", rep.removed)) };
        p.text(&sum, R::new(col.x, y, col.w, 22.0), 13.0, false, TEXT, 0.8, Align::Center);
        y += 28.0;
        for f in rep.failed.iter().take(5) {
            let s = shorten(p, f, 12.0, col.w);
            p.text(&s, R::new(col.x, y, col.w, 20.0), 12.0, false, ORANGE, 0.95, Align::Center);
            y += 20.0;
        }
        if let Some(b) = &rep.backup {
            let s = shorten(p, b, 12.0, col.w - 120.0);
            p.text(&t!(format!("Kayıt yedekleri: {s}"), format!("Registry backups: {s}")), R::new(col.x, y, col.w, 20.0), 12.0, false, TEXT, 0.6, Align::Center);
            y += 20.0;
        }
        p.button(R::new(cx - 80.0, y + 14.0, 160.0, 38.0), None, t!("Listeye dön", "Back to list"), A::Close, true, false);
    }
}
