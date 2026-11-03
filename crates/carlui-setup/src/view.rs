//! Bu modül kurucu penceresinin sayfalarını çiziyor: hoş geldin (konum +
//! seçenekler), ilerleme, sonuç ve kaldırma onayı. Ayarlar penceresiyle aynı
//! cam görünüm ve parçalar (`carlui_ui::paint`) kullanılır; her kare baştan
//! çizilir, tıklanabilir alanlar çizilirken kaydedilir.

use crate::{install, payload, Act, App, Mode, Opt, Page};
use carlui_ui::{
    paint::{mix, Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    tween,
};
use windows::Win32::Graphics::Direct2D::{Common::D2D_RECT_F, ID2D1Bitmap, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR};

pub type P<'a> = Painter<'a, Act>;

pub const W: f32 = 600.0;
pub const H: f32 = 562.0;
const PAD: f32 = 32.0;

/// Vurgu renginde ana düğme (Kur / Kaldır / Kapat).
fn primary(p: &mut P, r: R, label: &str, act: Act, color: u32) {
    let h = tween::to(tween::key(("pri", r.x as i32, r.y as i32)), if p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    p.round(r, 6.0, mix(color, 0xFFFFFF, 0.12 * h), 1.0);
    let fg = if color == RED { TEXT } else { 0x000000 };
    p.text(label, r, 13.5, true, fg, 0.9, Align::Center);
    p.hit(r, act);
}

fn header(p: &P, logo: Option<&ID2D1Bitmap>, sub: &str) {
    if let Some(b) = logo {
        let d = D2D_RECT_F { left: PAD, top: 30.0, right: PAD + 52.0, bottom: 82.0 };
        unsafe { p.rt.DrawBitmap(b, Some(&d), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
    }
    p.text("CarlUI", R::new(PAD + 68.0, 30.0, 300.0, 30.0), 22.0, true, TEXT, 0.96, Align::Left);
    p.text(sub, R::new(PAD + 68.0, 58.0, W - PAD * 2.0 - 68.0, 22.0), 12.5, false, TEXT, 0.6, Align::Left);
}

/// Başlık + açıklama + anahtar; satırın tamamı tıklanabilir.
fn option(p: &mut P, r: R, title: &str, desc: &str, on: bool, enabled: bool, opt: Opt) {
    let hv = tween::to(tween::key(("opt", r.y as i32)), if enabled && p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    if hv > 0.0 {
        p.round(r, 6.0, 0xFFFFFF, 0.04 * hv);
    }
    let a = if enabled { 1.0 } else { 0.45 };
    p.text(title, R::new(r.x + 16.0, r.y + 8.0, r.w - 90.0, 22.0), 13.5, false, TEXT, 0.95 * a, Align::Left);
    p.text(desc, R::new(r.x + 16.0, r.y + 29.0, r.w - 90.0, 18.0), 11.5, false, TEXT, 0.55 * a, Align::Left);
    p.switch(r.right() - 16.0 - 34.0, r.y + (r.h - 18.0) / 2.0, on && enabled, enabled, Act::Toggle(opt));
    if enabled {
        p.hit(r, Act::Toggle(opt));
    }
}

fn footer_left(p: &P, s: &str) {
    p.text(s, R::new(PAD, H - 58.0, 300.0, 34.0), 12.0, false, TEXT, 0.45, Align::Left);
}

impl App {
    pub fn draw(&self, p: &mut P, logo: Option<&ID2D1Bitmap>) {
        let sub = match self.mode {
            Mode::Install => format!("Sürüm {} · Windows 11 için kabuk", env!("CARGO_PKG_VERSION")),
            Mode::Uninstall => "Kaldırma".to_string(),
        };
        header(p, logo, &sub);
        let slide = tween::to(tween::key("page"), 0.0, 13.0);
        let y0 = 108.0 + slide;
        match self.page {
            Page::Welcome => self.welcome(p, y0),
            Page::Confirm => self.confirm(p, y0),
            Page::Work => self.work(p, y0),
            Page::Done => self.done(p, y0),
        }
    }

    fn welcome(&self, p: &mut P, y: f32) {
        let cw = W - PAD * 2.0;
        p.text("Kurulum konumu", R::new(PAD + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        let r = R::new(PAD, y + 24.0, cw, 46.0);
        p.card(r);
        p.glyph('\u{E8B7}', R::new(r.x + 10.0, r.y, 26.0, r.h), 15.0, TEXT, 0.75);
        p.text(&self.opts.dir.display().to_string(), R::new(r.x + 40.0, r.y, r.w - 160.0, r.h), 13.0, false, TEXT, 0.92, Align::Left);
        p.button(R::new(r.right() - 108.0, r.y + 7.0, 100.0, 32.0), None, "Değiştir…", Act::Browse, true, false);

        let y = y + 88.0;
        p.text("Seçenekler", R::new(PAD + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        let card = R::new(PAD, y + 24.0, cw, 52.0 * 5.0);
        p.card(card);
        let pawn = self.pawnio;
        let xpi = payload::XPI.is_some();
        let rows: [(&str, &str, bool, bool, Opt); 5] = [
            ("Masaüstü kısayolu", "Başlat menüsüne her durumda eklenir", self.opts.desktop, true, Opt::Desktop),
            ("Kurulunca CarlUI'yi başlat", "Açılışta başlama kaydı bu konuma taşınır", self.opts.launch, true, Opt::Launch),
            ("CPU sıcaklığı", if pawn { "Yönetici izni ister · açılışta çalışan küçük bir görev kurar" } else { "PawnIO sürücüsü kurulu değil" }, self.opts.sensors, pawn, Opt::Sensors),
            ("Firefox eklentisi", if xpi { "İmzalı eklenti Firefox'a otomatik tanıtılır" } else { "İmzalı paket yok · kurulumdan sonra elle yüklenir" }, self.opts.firefox, xpi, Opt::Firefox),
            ("CarlUI duvar kâğıdı", "Siyah zeminde CarlUI yazısı · masaüstü arka planı olur", self.opts.wallpaper, true, Opt::Wallpaper),
        ];
        for (i, (t, d, on, en, opt)) in rows.into_iter().enumerate() {
            let rr = R::new(card.x, card.y + 52.0 * i as f32, card.w, 52.0);
            if i > 0 {
                p.fill(R::new(rr.x + 16.0, rr.y, rr.w - 32.0, 1.0), 0xFFFFFF, 0.06);
            }
            option(p, rr, t, d, on, en, opt);
        }

        footer_left(p, &format!("{:.1} MB · yönetici izni gerekmez", payload::size_kb() as f32 / 1024.0));
        p.button(R::new(W - PAD - 120.0 - 12.0 - 100.0, H - 58.0, 100.0, 34.0), None, "Vazgeç", Act::Close, true, false);
        primary(p, R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0), if self.installed_before { "Güncelle" } else { "Kur" }, Act::Install, p.accent);
    }

    fn confirm(&self, p: &mut P, y: f32) {
        let cw = W - PAD * 2.0;
        p.text("CarlUI kaldırılsın mı?", R::new(PAD, y, cw, 32.0), 19.0, true, TEXT, 0.95, Align::Left);
        p.text(&format!("Konum: {}", self.opts.dir.display()), R::new(PAD, y + 34.0, cw, 20.0), 12.5, false, TEXT, 0.6, Align::Left);
        let lines = [
            "Kabuk kapatılır, görev çubuğu geri gelir.",
            "Başlangıç kaydı, kısayollar ve Programlar listesindeki giriş kaldırılır.",
            "Kurulan dosyalar Geri Dönüşüm Kutusu'na taşınır.",
        ];
        for (i, l) in lines.iter().enumerate() {
            let ry = y + 70.0 + i as f32 * 26.0;
            p.glyph('\u{E73E}', R::new(PAD, ry, 18.0, 22.0), 11.0, p.accent, 0.9);
            p.text(l, R::new(PAD + 26.0, ry, cw - 26.0, 22.0), 13.0, false, TEXT, 0.85, Align::Left);
        }
        let card = R::new(PAD, y + 160.0, cw, 52.0);
        p.card(card);
        option(p, card, "Ayarları ve verileri de kaldır", r"%APPDATA%\CarlUI ve %LOCALAPPDATA%\CarlUI · Geri Dönüşüm Kutusu'na", self.purge, true, Opt::Purge);

        p.button(R::new(W - PAD - 120.0 - 12.0 - 100.0, H - 58.0, 100.0, 34.0), None, "Vazgeç", Act::Close, true, false);
        primary(p, R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0), "Kaldır", Act::Uninstall, RED);
    }

    fn work(&self, p: &mut P, y: f32) {
        let (step, frac) = self.progress.lock().map(|g| (g.step.clone(), g.frac)).unwrap_or_default();
        let cw = W - PAD * 2.0;
        let title = if self.mode == Mode::Install { "Kuruluyor…" } else { "Kaldırılıyor…" };
        p.text(title, R::new(PAD, y + 50.0, cw, 32.0), 19.0, true, TEXT, 0.95, Align::Left);
        p.text(&step, R::new(PAD, y + 88.0, cw, 22.0), 13.0, false, TEXT, 0.65, Align::Left);
        let f = tween::to(tween::key("bar"), frac, 10.0);
        p.bar(R::new(PAD, y + 128.0, cw, 6.0), f, p.accent);
        p.text(&format!("%{}", (f * 100.0).round() as i32), R::new(PAD, y + 142.0, cw, 20.0), 12.0, false, TEXT, 0.5, Align::Right);
    }

    fn done(&self, p: &mut P, y: f32) {
        let out = self.progress.lock().ok().and_then(|g| g.done.clone()).unwrap_or(install::Outcome { ok: false, notes: vec![] });
        let cw = W - PAD * 2.0;
        let (glyph, color, title) = match (self.mode, out.ok) {
            (Mode::Install, true) => ('\u{E73E}', GREEN, if self.installed_before { "CarlUI güncellendi" } else { "CarlUI kuruldu" }),
            (Mode::Uninstall, true) => ('\u{E73E}', GREEN, "CarlUI kaldırıldı"),
            (Mode::Install, false) => ('\u{E711}', RED, "Kurulum tamamlanamadı"),
            (Mode::Uninstall, false) => ('\u{E711}', RED, "Kaldırma tamamlanamadı"),
        };
        let c = R::new(PAD, y, 34.0, 34.0);
        p.round(c, 17.0, color, 0.18);
        p.glyph(glyph, c, 15.0, color, 1.0);
        p.text(title, R::new(PAD + 46.0, y, cw - 46.0, 34.0), 19.0, true, TEXT, 0.95, Align::Left);
        let mut ny = y + 50.0;
        if self.mode == Mode::Install && out.ok {
            p.text(&self.opts.dir.display().to_string(), R::new(PAD, ny, cw, 20.0), 12.5, false, TEXT, 0.6, Align::Left);
            ny += 30.0;
        }
        for n in out.notes.iter().take(5) {
            p.glyph('\u{E946}', R::new(PAD, ny, 18.0, 22.0), 12.0, ORANGE, 0.9);
            p.text(n, R::new(PAD + 26.0, ny, cw - 26.0, 22.0), 12.5, false, TEXT, 0.82, Align::Left);
            ny += 26.0;
        }

        // Eklenti Firefox'a tanıtılmadıysa elle yükleme yolları.
        if self.mode == Mode::Install && out.ok && !(self.opts.firefox && payload::XPI.is_some()) {
            let card = R::new(PAD, (ny + 12.0).max(y + 150.0), cw, 98.0);
            p.card(card);
            p.text("Tarayıcı eklentisi (isteğe bağlı)", R::new(card.x + 16.0, card.y + 10.0, card.w - 32.0, 22.0), 13.5, false, TEXT, 0.95, Align::Left);
            p.text("İndirmeleri adaya taşır. Klasörü seçip geliştirici modunda yükleyin.", R::new(card.x + 16.0, card.y + 31.0, card.w - 32.0, 18.0), 11.5, false, TEXT, 0.55, Align::Left);
            let mut bx = card.x + 16.0;
            let by = card.y + 56.0;
            p.button(R::new(bx, by, 120.0, 30.0), Some('\u{E838}'), "Klasörü aç", Act::OpenExt, true, false);
            bx += 128.0;
            if self.firefox.is_some() {
                p.button(R::new(bx, by, 110.0, 30.0), None, "Firefox'ta", Act::Firefox, true, false);
                bx += 118.0;
            }
            if let Some((_, url)) = &self.chrome {
                let label = if url.starts_with("edge") { "Edge'de" } else { "Chrome'da" };
                p.button(R::new(bx, by, 110.0, 30.0), None, label, Act::Chrome, true, false);
            }
        }
        if self.mode == Mode::Uninstall && out.ok {
            p.text("Silinenleri geri almak için Geri Dönüşüm Kutusu'nu açabilirsiniz.", R::new(PAD, ny + 6.0, cw, 20.0), 12.0, false, TEXT, 0.5, Align::Left);
        }
        primary(p, R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0), "Kapat", Act::Close, p.accent);
    }
}
