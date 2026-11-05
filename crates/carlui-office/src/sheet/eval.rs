//! Bu modül "ayrıştırılmış formülü hesaplayalım" sorununu çözüyor: işleçler,
//! başvurular ve desteklenen işlevler (TOPLA, EĞER, DÜŞEYARA…) burada.

use super::{formula::Expr, Val};

/// Değerlendirme bağlamı: başvurulan hücrelerin değerleri.
pub trait Ctx {
    fn get(&mut self, sheet: Option<&str>, r: u32, c: u32) -> Val;
    /// Aralığın dolu hücreleri (boşları atlamak hız kazandırır).
    fn range(&mut self, sheet: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Val>;
    /// Aralığın tam boyutu (ETOPLA ölçüt/toplam eşlemesi için) satır satır.
    fn grid(&mut self, sheet: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Vec<Val>>;
}

pub const UNSUPPORTED: &str = "#NAME?";

fn num(v: &Val) -> Result<f64, Val> {
    match v {
        Val::Num(n) => Ok(*n),
        Val::Bool(b) => Ok(*b as i32 as f64),
        Val::Empty => Ok(0.0),
        Val::Str(s) => s.trim().replace(',', ".").parse().map_err(|_| Val::Err("#VALUE!".into())),
        Val::Err(e) => Err(Val::Err(e.clone())),
    }
}

fn text(v: &Val) -> String {
    match v {
        Val::Num(n) => super::general(*n),
        Val::Str(s) => s.clone(),
        Val::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Val::Empty => String::new(),
        Val::Err(e) => e.clone(),
    }
}

fn truthy(v: &Val) -> Result<bool, Val> {
    match v {
        Val::Bool(b) => Ok(*b),
        Val::Str(s) if s.eq_ignore_ascii_case("true") => Ok(true),
        Val::Str(s) if s.eq_ignore_ascii_case("false") => Ok(false),
        Val::Str(_) => Err(Val::Err("#VALUE!".into())),
        other => num(other).map(|n| n != 0.0),
    }
}

/// İşlev bağımsız değişkenini düz değer listesine açar (aralıklar dahil).
fn flat(e: &Expr, ctx: &mut dyn Ctx) -> Vec<(Val, bool)> {
    match e {
        Expr::Range(s, a, b) => ctx.range(s.as_deref(), *a, *b).into_iter().map(|v| (v, true)).collect(),
        Expr::Ref(s, r, c) => vec![(ctx.get(s.as_deref(), *r, *c), true)],
        other => vec![(eval(other, ctx), false)],
    }
}

fn round_to(n: f64, d: i32, mode: i8) -> f64 {
    let m = 10f64.powi(d);
    let x = n * m;
    let r = match mode {
        0 => (x.abs() + 1e-9).round() * x.signum(),
        1 => x.abs().ceil() * x.signum(),
        _ => x.abs().floor() * x.signum(),
    };
    r / m
}

fn criteria(c: &Val) -> impl Fn(&Val) -> bool {
    let s = text(c);
    let (op, rest) = ["<=", ">=", "<>", "<", ">", "="].iter().find_map(|o| s.strip_prefix(o).map(|r| (*o, r.to_string()))).unwrap_or(("=", s.clone()));
    let target_num = rest.trim().replace(',', ".").parse::<f64>().ok();
    let rest_l = rest.to_lowercase();
    move |v: &Val| {
        let vn = match v {
            Val::Num(n) => Some(*n),
            _ => None,
        };
        match (target_num, vn) {
            (Some(t), Some(n)) => match op {
                "<=" => n <= t,
                ">=" => n >= t,
                "<>" => n != t,
                "<" => n < t,
                ">" => n > t,
                _ => n == t,
            },
            _ => {
                let vs = text(v).to_lowercase();
                match op {
                    "<>" => vs != rest_l,
                    "=" => {
                        if rest_l.contains('*') {
                            let parts: Vec<&str> = rest_l.split('*').collect();
                            vs.starts_with(parts[0]) && vs.ends_with(parts[parts.len() - 1])
                        } else {
                            vs == rest_l
                        }
                    }
                    _ => false,
                }
            }
        }
    }
}

fn call(name: &str, args: &[Expr], ctx: &mut dyn Ctx) -> Val {
    let nums = |ctx: &mut dyn Ctx| -> Result<Vec<f64>, Val> {
        let mut v = Vec::new();
        for a in args {
            for (x, from_ref) in flat(a, ctx) {
                match x {
                    Val::Num(n) => v.push(n),
                    Val::Err(e) => return Err(Val::Err(e)),
                    Val::Bool(b) if !from_ref => v.push(b as i32 as f64),
                    Val::Str(s) if !from_ref => v.push(num(&Val::Str(s))?),
                    _ => {}
                }
            }
        }
        Ok(v)
    };
    let arg = |i: usize, ctx: &mut dyn Ctx| args.get(i).map(|e| eval(e, ctx)).unwrap_or(Val::Empty);
    let r: Result<Val, Val> = (|| match name {
        "SUM" => Ok(Val::Num(nums(ctx)?.iter().sum())),
        "PRODUCT" => Ok(Val::Num(nums(ctx)?.iter().product())),
        "AVERAGE" => {
            let v = nums(ctx)?;
            if v.is_empty() {
                Err(Val::Err("#DIV/0!".into()))
            } else {
                Ok(Val::Num(v.iter().sum::<f64>() / v.len() as f64))
            }
        }
        "MEDIAN" => {
            let mut v = nums(ctx)?;
            if v.is_empty() {
                return Err(Val::Err("#NUM!".into()));
            }
            v.sort_by(|a, b| a.total_cmp(b));
            let m = v.len() / 2;
            Ok(Val::Num(if v.len() % 2 == 0 { (v[m - 1] + v[m]) / 2.0 } else { v[m] }))
        }
        "MIN" => Ok(Val::Num(nums(ctx)?.into_iter().reduce(f64::min).unwrap_or(0.0))),
        "MAX" => Ok(Val::Num(nums(ctx)?.into_iter().reduce(f64::max).unwrap_or(0.0))),
        "COUNT" => Ok(Val::Num(args.iter().flat_map(|a| flat(a, ctx)).filter(|(v, _)| matches!(v, Val::Num(_))).count() as f64)),
        "COUNTA" => Ok(Val::Num(args.iter().flat_map(|a| flat(a, ctx)).filter(|(v, _)| !matches!(v, Val::Empty)).count() as f64)),
        "COUNTBLANK" => {
            let mut n = 0;
            for a in args {
                if let Expr::Range(s, x, y) = a {
                    n += ctx.grid(s.as_deref(), *x, *y).iter().flatten().filter(|v| matches!(v, Val::Empty) || matches!(v, Val::Str(s) if s.is_empty())).count();
                }
            }
            Ok(Val::Num(n as f64))
        }
        "IF" => {
            let c = truthy(&arg(0, ctx))?;
            Ok(if c {
                if args.len() > 1 {
                    arg(1, ctx)
                } else {
                    Val::Bool(true)
                }
            } else if args.len() > 2 {
                arg(2, ctx)
            } else {
                Val::Bool(false)
            })
        }
        "IFERROR" => {
            let v = arg(0, ctx);
            Ok(if matches!(v, Val::Err(_)) { arg(1, ctx) } else { v })
        }
        "AND" | "OR" => {
            let mut acc = name == "AND";
            for a in args {
                for (v, _) in flat(a, ctx) {
                    if matches!(v, Val::Empty) {
                        continue;
                    }
                    let b = truthy(&v)?;
                    acc = if name == "AND" { acc && b } else { acc || b };
                }
            }
            Ok(Val::Bool(acc))
        }
        "NOT" => Ok(Val::Bool(!truthy(&arg(0, ctx))?)),
        "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
            let n = num(&arg(0, ctx))?;
            let d = num(&arg(1, ctx))? as i32;
            Ok(Val::Num(round_to(n, d, if name == "ROUND" { 0 } else if name == "ROUNDUP" { 1 } else { 2 })))
        }
        "ABS" => Ok(Val::Num(num(&arg(0, ctx))?.abs())),
        "INT" => Ok(Val::Num(num(&arg(0, ctx))?.floor())),
        "SQRT" => {
            let n = num(&arg(0, ctx))?;
            if n < 0.0 {
                Err(Val::Err("#NUM!".into()))
            } else {
                Ok(Val::Num(n.sqrt()))
            }
        }
        "POWER" => Ok(Val::Num(num(&arg(0, ctx))?.powf(num(&arg(1, ctx))?))),
        "MOD" => {
            let (a, b) = (num(&arg(0, ctx))?, num(&arg(1, ctx))?);
            if b == 0.0 {
                Err(Val::Err("#DIV/0!".into()))
            } else {
                Ok(Val::Num(a - b * (a / b).floor()))
            }
        }
        "CONCAT" | "CONCATENATE" => Ok(Val::Str(args.iter().flat_map(|a| flat(a, ctx)).map(|(v, _)| text(&v)).collect())),
        "LEN" => Ok(Val::Num(text(&arg(0, ctx)).chars().count() as f64)),
        "UPPER" => Ok(Val::Str(text(&arg(0, ctx)).replace('i', "İ").to_uppercase())),
        "LOWER" => Ok(Val::Str(text(&arg(0, ctx)).replace('I', "ı").to_lowercase())),
        "TRIM" => Ok(Val::Str(text(&arg(0, ctx)).split_whitespace().collect::<Vec<_>>().join(" "))),
        "LEFT" | "RIGHT" => {
            let s = text(&arg(0, ctx));
            let n = if args.len() > 1 { num(&arg(1, ctx))?.max(0.0) as usize } else { 1 };
            let v: Vec<char> = s.chars().collect();
            let n = n.min(v.len());
            Ok(Val::Str(if name == "LEFT" { v[..n].iter().collect() } else { v[v.len() - n..].iter().collect() }))
        }
        "MID" => {
            let v: Vec<char> = text(&arg(0, ctx)).chars().collect();
            let st = (num(&arg(1, ctx))?.max(1.0) as usize - 1).min(v.len());
            let n = (num(&arg(2, ctx))?.max(0.0) as usize).min(v.len() - st);
            Ok(Val::Str(v[st..st + n].iter().collect()))
        }
        "VALUE" => Ok(Val::Num(num(&Val::Str(text(&arg(0, ctx))))?)),
        "SUMIF" | "COUNTIF" => {
            let (Some(Expr::Range(s, a, b)), Some(c)) = (args.first(), args.get(1)) else { return Err(Val::Err("#VALUE!".into())) };
            let crit = criteria(&eval(c, ctx));
            let test = ctx.grid(s.as_deref(), *a, *b);
            let sums = match args.get(2) {
                Some(Expr::Range(s2, a2, _)) => {
                    let h = (b.0 - a.0, b.1 - a.1);
                    ctx.grid(s2.as_deref(), *a2, (a2.0 + h.0, a2.1 + h.1))
                }
                _ => test.clone(),
            };
            let mut total = 0.0;
            let mut count = 0;
            for (ri, row) in test.iter().enumerate() {
                for (ci, v) in row.iter().enumerate() {
                    if crit(v) {
                        count += 1;
                        if let Some(Val::Num(n)) = sums.get(ri).and_then(|r| r.get(ci)) {
                            total += n;
                        }
                    }
                }
            }
            Ok(Val::Num(if name == "SUMIF" { total } else { count as f64 }))
        }
        _ => Err(Val::Err(UNSUPPORTED.into())),
    })();
    r.unwrap_or_else(|e| e)
}

fn cmp(a: &Val, b: &Val) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x.partial_cmp(y).unwrap_or(Equal),
        (Val::Empty, Val::Num(y)) => 0.0f64.partial_cmp(y).unwrap_or(Equal),
        (Val::Num(x), Val::Empty) => x.partial_cmp(&0.0).unwrap_or(Equal),
        (Val::Num(_), _) => Less,
        (_, Val::Num(_)) => Greater,
        _ => text(a).to_lowercase().cmp(&text(b).to_lowercase()),
    }
}

pub fn eval(e: &Expr, ctx: &mut dyn Ctx) -> Val {
    match e {
        Expr::Num(n) => Val::Num(*n),
        Expr::Str(s) => Val::Str(s.clone()),
        Expr::Bool(b) => Val::Bool(*b),
        Expr::Err(s) => Val::Err(s.clone()),
        Expr::Ref(s, r, c) => match ctx.get(s.as_deref(), *r, *c) {
            Val::Empty => Val::Num(0.0),
            v => v,
        },
        Expr::Range(s, a, _) => ctx.get(s.as_deref(), a.0, a.1),
        Expr::Pct(x) => match num(&eval(x, ctx)) {
            Ok(n) => Val::Num(n / 100.0),
            Err(e) => e,
        },
        Expr::Un(op, x) => match num(&eval(x, ctx)) {
            Ok(n) => Val::Num(if op == "-" { -n } else { n }),
            Err(e) => e,
        },
        Expr::Call(name, args) => call(name, args, ctx),
        Expr::Bin(op, a, b) => {
            let (va, vb) = (eval(a, ctx), eval(b, ctx));
            if let Val::Err(e) = &va {
                return Val::Err(e.clone());
            }
            if let Val::Err(e) = &vb {
                return Val::Err(e.clone());
            }
            match op.as_str() {
                "&" => Val::Str(text(&va) + &text(&vb)),
                "=" | "<>" | "<" | ">" | "<=" | ">=" => {
                    let o = cmp(&va, &vb);
                    use std::cmp::Ordering::*;
                    Val::Bool(match op.as_str() {
                        "=" => o == Equal,
                        "<>" => o != Equal,
                        "<" => o == Less,
                        ">" => o == Greater,
                        "<=" => o != Greater,
                        _ => o != Less,
                    })
                }
                _ => match (num(&va), num(&vb)) {
                    (Ok(x), Ok(y)) => match op.as_str() {
                        "+" => Val::Num(x + y),
                        "-" => Val::Num(x - y),
                        "*" => Val::Num(x * y),
                        "/" if y == 0.0 => Val::Err("#DIV/0!".into()),
                        "/" => Val::Num(x / y),
                        _ => Val::Num(x.powf(y)),
                    },
                    (Err(e), _) | (_, Err(e)) => e,
                },
            }
        }
    }
}
