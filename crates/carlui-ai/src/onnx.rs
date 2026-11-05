//! Bu modül "modeli ONNX dosyası olarak indirmeden nasıl çalıştırırız?"
//! sorununu çözüyor: resmi ağırlıklardan (.pth / .safetensors) ağın grafiğini
//! burada kurarız ve ONNX Runtime'a bellekten veririz. ONNX bir protobuf
//! dosyası; gereken birkaç mesajı (ModelProto, GraphProto, NodeProto,
//! TensorProto, ValueInfoProto, AttributeProto) elle kodluyoruz.

/// Tensör eleman türleri (onnx.TensorProto.DataType).
pub const F32: i32 = 1;
pub const I32: i32 = 6;
pub const I64: i32 = 7;
pub const BOOL: i32 = 9;
pub const F16: i32 = 10;

fn varint(b: &mut Vec<u8>, mut v: u64) {
    loop {
        let c = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            b.push(c);
            return;
        }
        b.push(c | 0x80);
    }
}
fn key(b: &mut Vec<u8>, field: u32, wire: u32) {
    varint(b, ((field << 3) | wire) as u64);
}
fn int(b: &mut Vec<u8>, field: u32, v: i64) {
    key(b, field, 0);
    varint(b, v as u64);
}
fn bytes(b: &mut Vec<u8>, field: u32, v: &[u8]) {
    key(b, field, 2);
    varint(b, v.len() as u64);
    b.extend_from_slice(v);
}

/// Boyut: sabit ya da adlı (dinamik).
#[derive(Clone)]
pub enum Dim {
    N(i64),
    P(&'static str),
}

/// Düğüm özniteliği.
pub enum A<'a> {
    I(i64),
    F(f32),
    S(&'a str),
    Is(Vec<i64>),
    T(i32, Vec<i64>, Vec<u8>),
}

fn tensor(name: &str, ty: i32, dims: &[i64], raw: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(raw.len() + 64);
    for &d in dims {
        int(&mut t, 1, d);
    }
    int(&mut t, 2, ty as i64);
    bytes(&mut t, 8, name.as_bytes());
    bytes(&mut t, 9, raw);
    t
}

fn value_info(name: &str, ty: i32, dims: &[Dim]) -> Vec<u8> {
    let mut shape = Vec::new();
    for d in dims {
        let mut dim = Vec::new();
        match d {
            Dim::N(n) => int(&mut dim, 1, *n),
            Dim::P(p) => bytes(&mut dim, 2, p.as_bytes()),
        }
        bytes(&mut shape, 1, &dim);
    }
    let mut tt = Vec::new();
    int(&mut tt, 1, ty as i64);
    bytes(&mut tt, 2, &shape);
    let mut tp = Vec::new();
    bytes(&mut tp, 1, &tt);
    let mut v = Vec::new();
    bytes(&mut v, 1, name.as_bytes());
    bytes(&mut v, 2, &tp);
    v
}

pub fn f32_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
pub fn i64_bytes(v: &[i64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Grafik kurucu. Her işlem yeni, benzersiz adlı bir çıktı döndürür.
#[derive(Default)]
pub struct G {
    nodes: Vec<u8>,
    inits: Vec<u8>,
    io: Vec<u8>,
    outs: Vec<u8>,
    n: usize,
}

impl G {
    pub fn new() -> Self {
        Self::default()
    }
    fn fresh(&mut self, p: &str) -> String {
        self.n += 1;
        format!("{p}{}", self.n)
    }
    pub fn input(&mut self, name: &str, ty: i32, dims: &[Dim]) -> String {
        bytes(&mut self.io, 11, &value_info(name, ty, dims));
        name.to_string()
    }
    pub fn output(&mut self, from: &str, name: &str, ty: i32, dims: &[Dim]) {
        let mut node = Vec::new();
        bytes(&mut node, 1, from.as_bytes());
        bytes(&mut node, 2, name.as_bytes());
        bytes(&mut node, 4, b"Identity");
        bytes(&mut self.nodes, 1, &node);
        bytes(&mut self.outs, 12, &value_info(name, ty, dims));
    }
    /// Ham baytlarla sabit tensör.
    pub fn raw(&mut self, ty: i32, dims: &[i64], raw: &[u8]) -> String {
        let name = self.fresh("w");
        bytes(&mut self.inits, 5, &tensor(&name, ty, dims, raw));
        name
    }
    pub fn f32(&mut self, dims: &[usize], data: &[f32]) -> String {
        let d: Vec<i64> = dims.iter().map(|&x| x as i64).collect();
        self.raw(F32, &d, &f32_bytes(data))
    }
    pub fn scalar(&mut self, v: f32) -> String {
        self.raw(F32, &[], &v.to_le_bytes())
    }
    pub fn i64s(&mut self, v: &[i64]) -> String {
        self.raw(I64, &[v.len() as i64], &i64_bytes(v))
    }
    pub fn op(&mut self, op: &str, ins: &[&str], attrs: &[(&str, A)]) -> String {
        self.op_n(op, ins, attrs, 1).pop().unwrap()
    }
    pub fn op_n(&mut self, op: &str, ins: &[&str], attrs: &[(&str, A)], nout: usize) -> Vec<String> {
        let outs: Vec<String> = (0..nout).map(|_| self.fresh("t")).collect();
        let mut node = Vec::new();
        for i in ins {
            bytes(&mut node, 1, i.as_bytes());
        }
        for o in &outs {
            bytes(&mut node, 2, o.as_bytes());
        }
        bytes(&mut node, 4, op.as_bytes());
        for (name, a) in attrs {
            let mut at = Vec::new();
            bytes(&mut at, 1, name.as_bytes());
            match a {
                A::F(f) => {
                    key(&mut at, 2, 5);
                    at.extend_from_slice(&f.to_le_bytes());
                    int(&mut at, 20, 1);
                }
                A::I(i) => {
                    int(&mut at, 3, *i);
                    int(&mut at, 20, 2);
                }
                A::S(s) => {
                    bytes(&mut at, 4, s.as_bytes());
                    int(&mut at, 20, 3);
                }
                A::T(ty, dims, raw) => {
                    bytes(&mut at, 5, &tensor("", *ty, dims, raw));
                    int(&mut at, 20, 4);
                }
                A::Is(v) => {
                    for &x in v {
                        int(&mut at, 8, x);
                    }
                    int(&mut at, 20, 7);
                }
            }
            bytes(&mut node, 5, &at);
        }
        bytes(&mut self.nodes, 1, &node);
        outs
    }

    // Sık kullanılan kısayollar.
    pub fn add(&mut self, a: &str, b: &str) -> String {
        self.op("Add", &[a, b], &[])
    }
    pub fn mul(&mut self, a: &str, b: &str) -> String {
        self.op("Mul", &[a, b], &[])
    }
    pub fn matmul(&mut self, a: &str, b: &str) -> String {
        self.op("MatMul", &[a, b], &[])
    }
    pub fn scale(&mut self, a: &str, s: f32) -> String {
        let c = self.scalar(s);
        self.mul(a, &c)
    }
    pub fn concat(&mut self, xs: &[&str], axis: i64) -> String {
        self.op("Concat", xs, &[("axis", A::I(axis))])
    }
    pub fn reshape(&mut self, x: &str, shape: &[i64]) -> String {
        let s = self.i64s(shape);
        self.op("Reshape", &[x, &s], &[])
    }
    pub fn transpose(&mut self, x: &str, perm: &[i64]) -> String {
        self.op("Transpose", &[x], &[("perm", A::Is(perm.to_vec()))])
    }

    /// Bitmiş modeli protobuf baytlarına çevirir (opset 17).
    pub fn finish(self) -> Vec<u8> {
        let mut g = Vec::with_capacity(self.nodes.len() + self.inits.len() + 1024);
        g.extend_from_slice(&self.nodes);
        bytes(&mut g, 2, b"carlui");
        g.extend_from_slice(&self.inits);
        g.extend_from_slice(&self.io);
        g.extend_from_slice(&self.outs);
        let mut m = Vec::with_capacity(g.len() + 64);
        int(&mut m, 1, 8);
        bytes(&mut m, 2, b"CarlUI");
        let mut ops = Vec::new();
        bytes(&mut ops, 1, b"");
        int(&mut ops, 2, 17);
        bytes(&mut m, 8, &ops);
        bytes(&mut m, 7, &g);
        m
    }
}
