//! Güncelleme sayfası. Sorun: yeni sürümü yalnızca adadaki günlük bildirim
//! haber veriyordu; elle denetlemenin ve kurulu sürümü görmenin yolu yoktu.
//! Çözüm: One UI'nin "Yazılım güncelleme" ekranı gibi sakin bir sayfa:
//! ortada büyük "CarlUI" yazısı ve altında kurulu sürüm, onun altında durum
//! satırı ve tek bir ana düğme (Denetle → Güncelle → Yeniden dene). Ağ,
//! indirme ve SHA-256 doğrulaması adayla ortak (`carlui_core::update`); iş
//! arka plan iş parçacığında yürür, pencere donmaz. Akışın durumu
//! `update_state`'te (sınanabilir).

use super::update_state::{bottom_stack_h, on_accent, place, Event, Phase, BUTTON_GAP, HERO_H, STATUS_MIN};
use crate::{
    ui::{lines, Flow, P},
    Act, App,
};
use carlui_core::update::{self as core, Stage};
use carlui_ui::paint::{Align, ORANGE, R, TEXT};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
    time::Instant,
};
use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::InvalidateRect,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::{GetClientRect, SetTimer, SW_SHOWNORMAL}},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAct {
    /// Güncellemeleri denetle (ya da denetimi yeniden dene).
    Check,
    /// Bulunan sürümü indir, doğrula, kurucuyu aç (ya da kurulumu yeniden dene).
    Install,
    /// Sürümün GitHub sayfasını aç.
    OpenPage,
}

struct State {
    phase: Phase,
    /// Arka plandan gelen, henüz işlenmemiş haberler.
    inbox: Arc<Mutex<Vec<Event>>>,
    /// Son başarılı denetimin zamanı (dosyadan bir kez okunur).
    last: Option<Option<String>>,
    since: Instant,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { phase: Phase::Idle, inbox: Arc::default(), last: None, since: Instant::now() });
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn a(x: UpdateAct) -> Act {
    Act::Update(x)
}

/// Gelen haberleri durum makinesine uygular (pencere iş parçacığında, boyarken).
fn drain() {
    with(|s| {
        let evs: Vec<Event> = s.inbox.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        let cur = core::current();
        for ev in evs {
            if matches!(ev, Event::Checked(Ok(_))) {
                // Başarılı denetimin zamanı dosyaya yazıldı; yeniden okunsun.
                s.last = None;
            }
            s.phase = s.phase.next(ev, &cur);
        }
        if s.last.is_none() {
            s.last = Some(core::last_check());
        }
    });
}

/// Durum satırı (ana, soluk alt satır) ve rengi.
fn status(phase: &Phase, last: Option<&str>) -> (String, String, Option<u32>) {
    let when = || match last {
        Some(t) => format!("{} {t}", t!("Son denetim:", "Last checked:")),
        None => t!("Henüz denetlenmedi", "Not checked yet").to_string(),
    };
    match phase {
        Phase::Idle => (when(), String::new(), None),
        Phase::Checking => (t!("Güncellemeler denetleniyor…", "Checking for updates…").into(), String::new(), None),
        Phase::UpToDate => (t!("Yazılımınız güncel", "Your software is up to date").into(), when(), None),
        Phase::Available(r) => (
            t!(format!("Yeni sürüm: {}", r.version()), format!("New version: {}", r.version())),
            if r.installable() { t!("İndirilir, SHA-256 ile doğrulanır ve kurucu açılır. Ayarların korunur.", "It's downloaded, verified with SHA-256 and the installer opens. Your settings are kept.") } else { t!("Bu sürüm uygulama içinden kurulamıyor; indirme sayfasından alabilirsin.", "This version can't be installed from here; get it from the download page.") }.into(),
            None,
        ),
        Phase::Installing(r, st) => (
            match st {
                Stage::Downloading => t!(format!("CarlUI {} indiriliyor…", r.version()), format!("Downloading CarlUI {}…", r.version())),
                Stage::Verifying => t!("İndirilen dosya doğrulanıyor (SHA-256)…", "Verifying the download (SHA-256)…").into(),
                Stage::Launching => t!("Kurucu açılıyor…", "Opening the installer…").into(),
            },
            t!("Kurulunca CarlUI yeniden açılacak.", "CarlUI will restart once it's installed.").into(),
            None,
        ),
        Phase::Launched => (t!("Kurucu açıldı", "The installer is open").into(), t!("CarlUI birazdan kapanıp yeni sürümle yeniden açılacak.", "CarlUI will close shortly and reopen with the new version.").into(), None),
        Phase::Failed { msg, release: Some(_) } => (format!("{} {msg}", t!("Güncelleme kurulamadı:", "Update failed:")), t!("Hiçbir şey değiştirilmedi.", "Nothing was changed.").into(), Some(ORANGE)),
        Phase::Failed { msg, release: None } => (msg.clone(), String::new(), Some(ORANGE)),
    }
}

/// Ana düğmenin yazısı ve eylemi (`None`: kapalı).
fn primary(phase: &Phase) -> (&'static str, Option<UpdateAct>) {
    match phase {
        Phase::Checking => (t!("Denetleniyor…", "Checking…"), None),
        Phase::Available(r) if r.installable() => (t!("Güncelle", "Update"), Some(UpdateAct::Install)),
        Phase::Available(_) => (t!("İndirme sayfasını aç", "Open download page"), Some(UpdateAct::OpenPage)),
        Phase::Installing(..) => (t!("Kuruluyor…", "Installing…"), None),
        Phase::Launched => (t!("Kurucu açıldı", "Installer opened"), None),
        Phase::Failed { release: Some(_), .. } if phase.installable().is_some() => (t!("Yeniden dene", "Try again"), Some(UpdateAct::Install)),
        Phase::Failed { .. } => (t!("Yeniden dene", "Try again"), Some(UpdateAct::Check)),
        Phase::Idle | Phase::UpToDate => (t!("Güncellemeleri denetle", "Check for updates"), Some(UpdateAct::Check)),
    }
}

/// Satırları ortalı çizer; kapladığı yüksekliği döner.
#[allow(clippy::too_many_arguments)]
fn centered(p: &P, text: &str, x: f32, y: f32, w: f32, size: f32, lh: f32, bold: bool, c: u32, alpha: f32) -> f32 {
    let v = lines(p, text, size, bold, w);
    for (i, s) in v.iter().enumerate() {
        p.text(s, R::new(x, y + i as f32 * lh, w, lh), size, bold, c, alpha, Align::Center);
    }
    v.len() as f32 * lh
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    drain();
    let (phase, last, since) = with(|s| (s.phase.clone(), s.last.clone().flatten(), s.since));
    f.flush(p);
    let (x, w) = (f.x, f.w);
    let top0 = f.y;

    // Önce ölçülür, sonra çizilir: marka + sürüm + durum (+ notlar) bir grup,
    // "Otomatik denetim" + ana düğme alttaki yığın (One UI gibi).
    let (main, sub, color) = status(&phase, last.as_deref());
    let tw = (w - 80.0).min(560.0);
    let tx = x + (w - tw) / 2.0;
    let bold = matches!(phase, Phase::UpToDate | Phase::Available(_));
    let main_h = lines(p, &main, 16.0, bold, tw).len() as f32 * 24.0;
    let sub_h = if sub.is_empty() { 0.0 } else { 4.0 + lines(p, &sub, 12.5, false, tw).len() as f32 * 18.0 };
    // Durum bloğu en az iki satır yer kaplar: alt satır gelip gidince öğeler zıplamasın.
    let status_h = (main_h + sub_h).max(STATUS_MIN);
    let cw = tw.min(520.0);
    let notes: Vec<Vec<String>> = match &phase {
        Phase::Available(r) => core::notes_excerpt(&r.notes, 4, 320).iter().map(|n| lines(p, n, 13.0, false, cw - 52.0)).collect(),
        _ => Vec::new(),
    };
    let nl: usize = notes.iter().map(|v| v.len()).sum();
    let ch = 16.0 + 22.0 + nl as f32 * 19.0 + 14.0;
    let notes_h = if notes.is_empty() { 0.0 } else { 20.0 + ch };
    let group_h = HERO_H + status_h + notes_h;
    let link = matches!(&phase, Phase::Available(r) if r.installable());
    let bottom_h = bottom_stack_h(link);
    let pl = place(page_height(app), group_h, bottom_h);

    // Sakin, havadar düzen: ortada büyük marka ve sürüm.
    let mut y = top0 + pl.group;
    p.text("CarlUI", R::new(x, y, w, 64.0), 48.0, true, TEXT, 0.96, Align::Center);
    y += 66.0;
    let ver = core::current();
    p.text(&t!(format!("Sürüm {ver}"), format!("Version {ver}")), R::new(x, y, w, 22.0), 15.0, false, TEXT, 0.5, Align::Center);
    y += 22.0 + 48.0;

    // Durum: meşgulken başlığın solunda dönen simge.
    let (c, al) = color.map(|c| (c, 0.95)).unwrap_or((TEXT, 0.9));
    let status_top = y;
    if phase.busy() {
        let mw = p.fonts.width(&main, 16.0, false).min(tw);
        let deg = (since.elapsed().as_millis() % 1000) as f32 * 0.36;
        p.glyph_rot('\u{E72C}', R::new(x + (w - mw) / 2.0 - 28.0, y, 20.0, 24.0), 14.0, p.accent, 0.95, deg);
        // Dönen simge için bir sonraki kare.
        unsafe {
            SetTimer(app.hwnd, crate::TIMER_ANIM, 33, None);
        }
    }
    centered(p, &main, tx, y, tw, 16.0, 24.0, bold, c, al);
    if !sub.is_empty() {
        centered(p, &sub, tx, y + main_h + 4.0, tw, 12.5, 18.0, false, TEXT, 0.5);
    }
    y = status_top + status_h;

    // Yeni sürümün notlarından kısa bir özet.
    if !notes.is_empty() {
        y += 20.0;
        let cx = x + (w - cw) / 2.0;
        p.card(R::new(cx, y, cw, ch));
        p.text(t!("Yenilikler", "What's new"), R::new(cx + 18.0, y + 12.0, cw - 36.0, 20.0), 12.5, true, TEXT, 0.7, Align::Left);
        let mut ly = y + 38.0;
        for v in &notes {
            p.text("•", R::new(cx + 18.0, ly, 12.0, 19.0), 13.0, false, TEXT, 0.6, Align::Left);
            for s in v {
                p.text(s, R::new(cx + 34.0, ly, cw - 52.0, 19.0), 13.0, false, TEXT, 0.85, Align::Left);
                ly += 19.0;
            }
        }
    }

    // Alttaki yığın: önce otomatik denetim anahtarı, altında ana düğme.
    f.y = top0 + pl.bottom;
    f.section(p, t!("Otomatik denetim", "Automatic check"));
    f.toggle(p, t!("Yeni sürümleri bildir", "Notify about new versions"), t!("Günde bir kez GitHub'a bakılır; yeni sürüm adada bildirilir, kendiliğinden kurulmaz.", "Checks GitHub once a day; a new version is announced on the island and never installed automatically."), app.cfg.general.check_updates, |c| c.general.check_updates ^= true);
    f.flush(p);
    let mut y = f.y + BUTTON_GAP;
    button(p, x, y, w, &phase);
    y += 44.0;

    // Sürümün sayfası (notların tamamı) ikincil bir bağlantı olarak.
    if link {
        y += 10.0;
        let label = t!("Sürüm notlarının tamamı", "Full release notes");
        let lw = p.fonts.width(label, 12.5, false) + 20.0;
        let lr = R::new(x + (w - lw) / 2.0, y, lw, 26.0);
        p.text(label, lr, 12.5, false, p.accent, if p.hover(lr) { 1.0 } else { 0.8 }, Align::Center);
        p.hit(lr, a(UpdateAct::OpenPage));
        y += 26.0;
    }
    // Sığıyorsa sayfa kaymaz (sonu tam sayfa dibi); sığmıyorsa akış + kaydırma.
    f.y = if pl.pinned { (top0 + pl.end).max(y + 16.0) } else { y + 16.0 };
}

/// Ana düğme: vurgu renginde, geniş hap, ortada. Yazı rengi vurgunun
/// parlaklığına göre siyah ya da beyaz (koyu vurguda da okunur).
fn button(p: &mut P, x: f32, y: f32, w: f32, phase: &Phase) {
    let (label, act) = primary(phase);
    let bw = (p.fonts.width(label, 14.0, true) + 96.0).clamp(280.0, 360.0).min(w);
    let br = R::new(x + (w - bw) / 2.0, y, bw, 44.0);
    let on = act.is_some();
    let hot = on && p.hover(br);
    let fill = p.accent;
    if on {
        // Dolgu ve yazı tam bu renklerle çizilir (açık temanın eşlemesi yok):
        // seçilen yazı rengi gerçekten çizilen dolguya göre hesaplandı.
        carlui_ui::tone::literal(|| {
            p.round(br, 22.0, fill, if hot { 1.0 } else { 0.92 });
            p.text(label, br, 14.0, true, on_accent(fill), 0.95, Align::Center);
        });
    } else {
        // Kapalıyken soluk dolgu; yazı zemine göre (temanın yazı rengi).
        p.round(br, 22.0, fill, 0.3);
        p.text(label, br, 14.0, true, TEXT, 0.6, Align::Center);
    }
    if let Some(act) = act {
        p.hit(br, a(act));
    }
}

/// Sayfa alanının (başlığın altı, kaydırma payları düşülmüş) yüksekliği.
fn page_height(app: &App) -> f32 {
    let mut rc = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetClientRect(app.hwnd, &mut rc);
    }
    // main.rs: kırpma alanı 66'dan başlar, akış +4'ten, altta 24 pay.
    (rc.bottom - rc.top) as f32 / app.scale - 66.0 - 4.0 - 24.0
}

/// Bir iş parçacığında `job`'u çalıştırır; haberler kutuya düşer, pencere yeniden çizilir.
fn spawn(app: &App, job: impl FnOnce(&dyn Fn(Event)) + Send + 'static) {
    let inbox = with(|s| s.inbox.clone());
    let h = app.hwnd.0 as isize;
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            // WinRT HttpClient çok iş parçacıklı daire ister.
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            let send = |ev: Event| {
                if let Ok(mut v) = inbox.lock() {
                    v.push(ev);
                }
                unsafe {
                    let _ = InvalidateRect(HWND(h as _), None, false);
                }
            };
            job(&send);
        })
        .ok();
}

/// Eylemler (pencere iş parçacığında).
pub fn act(app: &mut App, x: UpdateAct) {
    drain();
    match x {
        UpdateAct::Check => {
            if !with(|s| s.phase.can_check()) {
                return;
            }
            with(|s| {
                s.phase = Phase::Checking;
                s.since = Instant::now();
            });
            spawn(app, |send| {
                let r = core::latest(core::API);
                if r.is_ok() {
                    core::record_check();
                }
                send(Event::Checked(r));
            });
        }
        UpdateAct::Install => {
            let Some(r) = with(|s| s.phase.installable().cloned()) else { return };
            with(|s| {
                s.phase = Phase::Installing(r.clone(), Stage::Downloading);
                s.since = Instant::now();
            });
            spawn(app, move |send| {
                let res = core::install(&r, |st| send(Event::Stage(st)));
                if let Err(e) = &res {
                    carlui_core::warn!("güncelleme kurulamadı: {e}");
                }
                send(Event::Installed(res));
            });
        }
        UpdateAct::OpenPage => {
            let page = with(|s| match &s.phase {
                Phase::Available(r) | Phase::Failed { release: Some(r), .. } => Some(r.page.clone()),
                _ => None,
            });
            if let Some(url) = page.filter(|u| u.starts_with("https://github.com/")) {
                unsafe {
                    ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use carlui_core::update::Release;

    #[test]
    fn dugme_ve_durum() {
        let ok = Release { tag: "v2.0.0".into(), url: "u".into(), sha: "a".repeat(64), ..Default::default() };
        let bare = Release { tag: "v2.0.0".into(), ..Default::default() };
        assert_eq!(primary(&Phase::Idle).1, Some(UpdateAct::Check));
        assert_eq!(primary(&Phase::Checking).1, None);
        assert_eq!(primary(&Phase::Available(ok.clone())).1, Some(UpdateAct::Install));
        assert_eq!(primary(&Phase::Available(bare.clone())).1, Some(UpdateAct::OpenPage));
        assert_eq!(primary(&Phase::Failed { msg: "x".into(), release: Some(ok) }).1, Some(UpdateAct::Install));
        // Kurulamayan sürümde "Yeniden dene" kurmaz, yeniden denetler.
        assert_eq!(primary(&Phase::Failed { msg: "x".into(), release: Some(bare) }).1, Some(UpdateAct::Check));
        assert_eq!(primary(&Phase::Launched).1, None);
        let (m, s, c) = status(&Phase::Failed { msg: "çevrimdışı".into(), release: None }, None);
        assert_eq!((m.as_str(), s.as_str(), c), ("çevrimdışı", "", Some(ORANGE)));
        assert!(status(&Phase::Idle, Some("12.11.2026 14:05")).0.ends_with("12.11.2026 14:05"));
    }
}
