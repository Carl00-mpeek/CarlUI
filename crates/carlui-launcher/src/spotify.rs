//! Bu modül "Spotify'da şu şarkıyı çal" sorununu hesap bağlamadan çözüyor. Uygulama
//! kuruluysa arama adresiyle sonuçlar açılır, sonra ekran erişilebilirliğiyle
//! sonucun çal düğmesine basılır; düğme bulunamazsa sonuçlar ekranda kalır.
//! Uygulama yoksa tarayıcıda arama açılır, tarayıcıda otomatik çalma yoktur.
//! Spotify'ın resmi çalma arayüzü Premium ve kullanıcı girişi ister.

use crate::{
    actions,
    index::{self, App},
    search::{enc, norm, Act},
};
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::Accessibility::*,
    },
};

/// Sonuçların gelmesi için en uzun bekleme.
const WAIT_MAX: Duration = Duration::from_secs(8);
const WAIT_STEP: Duration = Duration::from_millis(600);
const PLAY_WORDS: &[&str] = &["play", "cal", "oynat"];

fn quoted(q: &str) -> String {
    enc(q).replace('+', "%20")
}

pub fn web_url(q: &str) -> String {
    format!("https://open.spotify.com/search/{}", quoted(q))
}

/// Bu sonuç düğmesi aranan şarkının çal düğmesi mi? Alt çubuktaki "Çal"
/// düğmesinin adında şarkı geçmez; ona basmak başka bir şeyi sürdürürdü.
fn pick(names: &[String], q: &str) -> Option<usize> {
    let q = norm(q);
    let words: Vec<&str> = q.split_whitespace().filter(|w| w.chars().count() >= 3).collect();
    names.iter().position(|n| {
        let n = norm(n);
        let mut parts = n.split(|c: char| !c.is_alphanumeric());
        parts.any(|p| PLAY_WORDS.iter().any(|w| p.starts_with(w))) && words.iter().any(|w| n.contains(w))
    })
}

fn press_result(hwnd: isize, q: &str) -> Result<bool> {
    unsafe {
        let ua: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let buttons = ua.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_ButtonControlTypeId.0))?;
        let list = ua.ElementFromHandle(HWND(hwnd as _))?.FindAll(TreeScope_Descendants, &buttons)?;
        let names: Vec<String> = (0..list.Length()?).map(|i| list.GetElement(i).and_then(|e| e.CurrentName()).map(|n| n.to_string()).unwrap_or_default()).collect();
        let Some(i) = pick(&names, q) else { return Ok(false) };
        list.GetElement(i as i32)?.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?.Invoke()?;
        Ok(true)
    }
}

/// `app` Spotify masaüstü uygulamasıdır; `None` ise tarayıcıda aranır.
pub fn play(q: &str, app: Option<&App>) {
    let Some(app) = app else {
        actions::run(&Act::Open(web_url(q)));
        return;
    };
    actions::run(&Act::Open(format!("spotify:search:{}", quoted(q))));
    let t0 = Instant::now();
    while t0.elapsed() < WAIT_MAX {
        std::thread::sleep(WAIT_STEP);
        let run = index::running();
        let Some(w) = index::window_of(app, &run) else { continue };
        if press_result(w.hwnd, q).unwrap_or(false) {
            return;
        }
    }
    carlui_core::info!("asistan: Spotify çal düğmesi bulunamadı, sonuçlar açık kaldı");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn picks_the_result_not_the_player_bar() {
        assert_eq!(pick(&names(&["Çal", "Sonraki", "Kuzu Kuzu şarkısını çal"]), "tarkan kuzu kuzu"), Some(2));
        assert_eq!(pick(&names(&["Play", "Play Shape of You by Ed Sheeran"]), "shape of you"), Some(1));
        assert_eq!(pick(&names(&["Play", "Next", "Home"]), "shape of you"), None);
        assert_eq!(pick(&names(&["Shape of You", "Beğen"]), "shape of you"), None);
    }

    #[test]
    fn builds_addresses() {
        assert_eq!(web_url("duman bu akşam"), format!("https://open.spotify.com/search/{}", enc("duman bu akşam").replace('+', "%20")));
        assert!(!web_url("a b").contains('+'));
    }
}
