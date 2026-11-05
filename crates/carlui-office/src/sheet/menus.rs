//! Bu modül "tablodaki açılır menüler: sayı biçimi, dolgu / yazı rengi ve
//! sağ tık menüsü" sorununu çözüyor; seçilen komutu görünüme uygular.

use super::{numfmt, view::{cursor, SheetView}};
use crate::app;
use carlui_core::t;
use carlui_ui::glassmenu::{self, Icon, Row};

impl SheetView {
    pub(super) fn fmt_label(&self) -> &'static str {
        let code = self.book.sheets[self.sh].get(self.act.0, self.act.1).map(|c| self.book.fmt_code(c.s)).unwrap_or("General");
        if code.eq_ignore_ascii_case("general") {
            t!("Genel", "General")
        } else if code.contains('%') {
            t!("Yüzde", "Percent")
        } else if numfmt::is_date(code) {
            t!("Tarih", "Date")
        } else if code.contains('₺') || code.contains('$') || code.contains('€') {
            t!("Para birimi", "Currency")
        } else {
            t!("Sayı", "Number")
        }
    }

    pub(super) fn num_fmt(&mut self, n: u8) {
        let tr = carlui_core::lang::is_tr();
        let code = match n {
            1 => "#,##0.00",
            2 if tr => "#,##0.00 \"₺\"",
            2 => "\"$\"#,##0.00",
            3 => "0%",
            4 if tr => "dd.mm.yyyy",
            4 => "m/d/yyyy",
            _ => "General",
        };
        self.restyle(Some(code), |_| {});
    }

    pub(super) fn fmt_menu(&mut self) {
        let items = [t!("Genel", "General"), t!("Sayı (1.234,56)", "Number (1,234.56)"), t!("Para birimi", "Currency"), t!("Yüzde", "Percent"), t!("Tarih", "Date")];
        let rows: Vec<Row> = items.iter().enumerate().map(|(i, s)| Row::Item { id: i + 1, text: s.to_string(), icon: Icon::None }).collect();
        if let Some(id) = glassmenu::run(app::hwnd(), rows, None, cursor()) {
            self.num_fmt((id - 1) as u8);
        }
    }

    pub(super) fn color_menu(&mut self) {
        let colors = [(0xFFF2A8, t!("Sarı", "Yellow")), (0xC6EFCE, t!("Yeşil", "Green")), (0xBDD7EE, t!("Mavi", "Blue")), (0xF8CBAD, t!("Turuncu", "Orange")), (0xFFC7CE, t!("Kırmızı", "Red")), (0xE7E6E6, t!("Gri", "Gray"))];
        let text = [(0xC00000, t!("Kırmızı", "Red")), (0x1E7B48, t!("Yeşil", "Green")), (0x2F5597, t!("Mavi", "Blue")), (0x7F7F7F, t!("Gri", "Gray"))];
        let mut rows = vec![Row::Header(t!("Dolgu rengi", "Fill color").into())];
        rows.push(Row::Item { id: 1, text: t!("Dolgu yok", "No fill").into(), icon: Icon::Glyph('\u{E711}', None) });
        for (i, (c, n)) in colors.iter().enumerate() {
            rows.push(Row::Item { id: 10 + i, text: n.to_string(), icon: Icon::Glyph('\u{E91F}', Some(*c)) });
        }
        rows.push(Row::Sep);
        rows.push(Row::Header(t!("Yazı rengi", "Font color").into()));
        rows.push(Row::Item { id: 2, text: t!("Otomatik", "Automatic").into(), icon: Icon::Glyph('\u{E8D3}', None) });
        for (i, (c, n)) in text.iter().enumerate() {
            rows.push(Row::Item { id: 20 + i, text: n.to_string(), icon: Icon::Glyph('\u{E8D3}', Some(*c)) });
        }
        match glassmenu::run(app::hwnd(), rows, None, cursor()) {
            Some(1) => self.restyle(None, |l| l.fill = None),
            Some(2) => self.restyle(None, |l| l.color = None),
            Some(id @ 10..=19) => {
                let c = colors[id - 10].0;
                self.restyle(None, move |l| l.fill = Some(c));
            }
            Some(id @ 20..=29) => {
                let c = text[id - 20].0;
                self.restyle(None, move |l| l.color = Some(c));
            }
            _ => {}
        }
    }

    pub(super) fn context_menu(&mut self) {
        let rows = vec![
            Row::Item { id: 1, text: t!("Kes", "Cut").into(), icon: Icon::Glyph('\u{E8C6}', None) },
            Row::Item { id: 2, text: t!("Kopyala", "Copy").into(), icon: Icon::Glyph('\u{E8C8}', None) },
            Row::Item { id: 3, text: t!("Yapıştır", "Paste").into(), icon: Icon::Glyph('\u{E77F}', None) },
            Row::Sep,
            Row::Item { id: 4, text: t!("İçeriği temizle", "Clear contents").into(), icon: Icon::Glyph('\u{E894}', None) },
            Row::Sep,
            Row::Item { id: 5, text: t!("Sayı biçimi…", "Number format…").into(), icon: Icon::Glyph('\u{E8EF}', None) },
            Row::Item { id: 6, text: t!("Renk…", "Color…").into(), icon: Icon::Glyph('\u{E790}', None) },
        ];
        match glassmenu::run(app::hwnd(), rows, None, cursor()) {
            Some(1) => {
                if self.copy() {
                    self.clear();
                }
            }
            Some(2) => {
                self.copy();
            }
            Some(3) => self.paste(),
            Some(4) => self.clear(),
            Some(5) => self.fmt_menu(),
            Some(6) => self.color_menu(),
            _ => {}
        }
    }

}
