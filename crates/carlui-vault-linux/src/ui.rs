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
    items: Vec<(String, String, Category)>,
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
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
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
                let mut l: Vec<_> = v.items().iter().map(|i| (i.id.clone(), i.meta.name.clone(), i.meta.category)).collect();
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
            egui::TopBottomPanel::top("asks").show(ctx, |ui| {
                for a in visible {
                    ui.add_space(6.0);
                    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let left = crate::core::ASK_TTL.saturating_sub(a.at.elapsed()).as_secs();
                        match &a.kind {
                            AskKind::Pair(who) => {
                                ui.label(RichText::new(t!("Eklenti bağlantısı", "Extension connection")).strong());
                                ui.label(crate::pairing::prompt(who));
                                ui.horizontal(|ui| {
                                    if ui.button(t!("Onayla", "Approve")).clicked() {
                                        answers.push((a.id, Some(0)));
                                    }
                                    if ui.button(t!("Hayır", "No")).clicked() {
                                        answers.push((a.id, None));
                                    }
                                    ui.weak(format!("{left} sn"));
                                });
                            }
                            AskKind::Passkey(p) => {
                                match &p.kind {
                                    PasskeyKind::Create { user_name, .. } => {
                                        ui.label(RichText::new(t!(format!("{} için passkey oluşturulsun mu?", p.site()), format!("Create a passkey for {}?", p.site()))).strong());
                                        ui.label(format!("{}: {user_name}", t!("Kullanıcı", "User")));
                                        let target = match p.choices.first() {
                                            Some(c) => t!(format!("“{}” kaydına eklenecek.", c.name), format!("Will be added to “{}”.", c.name)),
                                            None => t!("Yeni bir giriş kaydı oluşturulacak.", "A new login item will be created.").to_string(),
                                        };
                                        ui.weak(target);
                                        ui.horizontal(|ui| {
                                            if ui.button(t!("Oluştur", "Create")).clicked() {
                                                answers.push((a.id, Some(0)));
                                            }
                                            if ui.button(t!("Tarayıcıya bırak", "Let the browser handle it")).clicked() {
                                                answers.push((a.id, None));
                                            }
                                            ui.weak(format!("{left} sn"));
                                        });
                                    }
                                    PasskeyKind::Get { .. } => {
                                        ui.label(RichText::new(t!(format!("{} passkey ile giriş istiyor", p.site()), format!("{} wants to sign in with a passkey", p.site()))).strong());
                                        for (i, c) in p.choices.iter().enumerate() {
                                            if ui.button(format!("{} · {}", c.name, c.user)).clicked() {
                                                answers.push((a.id, Some(i)));
                                            }
                                        }
                                        ui.horizontal(|ui| {
                                            if ui.button(t!("Tarayıcıya bırak", "Let the browser handle it")).clicked() {
                                                answers.push((a.id, None));
                                            }
                                            ui.weak(format!("{left} sn"));
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
            egui::TopBottomPanel::bottom("toast").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(m);
                ui.add_space(4.0);
            });
        }
    }
}

// ------------------------------------------------------------ kurulum ve kilit

fn password_edit(ui: &mut egui::Ui, value: &mut Zeroizing<String>, hint: &str) -> egui::Response {
    ui.add(egui::TextEdit::singleline(&mut **value).password(true).hint_text(hint).desired_width(320.0))
}

impl App {
    fn setup(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading("Carl Vault");
            ui.label(t!("Bu bilgisayarda henüz kasa yok. Bir ana parola belirle.", "There's no vault on this computer yet. Choose a master password."));
            ui.add_space(12.0);
            password_edit(ui, &mut self.pw, t!("Ana parola", "Master password"));
            let enter = password_edit(ui, &mut self.pw2, t!("Ana parola (tekrar)", "Master password (again)")).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.weak(t!("Ana parola unutulursa kasa açılamaz; kurtarma anahtarı birazdan gösterilecek.", "If you forget the master password the vault can't be opened; a recovery key is shown next."));
            if ui.button(t!("Kasayı oluştur", "Create vault")).clicked() || enter {
                self.create();
            }
            if !self.error.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.error);
            }
            ui.add_space(20.0);
            ui.weak(t!(
                "Windows'taki kasayı taşımak için: kasa klasörünü ~/.local/share/CarlUI/vault altına kopyala ya da kasayı oluşturup Ayarlar'dan .carlvault yedeğini geri yükle.",
                "To move your Windows vault: copy the vault folder to ~/.local/share/CarlUI/vault, or create a vault and restore a .carlvault backup from Settings."
            ));
        });
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
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading("Carl Vault");
            if let Some(site) = waiting {
                ui.colored_label(Color32::LIGHT_BLUE, t!(format!("{site} passkey istiyor. Devam etmek için kasayı aç."), format!("{site} is asking for a passkey. Unlock the vault to continue.")));
            }
            ui.add_space(12.0);
            let enter = if self.use_recovery {
                ui.label(t!("Kurtarma anahtarı", "Recovery key"));
                let r = ui.add(egui::TextEdit::singleline(&mut *self.pw).desired_width(420.0));
                r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            } else {
                let r = password_edit(ui, &mut self.pw, t!("Ana parola", "Master password"));
                if self.pw.is_empty() && self.error.is_empty() {
                    r.request_focus();
                }
                r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            };
            match wait {
                Some(s) => {
                    ui.weak(t!(format!("Çok fazla yanlış deneme. {s} sn bekle."), format!("Too many wrong attempts. Wait {s} s.")));
                }
                None => {
                    if ui.button(t!("Kilidi aç", "Unlock")).clicked() || enter {
                        self.try_unlock();
                    }
                }
            }
            if !self.error.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.error);
            }
            ui.add_space(8.0);
            let has_recovery = self.shared.core().vault.as_ref().is_some_and(|v| v.has_recovery());
            if has_recovery {
                let label = if self.use_recovery { t!("Ana parolayla aç", "Use the master password") } else { t!("Parolamı unuttum (kurtarma anahtarı)", "Forgot password (recovery key)") };
                if ui.link(label).clicked() {
                    self.use_recovery = !self.use_recovery;
                    self.pw = Default::default();
                    self.error.clear();
                }
            }
            ui.add_space(20.0);
            if ui.button(t!("Çık", "Quit")).clicked() {
                self.quitting = true;
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });
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
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(t!("Kurtarma anahtarı", "Recovery key"));
            ui.label(t!(
                "Ana parolayı unutursan kasayı bu anahtarla açabilirsin. Bir kâğıda yaz ya da güvenli bir yerde sakla; bir daha gösterilmeyecek.",
                "If you forget the master password you can open the vault with this key. Write it down or keep it somewhere safe; it won't be shown again."
            ));
            ui.add_space(12.0);
            ui.label(RichText::new(key).monospace().size(18.0));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(t!("Kopyala", "Copy")).clicked() {
                    self.copy(key);
                }
                if ui.button(t!("Kaydettim", "I've saved it")).clicked() {
                    self.recovery = None;
                }
            });
        });
    }

    fn reset_password(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(t!("Yeni ana parola", "New master password"));
            ui.label(t!("Kasa kurtarma anahtarıyla açıldı. Devam etmeden yeni bir ana parola belirle.", "The vault was opened with the recovery key. Choose a new master password to continue."));
            password_edit(ui, &mut self.new_pw, t!("Yeni parola", "New password"));
            password_edit(ui, &mut self.new_pw2, t!("Yeni parola (tekrar)", "New password (again)"));
            if ui.button(t!("Kaydet", "Save")).clicked() {
                match self.set_password() {
                    Ok(()) => self.must_reset = false,
                    Err(e) => self.error = e,
                }
            }
            if !self.error.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.error);
            }
        });
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
    fn main(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Carl Vault");
                ui.add_space(12.0);
                ui.menu_button(t!("+ Yeni", "+ New"), |ui| {
                    for cat in Category::ALL {
                        if ui.button(cat.title()).clicked() {
                            self.editor = Some(Editor { id: None, name: String::new(), category: cat, body: { let mut b = Body::default(); b.fields = cat.template(); b }, reveal: HashSet::new(), confirm_delete: false });
                            self.pane = Pane::Items;
                            ui.close();
                        }
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("Çık", "Quit")).clicked() {
                        self.quitting = true;
                        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    }
                    if ui.button(t!("Kilitle", "Lock")).clicked() {
                        self.lock();
                    }
                    let settings = self.pane == Pane::Settings;
                    if ui.selectable_label(settings, t!("Ayarlar", "Settings")).clicked() {
                        self.pane = if settings { Pane::Items } else { Pane::Settings };
                    }
                });
            });
        });
        if self.pane == Pane::Settings {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.settings(ui));
            });
            return;
        }
        egui::SidePanel::left("list").resizable(true).default_width(280.0).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(t!("Ara", "Search")).desired_width(f32::INFINITY));
            egui::ComboBox::from_id_salt("filter").selected_text(self.filter.map(|c| c.title()).unwrap_or(t!("Tümü", "All"))).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter, None, t!("Tümü", "All"));
                for c in Category::ALL {
                    ui.selectable_value(&mut self.filter, Some(c), c.title());
                }
            });
            ui.separator();
            let q = self.search.to_lowercase();
            let mut open = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                let shown: Vec<_> = self.items.iter().filter(|(_, n, c)| self.filter.is_none_or(|f| f == *c) && (q.is_empty() || n.to_lowercase().contains(&q))).collect();
                if shown.is_empty() {
                    ui.weak(if self.items.is_empty() { t!("Kasa boş. “+ Yeni” ile kayıt ekle ya da Ayarlar'dan yedeği geri yükle.", "The vault is empty. Add an item with “+ New” or restore a backup from Settings.") } else { t!("Eşleşen kayıt yok.", "No matching items.") });
                }
                for (id, name, cat) in shown {
                    let sel = self.editor.as_ref().and_then(|e| e.id.as_deref()) == Some(id.as_str());
                    let label = format!("{name}\n{}", cat.title());
                    if ui.add_sized([ui.available_width(), 0.0], egui::Button::selectable(sel, label)).clicked() {
                        open = Some(id.clone());
                    }
                }
            });
            if let Some(id) = open {
                self.open_item(&id);
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.editor.is_some() {
                    self.editor(ui);
                } else {
                    ui.weak(t!("Soldan bir kayıt seç.", "Pick an item on the left."));
                }
            });
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
        let (gen_len, gen_symbols) = (self.gen_len, self.gen_symbols);
        let unix = now();
        let Some(e) = self.editor.as_mut() else { return };
        ui.horizontal(|ui| {
            ui.label(t!("Ad", "Name"));
            ui.add(egui::TextEdit::singleline(&mut e.name).desired_width(320.0));
            egui::ComboBox::from_id_salt("cat").selected_text(e.category.title()).show_ui(ui, |ui| {
                for c in Category::ALL {
                    ui.selectable_value(&mut e.category, c, c.title());
                }
            });
        });
        ui.separator();
        let mut remove = None;
        egui::Grid::new("fields").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
            for (i, f) in e.body.fields.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut f.label).desired_width(140.0));
                let shown = e.reveal.contains(&i);
                match f.kind {
                    FieldKind::Multiline => {
                        ui.add(egui::TextEdit::multiline(&mut f.value).desired_width(360.0).desired_rows(3));
                    }
                    _ => {
                        ui.add(egui::TextEdit::singleline(&mut f.value).password(f.kind.masked() && !shown).desired_width(360.0));
                    }
                }
                ui.horizontal(|ui| {
                    if f.kind.masked() && ui.small_button(if shown { t!("Gizle", "Hide") } else { t!("Göster", "Show") }).clicked() {
                        if shown {
                            e.reveal.remove(&i);
                        } else {
                            e.reveal.insert(i);
                        }
                    }
                    if f.kind != FieldKind::Totp && !f.value.is_empty() && ui.small_button(t!("Kopyala", "Copy")).clicked() {
                        copy = Some(Zeroizing::new(card::store(f.fmt, &f.value)));
                    }
                    if f.kind == FieldKind::Secret && f.fmt.is_free() {
                        if ui.small_button("🎲").on_hover_text(t!("Parola üret", "Generate password")).clicked() {
                            f.value = gen::generate(gen::Options { len: gen_len, symbols: gen_symbols }).to_string();
                            e.reveal.insert(i);
                        }
                        if !f.value.is_empty() {
                            let (txt, col) = match gen::strength(&f.value) {
                                gen::Strength::Weak => (t!("zayıf", "weak"), Color32::LIGHT_RED),
                                gen::Strength::Fair => (t!("orta", "fair"), Color32::YELLOW),
                                gen::Strength::Strong => (t!("güçlü", "strong"), Color32::LIGHT_GREEN),
                            };
                            ui.colored_label(col, txt);
                        }
                    }
                    if ui.small_button("✕").on_hover_text(t!("Alanı sil", "Remove field")).clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
                if f.kind == FieldKind::Totp && !f.value.trim().is_empty() {
                    ui.label("");
                    match totp::Totp::parse(&f.value) {
                        Ok(tp) => {
                            let code = tp.code(unix);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(totp::grouped(&code)).monospace().size(20.0));
                                ui.weak(format!("{} sn", tp.remaining(unix)));
                                if ui.small_button(t!("Kodu kopyala", "Copy code")).clicked() {
                                    copy = Some(Zeroizing::new(code.to_string()));
                                }
                            });
                        }
                        Err(_) => {
                            ui.colored_label(Color32::LIGHT_RED, t!("Geçersiz 2FA anahtarı.", "Invalid 2FA key."));
                        }
                    }
                    ui.label("");
                    ui.end_row();
                }
            }
        });
        if let Some(i) = remove {
            e.body.fields.remove(i);
            e.reveal.clear();
        }
        ui.horizontal(|ui| {
            ui.menu_button(t!("+ Alan", "+ Field"), |ui| {
                for (k, n) in [(FieldKind::Text, t!("Metin", "Text")), (FieldKind::Secret, t!("Gizli", "Secret")), (FieldKind::Url, t!("Adres", "Website")), (FieldKind::Multiline, t!("Not", "Note")), (FieldKind::Totp, "2FA")] {
                    if ui.button(n).clicked() {
                        e.body.fields.push(Field::new(n, k));
                        ui.close();
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.weak(t!("Üretici:", "Generator:"));
            ui.add(egui::DragValue::new(&mut self.gen_len).range(gen::MIN_LEN..=gen::MAX_LEN).suffix(t!(" karakter", " chars")));
            ui.checkbox(&mut self.gen_symbols, t!("Simgeler", "Symbols"));
        });
        let Some(e) = self.editor.as_mut() else { return };

        ui.separator();
        ui.label(RichText::new(t!("Ek dosya", "Attachment")).strong());
        match &e.body.file {
            Some(f) => {
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({} KB)", f.name, f.size.div_ceil(1024)));
                    if ui.button(t!("Dışa kaydet…", "Save as…")).clicked() {
                        export_file = true;
                    }
                    if ui.button(t!("Kaldır", "Remove")).clicked() {
                        remove_file = true;
                    }
                });
            }
            None => {
                if ui.button(t!("Dosya ekle…", "Attach file…")).clicked() {
                    attach = true;
                }
            }
        }

        if !e.body.passkeys.is_empty() {
            ui.separator();
            ui.label(RichText::new("Passkey").strong());
            let mut drop_pk = None;
            for (i, k) in e.body.passkeys.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} · {}", k.rp_id, k.user_name));
                    if ui.small_button(t!("Sil", "Delete")).clicked() {
                        drop_pk = Some(i);
                    }
                });
            }
            if let Some(i) = drop_pk {
                e.body.passkeys.remove(i);
                self.toast(t!("Passkey kaldırıldı; kaydetmeyi unutma.", "Passkey removed; don't forget to save."));
            }
        }

        let Some(e) = self.editor.as_mut() else { return };
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(RichText::new(t!("Kaydet", "Save")).strong()).clicked() {
                save = true;
            }
            if ui.button(t!("Kapat", "Close")).clicked() {
                close = true;
            }
            if e.id.is_some() {
                if e.confirm_delete {
                    ui.colored_label(Color32::LIGHT_RED, t!("Silinsin mi? Geri alınamaz.", "Delete? This can't be undone."));
                    if ui.button(t!("Evet, sil", "Yes, delete")).clicked() {
                        delete = true;
                    }
                    if ui.button(t!("Vazgeç", "Cancel")).clicked() {
                        e.confirm_delete = false;
                    }
                } else if ui.button(t!("Sil", "Delete")).clicked() {
                    e.confirm_delete = true;
                }
            }
        });

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
        ui.heading(t!("Ayarlar", "Settings"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(t!("Otomatik kilit (dakika, 0: kapalı)", "Auto-lock (minutes, 0: off)"));
            ui.add(egui::DragValue::new(&mut s.auto_lock).range(0..=720));
        });
        ui.horizontal(|ui| {
            ui.label(t!("Panoyu temizle (saniye, 0: kapalı)", "Clear clipboard (seconds, 0: off)"));
            ui.add(egui::DragValue::new(&mut s.clipboard).range(0..=600));
        });
        ui.checkbox(&mut s.browser_fill, t!("Tarayıcı eklentisiyle doldurma, kaydetme ve passkey", "Browser extension filling, saving and passkeys"));
        ui.checkbox(&mut s.keep_running, t!("Pencere kapatılınca arka planda çalışmaya devam et", "Keep running in the background when the window is closed"));
        ui.horizontal(|ui| {
            ui.label(t!("Dil", "Language"));
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
        ui.weak(t!(format!("Köprü: 127.0.0.1:{}", crate::bridge::port()), format!("Bridge: 127.0.0.1:{}", crate::bridge::port())));

        ui.separator();
        ui.label(RichText::new(t!("Ana parolayı değiştir", "Change master password")).strong());
        password_edit(ui, &mut self.old_pw, t!("Şu anki parola", "Current password"));
        password_edit(ui, &mut self.new_pw, t!("Yeni parola", "New password"));
        password_edit(ui, &mut self.new_pw2, t!("Yeni parola (tekrar)", "New password (again)"));
        if ui.button(t!("Parolayı değiştir", "Change password")).clicked() {
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

        ui.separator();
        ui.label(RichText::new(t!("Kurtarma anahtarı", "Recovery key")).strong());
        let has = self.shared.core().vault.as_ref().is_some_and(|v| v.has_recovery());
        ui.weak(if has { t!("Kurtarma anahtarı var.", "A recovery key is set.") } else { t!("Kurtarma anahtarı yok.", "No recovery key is set.") });
        ui.horizontal(|ui| {
            if ui.button(t!("Yeni anahtar oluştur", "Create a new key")).clicked() {
                let r = self.shared.core().vault.as_mut().ok_or_default().and_then(|v| v.new_recovery().map_err(|e| e.to_string()));
                match r {
                    Ok(k) => self.recovery = Some(k),
                    Err(m) => self.settings_msg = m,
                }
            }
            if has && ui.button(t!("Anahtarı kaldır", "Remove the key")).clicked() {
                let r = self.shared.core().vault.as_mut().ok_or_default().and_then(|v| v.remove_recovery().map_err(|e| e.to_string()));
                self.settings_msg = match r {
                    Ok(()) => t!("Kurtarma anahtarı kaldırıldı.", "Recovery key removed.").into(),
                    Err(m) => m,
                };
            }
        });

        ui.separator();
        ui.label(RichText::new(t!("Yedek", "Backup")).strong());
        ui.weak(t!("Şifreli .carlvault yedeği (Windows'taki Carl Vault'un dışa aktarımı) geri yüklenebilir; yedek almak ana parolayla şifrelenir.", "An encrypted .carlvault backup (exported from Carl Vault on Windows) can be restored; new backups are encrypted with the master password."));
        ui.horizontal(|ui| {
            if ui.button(t!("Yedekten geri yükle…", "Restore a backup…")).clicked() {
                self.pick_backup();
            }
            if ui.button(t!("Yedek al…", "Back up…")).clicked() {
                self.backup();
            }
        });
        let mut run = false;
        let mut cancel = false;
        if let Some(r) = self.restore.as_mut() {
            ui.label(&r.path);
            let enter = ui.add(egui::TextEdit::singleline(&mut *r.password).password(true).hint_text(t!("Yedeğin parolası", "Backup password"))).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                run = ui.button(t!("Geri yükle", "Restore")).clicked() || enter;
                cancel = ui.button(t!("Vazgeç", "Cancel")).clicked();
            });
        }
        if cancel {
            self.restore = None;
        }
        if run {
            self.settings_msg = self.run_restore();
        }
        if !self.settings_msg.is_empty() {
            ui.add_space(8.0);
            ui.label(&self.settings_msg);
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
