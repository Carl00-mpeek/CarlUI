//! Bu modül CarlUI'nin tek dosyalık kurucusu (`CarlUI-Setup.exe`). Sorun:
//! CarlUI on ayrı exe, bir tarayıcı eklentisi ve isteğe bağlı bir sensör
//! görevinden oluşuyor; elle kopyalamak, kısayol açmak ve kaldırmak zahmetli
//! ve hataya açık. Çözüm: hepsi bu exe'nin içine gömülü (`payload`); ayarlar
//! penceresiyle aynı cam pencerede konum ve seçenekler seçilir, kurulum
//! yönetici izni istemeden `%LOCALAPPDATA%\Programs\CarlUI`'ye yapılır.
//! Gömülü dosyalar tek bir zstd akışında sıkıştırılmış durur. Kurulum
//! klasörüne aynı kodun dosyasız küçük derlemesi (`uninstaller` özelliği)
//! kaldırıcı olarak konur (`CarlUI-Uninstall.exe --uninstall`).
//!
//! Komut satırı:
//!   --uninstall            kaldırma penceresi
//!   --dir <klasör>         kurulum/kaldırma klasörü
//!   --test <klasör>        deneme: yalnızca dosyalar yazılır (kayıt, kısayol, görev yok)
//!   --headless             pencere açmadan çalış: --test ile deneme, tek başına
//!                          varsayılan seçeneklerle sessiz kurulum
//!   --wallpaper            sessiz kurulumda CarlUI duvar kâğıdını da uygula
//!   --update               adadan güncelleme: mevcut kurulumun üstüne sessizce
//!                          kurar, yalnızca var olan kısayolları yeniler, yönetici
//!                          izni istemez, sonunda CarlUI'yi yeniden açar
//!   --snapshot <sayfa> <png>  sayfayı pencere açmadan PNG'ye çiz (welcome, work, done, confirm)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod install;
mod office;
mod payload;
mod shell;
mod space;
mod uninstall;
mod view;

use carlui_ui::{
    glassmenu,
    paint::{Fonts, Painter, R},
    tween,
};
use std::{cell::RefCell, path::PathBuf, sync::Arc};
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
            Imaging::*,
        },
        System::{Com::*, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::{Controls::MARGINS, HiDpi::*, WindowsAndMessaging::*},
    },
};

pub const WM_APP_PROGRESS: u32 = WM_APP + 1;
const WM_APP_BROWSE: u32 = WM_APP + 2;
const TIMER_ANIM: usize = 2;
const CLASS: PCWSTR = w!("CarlUI.Setup");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Welcome,
    Confirm,
    Work,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Opt {
    Desktop,
    Launch,
    Sensors,
    Firefox,
    Wallpaper,
    Office,
    Purge,
}

#[derive(Clone)]
pub enum Act {
    Install,
    Uninstall,
    Close,
    Browse,
    Toggle(Opt),
    OpenExt,
    Firefox,
    Chrome,
}

/// Eklenti sayfası açılabilen Chromium tarayıcı: (exe, adres).
pub type Chromium = (String, &'static str);

pub struct App {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    logo: Option<ID2D1Bitmap>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Act)>,
    accent: u32,
    pub mode: Mode,
    pub page: Page,
    pub opts: install::Options,
    pub purge: bool,
    pub pawnio: bool,
    pub installed_before: bool,
    /// Hedef sürücüdeki boş yer (ölçülemezse None).
    pub free: Option<u64>,
    /// Mevcut kurulumun diskte tuttuğu yer.
    pub now_bytes: u64,
    pub progress: install::Shared,
    pub firefox: Option<String>,
    pub chrome: Option<Chromium>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

struct Args {
    mode: Mode,
    dir: Option<PathBuf>,
    dry: bool,
    headless: bool,
    temp: bool,
    wallpaper: bool,
    update: bool,
    snapshot: Option<(String, PathBuf)>,
}

fn parse_args() -> Args {
    let v: Vec<String> = std::env::args().skip(1).collect();
    let after = |flag: &str, n: usize| v.iter().position(|a| a == flag).and_then(|i| v.get(i + n)).cloned();
    let has = |flag: &str| v.iter().any(|a| a == flag);
    let test = after("--test", 1).map(PathBuf::from);
    Args {
        mode: if has("--uninstall") { Mode::Uninstall } else { Mode::Install },
        dry: test.is_some(),
        dir: test.or_else(|| after("--dir", 1).map(PathBuf::from)),
        headless: has("--headless"),
        temp: has("--temp"),
        wallpaper: has("--wallpaper"),
        update: has("--update"),
        snapshot: after("--snapshot", 1).zip(after("--snapshot", 2).map(PathBuf::from)),
    }
}

/// Önceki kurulumun yeri (Programlar listesindeki kayıt), yoksa varsayılan.
fn existing_dir() -> PathBuf {
    shell::reg_get(windows::Win32::System::Registry::HKEY_CURRENT_USER, shell::UNINSTALL_KEY, "InstallLocation").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(shell::default_dir)
}

fn chromium() -> Option<Chromium> {
    shell::app_path("chrome.exe").map(|p| (p, "chrome://extensions")).or_else(|| shell::app_path("msedge.exe").map(|p| (p, "edge://extensions")))
}

impl App {
    fn new(hwnd: HWND, mode: Mode, dir: PathBuf, dry: bool) -> Result<Self> {
        let pawnio = shell::pawnio_installed();
        let installed_before = dir.join("carlui.exe").exists();
        Ok(Self {
            hwnd,
            rt: None,
            logo: None,
            factory: unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? },
            fonts: Fonts::new()?,
            scale: 1.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            mode,
            page: if mode == Mode::Install { Page::Welcome } else { Page::Confirm },
            opts: install::Options { dir: dir.clone(), desktop: true, launch: true, sensors: pawnio, firefox: payload::XPI.is_some(), wallpaper: false, office: office::wanted(), dry },
            purge: false,
            pawnio,
            installed_before,
            free: space::free_bytes(&dir),
            now_bytes: space::installed_now(&dir),
            progress: Arc::default(),
            firefox: shell::app_path("firefox.exe"),
            chrome: chromium(),
        })
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    fn reporter(&self) -> install::Reporter {
        install::Reporter { hwnd: self.hwnd.0 as isize, shared: self.progress.clone() }
    }

    fn go(&mut self, page: Page) {
        if self.page != page {
            tween::set(tween::key("page"), 18.0);
        }
        self.page = page;
    }

    /// Seçilen sürücüde kurulum için yeterli yer var mı (ölçülemezse var sayılır).
    pub fn space_ok(&self) -> bool {
        self.free.is_none_or(|f| f >= space::needed(&self.opts, self.now_bytes))
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::Install if !self.space_ok() => {}
            Act::Install => {
                self.go(Page::Work);
                install::start(self.opts.clone(), self.reporter());
            }
            Act::Uninstall => {
                self.go(Page::Work);
                uninstall::start(self.opts.dir.clone(), self.purge, self.opts.dry, self.reporter());
            }
            Act::Close => unsafe {
                let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            Act::Browse => unsafe {
                let _ = PostMessageW(self.hwnd, WM_APP_BROWSE, WPARAM(0), LPARAM(0));
            },
            Act::Toggle(o) => {
                let v = match o {
                    Opt::Desktop => &mut self.opts.desktop,
                    Opt::Launch => &mut self.opts.launch,
                    Opt::Sensors => &mut self.opts.sensors,
                    Opt::Firefox => &mut self.opts.firefox,
                    Opt::Wallpaper => &mut self.opts.wallpaper,
                    Opt::Office => &mut self.opts.office,
                    Opt::Purge => &mut self.purge,
                };
                *v = !*v;
            }
            Act::OpenExt => shell::open(&install::ext_dir(&self.opts.dir).display().to_string(), ""),
            Act::Firefox => {
                if let Some(f) = &self.firefox {
                    shell::open(f, "about:debugging#/runtime/this-firefox");
                }
            }
            Act::Chrome => {
                if let Some((exe, url)) = &self.chrome {
                    shell::open(exe, url);
                }
            }
        }
        self.invalidate();
    }

    fn progressed(&mut self) {
        let done = self.progress.lock().map(|p| p.done.is_some()).unwrap_or(false);
        if done && self.page == Page::Work {
            self.go(Page::Done);
        }
        self.invalidate();
    }

    /// Sayfayı verilen hedefe çizer (pencere ya da PNG için bitmap).
    fn render(&mut self, rt: &ID2D1RenderTarget, opaque: bool) {
        let s = self.scale;
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        tween::frame();
        if self.logo.is_none() {
            self.logo = logo_bitmap(rt);
        }
        let logo = self.logo.clone();
        unsafe {
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        {
            let mut p: view::P = Painter { rt, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            if opaque {
                p.fill(R::new(0.0, 0.0, view::W, view::H), 0x202028, 1.0);
            }
            p.fill(R::new(0.0, 0.0, view::W, view::H), 0x101014, 0.30);
            self.draw(&mut p, logo.as_ref());
        }
        self.hits = hits;
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
            self.logo = None;
        }
        let rt = self.rt.clone().unwrap();
        unsafe {
            let size = rt.GetPixelSize();
            if size.width != pw || size.height != ph {
                rt.Resize(&D2D_SIZE_U { width: pw, height: ph })?;
            }
            rt.BeginDraw();
        }
        let base: ID2D1RenderTarget = rt.cast()?;
        self.render(&base, false);
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
            if tween::busy() || self.page == Page::Work {
                SetTimer(self.hwnd, TIMER_ANIM, 16, None);
            }
        }
        Ok(())
    }
}

fn wic() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

/// Gömülü logo PNG'si → Direct2D bitmap.
fn logo_bitmap(rt: &ID2D1RenderTarget) -> Option<ID2D1Bitmap> {
    let wic = wic()?;
    unsafe {
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(payload::LOGO).ok()?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        rt.CreateBitmapFromWicBitmap(&conv, None).ok()
    }
}

/// Sayfayı pencere açmadan PNG'ye çizer (kullanıcıyı rahatsız etmeden görsel denetim).
fn snapshot(page: &str, out: &PathBuf, mode: Mode, dir: PathBuf) -> Result<()> {
    let mut app = App::new(HWND::default(), mode, dir, true)?;
    app.scale = 1.5;
    app.page = match page {
        "work" => Page::Work,
        "done" => Page::Done,
        "confirm" => Page::Confirm,
        _ => Page::Welcome,
    };
    if let Ok(mut p) = app.progress.lock() {
        p.step = t!("Kopyalanıyor · carlui-dock.exe", "Copying · carlui-dock.exe").into();
        p.frac = 0.42;
        if app.page == Page::Done {
            p.done = Some(install::Outcome { ok: true, notes: vec![t!("Firefox bir sonraki açılışta eklentiyi etkinleştirmek için onay isteyecek", "Firefox will ask to enable the extension next time it starts").into()] });
        }
    }
    let wic = wic().ok_or_else(|| Error::from(E_FAIL))?;
    let (pw, ph) = ((view::W * app.scale) as u32, (view::H * app.scale) as u32);
    unsafe {
        let bmp = wic.CreateBitmap(pw, ph, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES { r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE, pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
        let rt = app.factory.CreateWicBitmapRenderTarget(&bmp, &props)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        rt.BeginDraw();
        app.render(&rt, true);
        rt.EndDraw(None, None)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(out.as_os_str()), GENERIC_WRITE.0)?;
        let enc = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        enc.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(|| Error::from(E_FAIL))?;
        frame.Initialize(None)?;
        frame.WriteSource(&bmp, std::ptr::null())?;
        frame.Commit()?;
        enc.Commit()?;
    }
    Ok(())
}

/// Pencere açmadan çalıştırır; sonuç stdout'a ve klasördeki rapora yazılır.
fn headless(mode: Mode, opts: install::Options, purge: bool) {
    let shared: install::Shared = Arc::default();
    let rep = install::Reporter { hwnd: 0, shared: shared.clone() };
    let dir = opts.dir.clone();
    match mode {
        Mode::Install => install::start(opts, rep),
        Mode::Uninstall => uninstall::start(dir.clone(), purge, opts.dry, rep),
    }
    let out = loop {
        if let Some(o) = shared.lock().ok().and_then(|p| p.done.clone()) {
            break o;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let mut text = format!("ok={}\n", out.ok);
    for n in &out.notes {
        text += &format!("- {n}\n");
    }
    print!("{text}");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("setup-report.txt"), text);
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
        WM_TIMER => {
            let _ = KillTimer(hwnd, wp.0);
            with_app(|a| a.invalidate());
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                if let Some(act) = a.hits.iter().rev().find(|(r, _)| r.contains(a.mouse)).map(|(_, act)| act.clone()) {
                    a.act(act);
                }
            });
            LRESULT(0)
        }
        WM_APP_PROGRESS => {
            with_app(|a| a.progressed());
            LRESULT(0)
        }
        WM_APP_BROWSE => {
            // Klasör seçici kalıcı bir pencere: uygulama durumu ödünç alınmadan açılır.
            if let Some(start) = with_app(|a| a.opts.dir.clone()) {
                if let Some(dir) = shell::pick_folder(hwnd, &start) {
                    with_app(|a| {
                        a.installed_before = dir.join("carlui.exe").exists();
                        a.free = space::free_bytes(&dir);
                        a.now_bytes = space::installed_now(&dir);
                        a.opts.dir = dir;
                        a.invalidate();
                    });
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // Kurulum yarıda kesilmesin.
            if with_app(|a| a.page == Page::Work) != Some(true) {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
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
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Kaldırıcı kurulum klasöründen çalışıyorsa kendini kilitlememek için
/// %TEMP%'e kopyalanıp oradan yeniden başlar.
fn relaunch_from_temp(dir: &PathBuf) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let tmp = std::env::temp_dir().join(shell::temp_copy_name(std::process::id()));
    if std::fs::copy(&me, &tmp).is_err() {
        return false;
    }
    std::process::Command::new(&tmp).arg("--uninstall").arg("--temp").arg("--dir").arg(dir).spawn().is_ok()
}

fn main() -> Result<()> {
    let mut args = parse_args();
    // Kurulum klasöründeki küçük kaldırıcı yalnızca kaldırır; içinde kurulacak dosya yok.
    if cfg!(feature = "uninstaller") {
        args.mode = Mode::Uninstall;
        args.update = false;
    }
    // Kurulu CarlUI varsa (güncelleme, kaldırma) Ayarlar'da seçilen dil
    // kullanılır; ilk kurulumda Windows'un dili.
    if carlui_core::paths::config_file().exists() {
        let _ = carlui_core::config::load();
    } else {
        carlui_core::lang::apply("auto");
    }
    shell::sweep_temp_copies();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let dir = args.dir.clone().unwrap_or_else(|| {
        let me = std::env::current_exe().unwrap_or_default();
        let from_install = me.file_name().is_some_and(|n| n.eq_ignore_ascii_case(shell::UNINSTALLER));
        match (args.mode, me.parent()) {
            (Mode::Uninstall, Some(p)) if from_install => p.to_path_buf(),
            _ => existing_dir(),
        }
    });

    if let Some((page, out)) = &args.snapshot {
        return snapshot(page, out, args.mode, dir);
    }
    if args.headless && args.dry {
        let opts = install::Options { dir, desktop: false, launch: false, sensors: false, firefox: false, wallpaper: false, office: true, dry: true };
        headless(args.mode, opts, false);
        return Ok(());
    }
    if args.update {
        // Kullanıcının önceki seçimleri korunur: silinmiş masaüstü kısayolu geri
        // gelmez, sıcaklık görevi zaten kurulu (UAC sorulmaz), Firefox kaydı yalnızca
        // önceden varsa yenilenir.
        let firefox = payload::XPI.is_some() && shell::reg_get(windows::Win32::System::Registry::HKEY_CURRENT_USER, shell::FIREFOX_KEY, shell::EXT_ID).is_some();
        let desktop = shell::desktop_link().is_some_and(|l| l.exists());
        let exe = dir.join("carlui.exe");
        let opts = install::Options { dir, desktop, launch: true, sensors: false, firefox, wallpaper: false, office: office::wanted(), dry: false };
        headless(Mode::Install, opts, false);
        // Kurulum yarıda kaldıysa da kullanıcı CarlUI'siz kalmasın.
        if !shell::carlui_running() && exe.exists() {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !shell::carlui_running() {
                shell::open(&exe.display().to_string(), "");
            }
        }
        return Ok(());
    }
    if args.headless && args.mode == Mode::Install {
        // Sessiz gerçek kurulum: penceredeki varsayılan seçeneklerle.
        let opts = install::Options { dir, desktop: true, launch: true, sensors: shell::pawnio_installed(), firefox: payload::XPI.is_some(), wallpaper: args.wallpaper, office: office::wanted(), dry: false };
        headless(args.mode, opts, false);
        return Ok(());
    }
    if args.mode == Mode::Uninstall && !args.temp && !args.dry && relaunch_from_temp(&dir) {
        return Ok(());
    }

    unsafe {
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Setup"));
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
        let dpi = GetDpiForSystem().max(96);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut r = RECT { left: 0, top: 0, right: (view::W * dpi as f32 / 96.0) as i32, bottom: (view::H * dpi as f32 / 96.0) as i32 };
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dpi);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let title = HSTRING::from(if args.mode == Mode::Install { t!("CarlUI Kurulum", "CarlUI Setup") } else { t!("CarlUI'yi Kaldır", "Uninstall CarlUI") });
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, &title, style, (sw - ww) / 2, (sh - wh) / 2, ww, wh, None, None, inst, None)?;
        let on: i32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });

        let mut app = App::new(hwnd, args.mode, dir, args.dry)?;
        app.scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
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
