//! Bu modül "dönüştürücü (transformer) ağlarının ortak parçalarını her model
//! için yeniden mi yazacağız?" sorununu çözüyor: ağırlık tablosundan ONNX
//! düğümleri kuran küçük yardımcılar (doğrusal katman, LayerNorm, GELU, dikkat
//! başlarına bölme). Whisper ve Marian (Opus-MT) bunları paylaşır.

use crate::{
    onnx::{A, F16, F32, G},
    weights::{self, Tensor, Weights},
};

pub(crate) struct B<'a> {
    pub g: G,
    pub ws: &'a Weights,
    pub half: bool,
}

impl B<'_> {
    pub fn t(&self, k: &str) -> Result<&Tensor, String> {
        self.ws.get(k).ok_or_else(|| format!("ağırlık yok: {k}"))
    }
    /// Sabit; büyük modellerde F16 saklanıp F32'ye çevrilir (2 GB sınırı).
    pub fn konst(&mut self, shape: &[usize], data: &[f32]) -> String {
        if self.half && data.len() > 4096 {
            let raw: Vec<u8> = data.iter().flat_map(|&f| weights::f32_to_f16(f).to_le_bytes()).collect();
            let d: Vec<i64> = shape.iter().map(|&x| x as i64).collect();
            let h = self.g.raw(F16, &d, &raw);
            self.g.op("Cast", &[&h], &[("to", A::I(F32 as i64))])
        } else {
            self.g.f32(shape, data)
        }
    }
    pub fn param(&mut self, k: &str) -> Result<String, String> {
        let t = self.t(k)?;
        let (s, d) = (t.shape.clone(), t.data.clone());
        Ok(self.konst(&s, &d))
    }
    /// nn.Linear: ağırlık [out,in] → MatMul için [in,out].
    pub fn lin(&mut self, x: &str, name: &str) -> Result<String, String> {
        let w = self.t(&format!("{name}.weight"))?;
        let (o, i) = (w.shape[0], w.shape[1]);
        let mut tr = vec![0f32; o * i];
        for r in 0..o {
            for c in 0..i {
                tr[c * o + r] = w.data[r * i + c];
            }
        }
        let wn = self.konst(&[i, o], &tr);
        let y = self.g.matmul(x, &wn);
        match self.ws.contains_key(&format!("{name}.bias")) {
            true => {
                let b = self.param(&format!("{name}.bias"))?;
                Ok(self.g.add(&y, &b))
            }
            false => Ok(y),
        }
    }
    pub fn ln(&mut self, x: &str, name: &str) -> Result<String, String> {
        let s = self.param(&format!("{name}.weight"))?;
        let b = self.param(&format!("{name}.bias"))?;
        Ok(self.g.op("LayerNormalization", &[x, &s, &b], &[("axis", A::I(-1)), ("epsilon", A::F(1e-5))]))
    }
    pub fn gelu(&mut self, x: &str) -> String {
        let t = self.g.scale(x, std::f32::consts::FRAC_1_SQRT_2);
        let e = self.g.op("Erf", &[&t], &[]);
        let one = self.g.scalar(1.0);
        let a = self.g.add(&e, &one);
        let m = self.g.mul(x, &a);
        self.g.scale(&m, 0.5)
    }
    pub fn heads(&mut self, x: &str, h: usize, dh: usize, perm: &[i64]) -> String {
        let r = self.g.reshape(x, &[1, -1, h as i64, dh as i64]);
        self.g.transpose(&r, perm)
    }
    pub fn merge(&mut self, x: &str, d: usize) -> String {
        let t = self.g.transpose(x, &[0, 2, 1, 3]);
        self.g.reshape(&t, &[1, -1, d as i64])
    }
    pub fn slice(&mut self, x: &str, s: i64, e: i64, axis: i64) -> String {
        let (a, b, c) = (self.g.i64s(&[s]), self.g.i64s(&[e]), self.g.i64s(&[axis]));
        self.g.op("Slice", &[x, &a, &b, &c], &[])
    }
    pub fn mlp(&mut self, x: &str, p: &str) -> Result<String, String> {
        let h = self.ln(x, &format!("{p}.final_layer_norm"))?;
        let h = self.lin(&h, &format!("{p}.fc1"))?;
        let h = self.gelu(&h);
        let h = self.lin(&h, &format!("{p}.fc2"))?;
        Ok(self.g.add(x, &h))
    }
}
