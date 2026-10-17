//! Facings of units and how they turn (SPEC-0013).
//!
//! A facing is kept in radians in `0..2 pi`. Setting `a` degrees stores
//! `mul(a, pi/180)`, reduced with `frac` when it is at least `2 pi` in
//! size and moved up by `2 pi` when negative; `GetUnitFacing` gives
//! `mul(r, 180/pi)`.
//!
//! `SetUnitFacing` turns the unit: the facing changes every 6 ticks, the
//! first time 3 ticks after the call, by a step that starts at `s`, grows
//! by `s` per change and stays at `m`, both set by the type's
//! `orientInterp`.
//!
//! Each step is `min(w + s, m, B(r))` with the braking `B` of the
//! remaining angle (SPEC-0018); with less than 0.001 radians left the
//! facing takes its target.
//!
//! Values 3, 7 and 8 brake by the closed form of SPEC-0013, the others by
//! their table.

use ow3_math::{add, frac, mul, recip, sub};

/// pi / 180, 180 / pi, 2 pi and pi.
const DEGREES_TO_RADIANS: u32 = 0x3C8E_FA35;
const RADIANS_TO_DEGREES: u32 = 0x4265_2EE1;
const TWO_PI: u32 = 0x40C9_0FDB;
const PI: u32 = 0x4049_0FDB;
const SIGN: u32 = 0x8000_0000;

/// Ticks from `SetUnitFacing` to the first change, and between changes.
const FIRST_CHANGE: u64 = 3;
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

/// The first step and the largest step of `orientInterp`, in radians.
///
/// PROVISIONAL: the largest step of 2 was not reached within 180 degrees
/// (at least 0.285), so it has none here.
pub(super) fn steps(orient_interp: u8) -> (u32, u32) {
    let (s, m) = match orient_interp {
        1 => (0.03f32, 0.1f32),
        2 => return (0.015f32.to_bits(), PI),
        3 => (0.005, 0.1),
        4 => (0.04, 0.15),
        5 => (0.05, 0.18),
        6 => (0.1, 0.3),
        7 => (0.003, 0.08),
        8 => (0.001, 0.05),
        _ => (0.07, 0.2),
    };
    (s.to_bits(), m.to_bits())
}

/// A turn in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    target: u32,
    /// The tick of the call.
    started: u64,
    step: u32,
    orient_interp: u8,
}

impl Turn {
    pub fn new(target_degrees: u32, tick: u64, orient_interp: u8) -> Turn {
        Turn {
            target: from_degrees(target_degrees),
            started: tick,
            step: 0,
            orient_interp,
        }
    }

    /// The facing after tick `tick`, and whether the turn goes on.
    pub fn advance(&mut self, facing: u32, tick: u64) -> (u32, bool) {
        let since = tick.saturating_sub(self.started);
        if since < FIRST_CHANGE || (since - FIRST_CHANGE) % CHANGE_EVERY != 0 {
            return (facing, true);
        }
        let (next, step) = follow(facing, self.target, self.step, self.orient_interp);
        self.step = step;
        (next, next != self.target)
    }
}

/// The braking tables of SPEC-0013, by `orientInterp`: the largest step
/// with a remaining angle, both in degrees, as reals.
fn brake_table(orient_interp: u8) -> &'static [(u32, u32)] {
    static TABLES: std::sync::OnceLock<Vec<Vec<(u32, u32)>>> = std::sync::OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        let mut tables = vec![Vec::new(); 9];
        for line in include_str!("../../../../specs/SPEC-0013-facing-brake.tsv")
            .lines()
            .skip(1)
        {
            let mut fields = line.split('\t').map(str::trim);
            let (Some(kind), Some(r), Some(s)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let (Ok(kind), Ok(r), Ok(s)) =
                (kind.parse::<usize>(), r.parse::<f32>(), s.parse::<f32>())
            else {
                continue;
            };
            if let Some(table) = tables.get_mut(kind) {
                table.push((r.to_bits(), s.to_bits()));
            }
        }
        tables
    });
    tables
        .get(usize::from(orient_interp))
        .filter(|t| !t.is_empty())
        .unwrap_or(&tables[0])
}

/// The braking step `B(r)` for `remaining` degrees, interpolated linearly
/// between the points of the table, in the engine arithmetic. `None`
/// above the table, where no braking was seen.
///
/// PROVISIONAL: SPEC-0013 interpolates on a log-log scale, which needs
/// logarithms the simulation does not have; the points are close enough
/// for the difference to stay small. Below the first point the first
/// segment goes on.
fn brake(remaining: u32, orient_interp: u8) -> Option<u32> {
    let table = brake_table(orient_interp);
    let r = real(remaining);
    let last = table.last()?;
    if r > real(last.0) {
        return None;
    }
    let i = table
        .iter()
        .position(|&(x, _)| real(x) >= r)
        .unwrap_or(table.len() - 1)
        .max(1);
    let ((x0, y0), (x1, y1)) = (table[i - 1], table[i]);
    let t = ow3_math::div(sub(remaining, x0), sub(x1, x0));
    Some(add(y0, mul(sub(y1, y0), t)))
}

/// The brake of `orientInterp` 3, 7 and 8 for `r` radians left, with `m`
/// the largest step (SPEC-0013): `sqrt(C r)`, times `r / m` below `m`,
/// with `C = m^2 / 1.2` (3 and 7) or `m^2 / 1.4` (8). The square root is
/// the correctly rounded one of IEEE 754.
fn closed_brake(r: u32, m: u32, orient_interp: u8) -> u32 {
    let divisor = if orient_interp == 8 { 1.4f32 } else { 1.2f32 };
    let c = ow3_math::div(mul(m, m), divisor.to_bits());
    let b = real(mul(c, r)).sqrt().to_bits();
    if real(r) < real(m) {
        mul(b, ow3_math::div(r, m))
    } else {
        b
    }
}

/// Below this many radians left the facing takes the heading (0.001).
const SNAP: u32 = 0x3A83_126F;

/// One step of a facing towards `heading` (SPEC-0018): with `w` the step of
/// the change before (0 at rest) and `r` the angle left, the step is
/// `min(w + s, m, B(r))`; with less than 0.001 radians left the facing
/// takes the heading and the step becomes 0. The new facing and step.
pub(super) fn follow(facing: u32, heading: u32, w: u32, orient_interp: u8) -> (u32, u32) {
    // The difference, moved into -pi..pi.
    let mut diff = sub(heading, facing);
    if real(diff) > real(PI) {
        diff = sub(diff, TWO_PI);
    } else if real(diff) < -real(PI) {
        diff = add(diff, TWO_PI);
    }
    let r = diff & !SIGN;
    if real(r) < real(SNAP) {
        return (heading, 0);
    }
    let (first, largest) = steps(orient_interp);
    let mut step = add(w, first);
    if real(step) > real(largest) {
        step = largest;
    }
    let b = match orient_interp {
        3 | 7 | 8 => Some(closed_brake(r, largest, orient_interp)),
        _ => brake(mul(r, RADIANS_TO_DEGREES), orient_interp).map(|b| mul(b, DEGREES_TO_RADIANS)),
    };
    if let Some(b) = b {
        if real(b) < real(step) {
            step = b;
        }
    }
    if real(step) >= real(r) {
        return (heading, 0);
    }
    let signed = if real(diff) < 0.0 { step ^ SIGN } else { step };
    let mut next = add(facing, signed);
    if real(next) >= real(TWO_PI) {
        next = sub(next, TWO_PI);
    } else if real(next) < 0.0 {
        next = add(next, TWO_PI);
    }
    (next, step)
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

    /// The closed form of `orientInterp` 3, 7 and 8 against their tables.
    #[test]
    fn closed_brake_matches_table() {
        for kind in [3u8, 7, 8] {
            let (_, m) = steps(kind);
            let mut worst = 0.0f32;
            for &(r, step) in brake_table(kind) {
                let b = closed_brake(mul(r, DEGREES_TO_RADIANS), m, kind);
                worst = worst.max((real(mul(b, RADIANS_TO_DEGREES)) - real(step)).abs());
            }
            assert!(worst < 0.002, "{kind}: {worst} degrees");
        }
    }

    /// A footman (`orientInterp` 0) turning from 0 to 90 degrees: changes
    /// at ticks 3, 9, 15... after the call, by 0.07, 0.14, then 0.2.
    #[test]
    fn turning() {
        let mut turn = Turn::new(90.0f32.to_bits(), 100, 0);
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
        let mut turn = Turn::new(350.0f32.to_bits(), 0, 0);
        let (next, _) = turn.advance(from_degrees(10.0f32.to_bits()), 3);
        assert!(f32::from_bits(next) < f32::from_bits(from_degrees(10.0f32.to_bits())));
    }
}
