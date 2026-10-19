//! The places of a group's members in its formation (SPEC-0025,
//! "Layout").
//!
//! Positions and lengths are in cells of 32 world units. The members are
//! turned into the formation's frame (forward along the formation angle
//! and to the left), grouped by their rank (front first), set out in rows
//! as wide as the widest rank, centred and turned back by the angle.

use ow3_math::natives::{cos, sin};
use ow3_math::{add, div, i2r, mul, sub};

const SIGN: u32 = 0x8000_0000;
const ONE: u32 = 0x3F80_0000;
const TWO: u32 = 0x4000_0000;
const HALF: u32 = 0x3F00_0000;
/// The space between two rows of a rank, after the diameter (2.5).
const ROW_SPACE: u32 = 0x4020_0000;
/// The space between two ranks (5.5000005, not exactly 5.5).
const RANK_SPACE: u32 = 0x40B0_0001;
/// Radii from this on use the rows for big units (1.5 cells).
const BIG: u32 = 0x3FC0_0000;

/// The number of columns of a rank by its number of members (1 to 12),
/// for a formation of one rank or more, and for small or big units.
const COLUMNS: [[[u8; 12]; 2]; 2] = [
    [
        [1, 2, 3, 2, 3, 3, 3, 3, 3, 4, 4, 4],
        [1, 2, 3, 2, 3, 3, 3, 3, 3, 3, 3, 3],
    ],
    [
        [1, 2, 3, 4, 3, 3, 4, 4, 4, 4, 4, 4],
        [1, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3],
    ],
];

fn real(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// A member of a group: its position and collision radius in cells and
/// its formation rank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub position: [u32; 2],
    pub radius: u32,
    pub rank: u8,
}

/// Sorts `items` (indices) so that `key` decreases, deciding ties as the
/// game does (SPEC-0025, "Places").
pub(super) fn sort_decreasing(items: &mut [usize], key: impl Fn(usize) -> u32) {
    for k in (1..items.len()).rev() {
        let mut smallest = key(items[k]);
        let mut at = k;
        for i in (0..k).rev() {
            if real(key(items[i])) < real(smallest) {
                smallest = key(items[i]);
                at = i;
            }
        }
        items.swap(at, k);
    }
}

/// Sorts `items` so that `key` increases, ties decided as the game does.
fn sort_increasing(items: &mut [usize], key: impl Fn(usize) -> u32) {
    for k in (1..items.len()).rev() {
        let mut largest = key(items[k]);
        let mut at = k;
        for i in (0..k).rev() {
            if real(key(items[i])) > real(largest) {
                largest = key(items[i]);
                at = i;
            }
        }
        items.swap(at, k);
    }
}

/// The offsets of the members' places from the group's waypoint, for the
/// formation angle `angle` (radians), in member order.
pub fn offsets(members: &[Member], angle: u32) -> Vec<[u32; 2]> {
    let n = members.len();
    if n <= 1 {
        return vec![[0, 0]; n];
    }
    // Forward and sideways coordinates.
    let (s, c) = (sin(angle ^ SIGN), cos(angle ^ SIGN));
    let u: Vec<u32> = members
        .iter()
        .map(|m| sub(mul(m.position[0], c), mul(m.position[1], s)))
        .collect();
    let w: Vec<u32> = members
        .iter()
        .map(|m| add(mul(m.position[0], s), mul(m.position[1], c)))
        .collect();
    let ranks: Vec<Vec<usize>> = (0..16u8)
        .map(|rank| (0..n).filter(|&i| members[i].rank == rank).collect())
        .filter(|r: &Vec<usize>| !r.is_empty())
        .collect();
    let several = usize::from(ranks.len() > 1);
    // The diameter and columns of each rank, and the widest rank.
    let shapes: Vec<(u32, usize)> = ranks
        .iter()
        .map(|rank| {
            let rmax = rank.iter().fold(0, |r, &i| {
                if real(members[i].radius) > real(r) {
                    members[i].radius
                } else {
                    r
                }
            });
            let big = usize::from(real(rmax) >= real(BIG));
            let m = rank.len();
            let cols = m.min(usize::from(COLUMNS[several][big][m.min(12) - 1]));
            (add(rmax, rmax), cols)
        })
        .collect();
    let mut wmax = 0;
    for &(d, cols) in &shapes {
        let kf = i2r(cols as i32);
        let width = add(mul(d, kf), mul(add(d, TWO), sub(kf, ONE)));
        if real(width) > real(wmax) {
            wmax = width;
        }
    }
    // The places, rank by rank and row by row.
    let mut places = vec![[0u32; 2]; n];
    let mut depth = 0;
    for (rank, &(d, cols)) in ranks.iter().zip(&shapes) {
        let mut order = rank.clone();
        sort_decreasing(&mut order, |i| u[i]);
        let rows: Vec<&[usize]> = order.chunks(cols).collect();
        for (r, row) in rows.iter().enumerate() {
            if row.len() == 1 {
                places[row[0]] = [depth, mul(wmax, HALF)];
            } else {
                let mut row = row.to_vec();
                sort_increasing(&mut row, |i| w[i]);
                let g = div(sub(wmax, d), i2r(row.len() as i32 - 1));
                let b0 = mul(d, HALF);
                for (j, &i) in row.iter().enumerate() {
                    places[i] = [depth, add(b0, mul(g, i2r(j as i32)))];
                }
            }
            if r + 1 < rows.len() {
                depth = sub(depth, add(d, ROW_SPACE));
            }
        }
        depth = sub(depth, RANK_SPACE);
    }
    // Centred and turned by the angle.
    let inverse = div(ONE, i2r(n as i32));
    let (sa, sb) = places
        .iter()
        .fold((0, 0), |(a, b), p| (add(a, p[0]), add(b, p[1])));
    let (ma, mb) = (mul(sa, inverse), mul(sb, inverse));
    let (s, c) = (sin(angle), cos(angle));
    places
        .iter()
        .map(|p| {
            let (da, db) = (sub(p[0], ma), sub(p[1], mb));
            [sub(mul(da, c), mul(db, s)), add(mul(da, s), mul(db, c))]
        })
        .collect()
}

/// Which members are held at an update of their cluster (SPEC-0025,
/// "Members that wait for their rank to pass"): a member stands while a
/// member of a front rank is still behind it or beside it. `done` marks
/// the members that have reached their places, which are left out.
pub fn holds(members: &[Member], done: &[bool], angle: u32) -> Vec<bool> {
    let n = members.len();
    let (s, c) = (sin(angle ^ SIGN), cos(angle ^ SIGN));
    let (mut lo, mut hi) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for m in members {
        let u = sub(mul(m.position[0], c), mul(m.position[1], s));
        let e = add(m.radius, ONE);
        lo.push(sub(u, e));
        hi.push(add(u, e));
    }
    let mut order: Vec<usize> = (0..n).collect();
    sort_decreasing(&mut order, |i| lo[i]);
    let mut held = vec![false; n];
    let mut low = 16;
    for k in (0..n).rev() {
        let i = order[k];
        if done[i] {
            continue;
        }
        let rank = members[i].rank;
        if rank > low {
            held[i] = true;
            continue;
        }
        low = rank;
        held[i] = order[..k]
            .iter()
            .rev()
            .take_while(|&&j| real(lo[j]) < real(hi[i]))
            .any(|&j| !done[j] && members[j].rank < rank);
    }
    held
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(radius: f32, rank: u8) -> impl Fn(f32) -> Member {
        move |x| Member {
            position: [x.to_bits(), 384f32.to_bits()],
            radius: radius.to_bits(),
            rank,
        }
    }

    const XS: [f32; 4] = [379.3125, 382.4375, 385.5625, 388.6875];
    const ANGLE: u32 = 0x4094_4ED2;

    /// SPEC-0025, verification 1: four footmen in two rows of two.
    #[test]
    fn four_footmen() {
        let members: Vec<Member> = XS.iter().map(|&x| row(0.968_75, 0)(x)).collect();
        assert_eq!(
            offsets(&members, ANGLE),
            [
                [0xC046_76C1, 0xBFFD_EDF6],
                [0x4030_6646, 0xC01C_2D3F],
                [0xC030_6647, 0x401C_2D3F],
                [0x4046_76C1, 0x3FFD_EDF5],
            ]
        );
    }

    /// SPEC-0025, "Members that wait for their rank to pass": the mixed
    /// group side by side at the start of test RSP. The knight and the
    /// footman walk, the rifleman and the peasant are held; a member that
    /// has arrived no longer holds the others.
    #[test]
    fn holds_at_the_start() {
        let members = [
            row(0.5, 4)(XS[0]),
            row(1.0, 2)(XS[1]),
            row(1.0, 0)(XS[2]),
            row(0.968_75, 0)(XS[3]),
        ];
        assert_eq!(
            holds(&members, &[false; 4], ANGLE),
            [true, true, false, false]
        );
        // With the front rank arrived, the rifleman walks and the peasant,
        // ahead of it, still waits.
        assert_eq!(
            holds(&members, &[false, false, true, true], ANGLE),
            [true, false, false, false]
        );
    }

    /// SPEC-0025, verification 2: a peasant, a rifleman, a knight and a
    /// footman in three ranks.
    #[test]
    fn mixed_group() {
        let members = [
            row(0.5, 4)(XS[0]),
            row(1.0, 2)(XS[1]),
            row(1.0, 0)(XS[2]),
            row(0.968_75, 0)(XS[3]),
        ];
        assert_eq!(
            offsets(&members, ANGLE),
            [
                [0x3F08_BCA7, 0x40DB_55C2],
                [0x3DDA_C775, 0x3FAF_77D1],
                [0xC053_EE1D, 0xC078_4904],
                [0x402A_E8B8, 0xC08B_0F32],
            ]
        );
    }
}
