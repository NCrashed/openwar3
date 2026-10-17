//! Verification of SPEC-0011: every line of its test vectors, and the
//! examples of the spec.

use ow3_math::natives::*;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("bad hex {s}"))
}

#[test]
fn reproduces_every_test_vector() {
    let source = include_str!("../../../specs/SPEC-0011-vectors.tsv");
    let mut checked = std::collections::BTreeMap::<&str, usize>::new();
    let mut mismatches = Vec::new();
    for line in source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let [native, a, b, expected] = fields[..] else {
            panic!("bad line {line}");
        };
        let a = hex(a);
        let got = match native {
            "Sin" => sin(a),
            "Cos" => cos(a),
            "Tan" => tan(a),
            "Asin" => asin(a),
            "Acos" => acos(a),
            "Atan" => atan(a),
            "Atan2" => atan2(a, hex(b)),
            "SquareRoot" => square_root(a),
            "Pow" => pow(a, hex(b)),
            "Deg2Rad" => deg2rad(a),
            "Rad2Deg" => rad2deg(a),
            other => panic!("unknown native {other}"),
        };
        *checked.entry(native).or_default() += 1;
        if got != hex(expected) {
            mismatches.push(format!("{line}: got {got:08x}"));
        }
    }
    eprintln!("checked {checked:?}");
    assert_eq!(checked.len(), 11);
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches[..mismatches.len().min(30)].join("\n")
    );
}

#[test]
fn spec_examples() {
    let bits = f32::to_bits;
    assert_eq!(square_root(bits(9.0)), 0x4040_00F2);
    assert_eq!(square_root(bits(2.0)), 0x3FB5_04F3);
    assert_eq!(square_root(bits(0.0005)), 0);
    assert_eq!(pow(bits(2.0), bits(-1.0)), 0x3F00_0002);
    assert_eq!(pow(bits(2.0), bits(10.0)), bits(1024.0));
    assert_eq!(pow(bits(-2.0), bits(3.0)), bits(-8.0));
    assert_eq!(pow(bits(5.0), bits(0.0005)), bits(1.0));
    assert_eq!(pow(bits(0.0), bits(-1.0)), 0);
    assert_eq!(pow(bits(0.0), bits(0.0)), bits(1.0));
    assert!((f32::from_bits(pow(bits(-8.0), bits(0.5))) - 2.828_426_4).abs() < 1e-5);
    assert_eq!(cos(0), 0x3F7F_FFFF);
    assert_eq!(sin(0x4049_0FDB), 0x3000_0000);
    assert_eq!(atan2(bits(1.0), bits(1.0)), 0x3F49_0FDC);
    assert_eq!(atan2(bits(0.0005), bits(-0.0005)), 0);
    assert_eq!(atan2(0, bits(-1.0)), 0x4049_0FDB);
    assert_eq!(acos(bits(0.5)), 0x3F86_0A91);
    assert_eq!(asin(bits(1.5)), 0);
    assert_eq!(deg2rad(bits(180.0)), 0x4049_0FDA);
    // Does not hang (the game does).
    let _ = pow(bits(2.0), bits(3_000_000_000.0));
}
