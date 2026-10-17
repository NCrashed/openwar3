//! Real arithmetic of the game engine, bit for bit (SPEC-0004).
//!
//! Values are 32-bit patterns with the IEEE 754 single-precision layout, but
//! the operations round differently from IEEE and have no special cases for
//! infinities or NaN. Every function takes and returns raw bit patterns.

use std::sync::OnceLock;

pub mod natives;

const SIGN: u32 = 0x8000_0000;
const FRACTION: u32 = 0x007F_FFFF;
/// 1.0
pub const ONE: u32 = 0x3F80_0000;

fn exponent(x: u32) -> i32 {
    ((x >> 23) & 0xFF) as i32
}

fn fraction(x: u32) -> u32 {
    x & FRACTION
}

/// `value * 2^23` wrapped to 32 bits, for an exponent field that may be out
/// of range.
fn exponent_bits(value: i32) -> u32 {
    value.wrapping_mul(1 << 23) as u32
}

/// Shifts `a` so that its leading one lands on bit 23, dropping low bits.
fn normalise(a: u32) -> (u32, i32) {
    let n = a.leading_zeros() as i32;
    let shifted = if n >= 8 { a << (n - 8) } else { a >> (8 - n) };
    (shifted, n)
}

pub fn add(a: u32, b: u32) -> u32 {
    let (ea, eb) = (exponent(a), exponent(b));
    if ea == 0 {
        return b;
    }
    if eb == 0 {
        return a;
    }
    let d = eb - ea;
    if d >= 23 {
        return b;
    }
    if d <= -23 {
        return a;
    }
    let significand = |x: u32| {
        let m = (i64::from(fraction(x)) + (1 << 23)) * 2;
        if x & SIGN != 0 { -m } else { m }
    };
    let (mut ma, mut mb) = (significand(a), significand(b));
    let e = if d > 0 {
        ma >>= d;
        eb
    } else {
        mb >>= -d;
        ea
    };
    let sum = ma + mb;
    if sum == 0 {
        return 0;
    }
    let (n_bits, n) = normalise(sum.unsigned_abs() as u32);
    exponent_bits(e + 7 - n) | (n_bits & FRACTION) | if sum < 0 { SIGN } else { 0 }
}

pub fn sub(a: u32, b: u32) -> u32 {
    add(a, b ^ SIGN)
}

pub fn mul(a: u32, b: u32) -> u32 {
    let sign = (a ^ b) & SIGN;
    let (ea, eb) = (exponent(a), exponent(b));
    let (fa, fb) = (fraction(a), fraction(b));
    let x = ea + eb - 127;
    let candidate = if fa != 0 && fb != 0 {
        let p = ((1u64 << 31) + (u64::from(fa) << 8)) * ((1u64 << 31) + (u64::from(fb) << 8));
        let t = u32::from(p >= 1 << 63);
        let f = ((p >> (39 + t)) as u32) & FRACTION;
        f | exponent_bits(x + t as i32) | sign
    } else if ea != 0 && eb != 0 {
        exponent_bits(x) | fa | fb | sign
    } else {
        return 0;
    };
    if (exponent_bits(x - 1) as i32) < 0 {
        0
    } else {
        candidate
    }
}

/// `I2R`.
pub fn i2r(v: i32) -> u32 {
    if v == 0 {
        return 0;
    }
    let (n_bits, n) = normalise(v.unsigned_abs());
    exponent_bits(158 - n) | (n_bits & FRACTION) | if v < 0 { SIGN } else { 0 }
}

/// `R2I`: truncation toward zero, saturating at the 32-bit limits.
pub fn r2i(x: u32) -> i32 {
    let e = exponent(x);
    let negative = x & SIGN != 0;
    if e < 127 {
        return 0;
    }
    if e >= 158 {
        return if negative { i32::MIN } else { i32::MAX };
    }
    let m = fraction(x) + (1 << 23);
    let magnitude = if e <= 150 {
        m >> (150 - e)
    } else {
        m << (e - 150)
    } as i32;
    if negative { -magnitude } else { magnitude }
}

pub fn floor(x: u32) -> u32 {
    let e = exponent(x);
    let negative = x & SIGN != 0;
    if e < 127 {
        return if negative && x != SIGN {
            0xBF80_0000
        } else {
            0
        };
    }
    if e >= 150 {
        return x;
    }
    let shift = 150 - e;
    let mask = (1u32 << shift) - 1;
    let m = fraction(x) + (1 << 23);
    if !negative || m & mask == 0 {
        return x & !mask;
    }
    // A negative value with a fractional part rounds away from zero.
    i2r(-((m >> shift) as i32 + 1))
}

/// The reciprocal table `R[0..=1024]` of SPEC-0004.
pub fn recip_table() -> &'static [u32; 1025] {
    static TABLE: OnceLock<[u32; 1025]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let source = include_str!("../../../specs/SPEC-0004-recip-table.tsv");
        let mut table = [0u32; 1025];
        let mut filled = 0;
        for line in source
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (index, value) = line.split_once('\t').expect("index and value");
            let index: usize = index.parse().expect("index");
            table[index] = u32::from_str_radix(value, 16).expect("hex value");
            filled += 1;
        }
        assert_eq!(filled, 1025, "reciprocal table is incomplete");
        table
    })
}

pub fn recip(x: u32) -> u32 {
    let table = recip_table();
    let f = fraction(x);
    let (i, r) = ((f >> 13) as usize, f & 0x1FFF);
    let w = (r << 19) + (r << 6) + (r >> 7);
    let h = ((u64::from(table[i].wrapping_sub(table[i + 1])) * u64::from(w)) >> 32) as u32;
    let v = table[i]
        .wrapping_sub(h)
        .wrapping_sub(exponent_bits(exponent(x)));
    if v.wrapping_add(0x7E00_0000) & SIGN != 0 {
        0
    } else {
        v.wrapping_add(0x7E80_0000) | (x & SIGN)
    }
}

pub fn div(a: u32, b: u32) -> u32 {
    if a == b { ONE } else { mul(a, recip(b)) }
}

/// `10^n` computed by squaring with [`mul`].
fn power_of_ten(mut n: u32) -> u32 {
    let (mut result, mut base) = (ONE, 0x4120_0000); // 1.0, 10.0
    while n > 0 {
        if n & 1 != 0 {
            result = mul(result, base);
        }
        base = mul(base, base);
        n >>= 1;
    }
    result
}

/// Decimal text to real, as `S2R` and real literals read it. `S2R(null)`
/// is 0.0 without parsing; pass `""` for it.
pub fn s2r(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let (sign, digits) = match bytes.first() {
        Some(b'-') => (-1, &bytes[1..]),
        Some(b'+') => (1, &bytes[1..]),
        _ => (1, bytes),
    };
    // m: up to nine significant digits; c: significant digits seen;
    // k: power of ten to divide by; q: what each further digit adds to k.
    let (mut m, mut c, mut k, mut q) = (0i32, 0u32, 0i32, 0i32);
    let (mut significant, mut seen_point) = (false, false);
    for &byte in digits {
        match byte {
            b'0'..=b'9' => {
                let digit = i32::from(byte - b'0');
                significant |= digit != 0;
                if significant {
                    c += 1;
                }
                if c <= 9 {
                    m = m * 10 + digit;
                } else if c == 10 {
                    q -= 1;
                }
                k += q;
            }
            b'.' if !seen_point => {
                seen_point = true;
                q += 1;
            }
            _ => break,
        }
    }
    let value = i2r(m * sign);
    if k >= 0 {
        div(value, power_of_ten(k as u32))
    } else {
        mul(value, power_of_ten(k.unsigned_abs()))
    }
}

/// Fractional part, with the sign of `x` (SPEC-0004).
pub fn frac(x: u32) -> u32 {
    let e = exponent(x);
    if e < 127 {
        return x;
    }
    if e >= 150 {
        return 0;
    }
    let k = e - 127;
    let integer_part = x & !((1u32 << (23 - k)) - 1);
    sub(x, integer_part)
}

/// Remainder of `x` by `y`, for `x >= 0` (SPEC-0004).
pub fn fmod(x: u32, y: u32) -> u32 {
    let y = y & !SIGN;
    let r = mul(frac(mul(x, recip(y))), y);
    let (rf, yf) = (f32::from_bits(r), f32::from_bits(y));
    if rf > 0.0 && rf >= yf { sub(r, y) } else { r }
}

/// A real literal in a script, read by the compiler's own rule
/// (SPEC-0005): integer and fraction digits accumulate with 32-bit
/// wrap-around, and the value is `I + F / 10^n` in engine arithmetic.
/// `text` is digits with at most one point.
pub fn real_literal(text: &str) -> u32 {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let accumulate = |digits: &str| {
        digits.bytes().fold(0i32, |v, d| {
            v.wrapping_mul(10).wrapping_add(i32::from(d - b'0'))
        })
    };
    let integer = i2r(accumulate(whole));
    if fraction.is_empty() {
        return integer;
    }
    let scale = (0..fraction.len()).fold(1i32, |v, _| v.wrapping_mul(10));
    add(integer, div(i2r(accumulate(fraction)), i2r(scale)))
}

/// `R2SW(x, width, precision)`; `R2S(x)` is `r2sw(x, 0, 3)` (SPEC-0004).
pub fn r2sw(x: u32, width: i32, precision: i32) -> String {
    const TEN: u32 = 0x4120_0000;
    const HALF: u32 = 0x3F00_0000;
    let mut text = String::new();
    if x & SIGN != 0 && x != SIGN {
        text.push('-');
    }
    let a = x & !SIGN;
    if exponent(a) >= 158 {
        let nan = exponent(a) == 255 && fraction(a) != 0;
        if !nan && f32::from_bits(a) >= f32::from_bits(0x7F7F_FFFF) {
            text.push_str("inf");
            return text;
        }
        let (mut t, mut k) = (a, 0);
        loop {
            t = div(t, TEN);
            k += 1;
            if t <= 0x4EFF_FFFF {
                break;
            }
        }
        text.push_str(&r2i(t).to_string());
        text.push_str(&"0".repeat(k));
        text.push('.');
        text.push_str(&"0".repeat(precision.max(1) as usize));
        return text;
    }
    let mut q = if precision < 0 { 6 } else { precision.min(9) };
    let mut i = r2i(a);
    let scale = 10i32.pow(q as u32);
    let mut n = r2i(floor(add(mul(frac(a), i2r(scale)), HALF)));
    if n >= scale {
        i += 1;
        n -= scale;
    }
    if precision < 0 {
        while q > 1 && n % 10 == 0 {
            q -= 1;
            n /= 10;
        }
    }
    let pad = (width - q).max(1) as usize;
    text.push_str(&format!("{i:>pad$}."));
    if q == 0 {
        text.push_str(&n.to_string());
    } else {
        text.push_str(&format!("{n:0>q$}", q = q as usize));
    }
    text
}

/// JASS `==` on reals: the difference is below 0.001 in magnitude.
pub fn jass_equal(a: u32, b: u32) -> bool {
    f32::from_bits(sub(a, b) & !SIGN) < f32::from_bits(0x3A83_126F)
}

/// JASS `!=` on reals: the bit patterns differ. Not the negation of `==`.
pub fn jass_not_equal(a: u32, b: u32) -> bool {
    a != b
}

/// Whether a real counts as zero for division (`0.0` or `-0.0`).
pub fn is_zero(x: u32) -> bool {
    x & !SIGN == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_cases_from_the_spec() {
        assert_eq!(sub(ONE, 0x3BA3_D70A), 0x3F7E_B851); // 1.0 - 0.005
        assert_eq!(mul(0x47C3_5000, 0x3BA3_D70A), 0x43F9_FFFF); // 100000.0 * 0.005
        assert_eq!(f32::from_bits(div(ONE, 0x3BA3_D70A)), 200.00003);
        assert_eq!(i2r(16_777_217), 0x4B80_0000);
        let (one, near) = (ONE, 1.0005f32.to_bits());
        assert!(jass_equal(one, near) && jass_not_equal(one, near));
        assert!(jass_not_equal(0, SIGN));
        assert_eq!(floor(SIGN), 0);
        assert_eq!(floor(0x8000_0001), 0xBF80_0000);
        assert_eq!(floor((-2.5f32).to_bits()), (-3.0f32).to_bits());
    }

    #[test]
    fn s2r_examples_from_the_spec() {
        assert_eq!(s2r("0.1"), 0x3DCC_CCCE);
        assert_eq!(s2r("1.5"), 0x3FC0_0001);
        assert_eq!(s2r("123456789012"), 0x51E5_F4C8);
        assert_eq!(s2r(" 2"), 0);
        assert_eq!(s2r("1e3"), ONE);
        assert_eq!(s2r("12;34"), s2r("12"));
        assert_eq!(s2r("-0"), 0);
        assert_eq!(s2r(""), 0);
    }
}
