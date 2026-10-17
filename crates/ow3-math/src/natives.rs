//! The math natives of SPEC-0011: `Sin`, `Cos`, `Tan`, `Asin`, `Acos`,
//! `Atan`, `Atan2`, `SquareRoot`, `Pow`, `Deg2Rad` and `Rad2Deg`, on bit
//! patterns, with the operations of SPEC-0004 and integer arithmetic only.

use std::sync::OnceLock;

use crate::{ONE, SIGN, add, div, exponent, fraction, i2r, mul, recip, sub};

const K_ANG: u32 = 0x4822_F983;
const PI: u32 = 0x4049_0FDB;
const PI_2: u32 = 0x3FC9_0FDB;
const M_PI_2: u32 = 0xBFC9_0FDB;
const PI_6: u32 = 0x3F06_0A92;
const T12: u32 = 0x3E89_30A3;
const T6: u32 = 0x3F13_CD3A;
const M_T6: u32 = 0xBF13_CD3A;
const P0: u32 = 0x3F7F_FFF0;
const P1: u32 = 0x3E84_15A6;
const Q1: u32 = 0x3F17_592E;
const L1: u32 = 0xBE88_D424;
const L2: u32 = 0xBF19_BF59;
const LN2: u32 = 0x3F31_7218;
const ILN2: u32 = 0x3FB8_AA3B;
const E4: u32 = 0x3FA4_5AF2;
const C1: u32 = 0x3C1A_534C;
const C2: u32 = 0x3D29_6EC9;
const C3: u32 = 0x3E2A_B479;
const C4: u32 = 0x3EFF_FFBD;
const EPS: u32 = 0x3A83_126F;
const DEG: u32 = 0x3C8E_FA35;
const RAD: u32 = 0x4265_2EE1;
const MINUS_ONE: u32 = 0xBF80_0000;
/// The largest `|x|` that `Asin` and `Acos` take from the table `A`.
const ASIN_TABLE_LIMIT: u32 = 0x3F7E_8000;
/// `pi * 2^29` and `pi / 2 * 2^29`.
const PI_29: u32 = 0x6487_ED51;
const PI_2_29: u32 = 0x3243_F6A8;

struct Tables {
    s: Vec<u32>,
    a: Vec<u32>,
    n: Vec<u32>,
}

/// The tables `S`, `A` and `N` of SPEC-0011, from `SPEC-0011-tables.tsv`.
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let source = include_str!("../../../specs/SPEC-0011-tables.tsv");
        let mut tables = Tables {
            s: vec![0; 1025],
            a: vec![0; 1024],
            n: vec![0; 138],
        };
        for line in source
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, index, value] = fields[..] else {
                panic!("bad table line {line}");
            };
            let (index, value): (usize, u32) = (index.parse().unwrap(), value.parse().unwrap());
            let table = match name {
                "S" => &mut tables.s,
                "A" => &mut tables.a,
                "N" => &mut tables.n,
                _ => panic!("unknown table {name}"),
            };
            table[index] = value;
        }
        tables
    })
}

/// Truncation toward zero without saturation.
fn f2i(x: u32) -> i32 {
    let e = exponent(x);
    if e < 127 {
        return 0;
    }
    let m = fraction(x) + (1 << 23);
    let magnitude = if e <= 150 {
        m >> (150 - e)
    } else {
        m.wrapping_shl(((e - 150) % 32) as u32)
    };
    if x & SIGN != 0 {
        magnitude.wrapping_neg() as i32
    } else {
        magnitude as i32
    }
}

/// `x` with the fraction bits below its integer part cleared.
fn trunc(x: u32) -> u32 {
    let e = exponent(x);
    if e < 127 {
        0
    } else if e >= 150 {
        x
    } else {
        x & !((1u32 << (150 - e)) - 1)
    }
}

/// `i2r(v) / 2^k`, exactly.
fn scale(v: i32, k: u32) -> u32 {
    let r = i2r(v);
    if r == 0 { 0 } else { r.wrapping_sub(k << 23) }
}

fn hi(a: u32, w: u32) -> u32 {
    ((u64::from(a) * u64::from(w)) >> 32) as u32
}

/// Negative and not -0.0.
fn neg0(x: u32) -> bool {
    x & SIGN != 0 && x & !SIGN != 0
}

fn real(x: u32) -> f32 {
    f32::from_bits(x)
}

/// `|x|` below 0.001: `|sub(x, 0)| < EPS`.
fn tiny(x: u32) -> bool {
    real(sub(x, 0) & !SIGN) < real(EPS)
}

fn sinq(x: u32, s: i32) -> u32 {
    let table = &tables().s;
    let n = f2i(mul(x, K_ANG));
    let j = (n & 255) as u32;
    let k = ((n >> 8) & 1023) as usize;
    let q = ((n >> 18).wrapping_add(s)) & 3;
    let w = j.wrapping_mul(0x0101_0101);
    let r = if q % 2 == 0 {
        table[k].wrapping_add(hi(table[k + 1].wrapping_sub(table[k]), w))
    } else {
        table[1024 - k].wrapping_sub(hi(table[1024 - k].wrapping_sub(table[1023 - k]), w))
    } as i32;
    let r = if q >= 2 { r.wrapping_neg() } else { r };
    scale(r, 31)
}

pub fn sin(x: u32) -> u32 {
    sinq(x, 0)
}

pub fn cos(x: u32) -> u32 {
    sinq(x, 1)
}

pub fn tan(x: u32) -> u32 {
    div(sin(x), cos(x))
}

pub fn atan(x: u32) -> u32 {
    let mut a = x & !SIGN;
    let inverted = real(a) > 1.0;
    if inverted {
        a = recip(a);
    }
    let shifted = real(a) > real(T12);
    if shifted {
        a = div(add(a, M_T6), add(ONE, mul(T6, a)));
    }
    let s = mul(a, a);
    let mut r = div(mul(a, add(P0, mul(P1, s))), add(ONE, mul(Q1, s)));
    if shifted {
        r = add(r, PI_6);
    }
    if inverted {
        r = sub(PI_2, r);
    }
    if neg0(x) { r ^ SIGN } else { r }
}

fn atan2_core(y: u32, x: u32) -> u32 {
    let mut r = if exponent(x) != 0 {
        atan(div(y, x) & !SIGN)
    } else {
        PI_2
    };
    if neg0(x) {
        r = sub(PI, r);
    }
    if neg0(y) { r ^ SIGN } else { r }
}

/// `Atan2(y, x)`: 0 when both are below 0.001 in magnitude.
pub fn atan2(y: u32, x: u32) -> u32 {
    if tiny(x) && tiny(y) {
        0
    } else {
        atan2_core(y, x)
    }
}

/// `asin` (`acos` false) or `acos` of `x` in [-1, 1].
fn arc(x: u32, acos: bool) -> u32 {
    let magnitude = x & !SIGN;
    let negative = x & SIGN != 0;
    if magnitude <= ASIN_TABLE_LIMIT {
        let table = &tables().a;
        let n = if magnitude == 0 {
            0
        } else {
            f2i(x.wrapping_add(30 << 23))
        };
        let n = n.clamp(-((1 << 30) - 1), (1 << 30) - 1);
        let c = if acos { n } else { -n };
        let u = (c as u32).wrapping_shl(12);
        let w = u | (u >> 20);
        let k = ((c >> 20) & 1023) as usize;
        let mut r = if c >= 0 {
            table[k].wrapping_sub(hi(table[k].wrapping_sub(table[k + 1]), w))
        } else {
            PI_29
                .wrapping_sub(table[1024 - k])
                .wrapping_sub(hi(table[1023 - k].wrapping_sub(table[1024 - k]), w))
        };
        if !acos {
            r = r.wrapping_sub(PI_2_29);
        }
        return scale(r as i32, 29);
    }
    let table = &tables().n;
    let d = if real(magnitude) <= 1.0 {
        ONE - magnitude
    } else {
        0
    };
    let z = d.leading_zeros();
    let b = if z == 32 { 0 } else { (!d) << z };
    let i = (8 * z + (b >> 28)) as usize - 120;
    let w = b.wrapping_shl(4);
    let r = table[i].wrapping_sub(hi(table[i].wrapping_sub(table[i + 1]), w));
    let t = scale(r as i32, 34);
    match (acos, negative) {
        (false, true) => add(M_PI_2, t),
        (false, false) => sub(PI_2, t),
        (true, true) => sub(PI, t),
        (true, false) => t,
    }
}

fn outside_unit(x: u32) -> bool {
    real(x) < -1.0 || real(x) > 1.0
}

/// `Asin(x)`: 0 outside [-1, 1].
pub fn asin(x: u32) -> u32 {
    if outside_unit(x) { 0 } else { arc(x, false) }
}

/// `Acos(x)`: 0 outside [-1, 1].
pub fn acos(x: u32) -> u32 {
    if outside_unit(x) { 0 } else { arc(x, true) }
}

fn sqrt_core(x: u32) -> u32 {
    if x & SIGN != 0 || x == 0 {
        return 0;
    }
    let m = fraction(x);
    let e = exponent(x) - 127;
    let v = (1u64 << 31) + u64::from(m) * 256 + u64::from(m >> 15);
    let s = v.isqrt() as u32;
    let t = ONE | (s.wrapping_sub(46340).wrapping_mul(46341) >> 8);
    let h = e.div_euclid(2);
    let odd = if e.rem_euclid(2) == 1 { 0x0035_04F3 } else { 0 };
    let scale = ((h + 127) as u32) << 23 | odd;
    mul(t, scale)
}

/// `SquareRoot(x)`: 0 below 0.001 in magnitude and for negative `x`.
pub fn square_root(x: u32) -> u32 {
    if tiny(x) || real(x) < 0.0 {
        0
    } else {
        sqrt_core(x)
    }
}

fn ln(x: u32) -> u32 {
    let m = fraction(x) | ONE;
    let z = div(add(m, MINUS_ONE), add(m, ONE));
    let z2 = mul(z, z);
    let mut v = mul(z, add(ONE, mul(L1, z2)));
    if exponent(v) != 0 {
        v = v.wrapping_add(1 << 23);
    }
    let l = div(v, add(ONE, mul(L2, z2)));
    mul(LN2, add(i2r(exponent(x) - 127), mul(l, ILN2)))
}

/// `b^n` by repeated squaring. A negative `n` (an exponent of 2^31 or
/// more, wrapped) hangs the game (SPEC-0011); here it gives 0.
fn powi(mut b: u32, mut n: i32) -> u32 {
    if n < 0 {
        return 0;
    }
    let mut r = ONE;
    while n != 0 {
        if n & 1 != 0 {
            r = mul(r, b);
        }
        b = mul(b, b);
        n >>= 1;
    }
    r
}

fn expp(mut t: u32) -> u32 {
    if exponent(t) != 0 {
        t = t.wrapping_add(2 << 23);
    }
    let i = trunc(t);
    let f = sub(t, i);
    let g = if exponent(f) >= 3 {
        f.wrapping_sub(2 << 23)
    } else {
        0
    };
    let mut p = mul(C1, g);
    for c in [C2, C3, C4, ONE] {
        p = mul(add(p, c), g);
    }
    p = add(p, ONE);
    mul(p, powi(E4, f2i(i)))
}

fn exp(t: u32) -> u32 {
    if neg0(t) {
        recip(expp(t ^ SIGN))
    } else {
        expp(t)
    }
}

fn pow_core(x: u32, y: u32) -> u32 {
    if real(trunc(y)) == real(y) && !neg0(y) {
        powi(x, f2i(y))
    } else if exponent(x) != 0 {
        exp(mul(y, ln(x)))
    } else {
        0
    }
}

/// `Pow(x, y)`, with arguments below 0.001 in magnitude taken as 0.
pub fn pow(x: u32, y: u32) -> u32 {
    if tiny(x) {
        if real(y) < 0.0 { 0 } else { pow_core(x, y) }
    } else if tiny(y) {
        ONE
    } else {
        pow_core(x, y)
    }
}

pub fn deg2rad(x: u32) -> u32 {
    mul(x, DEG)
}

pub fn rad2deg(x: u32) -> u32 {
    mul(x, RAD)
}
