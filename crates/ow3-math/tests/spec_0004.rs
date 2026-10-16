//! Verification of SPEC-0004: every line of its test vectors, and the way
//! the reciprocal table can be observed from JASS.

use ow3_math::*;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("bad hex {s}"))
}

#[test]
fn reproduces_every_test_vector() {
    let source = include_str!("../../../specs/SPEC-0004-vectors.tsv");
    let mut checked = std::collections::BTreeMap::<&str, usize>::new();
    let mut mismatches = Vec::new();
    for line in source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let [op, a, b, expected] = fields[..] else {
            panic!("bad line {line}");
        };
        let (got, want) = match op {
            "add" => (add(hex(a), hex(b)), hex(expected)),
            "sub" => (sub(hex(a), hex(b)), hex(expected)),
            "mul" => (mul(hex(a), hex(b)), hex(expected)),
            "div" => (div(hex(a), hex(b)), hex(expected)),
            "recip" => (recip(hex(a)), hex(expected)),
            "floor" => (floor(hex(a)), hex(expected)),
            "i2r" => (i2r(a.parse().unwrap()), hex(expected)),
            "s2r" => (s2r(a.trim_matches('"')), hex(expected)),
            "r2i" => (r2i(hex(a)) as u32, expected.parse::<i32>().unwrap() as u32),
            "r2sw" => {
                let (w, p) = b.split_once(' ').unwrap();
                let text = r2sw(hex(a), w.parse().unwrap(), p.parse().unwrap());
                *checked.entry(op).or_default() += 1;
                if format!("\"{text}\"") != expected {
                    mismatches.push(format!("{line}: got \"{text}\""));
                }
                continue;
            }
            other => panic!("unknown op {other}"),
        };
        *checked.entry(op).or_default() += 1;
        if got != want {
            mismatches.push(format!("{line}: got {got:08x}"));
        }
    }
    eprintln!("checked {checked:?}");
    let ops = [
        "add", "sub", "mul", "div", "recip", "floor", "i2r", "r2i", "s2r", "r2sw",
    ];
    assert!(
        ops.iter()
            .all(|op| checked.get(op).is_some_and(|&n| n >= 1000))
    );
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(20)].join("\n")
    );
}

/// The table can be measured from JASS without reading the game's code:
/// with `x = I2R(1024 + i) / I2R(1024)`, the value
/// `R2I((I2R(1) / x) * I2R(16777216)) - 8388608` equals `R[i]` for every
/// `i` in 0..=1023, and `R[1024]` only matters through `x` with `i = 1023`.
/// Every operation on that path is exact in the engine's arithmetic, so
/// the measured numbers are the table itself.
#[test]
fn reciprocal_table_is_observable_from_jass() {
    let table = recip_table();
    let (one, scale, two_to_24) = (i2r(1), i2r(1024), i2r(16_777_216));
    for i in 0..1024 {
        let x = div(i2r(1024 + i), scale);
        assert_eq!(x, ONE | ((i as u32) << 13), "x_{i} is exact");
        let measured = r2i(mul(div(one, x), two_to_24)) - 8_388_608;
        assert_eq!(measured as u32, table[i as usize], "entry {i}");
    }

    // R[1024] enters only as R[1023] - R[1024] for x = 1023/1024..2 with a
    // non-zero low fraction. The 8191 such results pin it down: every
    // difference that reproduces them gives the same R[1024].
    let reference: Vec<u32> = (1..0x2000u32)
        .map(|r| recip(ONE | (1023 << 13) | r))
        .collect();
    let consistent: Vec<u32> = (0u32..=0x2000)
        .filter(|&candidate| {
            let d = table[1023].wrapping_sub(candidate);
            (1..0x2000u32).zip(&reference).all(|(r, &want)| {
                let w = (r << 19) + (r << 6) + (r >> 7);
                let h = ((u64::from(d) * u64::from(w)) >> 32) as u32;
                let v = table[1023].wrapping_sub(h).wrapping_sub(127 << 23);
                v.wrapping_add(0x7E80_0000) == want
            })
        })
        .collect();
    assert_eq!(consistent, [table[1024]]);
}

/// Compares a measurement made in the game with the ow3-R1 test map (see
/// the verification section of SPEC-0004) with the table. Place the file the
/// game writes at `game-data/1.27b/measurements/ow3-recip.txt`; the test
/// passes without checking anything when it is absent.
#[test]
fn measured_table_matches() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-data/1.27b/measurements/ow3-recip.txt");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipping: {} not found", path.display());
        return;
    };
    let table = recip_table();
    let (mut entries, mut tail) = (0, 0);
    // Preload files hold lines such as `call Preload( "R 5 8364097" )`.
    for quoted in text.split('"').skip(1).step_by(2) {
        let fields: Vec<&str> = quoted.split_whitespace().collect();
        match fields[..] {
            ["R", i, value] => {
                let (i, value): (usize, i32) = (i.parse().unwrap(), value.parse().unwrap());
                assert_eq!(value as u32, table[i], "R[{i}]");
                entries += 1;
            }
            ["T", r, value] => {
                let (r, value): (u32, i32) = (r.parse().unwrap(), value.parse().unwrap());
                let expected = recip(ONE | (1023 << 13) | r) & 0x007F_FFFF;
                assert_eq!(value as u32, expected, "1/x for r = {r}");
                tail += 1;
            }
            _ => {}
        }
    }
    assert_eq!((entries, tail), (1024, 8191), "incomplete measurement");
}

/// Examples of SPEC-0004 for `frac` and `fmod`.
#[test]
fn fractional_part_and_remainder() {
    let bits = f32::to_bits;
    assert_eq!(frac(bits(2.75)), bits(0.75));
    assert_eq!(frac(bits(-2.75)), bits(-0.75));
    assert_eq!(frac(bits(0.25)), bits(0.25));
    assert_eq!(frac(bits(1.0e9)), 0);
    assert_eq!(fmod(bits(310.0), bits(120.0)), 0x428B_FFFE);
    assert_eq!(fmod(bits(450.0), bits(120.0)), bits(90.0));
}

/// Examples of SPEC-0004 for `R2S` and `R2SW`.
#[test]
fn real_to_text_examples() {
    let bits = f32::to_bits;
    assert_eq!(r2sw(0x8000_0000, 0, 3), "0.000");
    assert_eq!(r2sw(bits(-0.0004), 0, 3), "-0.000");
    assert_eq!(r2sw(bits(2.5), 1, 0), "3.0");
    assert_eq!(r2sw(bits(-1.5), 10, 2), "-       1.50");
    assert_eq!(r2sw(div(i2r(1), i2r(3)), 1, 9), "0.333333312");
    assert_eq!(r2sw(bits(99.9995), 0, 3), "99.999");
}

/// Examples of SPEC-0005 for real literals.
#[test]
fn real_literal_examples() {
    assert_eq!(real_literal("0.1"), 0x3DCC_CCCE);
    assert_eq!(real_literal("1.5"), 1.5f32.to_bits());
    assert_eq!(real_literal("123456.789"), 123_456.78f32.to_bits());
    assert_eq!(real_literal("10000000000.0"), 1_410_065_408f32.to_bits());
    assert_eq!(real_literal("4294967296.0"), 0);
    assert_eq!(real_literal("2147483648.0"), (-2_147_483_648f32).to_bits());
    let wrapped = f32::from_bits(real_literal("0.1234567891234"));
    assert!((wrapped - 1.453).abs() < 1e-3, "{wrapped}");
    assert_eq!(real_literal("5."), 5.0f32.to_bits());
    assert_eq!(real_literal(".25"), 0.25f32.to_bits());
    assert_eq!(real_literal("1792.0"), 1792.0f32.to_bits());
}
