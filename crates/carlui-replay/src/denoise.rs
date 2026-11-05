//! Bu modül "kayıt sırasında mikrofona giren klavye, fan, uğultu sesi nasıl
//! temizlenir?" sorununu çözüyor: mikrofon örnekleri (48 kHz stereo) RNNoise
//! sinir ağından (saf Rust `nnnoiseless`, ek DLL yok) 10 ms'lik karelerle
//! geçirilir. RNNoise sürekli gürültüyü siler; konuşma yokken gelen klavye
//! tıkırtısı gibi kısa sesler için ayrıca konuşma olasılığına bağlı yumuşak
//! bir kapı vardır: konuşma yoksa ses kısılır, konuşma başlayınca hemen açılır.
//! İşlem yaklaşık 10 ms gecikme ekler; zaman damgası buna göre düzeltilir.

use crate::writer::frames_to_100ns;
use nnnoiseless::DenoiseState;
use std::sync::atomic::AtomicBool;

/// Açık mı? Ayardan ve panelden yazılır, ses iş parçacıkları okur.
pub static ON: AtomicBool = AtomicBool::new(false);

const N: usize = DenoiseState::FRAME_SIZE;
/// Konuşma yokken kalan düzey (tamamen susmasın, doğal dursun).
const FLOOR: f32 = 0.08;
/// Kapıyı açan konuşma olasılığı; düşük tutulursa klavye sesi de "konuşma"
/// sayılır (RNNoise hışırtıya 0,4–0,7 verir, konuşmaya 0,9 üstü).
const VAD_OPEN: f32 = 0.85;

pub struct Denoiser {
    st: Box<DenoiseState<'static>>,
    /// Bekleyen tek kanallı örnekler (en çok bir kare).
    inp: Vec<f32>,
    /// Bekleyen ilk örneğin zamanı.
    t: i64,
    out: [f32; N],
    pcm: Vec<i16>,
    gate: f32,
    /// Konuşma bittikten sonra kapının açık kalacağı kare sayısı.
    hold: u32,
    warm: bool,
}

impl Default for Denoiser {
    fn default() -> Self {
        Self { st: DenoiseState::new(), inp: Vec::with_capacity(N), t: 0, out: [0.0; N], pcm: Vec::new(), gate: 1.0, hold: 0, warm: false }
    }
}

impl Denoiser {
    /// Stereo `pcm`'i (zaman `t`) temizler; hazır olan kareleri `emit`'e verir.
    pub fn process(&mut self, pcm: &[i16], t: i64, mut emit: impl FnMut(&[i16], i64)) {
        if self.inp.is_empty() {
            self.t = t;
        }
        self.pcm.clear();
        let mut first_t = None;
        for f in pcm.chunks_exact(2) {
            self.inp.push((f[0] as f32 + f[1] as f32) * 0.5);
            if self.inp.len() < N {
                continue;
            }
            let vad = self.st.process_frame(&mut self.out, &self.inp);
            self.inp.clear();
            // İlk kare ağın ısınmasıdır, sessiz geçer.
            if !self.warm {
                self.warm = true;
                self.out = [0.0; N];
            }
            // Kapı: konuşma varsa hızlı aç, bitince 200 ms bekle, yavaş kapan.
            if vad > VAD_OPEN {
                self.hold = 20;
            } else if self.hold > 0 {
                self.hold -= 1;
            }
            let target = if self.hold > 0 { 1.0 } else { FLOOR };
            let k = if target > self.gate { 0.6 } else { 0.12 };
            let g0 = self.gate;
            self.gate += (target - self.gate) * k;
            for (i, v) in self.out.iter().enumerate() {
                // Kare içinde düzgün geçiş: tıkırtı olmasın.
                let g = g0 + (self.gate - g0) * i as f32 / N as f32;
                let s = (v * g).clamp(-32768.0, 32767.0) as i16;
                self.pcm.push(s);
                self.pcm.push(s);
            }
            first_t.get_or_insert(self.t);
            self.t += frames_to_100ns(N as u64);
        }
        if let Some(t0) = first_t {
            emit(&self.pcm, t0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_time() {
        let mut d = Denoiser::default();
        let mut got = Vec::new();
        // 15 ms: bir kare çıkar, yarım kare bekler.
        d.process(&vec![0; 720 * 2], 1_000_000, |p, t| got.push((p.len(), t)));
        assert_eq!(got, vec![(N * 2, 1_000_000)]);
        got.clear();
        // 5 ms daha: ikinci kare, ilk karenin 10 ms sonrasıyla.
        d.process(&vec![0; 240 * 2], 1_150_000, |p, t| got.push((p.len(), t)));
        assert_eq!(got, vec![(N * 2, 1_100_000)]);
    }

    #[test]
    fn steady_noise_is_reduced() {
        let mut d = Denoiser::default();
        // Sahte rastgele "hışırtı", 1 sn.
        let mut x: u32 = 12345;
        let pcm: Vec<i16> = (0..48_000 * 2)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((x >> 16) as i16) / 64
            })
            .collect();
        let mut out = Vec::new();
        d.process(&pcm, 0, |p, _| out.extend_from_slice(p));
        let e = |s: &[i16]| s.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
        let half = out.len() / 2;
        let r = e(&out[half..]) / e(&pcm[pcm.len() - (out.len() - half)..]);
        assert!(r < 0.1, "oran {r}");
    }
}
