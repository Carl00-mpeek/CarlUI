//! Bu modül ilk kurulumdaki karşılama akışı (`carlui-settings --welcome`).
//! Sorun: yeni kullanıcı CarlUI'yi varsayılan görünüşle görüyor, dock'un
//! yerini, adanın biçimini ve hangi modüllerin açık olduğunu bilmeden
//! ayarlar penceresinin on bir sayfasında arıyordu. Çözüm: dört kısa adım
//! (görünüm, dock, ada, modüller) ve bir özet; her seçim `config::update` ile
//! hemen uygulanır, her adım ve tüm akış atlanabilir.
//!
//! Karşılama işareti (`general.welcomed`) pencere açılır açılmaz yazılır:
//! atlamak, kapatmak ya da çökme akışı bir daha göstermez. Ana süreç
//! (`carlui`) akışı yalnızca ilk kurulumda ve tam ekran uygulama yokken açar.
//!
//! Parçalar: `model` adım makinesi ve ayar değişiklikleri, `draw` çerçeve
//! (başlık, ilerleme, düğmeler), `steps` adımların içeriği, `window` Win32
//! penceresi.

mod draw;
mod model;
mod steps;
#[cfg(test)]
mod tests;
mod window;

pub use window::run;

use carlui_core::config::{self, Config, IslandStyle};
use carlui_ui::paint::{Fonts, Painter, R};
use model::{DockChoice, Module, Nav, Outcome, Step};
use std::sync::{Arc, Mutex};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Direct2D::{ID2D1Factory, ID2D1HwndRenderTarget},
};

/// Karşılama penceresindeki tıklanabilir eylemler.
#[derive(Clone, Debug)]
pub enum Go {
    Nav(Nav),
    /// Hazır görünümü uygula (`looks::builtin::all` sırası).
    Look(usize),
    Dock(DockChoice),
    Island(IslandStyle),
    Module(Module),
    /// Tam ayarlar penceresini aç ve karşılamayı kapat.
    OpenSettings,
}

pub type W<'a> = Painter<'a, Go>;

pub struct Welcome {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Go)>,
    pub cfg: Config,
    pub step: Step,
    /// Kullanıcı bu adımda bir şey seçti mi ("Geç" yerine "İleri" yazılır).
    pub touched: [bool; 5],
    reload: Arc<Mutex<Option<Config>>>,
}

impl Welcome {
    fn invalidate(&self) {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Vurgu rengi: ayardaki palet/özel/duvar kâğıdı rengi, temaya göre.
    pub fn accent(&self) -> u32 {
        carlui_ui::tone::accent(carlui_core::accent::from_rgba(self.cfg.theme.accent_pair().0))
    }

    /// Köşe yuvarlaklığı çarpanı (görünüm ayarı).
    pub fn round(&self, r: f32) -> f32 {
        r * self.cfg.theme.corner_radius.clamp(0.0, 2.0)
    }

    /// Değişikliği dosyadaki güncel ayara uygular ve hemen kaydeder; tüm
    /// modüller dosyayı izlediği için anında görünür.
    fn edit(&mut self, f: impl Fn(&mut Config)) {
        match config::update(&f) {
            Some(c) => self.cfg = c,
            None => {
                // Dosya okunamadı: üzerine yazılmaz, yalnızca pencerede gösterilir.
                f(&mut self.cfg);
            }
        }
        self.touched[self.step.index()] = true;
        carlui_ui::tone::follow_window(&self.cfg.theme, self.hwnd);
    }

    fn act(&mut self, g: Go) {
        match g {
            Go::Nav(n) => match model::navigate(self.step, n) {
                Outcome::Stay(s) => {
                    if s != self.step {
                        // Yeni adım geldiği yönden kayarak girer (hareketi azalt açıksa anında).
                        let dir = if s.index() > self.step.index() { 1.0 } else { -1.0 };
                        carlui_ui::tween::set(carlui_ui::tween::key("w-step"), 28.0 * dir);
                        self.step = s;
                    }
                }
                Outcome::Close => {
                    if n == Nav::Skip {
                        // Atla: ayarın geri kalanına dokunulmaz.
                        let _ = config::update(model::mark);
                    }
                    window::close(self.hwnd);
                    return;
                }
            },
            Go::Look(i) => {
                let Some(look) = carlui_core::looks::builtin::all().into_iter().nth(i) else { return };
                if !carlui_core::looks::matches(&look, &self.cfg) {
                    self.edit(move |c| carlui_core::looks::apply(&look, c));
                }
                self.touched[self.step.index()] = true;
            }
            Go::Dock(d) => self.edit(move |c| model::set_dock(c, d)),
            Go::Island(s) => self.edit(move |c| model::set_island(c, s)),
            Go::Module(m) => self.edit(move |c| m.toggle(c)),
            Go::OpenSettings => {
                window::launch_settings();
                window::close(self.hwnd);
                return;
            }
        }
        self.invalidate();
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<Go> {
        self.hits.iter().rev().find(|(r, _)| r.contains(pt)).map(|(_, g)| g.clone())
    }
}
