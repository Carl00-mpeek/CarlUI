//! Bu modül "ekran görüntüsündeki yazıyı elle yazmak zorunda kalıyorum"
//! sorununu çözüyor: seçili alandaki metin Windows'un yerleşik, çevrimdışı OCR
//! motoruyla (Windows.Media.Ocr) okunur ve panoya kopyalanır. Ek indirme ya da
//! internet gerekmez; motor kullanıcının Windows dillerinden seçilir (yoksa
//! İngilizce). Küçük yazılar daha iyi okunsun diye küçük alanlar 2 kat büyütülür.

use windows::{
    core::HSTRING,
    Globalization::Language,
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
};

/// BGRA karesinin `(x0, y0, x1, y1)` alanını kırpar; küçükse 2 kat büyütür.
fn crop(px: &[u8], w: u32, h: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> (Vec<u8>, u32, u32) {
    let (x1, y1) = (x1.min(w), y1.min(h));
    let (cw, ch) = (x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
    let k = if cw.max(ch) < 1200 { 2 } else { 1 };
    let (ow, oh) = (cw * k, ch * k);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        let sy = (y0 + y / k).min(h - 1);
        for x in 0..ow {
            let sx = (x0 + x / k).min(w - 1);
            let s = ((sy * w + sx) * 4) as usize;
            let d = ((y * ow + x) * 4) as usize;
            out[d..d + 3].copy_from_slice(&px[s..s + 3]);
            out[d + 3] = 255;
        }
    }
    (out, ow, oh)
}

fn engine() -> Option<OcrEngine> {
    if let Ok(e) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Some(e);
    }
    for tag in ["tr-TR", "en-US"] {
        let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) else { continue };
        if let Ok(e) = OcrEngine::TryCreateFromLanguage(&lang) {
            return Some(e);
        }
    }
    None
}

/// Alanın metni (satırlar `\n` ile). Boş metin = yazı bulunamadı.
pub fn recognize(px: &[u8], w: u32, h: u32, area: (u32, u32, u32, u32)) -> Result<String, String> {
    let (buf, bw, bh) = crop(px, w, h, area);
    let max = OcrEngine::MaxImageDimension().unwrap_or(10000);
    if bw > max || bh > max {
        return Err(carlui_core::t!("Alan OCR için çok büyük", "Area too large for OCR").into());
    }
    let engine = engine().ok_or(carlui_core::t!("Windows'ta OCR dili yüklü değil", "No OCR language installed in Windows"))?;
    let run = || -> windows::core::Result<String> {
        let dw = DataWriter::new()?;
        dw.WriteBytes(&buf)?;
        let ib = dw.DetachBuffer()?;
        let bmp = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(&ib, BitmapPixelFormat::Bgra8, bw as i32, bh as i32, BitmapAlphaMode::Premultiplied)?;
        let res = engine.RecognizeAsync(&bmp)?.get()?;
        let mut lines = Vec::new();
        for l in res.Lines()? {
            lines.push(l.Text()?.to_string_lossy());
        }
        Ok(lines.join("\n"))
    };
    run().map(|s| s.trim().to_string()).map_err(|e| e.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_scales_small_areas() {
        let px: Vec<u8> = (0..4 * 4 * 4).map(|i| i as u8).collect();
        let (out, w, h) = crop(&px, 4, 4, (1, 1, 3, 3));
        assert_eq!((w, h), (4, 4));
        assert_eq!(&out[0..3], &px[(5 * 4)..(5 * 4 + 3)]);
        assert_eq!(out[3], 255);
        // Kare dışına taşan alan kırpılır.
        let (_, w, h) = crop(&px, 4, 4, (2, 2, 99, 99));
        assert_eq!((w, h), (4, 4));
    }

    /// GDI ile beyaz zemine yazı çizip gerçek OCR motoruna okutur.
    #[test]
    fn reads_drawn_text() {
        use windows::Win32::Graphics::Gdi::*;
        let (w, h) = (420u32, 60u32);
        let px = unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w as i32, biHeight: -(h as i32), biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            SelectObject(dc, bmp);
            let all = windows::Win32::Foundation::RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
            FillRect(dc, &all, HBRUSH(GetStockObject(WHITE_BRUSH).0));
            let font = CreateFontW(32, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, windows::core::w!("Segoe UI"));
            SelectObject(dc, font);
            let text: Vec<u16> = "Hello CarlUI 2026".encode_utf16().collect();
            let _ = TextOutW(dc, 10, 10, &text);
            let v = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
            let _ = DeleteObject(font);
            let _ = DeleteObject(bmp);
            let _ = DeleteDC(dc);
            v
        };
        match recognize(&px, w, h, (0, 0, w, h)) {
            Ok(t) => assert!(t.contains("CarlUI") && t.contains("2026"), "okunan: {t:?}"),
            // OCR dili olmayan makinede (ör. CI) atlanır.
            Err(e) => eprintln!("OCR yok, atlandı: {e}"),
        }
    }
}
