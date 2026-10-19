//! Facings of units and how they turn (SPEC-0013).
//!
//! A facing is kept in radians in `0..2 pi`. Setting `a` degrees stores
//! `mul(a, pi/180)`, reduced with `frac` when it is at least `2 pi` in
//! size and moved up by `2 pi` when negative; `GetUnitFacing` gives
//! `mul(r, 180/pi)`.
//!
//! `SetUnitFacing` turns the unit: the facing changes every 6 ticks, the
//! first time 3 ticks after the call. A walking unit's facing turns
//! towards its stored heading at its updates (SPEC-0018).
//!
//! Each change adds `s` to the signed step `w` towards the target, limits
//! `w` to `min(m, r) * (r / R)^P` (the last factor only below `R`) with
//! `r` the angle left, and turns by `w`; with less than 0.001 radians left
//! the facing takes its target. `s`, `m`, `R` and `P` come from the
//! type's `orientInterp`.

use ow3_math::natives::pow;
use ow3_math::{add, div, frac, mul, recip, sub};

/// pi / 180, 180 / pi, 2 pi and pi.
const DEGREES_TO_RADIANS: u32 = 0x3C8E_FA35;
const RADIANS_TO_DEGREES: u32 = 0x4265_2EE1;
const TWO_PI: u32 = 0x40C9_0FDB;
const PI: u32 = 0x4049_0FDB;
const SIGN: u32 = 0x8000_0000;
const ONE: u32 = 0x3F80_0000;

/// Ticks between the changes of a turn: the shared update cycle of the
/// units (SPEC-0013, SPEC-0018 "Phase").
const CHANGE_EVERY: u64 = 6;

fn real(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// The stored facing for `degrees`.
pub fn from_degrees(degrees: u32) -> u32 {
    let mut r = mul(degrees, DEGREES_TO_RADIANS);
    if real(r).abs() >= real(TWO_PI) {
        r = mul(frac(mul(r, recip(TWO_PI))), TWO_PI);
    }
    if real(r) < 0.0 {
        r = add(r, TWO_PI);
    }
    r
}

/// `GetUnitFacing` of a stored facing.
pub fn to_degrees(radians: u32) -> u32 {
    mul(radians, RADIANS_TO_DEGREES)
}

/// The constants of a turn by `orientInterp` (SPEC-0013): the step
/// added per change `s`, the largest step `m`, the angle `R` below which
/// the step is braked and the power `P` of the brake, as bits. Values 9
/// to 15 behave as 0.
pub(super) fn constants(orient_interp: u8) -> [u32; 4] {
    const P25: u32 = 0x3E80_0000;
    const P50: u32 = 0x3F00_0000;
    const P60: u32 = 0x3F19_999A;
    const P40: u32 = 0x3ECC_CCCE;
    match orient_interp {
        1 => [0x3CF5_C290, 0x3DCC_CCCE, 0x3E4C_CCCE, P25],
        2 => [0x3C75_C290, 0x3ECC_CCCE, 0x3F4C_CCCE, P25],
        3 => [0x3BA3_D70A, 0x3DCC_CCCE, 0x3F99_999A, P50],
        4 => [0x3D23_D70B, 0x3E19_999A, 0x3F99_999A, P60],
        5 => [0x3D4C_CCCD, 0x3E38_51EC, 0x3F99_999A, P60],
        6 => [0x3DCC_CCCE, 0x3E99_999A, 0x3F4C_CCCE, P40],
        7 => [0x3B44_9BA6, 0x3DA3_D70B, 0x3F99_999A, P50],
        8 => [0x3A83_126F, 0x3D4C_CCCD, 0x3FB3_3334, P50],
        _ => [0x3D8F_5C29, 0x3E4C_CCCE, 0x3E4C_CCCE, P25],
    }
}

/// A turn in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    target: u32,
    /// The tick of the next change.
    next: u64,
    step: u32,
    orient_interp: u8,
}

impl Turn {
    /// A turn to `target` (radians) that changes the facing from the
    /// update `first` on.
    pub fn new(target: u32, first: u64, orient_interp: u8) -> Turn {
        Turn {
            target,
            next: first,
            step: 0,
            orient_interp,
        }
    }

    /// The facing after tick `tick`, and whether the turn goes on.
    pub fn advance(&mut self, facing: u32, tick: u64) -> (u32, bool) {
        if tick != self.next {
            return (facing, true);
        }
        self.next += CHANGE_EVERY;
        let (next, step) = follow(facing, self.target, self.step, self.orient_interp);
        self.step = step;
        (next, next != self.target)
    }
}

/// The signed difference from the angle `current` to the angle `target`,
/// the shorter way (SPEC-0018, "Angle difference"): `t = sub(current,
/// target)`, then `sub(-2 pi, t)` or `sub(2 pi, t)` when `|t| > pi`, else
/// `-t`.
pub(super) fn angle_difference(target: u32, current: u32) -> u32 {
    let t = sub(current, target);
    if real(t) < 0.0 && -real(t) > real(PI) {
        sub(TWO_PI ^ SIGN, t)
    } else if real(t) >= 0.0 && real(t) > real(PI) {
        sub(TWO_PI, t)
    } else {
        t ^ SIGN
    }
}

/// Below this many radians left the facing takes the heading (0.001).
const SNAP: u32 = 0x3A83_126F;

/// One change of a facing towards `heading` (SPEC-0013, "Facing turn
/// rule"), with `w` the signed step of the change before (0 at rest). The
/// new facing and step.
pub(super) fn follow(facing: u32, heading: u32, w: u32, orient_interp: u8) -> (u32, u32) {
    let d = angle_difference(heading, facing);
    let r = d & !SIGN;
    if real(r) < real(SNAP) {
        return (heading, 0);
    }
    let [s, m, big_r, p] = constants(orient_interp);
    let w = if real(d) >= 0.0 {
        add(w, s)
    } else {
        add(w, s ^ SIGN)
    };
    let mut c = if real(m) < real(r) { m } else { r };
    if real(r) < real(big_r) {
        c = mul(c, pow(div(r, big_r), p));
    }
    let w = if real(w).abs() > real(c) {
        c | (w & SIGN)
    } else {
        w
    };
    let mut f = add(facing, w);
    if real(f).abs() >= real(TWO_PI) {
        let size = mul(frac(mul(f & !SIGN, div(ONE, TWO_PI))), TWO_PI);
        f = size | (f & SIGN);
    }
    if real(f) < 0.0 {
        f = add(f, TWO_PI);
    }
    (f, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values of SPEC-0013.
    #[test]
    fn conversion() {
        let read = |a: f32| f32::from_bits(to_degrees(from_degrees(a.to_bits())));
        assert_eq!(to_degrees(from_degrees(33.3f32.to_bits())), 0x4205_3332);
        for (set, got) in [
            (40.0f32, 39.999_996f32),
            (360.0, 359.999_97),
            (400.0, 40.000_02),
            (-90.0, 270.0),
            (-0.001, 359.999),
            (-1000.25, 79.749_94),
            (10000.7, 280.700_65),
        ] {
            assert_eq!(read(set), got, "{set}");
        }
    }

    /// The brake against the measured table `SPEC-0013-facing-brake.tsv`
    /// (remaining angle and step in degrees, by `orientInterp`).
    #[test]
    fn brake_matches_table() {
        let mut worst = [0.0f32; 9];
        for line in include_str!("../../../../specs/SPEC-0013-facing-brake.tsv")
            .lines()
            .skip(1)
        {
            let w: Vec<&str> = line.split('\t').map(str::trim).collect();
            let (Ok(kind), Ok(r), Ok(step)) =
                (w[0].parse::<u8>(), w[1].parse::<f32>(), w[2].parse::<f32>())
            else {
                continue;
            };
            let [_, m, big_r, p] = constants(kind);
            let r = mul(r.to_bits(), DEGREES_TO_RADIANS);
            let mut c = if real(m) < real(r) { m } else { r };
            if real(r) < real(big_r) {
                c = mul(c, pow(div(r, big_r), p));
            }
            let k = usize::from(kind);
            worst[k] = worst[k].max((real(mul(c, RADIANS_TO_DEGREES)) - step).abs());
        }
        for (kind, w) in worst.iter().enumerate() {
            assert!(*w < 0.01, "{kind}: {w} degrees");
        }
    }

    /// SPEC-0018, verification 5: a footman's first facings towards the
    /// stored heading `0x3EB7B64C`; the third depends on the heading of
    /// that update, which moves a little.
    #[test]
    fn first_facings_of_a_walk() {
        let (mut f, mut w) = (0, 0);
        let mut got = Vec::new();
        for _ in 0..2 {
            (f, w) = follow(f, 0x3EB7_B64C, w, 0);
            got.push(f);
        }
        assert_eq!(got, [0x3D8F_5C29, 0x3E57_0A3D]);
    }

    /// A footman (`orientInterp` 0) turning from 0 to 90 degrees: changes
    /// every 6 ticks from the first update (here 3 ticks after the call),
    /// by 0.07, 0.14, then 0.2.
    #[test]
    fn turning() {
        let mut turn = Turn::new(from_degrees(90.0f32.to_bits()), 103, 0);
        let mut facing = 0;
        let mut changes = Vec::new();
        for tick in 101..=200 {
            let (next, going) = turn.advance(facing, tick);
            if next != facing {
                changes.push((tick - 100, f32::from_bits(next)));
            }
            facing = next;
            if !going {
                break;
            }
        }
        let ticks: Vec<u64> = changes.iter().map(|c| c.0).collect();
        assert_eq!(ticks[..4], [3, 9, 15, 21]);
        assert!((changes[0].1 - 0.07).abs() < 1e-6);
        assert!((changes[1].1 - 0.21).abs() < 1e-6);
        assert!((changes[2].1 - 0.41).abs() < 1e-6);
        assert_eq!(facing, from_degrees(90.0f32.to_bits()));
        // The shorter way: from 10 to 350 degrees turns down through 0.
        let mut turn = Turn::new(from_degrees(350.0f32.to_bits()), 3, 0);
        let (next, _) = turn.advance(from_degrees(10.0f32.to_bits()), 3);
        assert!(f32::from_bits(next) < f32::from_bits(from_degrees(10.0f32.to_bits())));
    }
}
