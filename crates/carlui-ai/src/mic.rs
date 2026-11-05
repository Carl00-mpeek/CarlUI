//! Bu modül "konuşma tanıma için mikrofondan nasıl ses alırız?" sorununu
//! çözüyor: varsayılan mikrofon WASAPI paylaşımlı kipte açılır, gelen paketler
//! tek kanala indirilip 16 kHz'e çevrilir (Whisper'ın istediği biçim). Çağıran
//! iş parçacığında COM açık olmalı.

use windows::Win32::{
    Media::Audio::*,
    System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
};

#[derive(Clone, Copy)]
enum Sample {
    F32,
    I16,
    I32,
}

pub struct Mic {
    client: IAudioClient,
    cap: IAudioCaptureClient,
    ch: usize,
    kind: Sample,
    rs: Rs,
}

impl Mic {
    /// Varsayılan mikrofonu açar ve kaydı başlatır.
    pub fn open() -> Result<Self, String> {
        unsafe { Self::open_inner().map_err(|e| e.message()) }
    }

    unsafe fn open_inner() -> windows::core::Result<Self> {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let wf = client.GetMixFormat()?;
        let f = &*wf;
        let (ch, rate, bits) = (f.nChannels as usize, f.nSamplesPerSec, f.wBitsPerSample);
        let tag = if f.wFormatTag == 0xFFFE { (*(wf as *const WAVEFORMATEXTENSIBLE)).SubFormat.data1 as u16 } else { f.wFormatTag };
        let kind = match (tag, bits) {
            (3, 32) => Some(Sample::F32),
            (1, 16) => Some(Sample::I16),
            (1, 32) => Some(Sample::I32),
            _ => None,
        };
        let r = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, wf, None);
        CoTaskMemFree(Some(wf as _));
        r?;
        let Some(kind) = kind.filter(|_| ch > 0) else {
            return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "desteklenmeyen ses biçimi"));
        };
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        Ok(Self { client, cap, ch, kind, rs: Rs::new(rate) })
    }

    /// Bekleyen paketleri 16 kHz tek kanal olarak `out`'a ekler; bu turdaki
    /// en yüksek seviye (0..1) döner. Birkaç on ms'de bir çağrılmalı.
    pub fn read(&mut self, out: &mut Vec<f32>) -> Result<f32, String> {
        let mut peak = 0f32;
        unsafe {
            loop {
                if self.cap.GetNextPacketSize().map_err(|e| e.message())? == 0 {
                    return Ok(peak);
                }
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                self.cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None).map_err(|e| e.message())?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                for i in 0..frames as usize {
                    let mut v = 0f32;
                    if !silent {
                        for c in 0..self.ch {
                            let k = i * self.ch + c;
                            v += match self.kind {
                                Sample::F32 => *(data as *const f32).add(k),
                                Sample::I16 => *(data as *const i16).add(k) as f32 / 32768.0,
                                Sample::I32 => *(data as *const i32).add(k) as f32 / 2147483648.0,
                            };
                        }
                        v /= self.ch as f32;
                    }
                    peak = peak.max(v.abs());
                    self.rs.push(v, out);
                }
                self.cap.ReleaseBuffer(frames).map_err(|e| e.message())?;
            }
        }
    }
}

/// Doğrusal yeniden örnekleme (tek kanal → 16 kHz); paketler arasında durum tutar.
struct Rs {
    /// Giriş örnekleri / çıkış örneği (ör. 48000/16000 = 3).
    step: f64,
    /// Bir sonraki çıkış örneğinin önceki giriş (0) ile şimdiki giriş (1) arasındaki yeri.
    pos: f64,
    prev: f32,
}

impl Rs {
    fn new(rate: u32) -> Self {
        Self { step: rate as f64 / crate::whisper::SR as f64, pos: 0.0, prev: 0.0 }
    }

    fn push(&mut self, x: f32, out: &mut Vec<f32>) {
        while self.pos <= 1.0 {
            out.push(self.prev + (x - self.prev) * self.pos as f32);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = x;
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_rate() {
        for rate in [16_000u32, 44_100, 48_000] {
            let mut r = Rs::new(rate);
            let mut out = Vec::new();
            for i in 0..rate {
                r.push((i as f32 * 0.001).sin(), &mut out);
            }
            assert!((out.len() as i64 - 16_000).abs() <= 2, "{rate}: {}", out.len());
        }
    }
}
