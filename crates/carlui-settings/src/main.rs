//! Bu modül CarlUI'nin ayarlar penceresi (`carlui-settings.exe`). Sorun: tüm
//! ayarlar `config.toml`'daydı; değiştirmek için dosyayı elle düzenlemek
//! gerekiyordu ve yanlış bir değer (ör. fare tuşu kısayolu) fark edilmiyordu.
//! Çözüm: görev yöneticisiyle aynı akrilik cam pencere; solda kategoriler,
//! sağda anahtar/kaydırıcı/seçici satırları ve bir kısayol kaydedici. Her
//! değişiklik hemen `config.toml`'a yazılır; tüm modüller dosyayı izlediği için
//! yeniden başlatmaya gerek kalmaz. Dosya dışarıdan değişirse pencere de
//! yenilenir. Dock'un sağ tık menüsündeki "Ayarlar" bu pencereyi açar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod input;
mod pages;
mod recorder;
mod ui;

use carlui_core::{config, config::Config, log};
use carlui_ui::{
    glassmenu,
    paint::{Align, Fonts, R, TEXT},
    tween,
};
use pages::{shortcuts::Key, Page};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};
use ui::{Flow, P};
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dwm::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        System::{Com::*, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::{Controls::MARGINS, HiDpi::*, Input::KeyboardAndMouse::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

const WM_APP_RELOAD: u32 = WM_APP + 1;
const TIMER_ANIM: usize = 2;
const SIDEBAR: f32 = 220.0;
const CLASS: PCWSTR = w!("CarlUI.Settings");

#[derive(Clone)]
pub enum Act {
    Page(Page),
    /// Ayarı değiştir ve kaydet.
    Edit(Rc<dyn Fn(&mut Config)>),
    /// Kaydırıcı: değer = fare konumunun izdeki oranı (0..1).
    Slider(Rc<dyn Fn(&mut Config, f32)>, R),
    Record(Key),
    OpenFile,
    /// Tarayıcı eklentisi: 0 klasörü aç, 1 Firefox eklenti sayfası, 2 Chrome/Edge eklenti sayfası.
    Ext(u8),
    /// Araç programını (`carlui-tools.exe`) verilen bayrakla başlat.
    Tool(&'static str),
    /// Anlık tekrar kayıtlarının klasörünü Gezgin'de aç.
    ReplayFolder,
    ReplayPanel,
    /// Ses stüdyosunu (`carlui-convert --studio`) aç.
    Studio,
    /// Ayarları dosyaya aktar (true) ya da dosyadan al (false).
    Backup(bool),
    /// Yapay zekâ modelini indir (true) ya da sil (false).
    Model(&'static str, bool),
    /// İçerik alanı (tekerlek kaydırma hedefi).
    Scroll,
    /// Metin formu: 0 arama motoru, 5 kısaltma, 6 bildirim engeli, 7 pano hariç, 8 genişletici hariç formunu aç; 1/2 alan, 3 ekle, 4 vazgeç.
    Input(u8),
}

pub struct App {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Act)>,
    accent: u32,
    pub cfg: Config,
    pub page: Page,
    pub recording: Option<Key>,
    pub error: Option<(Key, String)>,
    /// İçe/dışa aktarma sonucu (Genel sayfasında gösterilir).
    pub notice: Option<String>,
    scroll: HashMap<Page, f32>,
    max_scroll: HashMap<Page, f32>,
    drag: Option<(Rc<dyn Fn(&mut Config, f32)>, R)>,
    reload: Arc<Mutex<Option<Config>>>,
    saved: Option<Instant>,
    /// Açık metin formu (arama motoru ekleme).
    pub input: Option<input::Input>,
    /// Şu an indirilen yapay zekâ modelleri (dönüştürücü süreci bitince çıkar).
    pub fetching: Arc<Mutex<Vec<&'static str>>>,
    /// Model silinemediyse nedeni.
    pub ai_error: Option<String>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

impl App {
    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn save(&mut self) {
        config::save(&self.cfg);
        carlui_ui::tone::follow_window(&self.cfg.theme, self.hwnd);
        self.saved = Some(Instant::now());
        unsafe {
            SetTimer(self.hwnd, 1, 1600, None);
        }
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            recorder::stop();
        }
    }

    fn act(&mut self, a: Act) {
        if !matches!(a, Act::Record(_)) {
            self.stop_recording();
        }
        match a {
            Act::Page(pg) => {
                if self.page != pg {
                    // Yeni sayfa biraz aşağıdan süzülerek gelsin.
                    tween::set(tween::key("page"), 18.0);
                    self.input = None;
                }
                self.page = pg;
                self.error = None;
            }
            Act::Edit(f) => {
                f(&mut self.cfg);
                self.error = None;
                self.save();
            }
            Act::Slider(f, track) => {
                f(&mut self.cfg, (self.mouse.0 - track.x) / track.w);
                self.drag = Some((f, track));
                unsafe {
                    SetCapture(self.hwnd);
                }
            }
            Act::Record(k) => {
                if self.recording == Some(k) {
                    self.stop_recording();
                } else {
                    self.error = None;
                    self.recording = Some(k);
                    recorder::start(self.hwnd);
                }
            }
            Act::OpenFile => unsafe {
                ShellExecuteW(HWND::default(), w!("open"), w!("notepad.exe"), &HSTRING::from(carlui_core::paths::config_file().as_os_str()), PCWSTR::null(), SW_SHOWNORMAL);
            },
            Act::ReplayFolder => unsafe {
                let dir = self.cfg.replay.folder();
                let _ = std::fs::create_dir_all(&dir);
                ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(dir.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
            },
            Act::Backup(export) => {
                let r = if export { pages::backup::export(&self.cfg) } else { pages::backup::import(&mut self.cfg) };
                match r {
                    Ok(false) => {}
                    Ok(true) if !export => {
                        self.notice = Some(t!("Ayarlar içe aktarıldı.", "Settings imported.").into());
                        self.save();
                    }
                    Ok(true) => self.notice = Some(t!("Ayarlar dışa aktarıldı.", "Settings exported.").into()),
                    Err(e) => self.notice = Some(e),
                }
            }
            Act::ReplayPanel => {
                let _ = std::process::Command::new(carlui_core::paths::sibling_exe("carlui-replay.exe")).arg("--panel").spawn();
            }
            Act::Studio => {
                let _ = std::process::Command::new(carlui_core::paths::sibling_exe("carlui-convert.exe")).arg("--studio").spawn();
            }
            Act::Tool(arg) => unsafe {
                ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(carlui_core::paths::sibling_exe("carlui-tools.exe").as_os_str()), &HSTRING::from(arg), PCWSTR::null(), SW_SHOWNORMAL);
            },
            Act::Ext(k) => {
                let b = pages::extension::browsers();
                let target = match k {
                    0 => b.dir.as_ref().map(|d| (d.display().to_string(), String::new())),
                    1 => b.firefox.clone().map(|f| (f, "about:debugging#/runtime/this-firefox".to_string())),
                    _ => b.chromium.as_ref().map(|(exe, url, _)| (exe.clone(), url.to_string())),
                };
                if let Some((file, args)) = target {
                    unsafe {
                        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(file), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL);
                    }
                }
            }
            Act::Model(id, true) => pages::ai::fetch(self, id),
            Act::Model(id, false) => self.ai_error = pages::ai::remove(id).err(),
            Act::Scroll => {}
            Act::Input(k) => match k {
                0 => self.input = Some(input::Input::new(input::Form::Engine)),
                5 => self.input = Some(input::Input::new(input::Form::Snippet)),
                6 => self.input = Some(input::Input::new(input::Form::NotifyApp)),
                7 => self.input = Some(input::Input::new(input::Form::ClipApp)),
                8 => self.input = Some(input::Input::new(input::Form::ExpandApp)),
                9 => {
                    let mut i = input::Input::new(input::Form::ShotName);
                    i.name = self.cfg.shots.name.clone();
                    self.input = Some(i);
                }
                1 | 2 => {
                    if let Some(i) = self.input.as_mut() {
                        i.field = k - 1;
                    }
                }
                3 => self.submit_input(),
                _ => self.input = None,
            },
        }
        self.invalidate();
    }

    fn submit_input(&mut self) {
        let Some(mut i) = self.input.take() else { return };
        if i.apply(&mut self.cfg) {
            self.save();
        } else {
            self.input = Some(i);
        }
    }

    fn input_char(&mut self, c: u32) -> bool {
        let hwnd = self.hwnd;
        let Some(i) = self.input.as_mut() else { return false };
        match i.char(hwnd, c) {
            Some(true) => self.submit_input(),
            Some(false) => self.input = None,
            None => {}
        }
        // İmleç yanıp sönsün.
        unsafe {
            SetTimer(self.hwnd, TIMER_ANIM, 16, None);
        }
        self.invalidate();
        true
    }

    /// Kaydediciden gelen tuşlar.
    fn key(&mut self, bits: usize, vk: u32) {
        let Some(k) = self.recording else { return };
        let mods = recorder::mods_of(bits);
        if vk == 0x1B && !mods.any() {
            self.stop_recording();
        } else if vk == 0 && !k.mods_only() {
            // Değiştiriciler bırakıldı ama ana tuş yok: beklemeye devam.
            return;
        } else {
            match k.accept(&self.cfg, mods, vk) {
                Ok(s) => {
                    k.set(&mut self.cfg, s);
                    self.error = None;
                    self.save();
                }
                Err(e) => self.error = Some((k, e)),
            }
            self.stop_recording();
        }
        self.invalidate();
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<Act> {
        self.hits.iter().rev().filter(|(r, a)| r.contains(pt) && !matches!(a, Act::Scroll)).map(|(_, a)| a.clone()).next()
    }

    fn paint(&mut self) -> Result<()> {
        let mut rc = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut rc)? };
        let (pw, ph) = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: pw, height: ph }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.rt = Some(rt);
        }
        let rt = self.rt.clone().unwrap();
        unsafe {
            let size = rt.GetPixelSize();
            if size.width != pw || size.height != ph {
                rt.Resize(&D2D_SIZE_U { width: pw, height: ph })?;
            }
        }
        let s = self.scale;
        let (w, h) = (pw as f32 / s, ph as f32 / s);
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        tween::frame();
        carlui_ui::paint::hover_frame();
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = P { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            p.fill(R::new(0.0, 0.0, w, h), 0x101014, 0.30);
            p.fill(R::new(0.0, 0.0, SIDEBAR, h), 0x000000, 0.12);
            self.sidebar(&mut p, h);

            // Başlık sabit, satırlar kaydırılır ve başlığın altında kırpılır.
            let cw = (w - SIDEBAR - 64.0).min(ui::W_MAX);
            let x = SIDEBAR + 32.0 + ((w - SIDEBAR - 64.0 - cw) / 2.0).max(0.0);
            p.text(self.page.title(), R::new(x, 20.0, cw, 36.0), 24.0, true, TEXT, 0.95, Align::Left);
            let clip = R::new(SIDEBAR, 66.0, w - SIDEBAR, h - 66.0);
            p.hit(clip, Act::Scroll);
            let start = p.hits.len();
            let scroll = self.scroll.get(&self.page).copied().unwrap_or(0.0);
            let slide = tween::to(tween::key("page"), 0.0, 13.0);
            let mut f = Flow::new(x, clip.y + 4.0 - scroll + slide, cw);
            // Anahtarlar sayfaya ve içeriğe göre anahtarlanır: sayfa değişince ya da
            // kaydırınca önceki öğenin durumundan "açılıp kapanma" oynamaz.
            tween::place(Page::ALL.iter().position(|pg| *pg == self.page).unwrap_or(0) as u64 + 1, x, f.y);
            unsafe {
                base.PushAxisAlignedClip(&clip.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
                let lp = D2D1_LAYER_PARAMETERS { contentBounds: clip.d2d(), opacity: (1.0 - slide / 18.0).clamp(0.0, 1.0), maskTransform: Default::default(), ..Default::default() };
                base.PushLayer(&lp, None);
            }
            pages::paint(self, &mut p, &mut f);
            tween::place(0, 0.0, 0.0);
            unsafe {
                base.PopLayer();
                base.PopAxisAlignedClip();
            }
            // Kırpılan (görünmeyen) kısımlar tıklanamaz.
            let tail = p.hits.split_off(start);
            for (r, a) in tail {
                let top = r.y.max(clip.y);
                let bottom = r.bottom().min(clip.bottom());
                if bottom > top {
                    p.hits.push((R::new(r.x, top, r.w, bottom - top), a));
                }
            }
            let content = f.y + scroll - clip.y + 24.0;
            self.max_scroll.insert(self.page, (content - clip.h).max(0.0));

            // "Kaydedildi": alttan kayarak gelir, süresi dolunca kayarak gider.
            let shown = self.saved.is_some_and(|at| at.elapsed().as_millis() < 1500);
            let t = tween::to(tween::key("toast"), if shown { 1.0 } else { 0.0 }, 14.0);
            if t > 0.01 {
                let m = carlui_core::t!("Kaydedildi · hemen uygulandı", "Saved · applied right away");
                let tw = p.fonts.width(m, 13.0, false) + 40.0;
                let r = R::new(SIDEBAR + (w - SIDEBAR - tw) / 2.0, h - 52.0 + 24.0 * (1.0 - t), tw, 34.0);
                p.round(r, 17.0, 0x202024, 0.96 * t);
                p.outline(r, 17.0, 0xFFFFFF, 0.12 * t);
                p.glyph('\u{E73E}', R::new(r.x + 12.0, r.y, 16.0, r.h), 12.0, self.accent, t);
                p.text(m, R::new(r.x + 30.0, r.y, r.w - 38.0, r.h), 13.0, false, TEXT, 0.95 * t, Align::Left);
            }
        }
        self.hits = hits;
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
            // Oturmamış animasyon varsa bir sonraki kare (~60 fps); yoksa boşta.
            if tween::busy() {
                SetTimer(self.hwnd, TIMER_ANIM, 16, None);
            } else if self.input.is_some() {
                // Form açık: imleç yanıp sönsün.
                SetTimer(self.hwnd, TIMER_ANIM, 530, None);
            }
        }
        Ok(())
    }

    fn sidebar(&self, p: &mut P, h: f32) {
        p.text(carlui_core::t!("Ayarlar", "Settings"), R::new(20.0, 14.0, SIDEBAR - 30.0, 30.0), 15.0, true, TEXT, 0.95, Align::Left);
        // Seçili sayfanın zemini ve vurgu çizgisi sayfalar arasında kayar.
        let sel = Page::ALL.iter().position(|pg| *pg == self.page).unwrap_or(0) as f32;
        let at = tween::to(tween::key("side"), sel, 16.0);
        // Alçak pencerede (ör. 1366×768) öğeler sıklaşır; alttaki düğmeyle çakışmaz.
        let step = ((h - 56.0 - 58.0) / Page::ALL.len() as f32).clamp(30.0, 42.0);
        let ih = step - 4.0;
        let sr = R::new(10.0, 56.0 + at * step, SIDEBAR - 20.0, ih);
        p.round(sr, 7.0, 0xFFFFFF, 0.10);
        let stretch = ((at - sel).abs() * 10.0).min(8.0);
        p.round(R::new(sr.x, sr.y + ih * 0.26 - stretch, 3.0, sr.h - ih * 0.52 + 2.0 * stretch), 1.5, self.accent, 1.0);
        for (i, pg) in Page::ALL.iter().enumerate() {
            let r = R::new(10.0, 56.0 + i as f32 * step, SIDEBAR - 20.0, ih);
            let hv = tween::to(tween::key(("side-h", i)), if self.page != *pg && p.hover(r) { 1.0 } else { 0.0 }, 18.0);
            if hv > 0.0 {
                p.round(r, 7.0, 0xFFFFFF, 0.06 * hv);
            }
            p.glyph(pg.glyph(), R::new(r.x + 10.0, r.y, 24.0, r.h), 15.0, TEXT, 0.9);
            p.text(pg.title(), R::new(r.x + 42.0, r.y, r.w - 50.0, r.h), 13.5, self.page == *pg, TEXT, 0.92, Align::Left);
            p.hit(r, Act::Page(*pg));
        }
        let r = R::new(10.0, h - 50.0, SIDEBAR - 20.0, 38.0);
        if p.hover(r) {
            p.round(r, 7.0, 0xFFFFFF, 0.06);
        }
        p.glyph('\u{E8A5}', R::new(r.x + 10.0, r.y, 24.0, r.h), 14.0, TEXT, 0.6);
        p.text(carlui_core::t!("Ayar dosyasını aç", "Open settings file"), R::new(r.x + 42.0, r.y, r.w - 50.0, r.h), 12.5, false, TEXT, 0.6, Align::Left);
        p.hit(r, Act::OpenFile);
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with_app(|a| a.paint().ok());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            with_app(|a| a.invalidate());
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = KillTimer(hwnd, wp.0);
            with_app(|a| a.invalidate());
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|a| {
                let (x, y) = xy(lp);
                let pt = (x / a.scale, y / a.scale);
                // Yalnızca sürükleme varsa ya da bir vurgu değişiyorsa çiz; her
                // fare hareketinde bütün pencereyi çizmek GPU'yu boşuna yorar.
                let changed = carlui_ui::paint::hover_changed(&a.hits, a.mouse, pt);
                a.mouse = pt;
                if let Some((f, track)) = a.drag.clone() {
                    f(&mut a.cfg, (a.mouse.0 - track.x) / track.w);
                    a.invalidate();
                } else if changed {
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                match a.hit_at(a.mouse) {
                    Some(act) => a.act(act),
                    // Kayıt sırasında boş yere tıklamak kaydı iptal eder (fare atanamaz).
                    None => {
                        a.stop_recording();
                        a.invalidate();
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|a| {
                if a.drag.take().is_some() {
                    let _ = ReleaseCapture();
                    a.save();
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
            with_app(|a| {
                a.stop_recording();
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            with_app(|a| {
                let d = (wp.0 >> 16) as i16 as f32 / 120.0;
                let max = a.max_scroll.get(&a.page).copied().unwrap_or(0.0);
                let s = a.scroll.entry(a.page).or_insert(0.0);
                *s = (*s - d * 80.0).clamp(0.0, max);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            if with_app(|a| a.input_char(wp.0 as u32)) == Some(true) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        recorder::WM_APP_KEY => {
            with_app(|a| a.key(wp.0, lp.0 as u32));
            LRESULT(0)
        }
        recorder::WM_APP_UNHOOK => {
            recorder::unhook();
            LRESULT(0)
        }
        WM_APP_RELOAD => {
            with_app(|a| {
                if a.drag.is_none() {
                    if let Some(c) = a.reload.lock().ok().and_then(|mut s| s.take()) {
                        a.cfg = c;
                        carlui_ui::tone::follow_window(&a.cfg.theme, a.hwnd);
                        a.invalidate();
                    }
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if wp.0 & 0xFFFF == 0 {
                // Pencere arkaya geçti: kayıt sürmesin (tuşlar başka uygulamaya ait).
                with_app(|a| {
                    a.stop_recording();
                    a.invalidate();
                });
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DPICHANGED => {
            with_app(|a| {
                a.scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                a.rt = None;
            });
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lp.0 as *mut MINMAXINFO);
            mm.ptMinTrackSize = POINT { x: 860, y: 560 };
            LRESULT(0)
        }
        WM_DESTROY => {
            with_app(|a| a.stop_recording());
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() -> Result<()> {
    log::init("settings");
    // Pencere başlığı da doğru dilde çıksın diye ayar en başta okunur.
    let cfg = config::load();
    carlui_ui::tone::follow(&cfg.theme);
    let page = std::env::args().skip_while(|a| a != "--page").nth(1).and_then(|s| Page::from_arg(&s)).unwrap_or(Page::General);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Settings"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(CLASS, None) {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
            return Ok(());
        }
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let icon = LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: icon,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem().max(96) as f32 / 96.0;
        // Küçük ekranda (ör. 1366×768, %125) pencere çalışma alanına sığdırılır.
        let mut wa = windows::Win32::Foundation::RECT::default();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_err() {
            wa = windows::Win32::Foundation::RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) };
        }
        let (aw, ah) = (wa.right - wa.left, wa.bottom - wa.top);
        let (ww, wh) = (((1040.0 * dpi) as i32).min(aw - 16), ((740.0 * dpi) as i32).min(ah - 16));
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, &HSTRING::from(carlui_core::t!("CarlUI Ayarları", "CarlUI Settings")), WS_OVERLAPPEDWINDOW, wa.left + (aw - ww) / 2, wa.top + (ah - wh) / 2, ww, wh, None, None, inst, None)?;
        let on = i32::from(!carlui_ui::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });

        let reload: Arc<Mutex<Option<Config>>> = Arc::default();
        {
            let slot = reload.clone();
            let h = hwnd.0 as isize;
            config::watch(move |c| {
                if let Ok(mut s) = slot.lock() {
                    *s = Some(c);
                }
                let _ = PostMessageW(HWND(h as _), WM_APP_RELOAD, WPARAM(0), LPARAM(0));
            });
        }
        let app = App {
            hwnd,
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            cfg,
            page,
            recording: None,
            error: None,
            notice: None,
            scroll: HashMap::new(),
            max_scroll: HashMap::new(),
            drag: None,
            reload,
            saved: None,
            input: None,
            fetching: Arc::default(),
            ai_error: None,
        };
        tween::enable();
        APP.with(|a| *a.borrow_mut() = Some(app));
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
