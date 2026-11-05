//! Bu modül "kare ve sesi diske hafifçe nasıl yazarız?" sorununu çözüyor:
//! Media Foundation `IMFSinkWriter` ile kısa bir MP4 segmenti. Kareler GPU
//! dokusu olarak verilir; D3D yöneticisi ve donanım dönüşümleri açık olduğu
//! için BGRA→NV12 dönüşümü ve H.264 kodlama ekran kartında yapılır (NVENC,
//! AMF, Quick Sync). Ses 48 kHz 16 bit stereo PCM verilir, AAC'ye kodlanır.

use windows::{
    core::*,
    Win32::{
        Graphics::Direct3D11::ID3D11Texture2D,
        Media::MediaFoundation::*,
    },
};

pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CH: u32 = 2;

/// Kodlama ayarları; bir kayıt boyunca segmentler aynı ayarla açılır, böylece
/// kaydederken yeniden kodlamadan birleştirilebilirler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub audio: bool,
}

pub unsafe fn attrs(n: u32) -> Result<IMFAttributes> {
    let mut a = None;
    MFCreateAttributes(&mut a, n)?;
    a.ok_or_else(|| Error::from(windows::Win32::Foundation::E_POINTER))
}

fn pair(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

unsafe fn video_type(f: &Format, sub: &GUID) -> Result<IMFMediaType> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    t.SetGUID(&MF_MT_SUBTYPE, sub)?;
    t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    t.SetUINT64(&MF_MT_FRAME_SIZE, pair(f.w, f.h))?;
    t.SetUINT64(&MF_MT_FRAME_RATE, pair(f.fps, 1))?;
    t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
    Ok(t)
}

unsafe fn audio_type(sub: &GUID) -> Result<IMFMediaType> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
    t.SetGUID(&MF_MT_SUBTYPE, sub)?;
    t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CH)?;
    t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE)?;
    t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
    Ok(t)
}

pub struct Writer {
    sink: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    pub frames: u32,
}

// Sink writer serbest iş parçacıklı (MTA) bir COM nesnesidir.
unsafe impl Send for Writer {}

impl Writer {
    /// Yeni bir segment dosyası açar ve yazmaya hazırlar (kodlayıcı burada başlar).
    pub unsafe fn open(path: &std::path::Path, f: &Format, dm: &IMFDXGIDeviceManager) -> Result<Self> {
        let a = attrs(4)?;
        a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        a.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, dm)?;
        // B-kare yok: örnekler sırayla gelir, birleştirme basit kalır ve
        // kodlayıcı kareleri bekletmez (oyunda takılma olmaz).
        a.SetUINT32(&MF_LOW_LATENCY, 1)?;
        let sink = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &a)?;

        let out = video_type(f, &MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, f.bitrate)?;
        out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
        let video = sink.AddStream(&out)?;
        let inp = video_type(f, &MFVideoFormat_ARGB32)?;
        // Pozitif adım: üstten alta (DXGI dokuları böyledir).
        inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, f.w * 4)?;
        inp.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        sink.SetInputMediaType(video, &inp, None)?;

        let audio = if f.audio {
            let out = audio_type(&MFAudioFormat_AAC)?;
            // 24000 bayt/sn = 192 kbps (AAC kodlayıcının kabul ettiği değerlerden).
            out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
            let idx = sink.AddStream(&out)?;
            let inp = audio_type(&MFAudioFormat_PCM)?;
            inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, AUDIO_CH * 2)?;
            inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * AUDIO_CH * 2)?;
            sink.SetInputMediaType(idx, &inp, None)?;
            Some(idx)
        } else {
            None
        };
        sink.BeginWriting()?;
        Ok(Self { sink, video, audio, frames: 0 })
    }

    /// `tex`: BGRA doku; `t`, `dur`: 100 ns birimi, segment başına göre.
    pub unsafe fn video(&mut self, tex: &ID3D11Texture2D, t: i64, dur: i64) -> Result<()> {
        let buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, tex, 0, false)?;
        // Uzunluk ayarlanmazsa bazı kodlayıcılar boş kare sayar.
        if let Ok(b2) = buf.cast::<IMF2DBuffer>() {
            if let Ok(n) = b2.GetContiguousLength() {
                let _ = buf.SetCurrentLength(n);
            }
        }
        let s = MFCreateSample()?;
        s.AddBuffer(&buf)?;
        s.SetSampleTime(t)?;
        s.SetSampleDuration(dur)?;
        self.sink.WriteSample(self.video, &s)?;
        self.frames += 1;
        Ok(())
    }

    /// `pcm`: 48 kHz stereo i16 örnekler (iç içe).
    pub unsafe fn audio(&mut self, pcm: &[i16], t: i64) -> Result<()> {
        let Some(idx) = self.audio else { return Ok(()) };
        if pcm.is_empty() {
            return Ok(());
        }
        let bytes = (pcm.len() * 2) as u32;
        let buf = MFCreateMemoryBuffer(bytes)?;
        let mut p = std::ptr::null_mut();
        buf.Lock(&mut p, None, None)?;
        std::ptr::copy_nonoverlapping(pcm.as_ptr() as *const u8, p, bytes as usize);
        buf.Unlock()?;
        buf.SetCurrentLength(bytes)?;
        let s = MFCreateSample()?;
        s.AddBuffer(&buf)?;
        s.SetSampleTime(t)?;
        s.SetSampleDuration(frames_to_100ns(pcm.len() as u64 / AUDIO_CH as u64))?;
        self.sink.WriteSample(idx, &s)
    }

    /// Dosyayı kapatır (MP4 dizini yazılır). Kodlayıcıda bekleyen kareler
    /// boşaltıldığı için birkaç yüz ms sürebilir.
    pub unsafe fn finish(self) -> Result<()> {
        self.sink.Finalize()
    }
}

pub fn frames_to_100ns(frames: u64) -> i64 {
    (frames * 10_000_000 / AUDIO_RATE as u64) as i64
}
