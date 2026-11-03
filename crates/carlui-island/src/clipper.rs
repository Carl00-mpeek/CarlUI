//! Bu modül adadaki pano geçmişinin mantığını çiziminden ve pencere işlerinden
//! ayrı tutuyor: yazarak süzme, seçim, kaydırma, eylem listesi (Tab / sağ tık)
//! ve metin dönüşümleri. Kayıtlar `carlui-clip::store` içinde, birden çok iş
//! parçacığı (panoyu dinleyen işçi + arayüz) paylaştığı için Mutex arkasında.
//! Çizim `island/clipview.rs`, pencere tarafı `clipping.rs` içindedir.

use carlui_clip::{
    store::{Clip, Kind, Store},
    transform::{self, Op},
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use windows::Win32::Foundation::HWND;

pub const ROWS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipMode {
    List,
    Actions,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Paste,
    Copy,
    Edit,
    Pin,
    Delete,
    Clear,
    Wrap,
    Transform(Op),
}

pub struct Action {
    pub label: String,
    pub key: &'static str,
    pub glyph: char,
    pub act: Act,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipKey {
    Esc,
    Enter { shift: bool },
    Tab,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Back { ctrl: bool },
    Delete,
    Edit,
    Pin,
    Wrap,
    Undo,
}

pub enum ClipOut {
    Nothing,
    Redraw,
    Close(bool),
    /// Seçileni panoya koy; `paste`: önceki pencereye yapıştır da.
    Use { id: String, paste: bool },
    Edit(String),
    /// Önizlemede satır kaydırmayı aç/kapat.
    Wrap,
}

/// Çizim için anlık görüntü.
pub struct ClipView<'a> {
    pub store: &'a Store,
    pub items: Vec<&'a Clip>,
    pub filter: &'a str,
    pub sel: usize,
    pub scroll: usize,
    pub mode: ClipMode,
    pub actions: &'a [Action],
    pub act_sel: usize,
    pub confirm_clear: bool,
    /// Seçili kayıt için "Silinsin mi? ✓ ✗" sorusu açık.
    pub confirm_delete: bool,
    /// Az önce silinen kayıt geri alınabilir: ipucu satırında "Geri al".
    pub undo: bool,
    pub wrap: bool,
    pub hint: &'static str,
}

pub struct Clipper {
    pub store: Arc<Mutex<Store>>,
    pub open: bool,
    pub filter: String,
    /// Süzgece uyan kayıtların kimlikleri (görünen sıra).
    shown: Vec<String>,
    pub sel: usize,
    pub scroll: usize,
    pub mode: ClipMode,
    actions: Vec<Action>,
    act_sel: usize,
    confirm_clear: bool,
    /// Silme onayı bekleyen kayıt (ikinci onay: ✓ sil, ✗ vazgeç).
    confirm_delete: Option<String>,
    /// Son silinen kayıt ve eski sırası; süre dolunca ya da panel kapanınca
    /// `finish_undo` dosyasını da siler.
    undo: Option<(usize, Clip, Instant)>,
    pub wrap: bool,
    pub prev_fg: HWND,
    last_click: Option<(usize, Instant)>,
    high_surrogate: Option<u16>,
}

/// Arama için harf katlama: Türkçe I/İ/ı/i hepsi "i" sayılır, gerisi küçük harf.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'İ' | 'I' | 'ı' => vec!['i'],
            _ => c.to_lowercase().collect(),
        })
        .collect()
}

fn matches(c: &Clip, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let hay = match c.kind {
        Kind::Image => format!("resim görsel image picture {}x{}", c.w, c.h),
        // Gizli kaydın içeriği aramayla da açığa çıkmasın.
        _ if c.secret => "gizli şifre anahtar secret password key".into(),
        _ => fold(&c.text),
    };
    q.split_whitespace().all(|w| hay.contains(w))
}

impl Clipper {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            store,
            open: false,
            filter: String::new(),
            shown: Vec::new(),
            sel: 0,
            scroll: 0,
            mode: ClipMode::List,
            actions: Vec::new(),
            act_sel: 0,
            confirm_clear: false,
            confirm_delete: None,
            undo: None,
            wrap: true,
            prev_fg: HWND::default(),
            last_click: None,
            high_surrogate: None,
        }
    }

    pub fn open(&mut self, prev: HWND) {
        self.open = true;
        self.prev_fg = prev;
        self.filter.clear();
        self.mode = ClipMode::List;
        self.sel = 0;
        self.scroll = 0;
        self.confirm_clear = false;
        self.confirm_delete = None;
        self.rebuild();
    }

    /// Geri alma süresi bitti: silinen kaydın dosyasını da sil. Şerit açıktıysa true.
    pub fn finish_undo(&mut self) -> bool {
        let Some((_, c, _)) = self.undo.take() else { return false };
        if let Ok(s) = self.store.lock() {
            s.forget(&c);
        }
        true
    }

    /// Son silineni eski yerine koyup seçer.
    pub fn undo(&mut self) -> ClipOut {
        let Some((at, c, _)) = self.undo.take() else { return ClipOut::Nothing };
        let id = c.id.clone();
        if let Ok(mut s) = self.store.lock() {
            s.restore(at, c);
        }
        self.rebuild();
        if let Some(i) = self.shown.iter().position(|x| *x == id) {
            self.sel = i;
            self.fix_scroll();
        }
        ClipOut::Redraw
    }

    pub fn undo_pending(&self) -> bool {
        self.undo.is_some()
    }

    /// Geri alma şeridi şu kadar ms'dir açık.
    pub fn undo_age_ms(&self) -> u128 {
        self.undo.as_ref().map(|u| u.2.elapsed().as_millis()).unwrap_or(0)
    }

    pub fn close(&mut self) {
        self.finish_undo();
        self.open = false;
        self.mode = ClipMode::List;
        self.prev_fg = HWND::default();
    }

    /// Kayıtlar ya da süzgeç değişti: görünen listeyi yeniden kur, seçimi koru.
    pub fn rebuild(&mut self) {
        let keep = self.selected_id();
        let q = fold(&self.filter);
        let Ok(st) = self.store.lock() else { return };
        // Sabitlenenler üstte, sonra en yeniden eskiye.
        let mut v: Vec<&Clip> = st.items.iter().filter(|c| matches(c, &q)).collect();
        v.sort_by_key(|c| !c.pinned);
        self.shown = v.iter().map(|c| c.id.clone()).collect();
        drop(st);
        self.sel = keep.and_then(|id| self.shown.iter().position(|x| *x == id)).unwrap_or(self.sel).min(self.shown.len().saturating_sub(1));
        self.fix_scroll();
    }

    fn fix_scroll(&mut self) {
        let (n, sel) = match self.mode {
            ClipMode::List => (self.shown.len(), self.sel),
            ClipMode::Actions => (self.actions.len(), self.act_sel),
        };
        if sel < self.scroll {
            self.scroll = sel;
        } else if sel >= self.scroll + ROWS {
            self.scroll = sel + 1 - ROWS;
        }
        self.scroll = self.scroll.min(n.saturating_sub(ROWS));
    }

    pub fn selected_id(&self) -> Option<String> {
        self.shown.get(self.sel).cloned()
    }

    pub fn view<'a>(&'a self, st: &'a Store) -> ClipView<'a> {
        let items: Vec<&Clip> = self.shown.iter().filter_map(|id| st.get(id)).collect();
        let hint = match self.mode {
            ClipMode::Actions => t!("↑↓ seç   Enter uygula   Esc geri", "↑↓ select   Enter apply   Esc back"),
            ClipMode::List if self.undo.is_some() => t!("Kayıt silindi", "Item deleted"),
            ClipMode::List => t!("Enter yapıştır · Shift+Enter kopyala · Tab diğer", "Enter paste · Shift+Enter copy · Tab more"),
        };
        ClipView {
            store: st,
            items,
            filter: &self.filter,
            sel: self.sel,
            scroll: self.scroll,
            mode: self.mode,
            actions: &self.actions,
            act_sel: self.act_sel,
            confirm_clear: self.confirm_clear,
            confirm_delete: self.confirm_delete.is_some() && self.confirm_delete == self.selected_id(),
            undo: self.undo.is_some() && self.mode == ClipMode::List,
            wrap: self.wrap,
            hint,
        }
    }

    pub fn debug(&self) -> serde_json::Value {
        serde_json::json!({
            "open": self.open,
            "filter": self.filter,
            "shown": self.shown.len(),
            "sel": self.sel,
            "sel_id": self.selected_id(),
            "mode": format!("{:?}", self.mode),
            "actions": self.actions.iter().map(|a| a.label.clone()).collect::<Vec<_>>(),
            "act_sel": self.act_sel,
            "wrap": self.wrap,
            "confirm_delete": self.confirm_delete,
            "undo": self.undo.as_ref().map(|(i, c, _)| format!("{i}:{}", c.id)),
        })
    }

    fn open_actions(&mut self) -> ClipOut {
        let Some(id) = self.selected_id() else { return ClipOut::Nothing };
        let Ok(st) = self.store.lock() else { return ClipOut::Nothing };
        let Some(c) = st.get(&id) else { return ClipOut::Nothing };
        let mut a = vec![
            Action { label: t!("Yapıştır", "Paste").into(), key: "Enter", glyph: '\u{E77F}', act: Act::Paste },
            Action { label: t!("Yalnızca panoya kopyala", "Copy to clipboard only").into(), key: "Shift+Enter", glyph: '\u{E8C8}', act: Act::Copy },
        ];
        if c.kind == Kind::Text {
            a.push(Action { label: t!("Metni düzenle", "Edit text").into(), key: "Ctrl+E", glyph: '\u{E70F}', act: Act::Edit });
            for (op, _, glyph) in transform::ALL {
                let label = transform::name(*op);
                a.push(Action { label: t!(format!("{label} (yeni kayıt)"), format!("{label} (new item)")), key: "", glyph: *glyph, act: Act::Transform(*op) });
            }
            let w = if self.wrap { t!("Önizlemede satır kaydırmayı kapat", "Turn off line wrap in preview") } else { t!("Önizlemede satır kaydırmayı aç", "Turn on line wrap in preview") };
            a.push(Action { label: w.into(), key: "Alt+Z", glyph: '\u{E8C1}', act: Act::Wrap });
        }
        a.push(Action { label: if c.pinned { t!("Sabitlemeyi kaldır", "Unpin").into() } else { t!("Sabitle", "Pin").into() }, key: "Ctrl+P", glyph: '\u{E718}', act: Act::Pin });
        a.push(Action { label: t!("Sil", "Delete").into(), key: "Delete", glyph: '\u{E74D}', act: Act::Delete });
        a.push(Action { label: t!("Sabitlenmemişlerin hepsini temizle", "Clear all unpinned").into(), key: "", glyph: '\u{E894}', act: Act::Clear });
        drop(st);
        self.actions = a;
        self.act_sel = 0;
        self.mode = ClipMode::Actions;
        self.scroll = 0;
        ClipOut::Redraw
    }

    fn back_to_list(&mut self) {
        self.mode = ClipMode::List;
        self.confirm_clear = false;
        self.scroll = 0;
        self.fix_scroll();
    }

    pub fn run(&mut self, act: Act) -> ClipOut {
        let Some(id) = self.selected_id() else { return ClipOut::Nothing };
        let out = match act {
            Act::Paste => return ClipOut::Use { id, paste: true },
            Act::Copy => return ClipOut::Use { id, paste: false },
            Act::Edit => {
                let is_text = self.store.lock().ok().and_then(|s| s.get(&id).map(|c| c.kind == Kind::Text)).unwrap_or(false);
                return if is_text { ClipOut::Edit(id) } else { ClipOut::Nothing };
            }
            Act::Wrap => {
                self.wrap = !self.wrap;
                ClipOut::Wrap
            }
            Act::Pin => {
                if let Ok(mut s) = self.store.lock() {
                    s.toggle_pin(&id);
                }
                ClipOut::Redraw
            }
            Act::Delete => {
                // İlk basış yalnızca soruyu açar; silme ✓ (ya da tekrar Sil/Enter) ile olur.
                if self.confirm_delete.as_deref() != Some(id.as_str()) {
                    self.confirm_delete = Some(id);
                    self.back_to_list();
                    return ClipOut::Redraw;
                }
                self.confirm_delete = None;
                // Önceki silinen artık geri alınamaz; bu yenisi birkaç saniye geri alınabilir.
                self.finish_undo();
                if let Ok(mut s) = self.store.lock() {
                    self.undo = s.take(&id).map(|(i, c)| (i, c, Instant::now()));
                }
                ClipOut::Redraw
            }
            Act::Clear => {
                if !self.confirm_clear {
                    self.confirm_clear = true;
                    return ClipOut::Redraw;
                }
                if let Ok(mut s) = self.store.lock() {
                    s.clear();
                }
                self.sel = 0;
                ClipOut::Redraw
            }
            Act::Transform(op) => {
                // Asıl kayıt bozulmasın: sonuç yeni kayıt olarak en üste eklenir ve seçilir.
                let mut new_id = None;
                if let Ok(mut s) = self.store.lock() {
                    if let Some(t) = s.get(&id).filter(|c| c.kind == Kind::Text).map(|c| transform::apply(op, &c.text)) {
                        if !t.trim().is_empty() {
                            s.add(carlui_clip::capture::Content::Text(t));
                            new_id = s.items.first().map(|c| c.id.clone());
                        }
                    }
                }
                self.filter.clear();
                self.back_to_list();
                self.rebuild();
                if let Some(n) = new_id.and_then(|n| self.shown.iter().position(|x| *x == n)) {
                    self.sel = n;
                    self.fix_scroll();
                }
                return ClipOut::Redraw;
            }
        };
        self.back_to_list();
        self.rebuild();
        out
    }

    fn step(&mut self, d: i32) -> ClipOut {
        match self.mode {
            ClipMode::List => {
                if self.shown.is_empty() {
                    return ClipOut::Nothing;
                }
                self.sel = (self.sel as i32 + d).clamp(0, self.shown.len() as i32 - 1) as usize;
            }
            ClipMode::Actions => {
                self.act_sel = (self.act_sel as i32 + d).clamp(0, self.actions.len() as i32 - 1) as usize;
                self.confirm_clear = false;
            }
        }
        self.fix_scroll();
        ClipOut::Redraw
    }

    /// Silme sorusunu kapat (✗). Açık bir soru varsa true.
    pub fn cancel_delete(&mut self) -> bool {
        self.confirm_delete.take().is_some()
    }

    pub fn key(&mut self, k: ClipKey) -> ClipOut {
        // Silme sorusu açıkken: Enter/Delete onaylar, Esc vazgeçer, başka tuş soruyu kapatıp işini yapar.
        if self.confirm_delete.is_some() {
            match k {
                ClipKey::Esc => {
                    self.confirm_delete = None;
                    return ClipOut::Redraw;
                }
                ClipKey::Enter { .. } | ClipKey::Delete => return self.run(Act::Delete),
                _ => self.confirm_delete = None,
            }
        }
        match k {
            ClipKey::Esc => {
                if self.mode == ClipMode::Actions {
                    self.back_to_list();
                    return ClipOut::Redraw;
                }
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.rebuild();
                    return ClipOut::Redraw;
                }
                ClipOut::Close(true)
            }
            ClipKey::Enter { shift } => match self.mode {
                ClipMode::List => self.run(if shift { Act::Copy } else { Act::Paste }),
                ClipMode::Actions => match self.actions.get(self.act_sel).map(|a| a.act) {
                    Some(a) => self.run(a),
                    None => ClipOut::Nothing,
                },
            },
            ClipKey::Tab => {
                if self.mode == ClipMode::Actions {
                    self.back_to_list();
                    ClipOut::Redraw
                } else {
                    self.open_actions()
                }
            }
            ClipKey::Up => self.step(-1),
            ClipKey::Down => self.step(1),
            ClipKey::PageUp => self.step(-(ROWS as i32)),
            ClipKey::PageDown => self.step(ROWS as i32),
            ClipKey::Home => self.step(-100_000),
            ClipKey::End => self.step(100_000),
            ClipKey::Back { ctrl } => {
                if self.filter.is_empty() {
                    return ClipOut::Nothing;
                }
                if ctrl {
                    self.filter.clear();
                } else {
                    self.filter.pop();
                }
                self.sel = 0;
                self.rebuild();
                ClipOut::Redraw
            }
            ClipKey::Delete if self.mode == ClipMode::List => self.run(Act::Delete),
            ClipKey::Edit => self.run(Act::Edit),
            ClipKey::Pin => self.run(Act::Pin),
            ClipKey::Wrap => self.run(Act::Wrap),
            ClipKey::Undo => self.undo(),
            _ => ClipOut::Nothing,
        }
    }

    /// WM_CHAR: süzgece yazı.
    pub fn char(&mut self, u: u16) -> ClipOut {
        if (0xD800..0xDC00).contains(&u) {
            self.high_surrogate = Some(u);
            return ClipOut::Nothing;
        }
        let units: Vec<u16> = match self.high_surrogate.take() {
            Some(h) if (0xDC00..0xE000).contains(&u) => vec![h, u],
            _ => vec![u],
        };
        let s = String::from_utf16_lossy(&units);
        if s.chars().any(|c| c.is_control()) {
            return ClipOut::Nothing;
        }
        if self.mode == ClipMode::Actions {
            self.back_to_list();
        }
        self.confirm_delete = None;
        self.filter.push_str(&s);
        self.sel = 0;
        self.rebuild();
        ClipOut::Redraw
    }

    /// Satıra tıklandı: seç; aynı satıra çabuk ikinci tıklama yapıştırır.
    pub fn click(&mut self, i: usize, double_ms: u32) -> ClipOut {
        if self.mode == ClipMode::Actions {
            self.act_sel = i.min(self.actions.len().saturating_sub(1));
            return match self.actions.get(self.act_sel).map(|a| a.act) {
                Some(a) => self.run(a),
                None => ClipOut::Nothing,
            };
        }
        if i >= self.shown.len() {
            return ClipOut::Nothing;
        }
        let double = self.last_click.is_some_and(|(j, t)| j == i && t.elapsed().as_millis() <= double_ms as u128);
        self.sel = i;
        self.confirm_delete = None;
        if double {
            self.last_click = None;
            return self.run(Act::Paste);
        }
        self.last_click = Some((i, Instant::now()));
        ClipOut::Redraw
    }

    /// Sağ tık: o satırı seçip eylemleri aç.
    pub fn context(&mut self, i: usize) -> ClipOut {
        if self.mode == ClipMode::Actions {
            self.back_to_list();
            return ClipOut::Redraw;
        }
        if i >= self.shown.len() {
            return ClipOut::Nothing;
        }
        self.sel = i;
        self.open_actions()
    }

    pub fn hover(&mut self, i: usize) -> bool {
        if self.mode == ClipMode::Actions && i < self.actions.len() && self.act_sel != i {
            self.act_sel = i;
            self.confirm_clear = false;
            return true;
        }
        false
    }

    /// Liste üstünde tekerlek.
    pub fn wheel(&mut self, rows: i32) -> bool {
        let n = match self.mode {
            ClipMode::List => self.shown.len(),
            ClipMode::Actions => self.actions.len(),
        };
        let max = n.saturating_sub(ROWS) as i32;
        let new = (self.scroll as i32 - rows).clamp(0, max) as usize;
        if new == self.scroll {
            return false;
        }
        self.scroll = new;
        // Seçim görünür alanda kalsın.
        match self.mode {
            ClipMode::List => self.sel = self.sel.clamp(self.scroll, self.scroll + ROWS - 1).min(n.saturating_sub(1)),
            ClipMode::Actions => self.act_sel = self.act_sel.clamp(self.scroll, self.scroll + ROWS - 1).min(n.saturating_sub(1)),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn turkish_fold() {
        assert_eq!(super::fold("İstanbul ILIK"), "istanbul ilik");
        assert!(super::fold("Merhaba İstanbul").contains(&super::fold("istanbul")));
    }
}
