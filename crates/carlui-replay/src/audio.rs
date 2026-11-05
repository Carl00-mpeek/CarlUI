//! Bu modül "bilgisayarın çıkardığı sesi (oyun, müzik, sohbet) nasıl kayda
//! alırız?" sorununu çözüyor: WASAPI geri döngü (loopback) yakalama. Varsayılan
//! çıkış aygıtına giden karışım okunur, 48 kHz 16 bit stereoya çevrilir. Aygıt
//! değişince (kulaklık takıldı, varsayılan değişti) yakalama yeniden kurulur.
//! Ses aygıtı hiç çalmıyorsa Windows paket göndermez; boşluğu kaydedici
//! sessizlikle doldurur. Aynı kod `mic` ile varsayılan mikrofonu da okur.

use crate::writer::AUDIO_RATE;
use windows::{
    core::*,
    Win32::{
        Media::Audio::*,
        System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
    },
};

/// Doğrusal örnekleme hızı dönüştürücü (stereo). Paketler arasında durum tutar.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
}

impl Resampler {
    pub fn new(in_rate: u32) -> Self {
        Self { step: in_rate as f64 / AUDIO_RATE as f64, pos: 0.0, prev: [0.0; 2] }
    }

    /// `x`: stereo f32 (iç içe). Çıktı 48 kHz stereo i16 olarak `out`'a eklenir.
    pub fn push(&mut self, x: &[f32], out: &mut Vec<i16>) {
        let n = x.len() / 2;
        if n == 0 {
            return;
        }
        let conv = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend(x.iter().map(|&v| conv(v)));
            return;
        }
        // Sanal dizi: y[0] = önceki paketin son örneği, y[k+1] = x[k].
        let prev = self.prev;
        let y = |i: usize, c: usize| if i == 0 { prev[c] } else { x[(i - 1) * 2 + c] };
        while self.pos + 1.0 <= n as f64 {
            let i = self.pos.floor() as usize;
            let f = (self.pos - i as f64) as f32;
            for c in 0..2 {
                let (a, b) = (y(i, c), y(i + 1, c));
                out.push(conv(a + (b - a) * f));
            }
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = [x[(n - 1) * 2], x[(n - 1) * 2 + 1]];
    }
}

enum Sample {
    F32,
    I16,
    I32,
}

pub struct Loopback {
    client: IAudioClient,
    cap: IAudioCaptureClient,
    pub id: String,
    ch: usize,
    kind: Sample,
    rs: Resampler,
    tmp: Vec<f32>,
}

fn flow(mic: bool) -> EDataFlow {
    if mic {
        eCapture
    } else {
        eRender
    }
}

pub unsafe fn default_id(mic: bool) -> Option<String> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
    let dev = en.GetDefaultAudioEndpoint(flow(mic), eConsole).ok()?;
    let p = dev.GetId().ok()?;
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as _));
    s
}

impl Loopback {
    /// `mic`: varsayılan mikrofon; değilse varsayılan çıkışın geri döngüsü.
    pub unsafe fn open(mic: bool) -> Result<Self> {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(flow(mic), eConsole)?;
        let id = {
            let p = dev.GetId()?;
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as _));
            s
        };
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let wf = client.GetMixFormat()?;
        let f = &*wf;
        let ch = f.nChannels as usize;
        let rate = f.nSamplesPerSec;
        let bits = f.wBitsPerSample;
        let tag = if f.wFormatTag == 0xFFFE {
            // WAVEFORMATEXTENSIBLE: alt biçimin ilk alanı 1 = PCM, 3 = kayan nokta.
            (*(wf as *const WAVEFORMATEXTENSIBLE)).SubFormat.data1 as u16
        } else {
            f.wFormatTag
        };
        let kind = match (tag, bits) {
            (3, 32) => Some(Sample::F32),
            (1, 16) => Some(Sample::I16),
            (1, 32) => Some(Sample::I32),
            _ => None,
        };
        // 200 ms tampon; 10 ms'de bir okunur.
        let r = client.Initialize(AUDCLNT_SHAREMODE_SHARED, if mic { 0 } else { AUDCLNT_STREAMFLAGS_LOOPBACK }, 2_000_000, 0, wf, None);
        CoTaskMemFree(Some(wf as _));
        r?;
        let Some(kind) = kind.filter(|_| ch > 0) else {
            return Err(Error::new(windows::Win32::Foundation::E_FAIL, "desteklenmeyen ses biçimi"));
        };
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        Ok(Self { client, cap, id, ch, kind, rs: Resampler::new(rate), tmp: Vec::new() })
    }

    /// Bekleyen tüm paketleri okur; her paket için (48k stereo i16, QPC 100 ns)
    /// `f` çağrılır. Hata (aygıt gitti) dönerse çağıran yeniden açar.
    pub unsafe fn read(&mut self, mut f: impl FnMut(&[i16], i64)) -> Result<bool> {
        let mut any = false;
        let mut out = Vec::new();
        loop {
            if self.cap.GetNextPacketSize()? == 0 {
                return Ok(any);
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut qpc = 0u64;
            self.cap.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))?;
            let n = frames as usize;
            self.tmp.clear();
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
            for i in 0..n {
                for c in 0..2 {
                    // Mono ise iki kanala da aynı örnek; çok kanallıda ilk ikisi.
                    let src = c.min(self.ch - 1);
                    let v = if silent {
                        0.0
                    } else {
                        let k = i * self.ch + src;
                        match self.kind {
                            Sample::F32 => *(data as *const f32).add(k),
                            Sample::I16 => *(data as *const i16).add(k) as f32 / 32768.0,
                            Sample::I32 => *(data as *const i32).add(k) as f32 / 2147483648.0,
                        }
                    };
                    self.tmp.push(v);
                }
            }
            self.cap.ReleaseBuffer(frames)?;
            out.clear();
            self.rs.push(&self.tmp, &mut out);
            let t = if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0 || qpc == 0 { crate::ring::now() } else { qpc as i64 };
            if !out.is_empty() {
                f(&out, t);
                any = true;
            }
        }
    }
}

impl Drop for Loopback {
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
    fn resample_length() {
        for rate in [44_100u32, 48_000, 96_000] {
            let mut r = Resampler::new(rate);
            let mut out = Vec::new();
            // 1 sn, 100 paket halinde.
            let pkt = vec![0.5f32; (rate / 100) as usize * 2];
            for _ in 0..100 {
                r.push(&pkt, &mut out);
            }
            let frames = out.len() / 2;
            assert!((frames as i64 - 48_000).abs() < 4, "{rate}: {frames}");
            // İlk örnek öncekiyle (sessizlik) karışır.
            assert!(out.iter().skip(4).all(|&v| (v - 16383).abs() < 2));
        }
    }
}
