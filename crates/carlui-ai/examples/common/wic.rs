// Örnekler için küçük WIC okuma/yazma.
use carlui_ai::Image;
use windows::{core::*, Win32::{Foundation::GENERIC_READ, Graphics::Imaging::*, System::Com::*}};

fn factory() -> Result<IWICImagingFactory> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
    }
}

pub fn load(path: &str) -> Image {
    let f = factory().unwrap();
    unsafe {
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).unwrap();
        let frame = dec.GetFrame(0).unwrap();
        let conv = f.CreateFormatConverter().unwrap();
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).unwrap();
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).unwrap();
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut bgra).unwrap();
        Image { w, h, bgra }
    }
}

pub fn save(path: &str, p: &Image) {
    let f = factory().unwrap();
    unsafe {
        let stream = f.CreateStream().unwrap();
        stream.InitializeFromFilename(&HSTRING::from(path), 0x4000_0000).unwrap();
        let enc = f.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).unwrap();
        enc.Initialize(&stream, WICBitmapEncoderNoCache).unwrap();
        let (mut frame, mut bag) = (None, None);
        enc.CreateNewFrame(&mut frame, &mut bag).unwrap();
        let frame = frame.unwrap();
        frame.Initialize(bag.as_ref()).unwrap();
        frame.SetSize(p.w, p.h).unwrap();
        let mut fmt = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut fmt).unwrap();
        let src = f.CreateBitmapFromMemory(p.w, p.h, &GUID_WICPixelFormat32bppBGRA, p.w * 4, &p.bgra).unwrap();
        frame.WriteSource(&src, std::ptr::null()).unwrap();
        frame.Commit().unwrap();
        enc.Commit().unwrap();
    }
}
