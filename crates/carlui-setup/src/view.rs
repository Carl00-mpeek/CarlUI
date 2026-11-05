//! Bu modül kurucu penceresinin sayfalarını çiziyor: hoş geldin (konum +
//! seçenekler), ilerleme, sonuç ve kaldırma onayı. Ayarlar penceresiyle aynı
//! cam görünüm ve parçalar (`carlui_ui::paint`) kullanılır; her kare baştan
//! çizilir, tıklanabilir alanlar çizilirken kaydedilir.

use crate::{install, payload, space, Act, App, Mode, Opt, Page};
use carlui_ui::{
    paint::{mix, Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    tween,
};
use windows::Win32::Graphics::Direct2D::{Common::D2D_RECT_F, ID2D1Bitmap, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR};

pub type P<'a> = Painter<'a, Act>;

pub const W: f32 = 600.0;
pub const H: f32 = 614.0;
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

/// Seçenek satırı: başlık, açıklama, açık mı, seçilebilir mi, anahtar ve
/// (varsa) seçeneğin diske eklediği bayt.
struct Row<'a>(&'a str, &'a str, bool, bool, Opt, Option<u64>);

/// Başlık + açıklama + anahtar; satırın tamamı tıklanabilir. Boyut verilirse
/// anahtarın solunda yazar ("+3,7 MB").
fn option(p: &mut P, r: R, Row(title, desc, on, enabled, opt, size): Row) {
    let hv = tween::to(tween::key(("opt", r.y as i32)), if enabled && p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    if hv > 0.0 {
        p.round(r, 6.0, 0xFFFFFF, 0.04 * hv);
    }
    let a = if enabled { 1.0 } else { 0.45 };
    let right = if size.is_some() { 160.0 } else { 90.0 };
    p.text(title, R::new(r.x + 16.0, r.y + 8.0, r.w - right, 22.0), 13.5, false, TEXT, 0.95 * a, Align::Left);
    p.text(desc, R::new(r.x + 16.0, r.y + 29.0, r.w - right, 18.0), 11.5, false, TEXT, 0.55 * a, Align::Left);
    if let Some(b) = size {
        let on = on && enabled;
        p.text(&format!("+{}", payload::mb(b)), R::new(r.right() - 16.0 - 34.0 - 12.0 - 80.0, r.y, 80.0, r.h), 12.0, on, TEXT, if on { 0.85 } else { 0.4 } * a, Align::Right);
    }
    p.switch(r.right() - 16.0 - 34.0, r.y + (r.h - 18.0) / 2.0, on && enabled, enabled, Act::Toggle(opt));
    if enabled {
        p.hit(r, Act::Toggle(opt));
    }
}

fn footer_left(p: &P, s: &str) {
    p.text(s, R::new(PAD, H - 58.0, 300.0, 34.0), 12.0, false, TEXT, 0.45, Align::Left);
}

/// İki satırlık alt bilgi: üstte kurulacak boyut, altta güncelleme farkı ya da uyarı.
fn footer_two(p: &P, top: &str, bottom: &str, color: u32, alpha: f32) {
    p.text(top, R::new(PAD, H - 62.0, 300.0, 20.0), 12.0, false, TEXT, 0.45, Align::Left);
    p.text(bottom, R::new(PAD, H - 43.0, 300.0, 20.0), 11.5, false, color, alpha, Align::Left);
}

/// Basılamayan ana düğme (yer yetmediğinde).
fn primary_off(p: &P, r: R, label: &str) {
    p.round(r, 6.0, 0xFFFFFF, 0.08);
    p.text(label, r, 13.5, true, TEXT, 0.35, Align::Center);
}

impl App {
    pub fn draw(&self, p: &mut P, logo: Option<&ID2D1Bitmap>) {
        let sub = match self.mode {
            Mode::Install => t!(format!("Sürüm {} · Windows 11 için kabuk", env!("CARGO_PKG_VERSION")), format!("Version {} · a shell for Windows 11", env!("CARGO_PKG_VERSION"))),
            Mode::Uninstall => t!("Kaldırma", "Uninstall").to_string(),
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
        p.text(t!("Kurulum konumu", "Install location"), R::new(PAD + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        let r = R::new(PAD, y + 24.0, cw, 46.0);
        p.card(r);
        p.glyph('\u{E8B7}', R::new(r.x + 10.0, r.y, 26.0, r.h), 15.0, TEXT, 0.75);
        p.text(&self.opts.dir.display().to_string(), R::new(r.x + 40.0, r.y, r.w - 160.0, r.h), 13.0, false, TEXT, 0.92, Align::Left);
        p.button(R::new(r.right() - 108.0, r.y + 7.0, 100.0, 32.0), None, t!("Değiştir…", "Change…"), Act::Browse, true, false);

        let y = y + 88.0;
        p.text(t!("Seçenekler", "Options"), R::new(PAD + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        let card = R::new(PAD, y + 24.0, cw, 52.0 * 6.0);
        p.card(card);
        let pawn = self.pawnio;
        let xpi = payload::XPI.is_some();
        let rows = [
            Row(t!("Masaüstü kısayolu", "Desktop shortcut"), t!("Başlat menüsüne her durumda eklenir", "Always added to the Start menu"), self.opts.desktop, true, Opt::Desktop, None),
            Row(t!("Kurulunca CarlUI'yi başlat", "Start CarlUI after install"), t!("Açılışta başlama kaydı bu konuma taşınır", "The startup entry points to this location"), self.opts.launch, true, Opt::Launch, None),
            Row(t!("CPU sıcaklığı", "CPU temperature"), if pawn { t!("Yönetici izni ister · açılışta çalışan küçük bir görev kurar", "Needs admin · sets up a small task that runs at sign-in") } else { t!("PawnIO sürücüsü kurulu değil", "PawnIO driver not installed") }, self.opts.sensors, pawn, Opt::Sensors, None),
            Row(t!("Firefox eklentisi", "Firefox extension"), if xpi { t!("İmzalı eklenti Firefox'a otomatik tanıtılır", "The signed extension is registered with Firefox") } else { t!("İmzalı paket yok · kurulumdan sonra elle yüklenir", "No signed package · load it manually after install") }, self.opts.firefox, xpi, Opt::Firefox, None),
            Row(t!("Carl Office", "Carl Office"), t!("Excel, Word, PowerPoint ve PDF açar · arka planda çalışmaz", "Opens Excel, Word, PowerPoint and PDF · no background process"), self.opts.office, true, Opt::Office, Some(payload::office_bytes())),
            Row(t!("CarlUI duvar kâğıdı", "CarlUI wallpaper"), t!("Siyah zeminde CarlUI yazısı · masaüstü arka planı olur", "CarlUI lettering on black · set as desktop background"), self.opts.wallpaper, true, Opt::Wallpaper, Some(payload::wallpaper_bytes())),
        ];
        for (i, row) in rows.into_iter().enumerate() {
            let rr = R::new(card.x, card.y + 52.0 * i as f32, card.w, 52.0);
            if i > 0 {
                p.fill(R::new(rr.x + 16.0, rr.y, rr.w - 32.0, 1.0), 0xFFFFFF, 0.06);
            }
            option(p, rr, row);
        }

        let total = payload::installed_bytes(&self.opts);
        let top = format!("{}: {} · {}", t!("Kurulacak", "To install"), payload::mb(total), t!("yönetici izni gerekmez", "no admin rights needed"));
        let ok = self.space_ok();
        if !ok {
            let (need, free) = (space::needed(&self.opts, self.now_bytes), self.free.unwrap_or(0));
            footer_two(p, &top, &t!(format!("Diskte yer yok: {} gerekli, {} boş", payload::mb(need), payload::mb(free)), format!("Not enough space: {} needed, {} free", payload::mb(need), payload::mb(free))), RED, 0.95);
        } else if self.installed_before && self.now_bytes > 0 {
            let d = space::diff(total, self.now_bytes);
            footer_two(p, &top, &t!(format!("Bu güncelleme {d} · şu an {}", payload::mb(self.now_bytes)), format!("This update {d} · now {}", payload::mb(self.now_bytes))), TEXT, 0.6);
        } else {
            footer_left(p, &top);
        }
        p.button(R::new(W - PAD - 120.0 - 12.0 - 100.0, H - 58.0, 100.0, 34.0), None, t!("Vazgeç", "Cancel"), Act::Close, true, false);
        let label = if self.installed_before { t!("Güncelle", "Update") } else { t!("Kur", "Install") };
        let r = R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0);
        if ok {
            primary(p, r, label, Act::Install, p.accent);
        } else {
            primary_off(p, r, label);
        }
    }

    fn confirm(&self, p: &mut P, y: f32) {
        let cw = W - PAD * 2.0;
        p.text(t!("CarlUI kaldırılsın mı?", "Uninstall CarlUI?"), R::new(PAD, y, cw, 32.0), 19.0, true, TEXT, 0.95, Align::Left);
        p.text(&format!("{}: {}", t!("Konum", "Location"), self.opts.dir.display()), R::new(PAD, y + 34.0, cw, 20.0), 12.5, false, TEXT, 0.6, Align::Left);
        let lines = [
            t!("Kabuk kapatılır, görev çubuğu geri gelir.", "The shell closes and the taskbar comes back."),
            t!("Başlangıç kaydı, kısayollar ve Programlar listesindeki giriş kaldırılır.", "The startup entry, shortcuts and installed-apps entry are removed."),
            t!("Kurulan dosyalar Geri Dönüşüm Kutusu'na taşınır.", "Installed files are moved to the Recycle Bin."),
        ];
        for (i, l) in lines.iter().enumerate() {
            let ry = y + 70.0 + i as f32 * 26.0;
            p.glyph('\u{E73E}', R::new(PAD, ry, 18.0, 22.0), 11.0, p.accent, 0.9);
            p.text(l, R::new(PAD + 26.0, ry, cw - 26.0, 22.0), 13.0, false, TEXT, 0.85, Align::Left);
        }
        let card = R::new(PAD, y + 160.0, cw, 52.0);
        p.card(card);
        option(p, card, Row(t!("Ayarları ve verileri de kaldır", "Also remove settings and data"), t!(r"%APPDATA%\CarlUI (Carl Vault kasası dahil) ve %LOCALAPPDATA%\CarlUI · Geri Dönüşüm Kutusu'na", r"%APPDATA%\CarlUI (Carl Vault included) and %LOCALAPPDATA%\CarlUI · to the Recycle Bin"), self.purge, true, Opt::Purge, None));

        p.button(R::new(W - PAD - 120.0 - 12.0 - 100.0, H - 58.0, 100.0, 34.0), None, t!("Vazgeç", "Cancel"), Act::Close, true, false);
        primary(p, R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0), t!("Kaldır", "Uninstall"), Act::Uninstall, RED);
    }

    fn work(&self, p: &mut P, y: f32) {
        let (step, frac) = self.progress.lock().map(|g| (g.step.clone(), g.frac)).unwrap_or_default();
        let cw = W - PAD * 2.0;
        let title = if self.mode == Mode::Install { t!("Kuruluyor…", "Installing…") } else { t!("Kaldırılıyor…", "Uninstalling…") };
        p.text(title, R::new(PAD, y + 50.0, cw, 32.0), 19.0, true, TEXT, 0.95, Align::Left);
        p.text(&step, R::new(PAD, y + 88.0, cw, 22.0), 13.0, false, TEXT, 0.65, Align::Left);
        let f = tween::to(tween::key("bar"), frac, 10.0);
        p.bar(R::new(PAD, y + 128.0, cw, 6.0), f, p.accent);
        p.text(&carlui_core::lang::pct((f * 100.0).round()), R::new(PAD, y + 142.0, cw, 20.0), 12.0, false, TEXT, 0.5, Align::Right);
    }

    fn done(&self, p: &mut P, y: f32) {
        let out = self.progress.lock().ok().and_then(|g| g.done.clone()).unwrap_or(install::Outcome { ok: false, notes: vec![] });
        let cw = W - PAD * 2.0;
        let (glyph, color, title) = match (self.mode, out.ok) {
            (Mode::Install, true) => ('\u{E73E}', GREEN, if self.installed_before { t!("CarlUI güncellendi", "CarlUI updated") } else { t!("CarlUI kuruldu", "CarlUI installed") }),
            (Mode::Uninstall, true) => ('\u{E73E}', GREEN, t!("CarlUI kaldırıldı", "CarlUI uninstalled")),
            (Mode::Install, false) => ('\u{E711}', RED, t!("Kurulum tamamlanamadı", "Install didn't complete")),
            (Mode::Uninstall, false) => ('\u{E711}', RED, t!("Kaldırma tamamlanamadı", "Uninstall didn't complete")),
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
            p.text(t!("Tarayıcı eklentisi (isteğe bağlı)", "Browser extension (optional)"), R::new(card.x + 16.0, card.y + 10.0, card.w - 32.0, 22.0), 13.5, false, TEXT, 0.95, Align::Left);
            p.text(t!("İndirmeleri adaya taşır. Klasörü seçip geliştirici modunda yükleyin.", "Moves downloads to the island. Load the folder in developer mode."), R::new(card.x + 16.0, card.y + 31.0, card.w - 32.0, 18.0), 11.5, false, TEXT, 0.55, Align::Left);
            let mut bx = card.x + 16.0;
            let by = card.y + 56.0;
            p.button(R::new(bx, by, 120.0, 30.0), Some('\u{E838}'), t!("Klasörü aç", "Open folder"), Act::OpenExt, true, false);
            bx += 128.0;
            if self.firefox.is_some() {
                p.button(R::new(bx, by, 110.0, 30.0), None, t!("Firefox'ta", "In Firefox"), Act::Firefox, true, false);
                bx += 118.0;
            }
            if let Some((_, url)) = &self.chrome {
                let label = if url.starts_with("edge") { "Edge'de" } else { "Chrome'da" };
                p.button(R::new(bx, by, 110.0, 30.0), None, label, Act::Chrome, true, false);
            }
        }
        if self.mode == Mode::Uninstall && out.ok {
            p.text(t!("Silinenleri geri almak için Geri Dönüşüm Kutusu'nu açabilirsiniz.", "Open the Recycle Bin to restore removed files."), R::new(PAD, ny + 6.0, cw, 20.0), 12.0, false, TEXT, 0.5, Align::Left);
        }
        primary(p, R::new(W - PAD - 120.0, H - 58.0, 120.0, 34.0), t!("Kapat", "Close"), Act::Close, p.accent);
    }
}
