//! Bu modül adanın "üstüne gelince genişleme" davranışını ve saklanan
//! bildirimleri yönetiyor. Sorun: bildirim birkaç saniye görünüp kayboluyor,
//! kaçırılınca bir daha bulunamıyordu; sistem durumuna bakmak için de ayrı
//! bir uygulama açmak gerekiyordu. Çözüm:
//! - Gösterilen her bildirim süresi dolunca kaybolmaz, adada saklanır (rozet
//!   sayısı görünür). Yalnızca çöp kutusu ya da X ile silinir.
//! - Fare hapın üstünde `stats.hover_ms` kalınca üst satır (tarih + göstergeler)
//!   açılır; 2 sn kalınca bildirim listesi de açılır. Tıklamak beklemeden açar.
//! - Göstergeler yalnızca ada açıkken, saniyede bir okunur; boştayken hiçbir
//!   şey yoklanmaz. CPU sıcaklığı yönetici yardımcısının paylaştığı bellekten,
//!   GPU NVIDIA sürücüsünden, RAM Windows'tan gelir (bkz. `carlui-sys`).

use crate::*;
use island::{HubView, Stat};

pub const TIMER_HUB_PEEK: usize = 15;
pub const TIMER_HUB_FULL: usize = 16;
pub const TIMER_HUB_TICK: usize = 17;
/// Silinme animasyonu bitince bildirimler listeden gerçekten kalkar.
pub const TIMER_HUB_DEL: usize = 18;
const DEL_MS: u32 = 170;
/// Fare bu kadar kalınca bildirim listesi de açılır.
const FULL_MS: u32 = 2000;
/// Adada en fazla bu kadar bildirim saklanır (en eskiler düşer).
const KEEP_MAX: usize = 50;

#[derive(Default)]
pub struct Hub {
    /// 0 kapalı, 1 üst satır, 2 tam.
    pub level: u8,
    pub notes: Vec<Note>,
    next_id: u64,
    stats: Vec<Stat>,
    /// Fare hapın üstünde mi (bekleme süreleri bunu kullanır).
    hovering: bool,
    /// Silinme animasyonundaki bildirim kimlikleri.
    dying: Vec<u64>,
    /// Daraltılmış uygulama grupları (uygulama adıyla).
    collapsed: std::collections::HashSet<String>,
}

fn level_of(t: f32, warm: f32, hot: f32) -> u8 {
    if t >= hot { 2 } else if t >= warm { 1 } else { 0 }
}

impl Hub {
    pub fn view<'a>(&'a self, media: bool) -> HubView<'a> {
        HubView { full: self.level >= 2, stats: self.stats.clone(), notes: &self.notes, media, dying: &self.dying, collapsed: &self.collapsed }
    }

    /// Açık göstergeleri yeniden oku.
    pub fn sample(&mut self, cfg: &carlui_core::config::StatsConfig) {
        let mut v = Vec::new();
        if cfg.cpu_temp {
            match carlui_sys::temp::read() {
                Some(t) => v.push(("CPU", format!("{:.0}°", t.package), level_of(t.package, 75.0, 90.0))),
                None => v.push(("CPU", "—".into(), 0)),
            }
        }
        if cfg.gpu_temp {
            if let Some(g) = carlui_sys::gpu::read() {
                v.push(("GPU", format!("{:.0}°", g.temp), level_of(g.temp, 72.0, 84.0)));
            }
        }
        if cfg.ram {
            let r = carlui_sys::ram::read();
            let p = r.percent();
            v.push(("RAM", carlui_core::lang::pct(p), level_of(p, 80.0, 92.0)));
        }
        // Durum göstergeleri: yalnızca bir şey varken görünür. Başa eklenir ki
        // yer kalmazsa ayarlardan açılan CPU/GPU/RAM değil bunlar düşsün.
        let fixed = std::mem::take(&mut v);
        let dl = crate::downloads::active();
        if let Some(p) = crate::dlman::pill() {
            v.push((t!("İndirme", "Download"), p.trim_start_matches("↓ ").to_string(), 0));
        } else if !dl.is_empty() {
            let total: u64 = dl.iter().map(|d| d.1).sum();
            let label = if dl.len() > 1 { t!(format!("{} dosya · {}", dl.len(), carlui_sys::bytes(total as f64)), format!("{} files · {}", dl.len(), carlui_sys::bytes(total as f64))) } else { carlui_sys::bytes(total as f64) };
            v.push((t!("İndirme", "Download"), label, 0));
        }
        if let Some(app) = carlui_sys::privacy::users("microphone").into_iter().next() {
            v.push((t!("Mikrofon", "Microphone"), app, 1));
        }
        if let Some(app) = carlui_sys::privacy::users("webcam").into_iter().next() {
            v.push((t!("Kamera", "Camera"), app, 1));
        }
        if let Some((_, p)) = carlui_sys::bt::batteries().into_iter().next() {
            v.push((t!("Kulaklık", "Headphones"), t!(format!("%{p}"), format!("{p}%")), if p <= 15 { 2 } else if p <= 30 { 1 } else { 0 }));
        }
        v.extend(fixed);
        self.stats = v;
    }

    pub fn keep(&mut self, mut n: Note) {
        self.next_id += 1;
        n.id = self.next_id;
        if n.at.is_empty() {
            n.at = island::now_hm().2;
        }
        self.notes.push(n);
        if self.notes.len() > KEEP_MAX {
            self.notes.remove(0);
        }
    }

    /// Listede i. sıradaki (en yeni 0) bildirimin dizindeki yeri.
    fn index(&self, i: usize) -> Option<usize> {
        self.notes.len().checked_sub(1 + i)
    }
}

impl App {
    /// Ada bir şey göstermeye uygun mu (hub açılabilir mi)?
    pub(crate) fn hub_possible(&self) -> bool {
        let c = &self.cfg.stats;
        (c.cpu_temp || c.gpu_temp || c.ram) || !self.hub.notes.is_empty() || self.media_active() || !crate::downloads::active().is_empty() || crate::dlman::pill().is_some()
    }

    fn hub_open(&mut self, level: u8) {
        if level <= self.hub.level || !self.hub_possible() {
            return;
        }
        let first = self.hub.level == 0;
        self.hub.level = level;
        if first {
            self.hub.sample(&self.cfg.stats);
            unsafe {
                SetTimer(self.island.hwnd, TIMER_HUB_TICK, 1000, None);
            }
            if self.media_active() {
                self.vols.clear();
            }
        }
        self.refresh(true);
    }

    pub(crate) fn hub_close(&mut self) {
        let hwnd = self.island.hwnd;
        unsafe {
            for t in [TIMER_HUB_PEEK, TIMER_HUB_FULL, TIMER_HUB_TICK] {
                let _ = KillTimer(hwnd, t);
            }
        }
        if self.hub.level > 0 {
            self.hub.level = 0;
            self.expanded = false;
            self.refresh(false);
        }
    }

    /// Fare hapın üstünde kımıldadı.
    pub(crate) fn hub_hover(&mut self) {
        if self.hub.hovering || matches!(self.island.view, View::Search | View::Note | View::Hidden) {
            return;
        }
        self.hub.hovering = true;
        let hwnd = self.island.hwnd;
        unsafe {
            if self.hub.level == 0 {
                SetTimer(hwnd, TIMER_HUB_PEEK, self.cfg.stats.hover_ms.clamp(0, 3000).max(1), None);
            }
            if self.hub.level < 2 {
                SetTimer(hwnd, TIMER_HUB_FULL, FULL_MS, None);
            }
        }
    }

    pub(crate) fn hub_leave(&mut self) {
        self.hub.hovering = false;
        unsafe {
            let _ = KillTimer(self.island.hwnd, TIMER_HUB_PEEK);
            let _ = KillTimer(self.island.hwnd, TIMER_HUB_FULL);
        }
    }

    /// Hapa tıklandı: beklemeden tam aç. Açacak bir şey yoksa false.
    pub(crate) fn hub_click(&mut self) -> bool {
        if !self.hub_possible() {
            return false;
        }
        self.hub_open(2);
        true
    }

    /// Gösterimi biten bildirimi sakla.
    pub(crate) fn archive(&mut self, n: Note) {
        self.hub.keep(n);
    }

    pub(crate) fn hub_hit(&mut self, h: Hit) {
        match h {
            Hit::HubClear => {
                let shots = self.hub.notes.iter().filter(|n| n.shot.is_some()).count();
                info!("adadaki {} bildirim silindi ({} ekran görüntüsü dosyası kaldı)", self.hub.notes.len(), shots);
                let ids: Vec<u64> = self.hub.notes.iter().map(|n| n.id).collect();
                self.hub_remove(ids);
            }
            Hit::GroupToggle(g) => {
                if let Some((name, _)) = island::groups(&self.hub.notes).into_iter().nth(g) {
                    if !self.hub.collapsed.remove(&name) {
                        self.hub.collapsed.insert(name);
                    }
                    self.refresh(true);
                }
            }
            Hit::GroupClear(g) => {
                if let Some((name, idx)) = island::groups(&self.hub.notes).into_iter().nth(g) {
                    let ids: Vec<u64> = idx.iter().filter_map(|&i| self.hub.index(i)).map(|k| self.hub.notes[k].id).collect();
                    info!("{name} grubundaki {} bildirim silindi", ids.len());
                    self.hub.collapsed.remove(&name);
                    self.hub_remove(ids);
                }
            }
            Hit::NoteDel(i) => {
                if let Some(k) = self.hub.index(i) {
                    self.hub_remove(vec![self.hub.notes[k].id]);
                }
            }
            Hit::NoteRow(i) => {
                if let Some(k) = self.hub.index(i) {
                    let n = self.hub.notes[k].clone();
                    if let Some(p) = &n.shot {
                        crate::shots::open(p);
                    } else if let Some(f) = &n.file {
                        crate::dlman::open(f);
                    } else if !activate::open_app(&n.aumid) && !n.app.is_empty() {
                        activate::open_app(&n.app);
                    }
                    self.hub_close();
                }
            }
            Hit::NoteAct(i, act) => {
                let Some(k) = self.hub.index(i) else { return };
                let n = self.hub.notes[k].clone();
                let Some(path) = n.shot.clone() else { return };
                match act {
                    0 => {
                        crate::shots::open(&path);
                        self.hub_close();
                    }
                    1 => {
                        self.shot_edit(&path);
                        self.hub_close();
                    }
                    _ => {
                        self.delete_shot(&path, n.clip_id.as_deref());
                        self.hub_remove(vec![n.id]);
                    }
                }
            }
            _ => {}
        }
    }

    /// Test ve diğer modüller için: `hub-open {level}`, `hub-close`, `hub-hit {hit, i, act}`.
    pub(crate) fn hub_event(&mut self, name: &str, data: &serde_json::Value) {
        let n = |k: &str| data.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        match name {
            "hub-open" => self.hub_open(n("level").clamp(1, 2) as u8),
            "hub-close" => self.hub_close(),
            "hub-hit" => {
                let i = n("i") as usize;
                let h = match data.get("hit").and_then(|v| v.as_str()).unwrap_or_default() {
                    "clear" => Hit::HubClear,
                    "del" => Hit::NoteDel(i),
                    "row" => Hit::NoteRow(i),
                    "act" => Hit::NoteAct(i, n("act") as u8),
                    "group" => Hit::GroupToggle(i),
                    "group-clear" => Hit::GroupClear(i),
                    _ => return,
                };
                self.hub_hit(h);
            }
            _ => {}
        }
    }

    /// Bildirimleri sil: satırları ekrandaysa önce kayarak solarlar, liste
    /// animasyon bitince güncellenir.
    fn hub_remove(&mut self, ids: Vec<u64>) {
        let fresh: Vec<u64> = ids.into_iter().filter(|id| !self.hub.dying.contains(id)).collect();
        if fresh.is_empty() {
            return;
        }
        let shown = self.island.hub_vanish(&fresh);
        self.hub.dying.extend(fresh);
        if shown {
            unsafe {
                SetTimer(self.island.hwnd, TIMER_HUB_DEL, DEL_MS, None);
            }
        } else {
            self.hub_reap();
        }
    }

    /// Silinme animasyonu biten bildirimleri listeden çıkar.
    fn hub_reap(&mut self) {
        let dying = std::mem::take(&mut self.hub.dying);
        self.hub.notes.retain(|n| !dying.contains(&n.id));
        self.hub_after_change();
    }

    fn hub_after_change(&mut self) {
        if !self.hub_possible() {
            self.hub_close();
        } else {
            self.refresh(true);
        }
    }

    /// Ekran görüntüsünü sil: dosya Geri Dönüşüm Kutusu'na, panodaki kaydı da kalkar.
    pub(crate) fn delete_shot(&mut self, path: &str, clip_id: Option<&str>) {
        crate::shots::recycle(path);
        if let Some(id) = clip_id {
            if let Ok(mut s) = self.clip.store.lock() {
                // Kopyalanan resmin kaydı pano dinleyicisince eklenir; kopyalama
                // anından sonraki ilk resim kaydı odur.
                let target = match id.strip_prefix("t:").and_then(|t| t.parse::<i64>().ok()) {
                    Some(t) => s.items.iter().filter(|c| c.kind == carlui_clip::store::Kind::Image && c.time >= t - 1500 && c.time <= t + 15_000).min_by_key(|c| c.time).map(|c| c.id.clone()),
                    None => s.get(id).map(|c| c.id.clone()),
                };
                if let Some(id) = target {
                    s.remove(&id);
                    info!("ekran görüntüsü panodan da silindi");
                }
            }
        }
    }
}

pub(crate) fn window_message(hwnd: HWND, msg: u32, wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
    if msg != WM_TIMER {
        return None;
    }
    unsafe {
        match wp.0 {
            TIMER_HUB_PEEK => {
                let _ = KillTimer(hwnd, TIMER_HUB_PEEK);
                with_app(|a| a.hub_open(1));
            }
            TIMER_HUB_DEL => {
                let _ = KillTimer(hwnd, TIMER_HUB_DEL);
                with_app(|a| a.hub_reap());
            }
            TIMER_HUB_FULL => {
                let _ = KillTimer(hwnd, TIMER_HUB_FULL);
                with_app(|a| a.hub_open(2));
            }
            TIMER_HUB_TICK => {
                with_app(|a| {
                    if a.hub.level == 0 {
                        let _ = KillTimer(hwnd, TIMER_HUB_TICK);
                        return;
                    }
                    let before = a.hub.stats.clone();
                    a.hub.sample(&a.cfg.stats);
                    if a.hub.stats != before && a.dragging == Hit::None {
                        // Yalnızca üst satırı yenile; olmazsa (satır sayısı değişti) tümünü.
                        let stats = a.hub.stats.clone();
                        if !matches!(a.island.hub_restat(&a.gfx, &stats), Ok(true)) {
                            a.refresh(true);
                        }
                    }
                });
            }
            _ => return None,
        }
    }
    Some(LRESULT(0))
}
