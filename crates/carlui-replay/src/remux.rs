//! Bu modül "kısayola basınca son dakikaları tek bir video dosyası olarak
//! nasıl veririz?" sorununu çözüyor: diskteki segmentler yeniden kodlanmadan
//! (sıkıştırılmış örnekler olduğu gibi) art arda tek bir MP4'e yazılır.
//! Segmentler aynı ayarla kodlandığı için akış biçimleri aynıdır; yalnızca
//! zaman damgaları her segmentin başlangıcına göre kaydırılır. Birkaç dakikalık
//! kayıt bu yüzden birkaç saniyede hazır olur ve işlemciyi yormaz.

use crate::{ring::Segment, writer::attrs};
use std::path::Path;
use windows::{core::*, Win32::Media::MediaFoundation::*};

const ANY_STREAM: u32 = 0xFFFF_FFFE;
const ENDOFSTREAM: u32 = 0x2;

unsafe fn major(t: &IMFMediaType) -> GUID {
    t.GetGUID(&MF_MT_MAJOR_TYPE).unwrap_or_default()
}

/// Segmentin akışlarını (doğal, sıkıştırılmış tür) listeler.
unsafe fn streams(r: &IMFSourceReader) -> Vec<IMFMediaType> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Ok(t) = r.GetNativeMediaType(i, 0) {
        out.push(t);
        i += 1;
    }
    out
}

unsafe fn reader(p: &Path) -> Result<IMFSourceReader> {
    MFCreateSourceReaderFromURL(&HSTRING::from(p.as_os_str()), None)
}

/// `segs`'i sırayla `out` dosyasına birleştirir; toplam süreyi (sn) döndürür.
pub unsafe fn join(segs: &[Segment], out: &Path) -> Result<f64> {
    let first = segs.first().ok_or_else(|| Error::new(windows::Win32::Foundation::E_FAIL, "kaydedilecek görüntü yok"))?;
    let types = streams(&reader(&first.path)?);
    if types.is_empty() {
        return Err(Error::new(windows::Win32::Foundation::E_FAIL, "segment okunamadı"));
    }
    let a = attrs(1)?;
    a.SetUINT32(&MF_READWRITE_DISABLE_CONVERTERS, 1)?;
    let sink = MFCreateSinkWriterFromURL(&HSTRING::from(out.as_os_str()), None, &a)?;
    // Çıktı akışı, ana türe (görüntü/ses) göre eşlenir.
    let mut outs: Vec<(GUID, u32)> = Vec::new();
    for t in &types {
        let idx = sink.AddStream(t)?;
        sink.SetInputMediaType(idx, t, None)?;
        outs.push((major(t), idx));
    }
    sink.BeginWriting()?;

    let mut offset: i64 = 0;
    for seg in segs {
        let r = match reader(&seg.path) {
            Ok(r) => r,
            Err(e) => {
                carlui_core::warn!("segment açılamadı {}: {e}", seg.path.display());
                continue;
            }
        };
        let map: Vec<Option<u32>> = streams(&r).iter().map(|t| outs.iter().find(|o| o.0 == major(t)).map(|o| o.1)).collect();
        let mut ended = vec![false; map.len()];
        while !ended.iter().all(|e| *e) {
            let mut si = 0u32;
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample = None;
            if r.ReadSample(ANY_STREAM, 0, Some(&mut si), Some(&mut flags), Some(&mut ts), Some(&mut sample)).is_err() {
                break;
            }
            let si = si as usize;
            if flags & ENDOFSTREAM != 0 {
                if let Some(e) = ended.get_mut(si) {
                    *e = true;
                }
            }
            let (Some(s), Some(Some(o))) = (sample, map.get(si)) else { continue };
            s.SetSampleTime(ts + offset)?;
            sink.WriteSample(*o, &s)?;
        }
        offset += seg.dur;
    }
    sink.Finalize()?;
    Ok(offset as f64 / 1e7)
}
