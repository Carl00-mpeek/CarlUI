//! GPU altyapısı: Windows.UI.Composition + Direct2D.
//!
//! Görseller ve animasyonlar DWM'in compositor thread'inde çalışır; animasyon
//! sırasında bizim süreç neredeyse hiç CPU kullanmaz. Direct2D yalnızca ikon
//! gibi statik içerikleri bir kez çizip yüzeye yazmak için kullanılır.

use windows::{
    core::*,
    Foundation::{Numerics::Matrix3x2, Size},
    Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat},
    UI::Composition::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::*,
            Direct3D11::*,
            Dxgi::*,
            Imaging::*,
        },
        System::{Com::*, Threading::*, WinRT::Composition::*},
    },
};

pub struct Gfx {
    pub comp: Compositor,
    pub gdev: CompositionGraphicsDevice,
    pub wic: IWICImagingFactory,
    pub round_stroke: ID2D1StrokeStyle,
    _d2d: ID2D1Device,
}

fn create_d3d() -> Result<ID3D11Device> {
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut dev = None;
        let ok = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                None,
            )
        };
        if let (Ok(()), Some(dev)) = (ok, dev) {
            return Ok(dev);
        }
    }
    Err(E_FAIL.into())
}

/// İş parçacığına geçirilen aygıt (yalnızca kayıp sorgusu yapılır; bu çağrı
/// her iş parçacığından güvenlidir).
struct Dev(ID3D11Device);
unsafe impl Send for Dev {}

/// Ekran kartı sürücüsü güncellenince ya da sıfırlanınca GPU aygıtı kaybolur;
/// bir kez çizilip bırakılan yüzeyler (dock simgeleri, tepsi…) boş kalır.
/// Bu iş parçacığı sürücü değişimini (yoksa 10 sn'de bir) bekler, aygıtın
/// kaybolduğunu görünce süreci hata koduyla kapatır; CarlUI'nin bekçisi modülü
/// yeni aygıtla baştan başlatır. Beklerken CPU harcamaz.
fn watch_device(dev: &ID3D11Device) {
    let dev = Dev(dev.clone());
    let _ = std::thread::Builder::new().name("gpu-watch".into()).spawn(move || unsafe {
        let dev = dev;
        let event = CreateEventW(None, false, false, None).ok();
        let factory: Option<IDXGIFactory7> = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).ok();
        if let (Some(f), Some(e)) = (&factory, event) {
            let _ = f.RegisterAdaptersChangedEvent(e);
        }
        loop {
            match event {
                Some(e) => {
                    WaitForSingleObject(e, 10_000);
                }
                None => std::thread::sleep(std::time::Duration::from_secs(10)),
            }
            if dev.0.GetDeviceRemovedReason().is_err() {
                // Sürücü yerleşsin; hemen başlarsak yeni aygıt da kurulamayabilir.
                std::thread::sleep(std::time::Duration::from_millis(1500));
                std::process::exit(3);
            }
        }
    });
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            let comp = Compositor::new()?;
            let d3d = create_d3d()?;
            watch_device(&d3d);
            let dxgi: IDXGIDevice = d3d.cast()?;
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d = factory.CreateDevice(&dxgi)?;
            let gdev = comp.cast::<ICompositorInterop>()?.CreateGraphicsDevice(&d2d)?;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 1.0,
                ..Default::default()
            };
            let round_stroke = factory.CreateStrokeStyle(&props, None)?.cast()?;
            Ok(Self { comp, gdev, wic, round_stroke, _d2d: d2d })
        }
    }

    /// `w`×`h` piksellik bir yüzey oluşturur ve `draw` ile bir kez içine çizer.
    pub fn surface(
        &self,
        w: f32,
        h: f32,
        draw: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
    ) -> Result<CompositionDrawingSurface> {
        let surface = self.gdev.CreateDrawingSurface(
            Size { Width: w, Height: h },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
        unsafe {
            let mut off = POINT::default();
            let dc: ID2D1DeviceContext = interop.BeginDraw(None, &mut off)?;
            dc.SetDpi(96.0, 96.0);
            dc.SetTransform(&Matrix3x2::translation(off.x as f32, off.y as f32));
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let drawn = draw(&dc);
            interop.EndDraw()?;
            drawn?;
        }
        Ok(surface)
    }
}
