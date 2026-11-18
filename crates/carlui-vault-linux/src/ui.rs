//! Kasa penceresi (egui). Ekranlar: ilk kurulum, kilit açma (parola ya da
//! kurtarma anahtarı), kayıt listesi ve düzenleyici, ayarlar. Tarayıcı
//! köprüsünden gelen onaylar (eklenti eşleştirmesi, passkey) üstte kart olarak
//! görünür.

use crate::clipboard::Clip;
use crate::core::{AskKind, PasskeyKind, Shared};
use carlui_vault::{
    card,
    crypto::KdfParams,
    entry::{now, Body, Category, Field, FieldKind, Meta},
    export, gen, t,
    totp,
    vault::{Vault, MAX_FILE},
};
use eframe::egui::{self, Color32, RichText, ViewportCommand};
use std::{
    collections::HashSet,
    sync::Arc,
    time::Duration,
};
use zeroize::Zeroizing;

const MIN_PASSWORD: usize = 8;

// ------------------------------------------------------------ görünüm
// Windows'taki Carl Vault'un renkleri ve ölçüleri.

const ACCENT: Color32 = Color32::from_rgb(0x4F, 0x8B, 0xFF);
const ACCENT2: Color32 = Color32::from_rgb(0x8B, 0x5C, 0xF6);
const GREEN: Color32 = Color32::from_rgb(0x6C, 0xCB, 0x5F);
const ORANGE: Color32 = Color32::from_rgb(0xFF, 0xB4, 0x54);
const RED: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x16);
const SIDEBAR: Color32 = Color32::from_rgb(0x17, 0x17, 0x1C);
const LIST: Color32 = Color32::from_rgb(0x15, 0x15, 0x1A);
const CARD: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x25);
const INPUT: Color32 = Color32::from_rgb(0x26, 0x26, 0x2E);
const LINE: Color32 = Color32::from_rgb(0x2C, 0x2C, 0x34);
const TEXT: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xF1);
const DIM: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA6);
const CARD_W: f32 = 460.0;

fn style(ctx: &egui::Context) {
    use egui::{FontId, Stroke, TextStyle};
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut(|s| {
        s.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(22.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.menu_margin = egui::Margin::same(8);
        let v = &mut s.visuals;
        v.override_text_color = Some(TEXT);
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.window_stroke = Stroke::new(1.0_f32, LINE);
        v.window_corner_radius = 12.into();
        v.menu_corner_radius = 10.into();
        v.extreme_bg_color = INPUT;
        v.faint_bg_color = CARD;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = ORANGE;
        v.error_fg_color = RED;
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = 8.into();
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x2A, 0x2A, 0x33);
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x2A, 0x2A, 0x33);
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x3E);
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x34, 0x34, 0x3E);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x44, 0x44, 0x50));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x3C, 0x3C, 0x48);
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        v.widgets.open.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x3E);
    });
}

/// Ortalanmış kilit ekranı kartı: simge, başlık, açıklama ve içerik.
fn lock_card(ui: &mut egui::Ui, title: &str, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let pad = ((ui.available_width() - CARD_W) / 2.0).max(0.0);
        ui.add_space((ui.available_height() * 0.12).clamp(16.0, 90.0));
        ui.horizontal(|ui| {
            ui.add_space(pad);
            egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(32).show(ui, |ui| {
                ui.set_width(CARD_W - 64.0);
                ui.vertical(|ui| {
                    lock_icon(ui);
                    ui.add_space(6.0);
                    ui.label(RichText::new(title).size(20.0).strong());
                    if !text.is_empty() {
                        ui.label(RichText::new(text).size(13.0).color(DIM));
                    }
                    ui.add_space(10.0);
                    add(ui);
                });
            });
        });
        ui.add_space(24.0);
    });
}

/// 44 piksellik vurgu renkli kilit simgesi.
fn lock_icon(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
    let p = ui.painter();
    gradient_rect(p, r, 12);
    let c = r.center();
    let body = egui::Rect::from_center_size(c + egui::vec2(0.0, 4.0), egui::vec2(18.0, 13.0));
    p.rect_filled(body, 3, Color32::WHITE);
    let pts: Vec<egui::Pos2> = (0..=16).map(|i| {
        let a = std::f32::consts::PI * (i as f32 / 16.0);
        egui::pos2(c.x - 5.5 * a.cos(), c.y - 2.0 - 6.0 * a.sin())
    }).collect();
    let mut shackle = vec![egui::pos2(c.x - 5.5, c.y)];
    shackle.extend(pts);
    shackle.push(egui::pos2(c.x + 5.5, c.y));
    p.add(egui::Shape::line(shackle, egui::Stroke::new(2.4_f32, Color32::WHITE)));
}

/// Mavi → mor geçişli yuvarlak dikdörtgen.
fn gradient_rect(p: &egui::Painter, r: egui::Rect, radius: u8) {
    p.rect_filled(r, radius, ACCENT);
    let mut mesh = egui::Mesh::default();
    let t = Color32::TRANSPARENT;
    let a = ACCENT2.gamma_multiply(0.9);
    let inner = r.shrink(radius as f32 * 0.3);
    mesh.colored_vertex(inner.left_top(), t);
    mesh.colored_vertex(inner.right_top(), a.gamma_multiply(0.5));
    mesh.colored_vertex(inner.right_bottom(), a);
    mesh.colored_vertex(inner.left_bottom(), a.gamma_multiply(0.5));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
}

/// Tam genişlikte, 38 piksellik giriş kutusu.
fn input<'a>(text: &'a mut dyn egui::TextBuffer, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 10))
}

fn password_edit(ui: &mut egui::Ui, value: &mut Zeroizing<String>, hint: &str) -> egui::Response {
    ui.add(input(&mut **value, hint).password(true))
}

/// Vurgu renkli birincil düğme.
fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(Color32::WHITE)).fill(ACCENT)
}

/// Tam genişlikte, 40 piksellik birincil düğme.
fn primary_wide(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized([ui.available_width(), 40.0], primary(text))
}

/// Çerçevesiz, kenarlığı ince ikincil düğme.
fn ghost(text: &str) -> egui::Button<'_> {
    egui::Button::new(text).fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0_f32, LINE))
}

fn error_text(ui: &mut egui::Ui, msg: &str) {
    if !msg.is_empty() {
        ui.label(RichText::new(msg).color(RED).size(13.0));
    }
}

/// Ayarlar ve düzenleyicideki bölüm kartı.
fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(20).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(15.0).strong());
        ui.add_space(4.0);
        add(ui);
    });
    ui.add_space(12.0);
}

fn cat_color(c: Category) -> Color32 {
    match c {
        Category::Password => ACCENT,
        Category::Email => Color32::from_rgb(0x3F, 0xB6, 0xC8),
        Category::Card => ORANGE,
        Category::Identity => GREEN,
        Category::Note => Color32::from_rgb(0xE8, 0xC5, 0x47),
        Category::License => ACCENT2,
        Category::TwoFactor => Color32::from_rgb(0xF0, 0x6B, 0xB0),
        Category::File => Color32::from_rgb(0x8E, 0x9A, 0xAF),
    }
}

fn cat_short(c: Category) -> &'static str {
    match c {
        Category::Password => "••",
        Category::Email => "@",
        Category::Card => "▭",
        Category::Identity => "ID",
        Category::Note => "¶",
        Category::License => "#",
        Category::TwoFactor => "2F",
        Category::File => "⎘",
    }
}

/// Kategori rengindeki yuvarlak kare rozet.
fn chip(ui: &mut egui::Ui, c: Category, size: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_chip(ui.painter(), r, c);
}

fn paint_chip(p: &egui::Painter, r: egui::Rect, c: Category) {
    let col = cat_color(c);
    p.rect_filled(r, (r.width() * 0.28) as u8, col.gamma_multiply(0.22));
    p.text(r.center(), egui::Align2::CENTER_CENTER, cat_short(c), egui::FontId::proportional(r.width() * 0.4), col);
}

/// Kenar çubuğu satırı: ad solda, sayı sağda.
fn nav_row(ui: &mut egui::Ui, selected: bool, label: &str, count: Option<usize>) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, 8, ACCENT.gamma_multiply(0.18));
        p.rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(0.0, 9.0), egui::vec2(3.0, 16.0)), 2, ACCENT);
    } else if resp.hovered() {
        p.rect_filled(r, 8, Color32::from_white_alpha(10));
    }
    let col = if selected { TEXT } else { Color32::from_rgb(0xC4, 0xC4, 0xCE) };
    p.text(r.left_center() + egui::vec2(14.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), col);
    if let Some(n) = count {
        p.text(r.right_center() - egui::vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(12.0), DIM);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Liste satırı: rozet, ad ve "kategori · tarih".
fn item_row(ui: &mut egui::Ui, selected: bool, name: &str, cat: Category, updated: u64) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, 10, ACCENT.gamma_multiply(0.18));
    } else if resp.hovered() {
        p.rect_filled(r, 10, Color32::from_white_alpha(8));
    }
    let chip_r = egui::Rect::from_min_size(r.min + egui::vec2(10.0, 11.0), egui::vec2(36.0, 36.0));
    paint_chip(p, chip_r, cat);
    let x = chip_r.right() + 12.0;
    let galley = p.layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), TEXT);
    let clip = egui::Rect::from_min_max(egui::pos2(x, r.top()), egui::pos2(r.right() - 8.0, r.bottom()));
    p.with_clip_rect(clip).galley(egui::pos2(x, r.top() + 11.0), galley, TEXT);
    p.with_clip_rect(clip).text(egui::pos2(x, r.top() + 31.0), egui::Align2::LEFT_TOP, format!("{} · {}", cat.title(), date(updated)), egui::FontId::proportional(12.0), DIM);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Unix saniyesini gg.aa.yyyy yazar (yalnızca gün).
fn date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Howard Hinnant'ın civil_from_days algoritması.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{d:02}.{m:02}.{y}")
}

/// Üç parçalı parola gücü çubuğu.
fn strength_bar(ui: &mut egui::Ui, pw: &str) {
    let (n, col, txt) = match gen::strength(pw) {
        gen::Strength::Weak => (1, RED, t!("Zayıf", "Weak")),
        gen::Strength::Fair => (2, ORANGE, t!("Orta", "Fair")),
        gen::Strength::Strong => (3, GREEN, t!("Güçlü", "Strong")),
    };
    ui.horizontal(|ui| {
        let w = 46.0;
        for i in 0..3 {
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, 4.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 2, if i < n { col } else { LINE });
        }
        ui.label(RichText::new(txt).size(12.0).color(col));
    });
}

fn small_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(DIM));
}

struct Editor {
    id: Option<String>,
    name: String,
    category: Category,
    body: Body,
    reveal: HashSet<usize>,
    confirm_delete: bool,
}

impl Drop for Editor {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.name.zeroize();
    }
}

#[derive(PartialEq)]
enum Pane {
    Items,
    Settings,
}

struct Restore {
    path: String,
    data: Vec<u8>,
    password: Zeroizing<String>,
}

pub struct App {
    shared: Arc<Shared>,
    clip: Clip,
    quitting: bool,
    // kilit ekranı
    pw: Zeroizing<String>,
    pw2: Zeroizing<String>,
    use_recovery: bool,
    error: String,
    /// Kurtarma anahtarıyla açıldı: yeni parola konmadan devam edilmez.
    must_reset: bool,
    /// Yeni oluşturulan kurtarma anahtarı (bir kez gösterilir).
    recovery: Option<Zeroizing<String>>,
    // liste
    generation: u64,
    items: Vec<(String, String, Category, u64)>,
    search: String,
    filter: Option<Category>,
    editor: Option<Editor>,
    pane: Pane,
    // ayarlar
    old_pw: Zeroizing<String>,
    new_pw: Zeroizing<String>,
    new_pw2: Zeroizing<String>,
    settings_msg: String,
    restore: Option<Restore>,
    gen_len: usize,
    gen_symbols: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>, minimized: bool) -> Self {
        if minimized {
            cc.egui_ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        let _ = shared.ctx.set(cc.egui_ctx.clone());
        style(&cc.egui_ctx);
        Self {
            shared,
            clip: Clip::default(),
            quitting: false,
            pw: Default::default(),
            pw2: Default::default(),
            use_recovery: false,
            error: String::new(),
            must_reset: false,
            recovery: None,
            generation: u64::MAX,
            items: Vec::new(),
            search: String::new(),
            filter: None,
            editor: None,
            pane: Pane::Items,
            old_pw: Default::default(),
            new_pw: Default::default(),
            new_pw2: Default::default(),
            settings_msg: String::new(),
            restore: None,
            gen_len: 20,
            gen_symbols: true,
        }
    }

    fn toast(&self, msg: impl Into<String>) {
        self.shared.core().toast(msg);
    }

    fn copy(&mut self, text: &str) {
        let secs = self.shared.core().settings.clipboard;
        if self.clip.copy(text, secs) {
            let tail = if secs > 0 { t!(format!(" ({secs} sn sonra silinir)"), format!(" (cleared in {secs} s)")) } else { String::new() };
            self.toast(t!(format!("Kopyalandı{tail}"), format!("Copied{tail}")));
        } else {
            self.toast(t!("Panoya yazılamadı.", "Couldn't copy."));
        }
    }

    fn lock(&mut self) {
        self.editor = None;
        self.items.clear();
        self.restore = None;
        self.shared.lock();
    }

    fn refresh(&mut self) {
        let c = self.shared.core();
        if c.generation == self.generation {
            return;
        }
        self.generation = c.generation;
        self.items = match c.vault.as_ref().filter(|v| v.is_open()) {
            Some(v) => {
                let mut l: Vec<_> = v.items().iter().map(|i| (i.id.clone(), i.meta.name.clone(), i.meta.category, i.meta.updated)).collect();
                l.sort_by_key(|a| a.1.to_lowercase());
                l
            }
            None => Vec::new(),
        };
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Kapatma: ayara göre simge durumuna küçülür, köprü açık kalır.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.shared.core().settings.keep_running {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            } else {
                self.quitting = true;
            }
        }
        if self.quitting {
            self.clip.clear_now();
            self.shared.lock();
        }
        if ctx.input(|i| !i.events.is_empty()) {
            self.shared.core().touch();
        }
        if let Some(d) = self.clip.tick() {
            ctx.request_repaint_after(d);
        }

        let (exists, open) = {
            let c = self.shared.core();
            (c.vault.is_some(), c.is_open())
        };
        if !open {
            self.editor = None;
            self.items.clear();
            self.generation = u64::MAX;
            self.must_reset &= false;
        }

        self.asks(ctx);
        self.toasts(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if !exists {
                self.setup(ui);
            } else if !open {
                self.unlock(ui);
            } else if let Some(key) = self.recovery.clone() {
                self.show_recovery(ui, &key);
            } else if self.must_reset {
                self.reset_password(ui);
            }
        });
        if open && self.recovery.is_none() && !self.must_reset {
            self.refresh();
            self.main(ctx);
        }
        // TOTP kodları ve kart süreleri için saniyede bir.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.clip.clear_now();
        self.shared.lock();
    }
}

// ------------------------------------------------------------ onay kartları

impl App {
    fn asks(&mut self, ctx: &egui::Context) {
        let mut answers: Vec<(u64, Option<usize>)> = Vec::new();
        {
            let asks = self.shared.asks();
            let visible: Vec<_> = asks.list.iter().filter(|a| !a.answered).collect();
            if visible.is_empty() {
                return;
            }
            egui::TopBottomPanel::top("asks").frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 4))).show_separator_line(false).show(ctx, |ui| {
                for a in visible {
                    ui.add_space(8.0);
                    egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.6))).corner_radius(12).inner_margin(16).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let left = crate::core::ASK_TTL.saturating_sub(a.at.elapsed()).as_secs();
                        match &a.kind {
                            AskKind::Pair(who) => {
                                ui.label(RichText::new(t!("Eklenti bağlantısı", "Extension connection")).size(15.0).strong());
                                ui.label(RichText::new(crate::pairing::prompt(who)).color(DIM));
                                ui.horizontal(|ui| {
                                    if ui.add(primary(t!("Onayla", "Approve"))).clicked() {
                                        answers.push((a.id, Some(0)));
                                    }
                                    if ui.add(ghost(t!("Hayır", "No"))).clicked() {
                                        answers.push((a.id, None));
                                    }
                                    small_label(ui, &format!("{left} sn"));
                                });
                            }
                            AskKind::Passkey(p) => {
                                match &p.kind {
                                    PasskeyKind::Create { user_name, .. } => {
                                        ui.label(RichText::new(t!(format!("{} için passkey oluşturulsun mu?", p.site()), format!("Create a passkey for {}?", p.site()))).size(15.0).strong());
                                        ui.label(format!("{}: {user_name}", t!("Kullanıcı", "User")));
                                        let target = match p.choices.first() {
                                            Some(c) => t!(format!("“{}” kaydına eklenecek.", c.name), format!("Will be added to “{}”.", c.name)),
                                            None => t!("Yeni bir giriş kaydı oluşturulacak.", "A new login item will be created.").to_string(),
                                        };
                                        ui.weak(target);
                                        ui.horizontal(|ui| {
                                            if ui.add(primary(t!("Oluştur", "Create"))).clicked() {
                                                answers.push((a.id, Some(0)));
                                            }
                                            if ui.add(ghost(t!("Tarayıcıya bırak", "Let the browser handle it"))).clicked() {
                                                answers.push((a.id, None));
                                            }
                                            small_label(ui, &format!("{left} sn"));
                                        });
                                    }
                                    PasskeyKind::Get { .. } => {
                                        ui.label(RichText::new(t!(format!("{} passkey ile giriş istiyor", p.site()), format!("{} wants to sign in with a passkey", p.site()))).size(15.0).strong());
                                        for (i, c) in p.choices.iter().enumerate() {
                                            if ui.add(primary(&format!("{} · {}", c.name, c.user))).clicked() {
                                                answers.push((a.id, Some(i)));
                                            }
                                        }
                                        ui.horizontal(|ui| {
                                            if ui.add(ghost(t!("Tarayıcıya bırak", "Let the browser handle it"))).clicked() {
                                                answers.push((a.id, None));
                                            }
                                            small_label(ui, &format!("{left} sn"));
                                        });
                                    }
                                }
                            }
                        }
                    });
                }
                ui.add_space(6.0);
            });
        }
        if !answers.is_empty() {
            let mut asks = self.shared.asks();
            for (id, ans) in answers {
                if let Some(a) = asks.list.iter_mut().find(|a| a.id == id) {
                    a.answered = true;
                    let _ = a.tx.send(ans);
                }
            }
            // Pencere yanıtlandı; tarayıcıya dönülür (odak Wayland'de derleyiciye bağlı).
            if !asks.list.iter().any(|a| !a.answered) {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        }
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let msg = {
            let mut c = self.shared.core();
            match &c.toast {
                Some((_, at)) if at.elapsed() > Duration::from_secs(4) => {
                    c.toast = None;
                    None
                }
                Some((m, _)) => Some(m.clone()),
                None => None,
            }
        };
        if let Some(m) = msg {
            egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0)).order(egui::Order::Foreground).interactable(false).show(ctx, |ui| {
                egui::Frame::new().fill(Color32::from_rgb(0x2A, 0x2A, 0x33)).stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(0x3A, 0x3A, 0x46))).corner_radius(10).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                    ui.label(m);
                });
            });
        }
    }
}

// ------------------------------------------------------------ kurulum ve kilit

impl App {
    fn setup(&mut self, ui: &mut egui::Ui) {
        let mut create = false;
        lock_card(ui, t!("Carl Vault'a hoş geldin", "Welcome to Carl Vault"), t!("Kasanı korumak için bir ana parola belirle.", "Choose a master password to protect your vault."), |ui| {
            password_edit(ui, &mut self.pw, t!("Ana parola", "Master password"));
            let enter = password_edit(ui, &mut self.pw2, t!("Ana parola (tekrar)", "Master password (again)")).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !self.pw.is_empty() {
                strength_bar(ui, &self.pw);
            }
            ui.label(RichText::new(t!("Ana parola unutulursa kasa açılamaz. Birazdan bir kurtarma anahtarı gösterilecek.", "If you forget the master password the vault can't be opened. A recovery key is shown next.")).size(13.0).color(ORANGE));
            error_text(ui, &self.error);
            ui.add_space(4.0);
            create = primary_wide(ui, t!("Kasayı oluştur", "Create vault")).clicked() || enter;
            ui.add_space(8.0);
            ui.label(RichText::new(t!(
                "Windows'taki kasayı taşımak için kasa klasörünü ~/.local/share/CarlUI/vault altına kopyala ya da kasayı oluşturup Ayarlar'dan .carlvault yedeğini geri yükle.",
                "To move your Windows vault, copy the vault folder to ~/.local/share/CarlUI/vault, or create a vault and restore a .carlvault backup from Settings."
            )).size(12.0).color(DIM));
        });
        if create {
            self.create();
        }
    }

    fn create(&mut self) {
        self.error.clear();
        if self.pw.chars().count() < MIN_PASSWORD {
            self.error = t!(format!("Parola en az {MIN_PASSWORD} karakter olmalı."), format!("The password must be at least {MIN_PASSWORD} characters."));
            return;
        }
        if *self.pw != *self.pw2 {
            self.error = t!("Parolalar aynı değil.", "The passwords don't match.").into();
            return;
        }
        let mut c = self.shared.core();
        let dir = c.dir.clone();
        let made = KdfParams::new().and_then(|k| Vault::create(&dir, &self.pw, k));
        match made {
            Ok(mut v) => {
                self.recovery = v.new_recovery().ok();
                c.vault = Some(v);
                c.touch();
                c.changed();
                self.pw = Default::default();
                self.pw2 = Default::default();
            }
            Err(e) => self.error = e.to_string(),
        }
    }

    fn unlock(&mut self, ui: &mut egui::Ui) {
        let waiting = self.shared.asks().waiting_unlock.clone();
        let wait = self.shared.core().throttle.left(now());
        let has_recovery = self.shared.core().vault.as_ref().is_some_and(|v| v.has_recovery());
        let (title, text) = if self.use_recovery {
            (t!("Kurtarma anahtarıyla aç", "Open with the recovery key"), t!("Kasayı oluştururken kaydettiğin anahtarı yaz. Sonra yeni bir ana parola belirleyeceksin.", "Type the key you saved when you created the vault. You'll then choose a new master password."))
        } else {
            (t!("Kasa kilitli", "Vault locked"), t!("Devam etmek için ana parolanı yaz.", "Type your master password to continue."))
        };
        let (mut unlock, mut toggle, mut quit) = (false, false, false);
        lock_card(ui, title, text, |ui| {
            if let Some(site) = &waiting {
                ui.label(RichText::new(t!(format!("{site} passkey istiyor. Devam etmek için kasayı aç."), format!("{site} is asking for a passkey. Unlock the vault to continue."))).size(13.0).color(ACCENT));
            }
            let r = if self.use_recovery {
                ui.add(input(&mut *self.pw, t!("Kurtarma anahtarı", "Recovery key")).font(egui::TextStyle::Monospace))
            } else {
                password_edit(ui, &mut self.pw, t!("Ana parola", "Master password"))
            };
            if self.pw.is_empty() && self.error.is_empty() {
                r.request_focus();
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            error_text(ui, &self.error);
            ui.add_space(4.0);
            match wait {
                Some(s) => {
                    ui.label(RichText::new(t!(format!("Çok fazla yanlış deneme. {s} sn bekle."), format!("Too many wrong attempts. Wait {s} s."))).color(ORANGE));
                }
                None => unlock = primary_wide(ui, t!("Kilidi aç", "Unlock")).clicked() || enter,
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if has_recovery {
                    let label = if self.use_recovery { t!("Ana parolayla aç", "Use the master password") } else { t!("Parolamı unuttum", "Forgot password") };
                    toggle = ui.link(label).clicked();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    quit = ui.add(egui::Button::new(RichText::new(t!("Çık", "Quit")).color(DIM)).frame(false)).clicked();
                });
            });
        });
        if unlock {
            self.try_unlock();
        }
        if toggle {
            self.use_recovery = !self.use_recovery;
            self.pw = Default::default();
            self.error.clear();
        }
        if quit {
            self.quitting = true;
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn try_unlock(&mut self) {
        self.error.clear();
        let mut c = self.shared.core();
        let dir = c.dir.clone();
        let Some(v) = c.vault.as_mut() else { return };
        let r = if self.use_recovery { v.unlock_recovery(self.pw.trim()) } else { v.unlock(&self.pw) };
        match r {
            Ok(()) => {
                c.throttle.reset(&dir);
                c.touch();
                c.changed();
                self.must_reset = self.use_recovery;
                self.use_recovery = false;
            }
            Err(e) => {
                eprintln!("kasa: kilit açılamadı");
                c.throttle.fail(&dir, now());
                self.error = e.to_string();
            }
        }
        self.pw = Default::default();
    }

    fn show_recovery(&mut self, ui: &mut egui::Ui, key: &str) {
        let (mut copy, mut done) = (false, false);
        lock_card(ui, t!("Kurtarma anahtarın", "Your recovery key"), t!(
            "Ana parolayı unutursan kasayı bu anahtarla açabilirsin.",
            "If you forget the master password you can open the vault with this key."
        ), |ui| {
            // Anahtar iki satıra bölünür (Windows'taki gibi).
            let groups: Vec<&str> = key.split('-').collect();
            let half = groups.len().div_ceil(2);
            let shown = if groups.len() > 2 { format!("{}\n{}", groups[..half].join("-"), groups[half..].join("-")) } else { key.to_string() };
            egui::Frame::new().fill(INPUT).stroke(egui::Stroke::new(1.0_f32, ACCENT)).corner_radius(10).inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| ui.label(RichText::new(shown).monospace().size(17.0).color(TEXT)));
            });
            ui.label(RichText::new(t!("Bir kâğıda yaz ya da güvenli bir yerde sakla. Bir daha gösterilmeyecek.", "Write it down or keep it somewhere safe. It won't be shown again.")).size(13.0).color(ORANGE));
            ui.add_space(4.0);
            ui.columns(2, |c| {
                copy = c[0].add_sized([c[0].available_width(), 40.0], ghost(t!("Kopyala", "Copy"))).clicked();
                done = primary_wide(&mut c[1], t!("Kaydettim", "I've saved it")).clicked();
            });
        });
        if copy {
            self.copy(key);
        }
        if done {
            self.recovery = None;
        }
    }

    fn reset_password(&mut self, ui: &mut egui::Ui) {
        let mut save = false;
        lock_card(ui, t!("Yeni ana parola", "New master password"), t!("Kasa kurtarma anahtarıyla açıldı. Devam etmeden yeni bir ana parola belirle.", "The vault was opened with the recovery key. Choose a new master password to continue."), |ui| {
            password_edit(ui, &mut self.new_pw, t!("Yeni parola", "New password"));
            let enter = password_edit(ui, &mut self.new_pw2, t!("Yeni parola (tekrar)", "New password (again)")).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !self.new_pw.is_empty() {
                strength_bar(ui, &self.new_pw);
            }
            error_text(ui, &self.error);
            ui.add_space(4.0);
            save = primary_wide(ui, t!("Kaydet", "Save")).clicked() || enter;
        });
        if save {
            match self.set_password() {
                Ok(()) => {
                    self.must_reset = false;
                    self.error.clear();
                }
                Err(e) => self.error = e,
            }
        }
    }

    fn set_password(&mut self) -> Result<(), String> {
        if self.new_pw.chars().count() < MIN_PASSWORD {
            return Err(t!(format!("Parola en az {MIN_PASSWORD} karakter olmalı."), format!("The password must be at least {MIN_PASSWORD} characters.")));
        }
        if *self.new_pw != *self.new_pw2 {
            return Err(t!("Parolalar aynı değil.", "The passwords don't match.").into());
        }
        let mut c = self.shared.core();
        let v = c.vault.as_mut().ok_or_default()?;
        v.change_password(&self.new_pw).map_err(|e| e.to_string())?;
        self.new_pw = Default::default();
        self.new_pw2 = Default::default();
        self.old_pw = Default::default();
        c.toast(t!("Ana parola değiştirildi.", "Master password changed."));
        Ok(())
    }
}

trait OkOrDefault<T> {
    fn ok_or_default(self) -> Result<T, String>;
}

impl<T> OkOrDefault<T> for Option<T> {
    fn ok_or_default(self) -> Result<T, String> {
        self.ok_or_else(|| t!("Kasa kilitli.", "The vault is locked.").to_string())
    }
}

// ------------------------------------------------------------ ana ekran

impl App {
    fn new_item(&mut self, cat: Category) {
        let mut body = Body::default();
        body.fields = cat.template();
        self.editor = Some(Editor { id: None, name: String::new(), category: cat, body, reveal: HashSet::new(), confirm_delete: false });
        self.pane = Pane::Items;
    }

    fn main(&mut self, ctx: &egui::Context) {
        // Sol kenar çubuğu: kategoriler, ayarlar, kilit.
        let mut lock = false;
        let mut quit = false;
        egui::SidePanel::left("nav").exact_width(212.0).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(12, 16))).show(ctx, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if nav_row(ui, false, t!("Çık", "Quit"), None) {
                    quit = true;
                }
                if nav_row(ui, false, t!("Kilitle", "Lock"), None) {
                    lock = true;
                }
                if nav_row(ui, self.pane == Pane::Settings, t!("Ayarlar", "Settings"), None) {
                    self.pane = Pane::Settings;
                }
                ui.add_space(6.0);
                ui.separator();
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        gradient_rect(ui.painter(), r, 8);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "C", egui::FontId::proportional(15.0), Color32::WHITE);
                        ui.label(RichText::new("Carl Vault").size(16.0).strong());
                    });
                    ui.add_space(14.0);
                    small_label(ui, t!("KATEGORİLER", "CATEGORIES"));
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let items = self.pane == Pane::Items;
                    if nav_row(ui, items && self.filter.is_none(), t!("Tümü", "All items"), Some(self.items.len())) {
                        self.filter = None;
                        self.pane = Pane::Items;
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for c in Category::ALL {
                            let n = self.items.iter().filter(|i| i.2 == c).count();
                            if nav_row(ui, items && self.filter == Some(c), c.title(), Some(n)) {
                                self.filter = Some(c);
                                self.pane = Pane::Items;
                            }
                        }
                    });
                });
            });
        });
        if quit {
            self.quitting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if lock {
            self.lock();
            return;
        }
        if self.pane == Pane::Settings {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(28, 22))).show(ctx, |ui| {
                ui.label(RichText::new(t!("Ayarlar", "Settings")).size(22.0).strong());
                ui.add_space(10.0);
                let w = ui.available_width().min(620.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_max_width(w);
                    self.settings(ui);
                });
            });
            return;
        }

        // Liste sütunu: başlık, arama, kayıtlar.
        let mut open = None;
        let mut add = None;
        egui::SidePanel::left("list").default_width(330.0).width_range(260.0..=460.0).frame(egui::Frame::new().fill(LIST).inner_margin(egui::Margin::symmetric(14, 18))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.filter.map(|c| c.title()).unwrap_or(t!("Tüm kayıtlar", "All items"))).size(22.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::containers::menu::MenuButton::from_button(primary(t!("+ Yeni", "+ New"))).ui(ui, |ui| {
                        for cat in Category::ALL {
                            let clicked = ui.horizontal(|ui| {
                                chip(ui, cat, 22.0);
                                ui.add(egui::Button::new(cat.title()).frame(false)).clicked()
                            }).inner;
                            if clicked {
                                add = Some(cat);
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
            ui.add(input(&mut self.search, t!("Ara…", "Search…")));
            ui.add_space(6.0);
            let q = self.search.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let shown: Vec<_> = self.items.iter().filter(|(_, n, c, _)| self.filter.is_none_or(|f| f == *c) && (q.is_empty() || n.to_lowercase().contains(&q))).collect();
                if shown.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(if self.items.is_empty() { t!("Kasa boş", "The vault is empty") } else { t!("Eşleşen kayıt yok", "No matching items") }).strong());
                        if self.items.is_empty() {
                            ui.label(RichText::new(t!("“+ Yeni” ile kayıt ekle ya da Ayarlar'dan yedeği geri yükle.", "Add an item with “+ New” or restore a backup from Settings.")).size(13.0).color(DIM));
                        }
                    });
                }
                for (id, name, cat, updated) in shown {
                    let sel = self.editor.as_ref().and_then(|e| e.id.as_deref()) == Some(id.as_str());
                    if item_row(ui, sel, name, *cat, *updated) {
                        open = Some(id.clone());
                    }
                }
            });
        });
        if let Some(cat) = add {
            self.new_item(cat);
        }
        if let Some(id) = open {
            self.open_item(&id);
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(28, 22))).show(ctx, |ui| {
            if self.editor.is_some() {
                let w = ui.available_width().min(720.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_max_width(w);
                    self.editor(ui);
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.label(RichText::new(t!("Bir kayıt seç", "Pick an item")).size(17.0).strong());
                    ui.label(RichText::new(t!("Soldaki listeden bir kayıt aç ya da yeni bir tane ekle.", "Open an item from the list or add a new one.")).color(DIM));
                    ui.add_space(8.0);
                    if ui.add(primary(t!("+ Yeni giriş", "+ New login"))).clicked() {
                        self.new_item(Category::Password);
                    }
                });
            }
        });
    }

    fn open_item(&mut self, id: &str) {
        let c = self.shared.core();
        let Some(v) = c.vault.as_ref() else { return };
        let Some(it) = v.items().iter().find(|i| i.id == id) else { return };
        match v.decrypt(id) {
            Ok(mut body) => {
                card::upgrade(it.meta.category, &mut body);
                for f in body.fields.iter_mut().filter(|f| f.fmt.numeric()) {
                    f.value = card::load(f.fmt, &f.value);
                }
                self.editor = Some(Editor { id: Some(id.to_string()), name: it.meta.name.clone(), category: it.meta.category, body, reveal: HashSet::new(), confirm_delete: false });
            }
            Err(e) => {
                drop(c);
                self.toast(e.to_string());
            }
        }
    }

    /// Düzenleyicideki kaydı yazar. Yeni kayıtta kimlik döner.
    fn save_item(&mut self) -> Result<String, String> {
        let e = self.editor.as_mut().ok_or_default()?;
        let name = e.name.trim().to_string();
        if name.is_empty() {
            return Err(t!("Kayda bir ad ver.", "Give the item a name.").into());
        }
        let mut body = e.body.clone();
        for f in body.fields.iter_mut().filter(|f| f.fmt.numeric()) {
            f.value = card::store(f.fmt, &f.value);
        }
        let meta = Meta { name, category: e.category, updated: now() };
        let mut c = self.shared.core();
        let v = c.vault.as_mut().ok_or_default()?;
        let id = match &e.id {
            Some(id) => {
                v.update(id, meta, &body).map_err(|e| e.to_string())?;
                id.clone()
            }
            None => {
                let id = v.add(meta, &body).map_err(|e| e.to_string())?;
                e.id = Some(id.clone());
                id
            }
        };
        c.changed();
        Ok(id)
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let mut copy: Option<Zeroizing<String>> = None;
        let mut save = false;
        let mut attach = false;
        let mut export_file = false;
        let mut remove_file = false;
        let mut delete = false;
        let mut close = false;
        let unix = now();
        let gen_len = &mut self.gen_len;
        let gen_symbols = &mut self.gen_symbols;
        let Some(e) = self.editor.as_mut() else { return };

        // Başlık: rozet, büyük ad, kategori.
        ui.horizontal(|ui| {
            chip(ui, e.category, 44.0);
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut e.name).hint_text(t!("Kayıt adı", "Item name")).font(egui::FontId::proportional(22.0)).frame(false).desired_width(f32::INFINITY));
                egui::ComboBox::from_id_salt("cat").selected_text(RichText::new(e.category.title()).color(DIM).size(13.0)).show_ui(ui, |ui| {
                    for c in Category::ALL {
                        ui.selectable_value(&mut e.category, c, c.title());
                    }
                });
            });
        });
        ui.add_space(14.0);

        let mut remove = None;
        egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(20).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, f) in e.body.fields.iter_mut().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                // Alan adı: küçük, soluk, düzenlenebilir.
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.label).font(egui::FontId::proportional(12.0)).text_color(DIM).frame(false).desired_width(220.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("×").color(DIM)).frame(false)).on_hover_text(t!("Alanı sil", "Remove field")).clicked() {
                            remove = Some(i);
                        }
                    });
                });
                let shown = e.reveal.contains(&i);
                // Giriş solda, düğmeler sağda. Genişlik önceden ayrılır ki "Kopyala"
                // belirince kutu kaymasın; sabit kimlik de yazarken odağı korur.
                let masked = f.kind.masked();
                let can_gen = f.kind == FieldKind::Secret && f.fmt.is_free();
                let copyable = f.kind != FieldKind::Totp;
                let btn = egui::vec2(84.0, 38.0);
                let n = masked as usize + can_gen as usize + copyable as usize;
                ui.horizontal_wrapped(|ui| {
                    // desired_width kenar boşluğunu (2×10) içermez; dar pencerede
                    // giriş tüm satırı alır, düğmeler alt satıra iner.
                    let full = ui.available_width() - 20.0;
                    let beside = full - n as f32 * (btn.x + ui.spacing().item_spacing.x);
                    let w = if beside >= 160.0 { beside } else { full };
                    if f.kind == FieldKind::Multiline {
                        ui.add(egui::TextEdit::multiline(&mut f.value).id_salt(("field", i)).desired_width(w).desired_rows(4).margin(egui::Margin::symmetric(10, 8)));
                    } else {
                        let mut te = input(&mut f.value, "").id_salt(("field", i)).desired_width(w).password(masked && !shown);
                        if f.kind == FieldKind::Totp || masked {
                            te = te.font(egui::TextStyle::Monospace);
                        }
                        ui.add(te);
                    }
                    if masked && ui.add(ghost(if shown { t!("Gizle", "Hide") } else { t!("Göster", "Show") }).min_size(btn)).clicked() {
                        if shown {
                            e.reveal.remove(&i);
                        } else {
                            e.reveal.insert(i);
                        }
                    }
                    if can_gen {
                        egui::containers::menu::MenuButton::from_button(ghost(t!("Üret", "Generate")).min_size(btn)).ui(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(t!("Uzunluk", "Length"));
                                ui.add(egui::DragValue::new(gen_len).range(gen::MIN_LEN..=gen::MAX_LEN));
                            });
                            ui.checkbox(gen_symbols, t!("Simgeler", "Symbols"));
                            if ui.add(primary(t!("Parola üret", "Generate password"))).clicked() {
                                f.value = gen::generate(gen::Options { len: *gen_len, symbols: *gen_symbols }).to_string();
                                e.reveal.insert(i);
                                ui.close();
                            }
                        });
                    }
                    if copyable && ui.add_enabled(!f.value.is_empty(), ghost(t!("Kopyala", "Copy")).min_size(btn)).clicked() {
                        copy = Some(Zeroizing::new(card::store(f.fmt, &f.value)));
                    }
                });
                if f.kind == FieldKind::Secret && f.fmt.is_free() && !f.value.is_empty() {
                    strength_bar(ui, &f.value);
                }
                if f.kind == FieldKind::Totp && !f.value.trim().is_empty() {
                    match totp::Totp::parse(&f.value) {
                        Ok(tp) => {
                            let code = tp.code(unix);
                            let left = tp.remaining(unix);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(totp::grouped(&code)).monospace().size(26.0).color(ACCENT));
                                ui.vertical(|ui| {
                                    ui.add_space(6.0);
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(90.0, 4.0), egui::Sense::hover());
                                    ui.painter().rect_filled(r, 2, LINE);
                                    let frac = left as f32 / tp.period.max(1) as f32;
                                    let col = if left <= 5 { RED } else { ACCENT };
                                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * frac, 4.0)), 2, col);
                                    small_label(ui, &format!("{left} sn"));
                                });
                                if ui.add(ghost(t!("Kodu kopyala", "Copy code"))).clicked() {
                                    copy = Some(Zeroizing::new(code.to_string()));
                                }
                            });
                        }
                        Err(_) => error_text(ui, t!("Geçersiz 2FA anahtarı.", "Invalid 2FA key.")),
                    }
                }
            }
            ui.add_space(10.0);
            egui::containers::menu::MenuButton::from_button(ghost(t!("+ Alan ekle", "+ Add field"))).ui(ui, |ui| {
                for (k, n) in [(FieldKind::Text, t!("Metin", "Text")), (FieldKind::Secret, t!("Gizli", "Secret")), (FieldKind::Url, t!("Adres", "Website")), (FieldKind::Multiline, t!("Not", "Note")), (FieldKind::Totp, "2FA")] {
                    if ui.button(n).clicked() {
                        e.body.fields.push(Field::new(n, k));
                        ui.close();
                    }
                }
            });
        });
        if let Some(i) = remove {
            e.body.fields.remove(i);
            e.reveal.clear();
        }
        ui.add_space(12.0);

        section(ui, t!("Ek dosya", "Attachment"), |ui| match &e.body.file {
            Some(f) => {
                ui.horizontal(|ui| {
                    chip(ui, Category::File, 30.0);
                    ui.vertical(|ui| {
                        ui.label(&f.name);
                        small_label(ui, &format!("{} KB", f.size.div_ceil(1024)));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(ghost(t!("Kaldır", "Remove"))).clicked() {
                            remove_file = true;
                        }
                        if ui.add(ghost(t!("Dışa kaydet…", "Save as…"))).clicked() {
                            export_file = true;
                        }
                    });
                });
            }
            None => {
                small_label(ui, t!("Bu kayda bir dosya ekleyebilirsin (en çok 25 MB).", "You can attach one file to this item (up to 25 MB)."));
                if ui.add(ghost(t!("Dosya ekle…", "Attach file…"))).clicked() {
                    attach = true;
                }
            }
        });

        let mut drop_pk = None;
        if !e.body.passkeys.is_empty() {
            section(ui, "Passkey", |ui| {
                for (i, k) in e.body.passkeys.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(&k.rp_id);
                            small_label(ui, &k.user_name);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(RichText::new(t!("Sil", "Delete")).color(RED)).fill(Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0_f32, LINE))).clicked() {
                                drop_pk = Some(i);
                            }
                        });
                    });
                }
            });
        }
        if let Some(i) = drop_pk {
            e.body.passkeys.remove(i);
        }

        ui.horizontal(|ui| {
            if ui.add(primary(t!("Kaydet", "Save")).min_size(egui::vec2(96.0, 36.0))).clicked() {
                save = true;
            }
            if ui.add(ghost(t!("Kapat", "Close")).min_size(egui::vec2(80.0, 36.0))).clicked() {
                close = true;
            }
            if e.id.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if e.confirm_delete {
                        if ui.add(ghost(t!("Vazgeç", "Cancel"))).clicked() {
                            e.confirm_delete = false;
                        }
                        if ui.add(egui::Button::new(RichText::new(t!("Evet, sil", "Yes, delete")).strong().color(Color32::WHITE)).fill(RED)).clicked() {
                            delete = true;
                        }
                        ui.label(RichText::new(t!("Geri alınamaz.", "This can't be undone.")).size(13.0).color(RED));
                    } else if ui.add(egui::Button::new(RichText::new(t!("Sil", "Delete")).color(RED)).frame(false)).clicked() {
                        e.confirm_delete = true;
                    }
                });
            }
        });
        if drop_pk.is_some() {
            self.toast(t!("Passkey kaldırıldı; kaydetmeyi unutma.", "Passkey removed; don't forget to save."));
        }

        if close {
            self.editor = None;
            return;
        }
        if let Some(v) = copy {
            self.copy(&v);
        }
        if save {
            match self.save_item() {
                Ok(_) => self.toast(t!("Kaydedildi.", "Saved.")),
                Err(m) => self.toast(m),
            }
        }
        if attach {
            self.attach();
        }
        if export_file {
            self.export_file();
        }
        if remove_file {
            self.remove_file();
        }
        if delete {
            self.delete_item();
        }
    }

    fn attach(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title(t!("Eklenecek dosya", "File to attach")).pick_file() else { return };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE {
            self.toast(t!(format!("Dosya en çok {} MB olabilir.", MAX_FILE / 1024 / 1024), format!("Files can be at most {} MB.", MAX_FILE / 1024 / 1024)));
            return;
        }
        let data = match std::fs::read(&path) {
            Ok(d) => Zeroizing::new(d),
            Err(e) => return self.toast(e.to_string()),
        };
        // Ek dosya kaydın kimliğiyle mühürlenir: yeni kayıt önce yazılır.
        let id = match self.save_item() {
            Ok(id) => id,
            Err(m) => return self.toast(m),
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let r = {
            let c = self.shared.core();
            c.vault.as_ref().ok_or_default().and_then(|v| v.attach(&id, &name, &data).map_err(|e| e.to_string()))
        };
        match r {
            Ok(a) => {
                if let Some(e) = self.editor.as_mut() {
                    e.body.file = Some(a);
                }
                match self.save_item() {
                    Ok(_) => self.toast(t!("Dosya eklendi.", "File attached.")),
                    Err(m) => self.toast(m),
                }
            }
            Err(m) => self.toast(m),
        }
    }

    fn export_file(&mut self) {
        let Some(e) = self.editor.as_ref() else { return };
        let (Some(id), Some(f)) = (e.id.clone(), e.body.file.as_ref()) else { return };
        let Some(path) = rfd::FileDialog::new().set_file_name(&f.name).save_file() else { return };
        let r = {
            let c = self.shared.core();
            c.vault.as_ref().ok_or_default().and_then(|v| v.read_file(&id).map_err(|e| e.to_string()))
        };
        match r.and_then(|d| std::fs::write(&path, &*d).map_err(|e| e.to_string())) {
            Ok(()) => self.toast(t!("Dosya kaydedildi.", "File saved.")),
            Err(m) => self.toast(m),
        }
    }

    fn remove_file(&mut self) {
        let Some(e) = self.editor.as_mut() else { return };
        e.body.file = None;
        let id = e.id.clone();
        match self.save_item() {
            Ok(_) => {
                if let (Some(id), Some(v)) = (id, self.shared.core().vault.as_ref()) {
                    v.remove_file(&id);
                }
                self.toast(t!("Ek dosya kaldırıldı.", "Attachment removed."));
            }
            Err(m) => self.toast(m),
        }
    }

    fn delete_item(&mut self) {
        let Some(id) = self.editor.as_ref().and_then(|e| e.id.clone()) else { return };
        let r = {
            let mut c = self.shared.core();
            let r = c.vault.as_mut().ok_or_default().and_then(|v| v.delete(&id).map_err(|e| e.to_string()));
            c.changed();
            r
        };
        match r {
            Ok(()) => {
                self.editor = None;
                self.toast(t!("Kayıt silindi.", "Item deleted."));
            }
            Err(m) => self.toast(m),
        }
    }
}

// ------------------------------------------------------------ ayarlar

impl App {
    fn settings(&mut self, ui: &mut egui::Ui) {
        let mut s = self.shared.core().settings.clone();
        let row = |ui: &mut egui::Ui, title: &str, sub: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(title);
                    if !sub.is_empty() {
                        small_label(ui, sub);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add(ui));
            });
            ui.add_space(4.0);
        };
        section(ui, t!("Güvenlik", "Security"), |ui| {
            row(ui, t!("Otomatik kilit", "Auto-lock"), t!("Dakika; 0 kapalı", "Minutes; 0 is off"), &mut |ui| {
                ui.add(egui::DragValue::new(&mut s.auto_lock).range(0..=720).suffix(t!(" dk", " min")));
            });
            row(ui, t!("Panoyu temizle", "Clear clipboard"), t!("Saniye; 0 kapalı", "Seconds; 0 is off"), &mut |ui| {
                ui.add(egui::DragValue::new(&mut s.clipboard).range(0..=600).suffix(t!(" sn", " s")));
            });
        });
        section(ui, t!("Genel", "General"), |ui| {
            ui.checkbox(&mut s.browser_fill, t!("Tarayıcı eklentisiyle doldurma, kaydetme ve passkey", "Browser extension filling, saving and passkeys"));
            ui.checkbox(&mut s.keep_running, t!("Pencere kapatılınca arka planda çalışmaya devam et", "Keep running in the background when the window is closed"));
            ui.add_space(4.0);
            row(ui, t!("Dil", "Language"), "", &mut |ui| {
                egui::ComboBox::from_id_salt("lang").selected_text(match s.lang.as_str() {
                    "tr" => "Türkçe",
                    "en" => "English",
                    _ => t!("Sistem", "System"),
                }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.lang, "auto".to_string(), t!("Sistem", "System"));
                    ui.selectable_value(&mut s.lang, "tr".to_string(), "Türkçe");
                    ui.selectable_value(&mut s.lang, "en".to_string(), "English");
                });
            });
            small_label(ui, &t!(format!("Köprü: 127.0.0.1:{}", crate::bridge::port()), format!("Bridge: 127.0.0.1:{}", crate::bridge::port())));
        });
        {
            let mut c = self.shared.core();
            if c.settings != s {
                if c.settings.lang != s.lang {
                    carlui_vault::lang::apply(&s.lang);
                }
                s.save();
                c.settings = s;
            }
        }

        let mut change = false;
        section(ui, t!("Ana parola", "Master password"), |ui| {
            password_edit(ui, &mut self.old_pw, t!("Şu anki parola", "Current password"));
            password_edit(ui, &mut self.new_pw, t!("Yeni parola", "New password"));
            if !self.new_pw.is_empty() {
                strength_bar(ui, &self.new_pw);
            }
            password_edit(ui, &mut self.new_pw2, t!("Yeni parola (tekrar)", "New password (again)"));
            change = ui.add(primary(t!("Parolayı değiştir", "Change password"))).clicked();
        });
        if change {
            let ok = self.shared.core().vault.as_ref().is_some_and(|v| v.verify_password(&self.old_pw));
            self.settings_msg = if !ok {
                t!("Şu anki parola yanlış.", "The current password is wrong.").into()
            } else {
                match self.set_password() {
                    Ok(()) => t!("Ana parola değiştirildi.", "Master password changed.").into(),
                    Err(e) => e,
                }
            };
        }

        let has = self.shared.core().vault.as_ref().is_some_and(|v| v.has_recovery());
        let (mut new_key, mut drop_key) = (false, false);
        section(ui, t!("Kurtarma anahtarı", "Recovery key"), |ui| {
            ui.horizontal(|ui| {
                let (col, txt) = if has { (GREEN, t!("Kurtarma anahtarı var.", "A recovery key is set.")) } else { (ORANGE, t!("Kurtarma anahtarı yok.", "No recovery key is set.")) };
                let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, col);
                ui.label(RichText::new(txt).size(13.0).color(DIM));
            });
            ui.horizontal(|ui| {
                new_key = ui.add(primary(t!("Yeni anahtar oluştur", "Create a new key"))).clicked();
                if has {
                    drop_key = ui.add(ghost(t!("Anahtarı kaldır", "Remove the key"))).clicked();
                }
            });
        });
        if new_key {
            let r = self.shared.core().vault.as_mut().ok_or_default().and_then(|v| v.new_recovery().map_err(|e| e.to_string()));
            match r {
                Ok(k) => self.recovery = Some(k),
                Err(m) => self.settings_msg = m,
            }
        }
        if drop_key {
            let r = self.shared.core().vault.as_mut().ok_or_default().and_then(|v| v.remove_recovery().map_err(|e| e.to_string()));
            self.settings_msg = match r {
                Ok(()) => t!("Kurtarma anahtarı kaldırıldı.", "Recovery key removed.").into(),
                Err(m) => m,
            };
        }

        let (mut pick, mut backup, mut run, mut cancel) = (false, false, false, false);
        section(ui, t!("Yedek", "Backup"), |ui| {
            small_label(ui, t!("Windows'taki Carl Vault'tan alınan şifreli .carlvault yedeği geri yüklenebilir. Yeni yedek ana parolayla şifrelenir.", "An encrypted .carlvault backup exported from Carl Vault on Windows can be restored. New backups are encrypted with the master password."));
            ui.horizontal(|ui| {
                pick = ui.add(ghost(t!("Yedekten geri yükle…", "Restore a backup…"))).clicked();
                backup = ui.add(ghost(t!("Yedek al…", "Back up…"))).clicked();
            });
            if let Some(r) = self.restore.as_mut() {
                ui.add_space(4.0);
                small_label(ui, &r.path);
                let enter = ui.add(input(&mut *r.password, t!("Yedeğin parolası", "Backup password")).password(true)).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    run = ui.add(primary(t!("Geri yükle", "Restore"))).clicked() || enter;
                    cancel = ui.add(ghost(t!("Vazgeç", "Cancel"))).clicked();
                });
            }
        });
        if pick {
            self.pick_backup();
        }
        if backup {
            self.backup();
        }
        if cancel {
            self.restore = None;
        }
        if run {
            self.settings_msg = self.run_restore();
        }
        if !self.settings_msg.is_empty() {
            ui.label(RichText::new(&self.settings_msg).size(13.0).color(ACCENT));
        }
    }

    fn pick_backup(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Carl Vault", &[export::EXT]).pick_file() else { return };
        match std::fs::read(&path) {
            Ok(d) if export::is_backup(&d) && (d.len() as u64) <= export::MAX_BACKUP => {
                self.restore = Some(Restore { path: path.display().to_string(), data: d, password: Default::default() });
                self.settings_msg.clear();
            }
            Ok(_) => self.settings_msg = t!("Bu dosya bir Carl Vault yedeği değil.", "This file isn't a Carl Vault backup.").into(),
            Err(e) => self.settings_msg = e.to_string(),
        }
    }

    fn run_restore(&mut self) -> String {
        let Some(r) = self.restore.as_ref() else { return String::new() };
        let restored = match export::open(&r.data, &r.password) {
            Ok(x) => x,
            Err(e) => return e.to_string(),
        };
        self.restore = None;
        let mut c = self.shared.core();
        let Some(v) = c.vault.as_mut().filter(|v| v.is_open()) else { return t!("Kasa kilitli.", "The vault is locked.").into() };
        // Aynı ad, tür ve tarihli kayıt zaten varsa atlanır (yedeği iki kez yüklemek).
        let have: HashSet<(String, Category, u64)> = v.items().iter().map(|i| (i.meta.name.clone(), i.meta.category, i.meta.updated)).collect();
        let mut files = restored.files;
        files.resize_with(restored.batch.items.len(), || None);
        let mut skipped = restored.batch.skipped;
        let (items, files): (Vec<(Meta, Body)>, Vec<_>) = restored
            .batch
            .items
            .into_iter()
            .zip(files)
            .filter(|(i, _)| {
                let dup = have.contains(&(i.meta.name.clone(), i.meta.category, i.meta.updated));
                skipped += dup as usize;
                !dup
            })
            .map(|(i, file)| {
                let mut body = i.body.clone();
                card::upgrade(i.meta.category, &mut body);
                if file.is_none() {
                    body.file = None;
                }
                ((i.meta.clone(), body), file)
            })
            .unzip();
        let metas: Vec<Meta> = items.iter().map(|(m, _)| m.clone()).collect();
        let ids = match v.add_many(items) {
            Ok(ids) => ids,
            Err(e) => return e.to_string(),
        };
        let mut lost = 0;
        for ((id, meta), file) in ids.iter().zip(metas).zip(files) {
            let Some(data) = file else { continue };
            let Ok(mut body) = v.decrypt(id) else { continue };
            let name = body.file.as_ref().map(|f| f.name.clone()).unwrap_or_default();
            if v.attach(id, &name, &data).is_err() {
                lost += 1;
                body.file = None;
                let _ = v.update(id, meta, &body);
            }
        }
        c.changed();
        let n = ids.len();
        eprintln!("kasa: yedekten {n} kayıt geri yüklendi");
        let mut msg = t!(format!("{n} kayıt geri yüklendi."), format!("{n} items restored."));
        if skipped > 0 {
            msg += &t!(format!(" {skipped} kayıt atlandı (zaten var ya da okunamadı)."), format!(" {skipped} items skipped (already present or unreadable)."));
        }
        if lost > 0 {
            msg += &t!(format!(" {lost} ek dosya yazılamadı."), format!(" {lost} attachments couldn't be written."));
        }
        msg
    }

    fn backup(&mut self) {
        if !self.shared.core().vault.as_ref().is_some_and(|v| v.verify_password(&self.old_pw)) {
            self.settings_msg = t!("Yedek ana parolayla şifrelenir: önce “Şu anki parola” kutusuna ana parolayı yaz.", "The backup is encrypted with the master password: type it into “Current password” first.").into();
            return;
        }
        let name = format!("carl-vault-{}.{}", now(), export::EXT);
        let Some(path) = rfd::FileDialog::new().set_file_name(&name).add_filter("Carl Vault", &[export::EXT]).save_file() else { return };
        let r = {
            let c = self.shared.core();
            c.vault.as_ref().ok_or_default().and_then(|v| {
                let col = export::collect(v, true).map_err(|e| e.to_string())?;
                export::encrypt(&col.entries, &self.old_pw).map_err(|e| e.to_string())
            })
        };
        self.settings_msg = match r.and_then(|d| std::fs::write(&path, d).map_err(|e| e.to_string())) {
            Ok(()) => t!(format!("Yedek kaydedildi: {}", path.display()), format!("Backup saved: {}", path.display())),
            Err(m) => m,
        };
        self.old_pw = Default::default();
    }
}
