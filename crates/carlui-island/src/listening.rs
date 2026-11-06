//! Bu modül sesli asistanı aramanın içine yerleştiriyor. Asistan ayrı süreçte
//! (`carlui-convert --assist`) mikrofonu ve modeli yönetir; olay yolundan
//! `assist` olayları gönderir, ada da aramayı önerisiz açıp şeritleri gösterir,
//! söylenenleri arama kutusuna yazar. Onay açıksa yazı kutuda bekler: düzeltilir,
//! ✓ ya da Enter ile gönderilir, ✕ ya da Esc ile bırakılır. Yanıt `assist-reply`
//! ile geri gider. Dinlerken odak alınmaz; kullanıcının penceresi önde kalır.

use crate::{
    island::ListenStage,
    launcher::{Key, Listen},
    *,
};
use std::time::{Duration, Instant};

const TIMER_LISTEN: usize = 32;
const TIMER_LISTEN_END: usize = 33;
/// Asistandan bu kadar haber gelmezse (süreç çökmüş) panel kapanır.
const STALE: Duration = Duration::from_secs(60);

impl App {
    fn listen_reply(&self, op: &str, text: &str) {
        self.bus.send(&Event::Custom { name: "assist-reply".into(), data: serde_json::json!({ "op": op, "text": text }) });
    }

    fn listen_set(&mut self, stage: ListenStage, status: String) {
        if let Some(l) = self.spot.listen.as_mut() {
            l.stage = stage;
            l.status = status;
        }
    }

    fn listen_open(&mut self) {
        let prev = unsafe { GetForegroundWindow() };
        self.search_prepare(prev);
        self.spot.listen = Some(Listen { stage: ListenStage::Listening, status: t!("Dinliyor…", "Listening…").into(), seen: Instant::now() });
        self.island.listen_level(0.0);
        self.refresh(true);
        unsafe {
            SetTimer(self.island.hwnd, TIMER_LISTEN, 5000, None);
        }
        info!("asistan araması açıldı");
    }

    /// Olay yolundan gelen asistan olayı.
    pub(crate) fn assist_event(&mut self, data: &serde_json::Value) {
        let g = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let op = g("op");
        if op == "open" {
            // Yanıt "busy" ise asistan kendi küçük kartını gösterir.
            if self.game || searching::game_in_front() || !self.cfg.launcher.enabled || self.spot.open {
                self.listen_reply("busy", "");
            } else {
                self.listen_open();
                self.listen_reply("ready", "");
            }
            return;
        }
        let Some(l) = self.spot.listen.as_mut() else { return };
        l.seen = Instant::now();
        let stage = l.stage;
        match op.as_str() {
            "level" => self.island.listen_level(data.get("v").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32),
            "text" if matches!(stage, ListenStage::Listening | ListenStage::Thinking) => {
                self.spot.set_text(&g("text"));
                self.refresh(true);
            }
            "thinking" => {
                self.island.listen_level(0.3);
                self.listen_set(ListenStage::Thinking, t!("Anlaşılıyor…", "Understanding…").into());
                self.refresh(true);
            }
            "review" => {
                self.island.listen_level(0.0);
                self.spot.set_text(&g("text"));
                self.listen_set(ListenStage::Review, t!("Enter gönderir · Esc vazgeçer", "Enter sends · Esc cancels").into());
                // Karar sırası kullanıcıda: yazı düzeltilebilsin diye klavye adaya gelir.
                let hwnd = self.island.hwnd;
                unsafe {
                    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
                }
                self.refresh(true);
                unsafe { searching::force_foreground(hwnd) };
            }
            "done" => {
                self.island.listen_level(0.0);
                let text = g("text");
                if !text.is_empty() {
                    self.spot.set_text(&text);
                }
                self.listen_set(ListenStage::Done, g("status"));
                self.refresh(true);
                let failed = data.get("failed").and_then(|v| v.as_bool()).unwrap_or(false);
                unsafe {
                    SetTimer(self.island.hwnd, TIMER_LISTEN_END, if failed { 1800 } else { 1000 }, None);
                }
            }
            "close" => self.close_search(false),
            _ => {}
        }
    }

    /// Arama kapanırken: iş bitmediyse asistana "vazgeçildi" der.
    pub(crate) fn listen_closing(&mut self) {
        let Some(l) = self.spot.listen.take() else { return };
        unsafe {
            let _ = KillTimer(self.island.hwnd, TIMER_LISTEN);
            let _ = KillTimer(self.island.hwnd, TIMER_LISTEN_END);
        }
        if l.stage != ListenStage::Done {
            self.listen_reply("cancel", "");
        }
    }

    fn listen_send(&mut self) {
        let text = self.spot.typed().trim().to_string();
        if text.is_empty() {
            return self.close_search(true);
        }
        self.spot.listen = None;
        unsafe {
            let _ = KillTimer(self.island.hwnd, TIMER_LISTEN);
        }
        self.listen_reply("send", &text);
        self.close_search(true);
    }

    fn listen_edit(&mut self) {
        if self.spot.listen.as_ref().is_some_and(|l| l.stage == ListenStage::Review) {
            self.listen_set(ListenStage::Edit, t!("Düzeltin, Enter ile gönderin", "Fix it, then press Enter").into());
            self.refresh(true);
        }
    }

    /// ✕ (0), Düzenle (1), ✓ (2) ya da kutuya tıklama.
    pub(crate) fn listen_click(&mut self, h: Hit) {
        match h {
            Hit::ListenBtn(0) => self.close_search(true),
            Hit::ListenBtn(1) | Hit::Field => self.listen_edit(),
            Hit::ListenBtn(_) => self.listen_send(),
            _ => {}
        }
    }

    /// Onay beklerken tuşlar: öneri olmadığı için yalnızca yazı tuşları çalışır.
    pub(crate) fn listen_key(&mut self, k: Key) {
        match k {
            Key::Esc => self.close_search(true),
            Key::Enter { .. } => self.listen_send(),
            Key::Left | Key::Right | Key::Home { .. } | Key::End { .. } | Key::Back { .. } | Key::Delete | Key::SelectAll | Key::Paste | Key::Copy => {
                self.listen_edit();
                let o = self.spot.key(&self.cfg, k);
                self.spot_outcome(o);
            }
            _ => {}
        }
    }

    /// Onay beklerken yazmaya başlandı.
    pub(crate) fn listen_typing(&mut self, unit: u16) {
        if unit >= 0x20 {
            self.listen_edit();
        }
    }

    /// Dinlerken ada odak almaz; tıklama da almasın.
    pub(crate) fn listen_passive(&self) -> bool {
        self.spot.listen.as_ref().is_some_and(|l| !matches!(l.stage, ListenStage::Review | ListenStage::Edit))
    }

    fn listen_check(&mut self) {
        if self.spot.listen.as_ref().is_some_and(|l| l.stage != ListenStage::Review && l.stage != ListenStage::Edit && l.seen.elapsed() > STALE) {
            carlui_core::warn!("asistan yanıt vermiyor, arama kapatılıyor");
            self.close_search(false);
        }
    }
}

/// Asistana ait pencere mesajları; ilgisizse None.
pub(crate) fn window_message(msg: u32, wp: WPARAM) -> Option<LRESULT> {
    match (msg, wp.0) {
        (WM_TIMER, TIMER_LISTEN) => {
            with_app(|a| a.listen_check());
        }
        (WM_TIMER, TIMER_LISTEN_END) => {
            with_app(|a| {
                if a.spot.listen.is_some() {
                    a.close_search(false);
                }
            });
        }
        _ => return None,
    }
    Some(LRESULT(0))
}
