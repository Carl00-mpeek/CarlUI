//! Görünüm sayfasının en üstündeki "Görünümler" bölümü: hazır ve kullanıcının
//! kaydettiği görünümler küçük önizlemeli kartlar halinde. Karta tıklamak
//! görünümü hemen uygular (tek adım "Geri al" ile). Hangi kartın seçili olduğu
//! saklanmaz; ayarlara bakılarak hesaplanır, elle bir değer değişince "Özel"
//! görünür. Kullanıcı görünümleri kaydedilir, yeniden adlandırılır, iki
//! tıklamayla silinir ve `.carllook` dosyası olarak paylaşılır.

use super::{looks_io, looks_preview};
use crate::{
    input::{Form, Input},
    ui::{Flow, P},
    Act, App,
};
use carlui_core::looks::{self, builtin, sanitize, store::Store, Look, Snapshot, Which};
use carlui_ui::{
    paint::{Align, R, RED, TEXT},
    tween,
};

/// Kartın en dar hali ve kartlar arası boşluk.
const CARD_MIN: f32 = 150.0;
const GAP: f32 = 8.0;
const PREVIEW_H: f32 = 76.0;
const NAME_H: f32 = 30.0;
const ICON: f32 = 22.0;

/// Hangi görünüm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Builtin(usize),
    User(usize),
}

/// Görünümler bölümündeki eylemler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookAct {
    Apply(Src),
    /// Son uygulanan görünümden önceki görünüşe dön.
    Undo,
    /// Şu anki görünüşü adla kaydet (formu açar).
    Save,
    Rename(usize),
    /// İlk tıklama onay ister, ikincisi siler.
    Delete(usize),
    /// Kullanıcı görünümünü ya da (`None`) şu anki görünüşü dosyaya yaz.
    Export(Option<usize>),
    Import,
}

/// Bölümün durumu (ayar dosyasında değil, pencere açıkken bellekte).
#[derive(Default)]
pub struct State {
    pub user: Store,
    /// Tek adımlık geri alma: uygulanmadan önceki görünüş.
    pub undo: Option<Snapshot>,
    /// Silme onayı bekleyen kullanıcı görünümü.
    pub confirm: Option<usize>,
    /// Yeniden adlandırılan kullanıcı görünümü.
    pub rename: Option<usize>,
    /// Son işlemin sonucu (kaydedildi, içe aktarıldı, hata…).
    pub note: Option<String>,
}

impl State {
    pub fn load() -> Self {
        Self { user: Store::load(), ..Self::default() }
    }
}

fn look_of(app: &App, src: Src) -> Option<Look> {
    match src {
        Src::Builtin(i) => builtin::all().into_iter().nth(i),
        Src::User(i) => app.looks.user.looks.get(i).cloned(),
    }
}

/// Şu an denk gelinen görünümün adı; hiçbiri değilse `None` ("Özel").
fn current_name(app: &App, builtins: &[Look]) -> Option<String> {
    match looks::current(&app.cfg, builtins, &app.looks.user.looks) {
        Which::Builtin(i) => builtins.get(i).map(|l| l.name.clone()),
        Which::User(i) => app.looks.user.looks.get(i).map(|l| l.name.clone()),
        Which::Custom => None,
    }
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let builtins = builtin::all();
    let user = &app.looks.user.looks;
    let which = looks::current(&app.cfg, &builtins, user);
    f.section_info(
        p,
        t!("Görünümler", "Looks"),
        t!(
            "Bir görünüm tema, vurgu rengi, cam, köşeler, dock'un şekli, ada stili ve imleci birlikte değiştirir. Sabitlenmiş uygulamalar, dock davranışı, kısayollar, dil ve hareketi azalt değişmez. Aşağıdan bir ayarı elle değiştirince görünüm \"Özel\" olur; beğendiğini + ile kaydedebilirsin.",
            "A look changes the theme, accent color, glass, corners, dock shape, island style and cursor together. Pinned apps, dock behavior, shortcuts, language and reduce motion stay as they are. Changing a setting below by hand makes it \"Custom\"; save the ones you like with +."
        ),
    );
    f.flush(p);
    let cols = ((f.w + GAP) / (CARD_MIN + GAP)).floor().max(2.0) as usize;
    let cw = (f.w - GAP * (cols as f32 - 1.0)) / cols as f32;
    let ch = PREVIEW_H + NAME_H + 8.0;
    let n = builtins.len() + user.len() + 1;
    let wall = app.cfg.theme.wallpaper_accent.as_str();
    for k in 0..n {
        let r = R::new(f.x + (k % cols) as f32 * (cw + GAP), f.y + (k / cols) as f32 * (ch + GAP), cw, ch);
        if k < builtins.len() {
            card(app, p, r, &builtins[k], Src::Builtin(k), which == Which::Builtin(k), wall);
        } else if k < n - 1 {
            let i = k - builtins.len();
            card(app, p, r, &user[i], Src::User(i), which == Which::User(i), wall);
        } else {
            add_card(app, p, r);
        }
    }
    f.y += n.div_ceil(cols) as f32 * (ch + GAP) - GAP + 10.0;

    // Şu anki görünüm satırı: ad ya da "Özel", paylaşım ve geri alma.
    let name = current_name(app, &builtins);
    let sub = match &name {
        Some(n) => n.clone(),
        None => t!("Özel · kayıtlı bir görünüme denk gelmiyor", "Custom · doesn't match a saved look").to_string(),
    };
    let bw = 112.0;
    let buttons = if app.looks.undo.is_some() { 3.0 } else { 2.0 };
    let r = f.row_sub(
        p,
        t!("Şu anki görünüm", "Current look"),
        t!(
            "Dışa aktar, şu anki görünüşü .carllook dosyasına yazar; dosyayı başkasıyla paylaşabilirsin. İçe aktar, dosyadaki görünümü listene ekler ve uygular. Dosyada yalnızca görünüm değerleri vardır. Geri al, son uygulanan görünümden önceki hale döner.",
            "Export writes the current appearance to a .carllook file you can share. Import adds the look from a file to your list and applies it. The file holds only appearance values. Undo returns to how things looked before the last look was applied."
        ),
        &sub,
        if name.is_none() { Some(p.accent) } else { None },
        buttons * (bw + 8.0),
    );
    let by = r.y + (r.h - 32.0) / 2.0;
    let mut x = r.right() - 16.0 - bw;
    let free = app.input.is_none();
    p.button(R::new(x, by, bw, 32.0), Some('\u{E8B5}'), t!("İçe aktar", "Import"), Act::Look(LookAct::Import), free, false);
    x -= bw + 8.0;
    p.button(R::new(x, by, bw, 32.0), Some('\u{E72D}'), t!("Dışa aktar", "Export"), Act::Look(LookAct::Export(None)), free, false);
    if app.looks.undo.is_some() {
        x -= bw + 8.0;
        p.button(R::new(x, by, bw, 32.0), Some('\u{E7A7}'), t!("Geri al", "Undo"), Act::Look(LookAct::Undo), free, false);
    }
    if let Some(n) = &app.looks.note {
        f.note(p, n);
    }
    super::form::draw(app, p, f, Form::LookName, &[(t!("Görünümün adı", "Look name"), t!("ör. Akşam", "e.g. Evening"))]);
    super::form::draw(app, p, f, Form::LookRename, &[(t!("Yeni ad", "New name"), "")]);
}

/// Tek görünüm kartı: önizleme, ad; seçiliyse vurgu çerçevesi ve tik.
#[allow(clippy::too_many_arguments)]
fn card(app: &App, p: &mut P, r: R, look: &Look, src: Src, selected: bool, wall: &str) {
    let hot = p.hover(r);
    let hv = tween::to(tween::at("look-card", r.x, r.y), if hot { 1.0 } else { 0.0 }, 18.0);
    let sel = tween::to(tween::at("look-sel", r.x, r.y), if selected { 1.0 } else { 0.0 }, 14.0);
    p.card(r);
    if hv > 0.0 {
        p.round(r, 8.0, 0xFFFFFF, 0.04 * hv);
    }
    if sel > 0.0 {
        p.outline(r, 8.0, p.accent, 0.95 * sel);
        p.outline(r.inset(1.0), 7.0, p.accent, 0.6 * sel);
    }
    p.hit(r, Act::Look(LookAct::Apply(src)));
    looks_preview::draw(p, R::new(r.x + 8.0, r.y + 8.0, r.w - 16.0, PREVIEW_H), look, wall);

    let ny = r.y + 8.0 + PREVIEW_H;
    let mut tx = r.x + 10.0;
    if sel > 0.01 {
        p.glyph('\u{E73E}', R::new(tx, ny, 16.0, NAME_H), 12.0, p.accent, sel);
        tx += 18.0;
    }
    let Src::User(i) = src else {
        p.text(&look.name, R::new(tx, ny, r.right() - 8.0 - tx, NAME_H), 13.0, selected, TEXT, 0.9, Align::Left);
        return;
    };
    let confirm = app.looks.confirm == Some(i);
    // Kullanıcı görünümünde sağda küçük düğmeler: dışa aktar, yeniden adlandır, sil.
    let tools = hot || confirm;
    let right = if tools { r.right() - 6.0 - if confirm { sure_w(p) } else { 3.0 * ICON + 4.0 } } else { r.right() - 8.0 };
    p.text(&look.name, R::new(tx, ny, right - tx - 4.0, NAME_H), 13.0, selected, TEXT, 0.9, Align::Left);
    if !tools || app.input.is_some() {
        return;
    }
    let y = ny + (NAME_H - ICON) / 2.0;
    if confirm {
        // İkinci tıklama siler; başka herhangi bir tıklama onayı kaldırır.
        let w = sure_w(p);
        let b = R::new(r.right() - 6.0 - w, y, w, ICON);
        let h = p.hover(b);
        p.round(b, 5.0, RED, if h { 0.28 } else { 0.18 });
        p.text(t!("Emin misin?", "Sure?"), b, 12.0, true, RED, 1.0, Align::Center);
        p.hit(b, Act::Look(LookAct::Delete(i)));
        return;
    }
    let mut x = r.right() - 6.0 - ICON;
    for (g, act, danger) in [('\u{E74D}', LookAct::Delete(i), true), ('\u{E70F}', LookAct::Rename(i), false), ('\u{E72D}', LookAct::Export(Some(i)), false)] {
        let b = R::new(x, y, ICON, ICON);
        let h = p.hover(b);
        if h {
            p.round(b, 5.0, 0xFFFFFF, 0.12);
        }
        p.glyph(g, b, 11.0, if danger && h { RED } else { TEXT }, if h { 0.95 } else { 0.6 });
        p.hit(b, Act::Look(act));
        x -= ICON + 2.0;
    }
}

fn sure_w(p: &P) -> f32 {
    p.fonts.width(t!("Emin misin?", "Sure?"), 12.0, true) + 16.0
}

/// Son kart: şu anki görünüşü yeni görünüm olarak kaydet.
fn add_card(app: &App, p: &mut P, r: R) {
    let full = app.looks.user.looks.len() >= looks::store::MAX_LOOKS;
    let ok = !full && app.input.is_none();
    let hv = tween::to(tween::at("look-add", r.x, r.y), if ok && p.hover(r) { 1.0 } else { 0.0 }, 18.0);
    p.round(r, 8.0, 0xFFFFFF, 0.02 + 0.04 * hv);
    p.outline(r, 8.0, carlui_ui::paint::mix(0xFFFFFF, p.accent, hv), 0.12 + 0.3 * hv);
    let a = if ok { 0.85 } else { 0.35 };
    p.glyph('\u{E710}', R::new(r.x, r.y + 8.0, r.w, PREVIEW_H), 20.0, TEXT, a);
    let label = if full { t!("Liste dolu", "List is full") } else { t!("Şu ankini kaydet", "Save current") };
    p.text(label, R::new(r.x + 8.0, r.y + 8.0 + PREVIEW_H, r.w - 16.0, NAME_H), 13.0, false, TEXT, a, Align::Center);
    if ok {
        p.hit(r, Act::Look(LookAct::Save));
    }
}

/// Görünümler bölümünün eylemi.
pub fn act(app: &mut App, a: LookAct) {
    if !matches!(a, LookAct::Delete(_)) {
        app.looks.confirm = None;
    }
    match a {
        LookAct::Apply(src) => {
            let Some(look) = look_of(app, src) else { return };
            // Zaten denk geliyorsa geri alma noktası bozulmasın.
            if looks::matches(&look, &app.cfg) {
                return;
            }
            app.looks.undo = Some(looks::apply_undoable(&look, &mut app.cfg));
            app.looks.note = None;
            app.save();
        }
        LookAct::Undo => {
            if let Some(s) = app.looks.undo.take() {
                s.restore(&mut app.cfg);
                app.looks.note = None;
                app.save();
            }
        }
        LookAct::Save => {
            let builtins = builtin::all();
            let base = current_name(app, &builtins).unwrap_or_else(|| t!("Görünümüm", "My look").to_string());
            let mut i = Input::new(Form::LookName);
            i.name = app.looks.user.free_name(&base);
            app.input = Some(i);
        }
        LookAct::Rename(k) => {
            let Some(l) = app.looks.user.looks.get(k) else { return };
            let mut i = Input::new(Form::LookRename);
            i.name = l.name.clone();
            app.looks.rename = Some(k);
            app.input = Some(i);
        }
        LookAct::Delete(k) => {
            if app.looks.confirm != Some(k) {
                app.looks.confirm = Some(k);
                return;
            }
            app.looks.confirm = None;
            if let Some(l) = app.looks.user.remove(k) {
                app.looks.note = Some(match app.looks.user.save() {
                    Ok(()) => t!(format!("«{}» silindi.", l.name), format!("Deleted “{}”.", l.name)),
                    Err(e) => save_error(e),
                });
            }
        }
        LookAct::Export(k) => {
            let look = match k {
                Some(k) => app.looks.user.looks.get(k).cloned(),
                None => {
                    let builtins = builtin::all();
                    let name = current_name(app, &builtins).unwrap_or_else(|| t!("Görünümüm", "My look").to_string());
                    Some(looks::capture(&app.cfg, &name))
                }
            };
            let Some(look) = look else { return };
            match looks_io::export(&look) {
                Ok(false) => {}
                Ok(true) => app.looks.note = Some(t!(format!("«{}» dışa aktarıldı.", look.name), format!("Exported “{}”.", look.name))),
                Err(e) => app.looks.note = Some(e),
            }
        }
        LookAct::Import => match looks_io::import() {
            Ok(None) => {}
            Ok(Some(mut look)) => {
                look.name = app.looks.user.free_name(&look.name);
                if !app.looks.user.put(look.clone()) {
                    app.looks.note = Some(full_error());
                    return;
                }
                if let Err(e) = app.looks.user.save() {
                    app.looks.note = Some(save_error(e));
                    return;
                }
                if !looks::matches(&look, &app.cfg) {
                    app.looks.undo = Some(looks::apply_undoable(&look, &mut app.cfg));
                    app.save();
                }
                app.looks.note = Some(t!(format!("«{}» içe aktarıldı ve uygulandı.", look.name), format!("Imported and applied “{}”.", look.name)));
            }
            Err(e) => app.looks.note = Some(e),
        },
    }
}

fn save_error(e: std::io::Error) -> String {
    t!(format!("Görünümler kaydedilemedi: {e}"), format!("Couldn't save looks: {e}"))
}

fn full_error() -> String {
    let n = looks::store::MAX_LOOKS;
    t!(format!("En çok {n} görünüm saklanabilir; önce birini sil."), format!("You can keep up to {n} looks; delete one first."))
}

/// Bu form görünümler bölümünün mü?
pub fn owns(kind: Form) -> bool {
    matches!(kind, Form::LookName | Form::LookRename)
}

/// Görünüm adı formunu uygular; olmazsa hatayı forma yazar ve `false` döner.
pub fn submit(app: &mut App, i: &mut Input) -> bool {
    let name = sanitize::clean_name(&i.name);
    if name.is_empty() {
        i.error = Some(t!("Bir ad yazın.", "Enter a name.").into());
        return false;
    }
    let st = &mut app.looks;
    if i.kind == Form::LookRename {
        let Some(k) = st.rename else { return true };
        if !st.user.rename(k, &name) {
            i.error = Some(t!("Bu adda başka bir görünüm var.", "Another look already has this name.").into());
            return false;
        }
        st.rename = None;
        st.note = st.user.save().err().map(save_error);
        return true;
    }
    let existed = st.user.looks.iter().any(|l| l.name.to_lowercase() == name.to_lowercase());
    if !st.user.put(looks::capture(&app.cfg, &name)) {
        i.error = Some(full_error());
        return false;
    }
    st.note = Some(match st.user.save() {
        Ok(()) if existed => t!(format!("«{name}» güncellendi."), format!("Updated “{name}”.")),
        Ok(()) => t!(format!("«{name}» kaydedildi."), format!("Saved “{name}”.")),
        Err(e) => save_error(e),
    });
    true
}
