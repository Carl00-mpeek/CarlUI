//! Küçük, belirlenimci rastgele sayı üreteci (xorshift32). Testlerde aynı
//! tohum hep aynı davranışı verir; bellek ayırmaz.

#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // Sıfır tohum xorshift'i sonsuza dek sıfırda tutar.
        Self(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    /// Saatten tohum (çalışırken; testler `new` kullanır).
    pub fn from_time() -> Self {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(1);
        Self::new(n)
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// [0, 1) aralığında.
    pub fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// [a, b) aralığında.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// [a, b] aralığında tam sayı (şimdilik yalnızca testlerde).
    #[cfg(test)]
    pub fn int(&mut self, a: u32, b: u32) -> u32 {
        if b <= a {
            return a;
        }
        a + self.next_u32() % (b - a + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn belirlenimci_ve_aralikta() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..10_000 {
            let x = a.f();
            assert_eq!(x, b.f());
            assert!((0.0..1.0).contains(&x));
            let i = a.int(3, 7);
            b.int(3, 7);
            assert!((3..=7).contains(&i));
        }
        assert_ne!(Rng::new(0).next_u32(), 0);
    }
}
