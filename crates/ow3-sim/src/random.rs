//! The script random generator of SPEC-0007: `SetRandomSeed`,
//! `GetRandomInt` and `GetRandomReal`.

use std::sync::OnceLock;

use ow3_formats::replay::bh;
use ow3_math::{add, mul, sub};

/// The 61-entry table of SPEC-0007, read from `SPEC-0007-rng-table.tsv`.
pub fn table() -> &'static [u32; 61] {
    static TABLE: OnceLock<[u32; 61]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let source = include_str!("../../../specs/SPEC-0007-rng-table.tsv");
        let mut table = [0u32; 61];
        let mut count = 0;
        for line in source
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (index, value) = line.split_once('\t').expect("index and value");
            let index: usize = index.parse().expect("index");
            table[index] = u32::from_str_radix(value.trim(), 16).expect("hex value");
            count += 1;
        }
        assert_eq!(count, 61, "SPEC-0007 table has 61 entries");
        table
    })
}

/// Offsets move down by these strides and wrap by these amounts.
const STEPS: [(i32, i32); 4] = [(4, 188), (12, 212), (24, 236), (28, 244)];
/// 2e-7: a range of reals narrower than this gives its low bound.
const MIN_REAL_RANGE: u32 = 0x3456_BF95;
const ONE: u32 = 0x3F80_0000;
const MINUS_ONE: u32 = 0xBF80_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Random {
    a: u32,
    /// Four byte offsets into the table, `o0` in the top byte.
    b: u32,
}

/// Game generators that `SetRandomSeed` reseeds besides the script's.
pub const GAME_GENERATORS: usize = 45;
/// The game generator that rolls the damage dice of attacks (SPEC-0016).
pub const ATTACK_DICE: usize = 2;
/// The game generator that draws the reaction delay of attack orders
/// (SPEC-0019).
pub const REACTION: usize = 9;

impl Random {
    /// The seeding formula, without the step that `seeded` discards.
    fn unstepped(seed: u32) -> Random {
        let s = seed;
        let b = ((s % 59) << 10)
            | ((s % 61) << 2)
            | ((s % 53) << 18)
            | ((s / 47).wrapping_mul(17).wrapping_add(s) << 26);
        Random { a: s, b }
    }

    /// The state after `SetRandomSeed(seed)`.
    pub fn seeded(seed: u32) -> Random {
        Random::reseed(seed).0
    }

    /// The script generator and the game generators after
    /// `SetRandomSeed(seed)` (SPEC-0007, "Game generators"): the output of
    /// the script generator's first step seeds a helper whose next 45
    /// outputs seed the game generators.
    pub fn reseed(seed: u32) -> (Random, Vec<Random>) {
        let mut script = Random::unstepped(seed);
        let r1 = script.step();
        let mut helper = Random::unstepped(r1);
        let game = (0..GAME_GENERATORS)
            .map(|_| Random::unstepped(helper.step()))
            .collect();
        (script, game)
    }

    /// One step of the generator; returns its output.
    pub fn step(&mut self) -> u32 {
        let table = table();
        let mut offsets = self.b.to_be_bytes().map(i32::from);
        for (offset, &(stride, wrap)) in offsets.iter_mut().zip(&STEPS) {
            *offset -= stride;
            if *offset < 0 {
                *offset += wrap;
            }
        }
        let [o0, o1, o2, o3] = offsets.map(|o| o as u32);
        self.b = (o0 << 24) | (o1 << 16) | (o2 << 8) | o3;
        let w = |o: u32| table[(o / 4) as usize];
        let mix = w(o2).rotate_left(3) ^ w(o1).rotate_left(2) ^ w(o3) ^ w(o0).rotate_left(1);
        self.a = self.a.wrapping_add(mix);
        self.a
    }

    /// `GetRandomInt(lo, hi)`. For `hi < lo` the result lies above `lo`.
    pub fn integer(&mut self, lo: i32, hi: i32) -> i32 {
        if lo == hi {
            return lo;
        }
        let d = (i64::from(hi) - i64::from(lo)).unsigned_abs() as u32;
        let r = u64::from(self.step());
        let span = u64::from(d.wrapping_add(1));
        lo.wrapping_add(((r * span) >> 32) as i32)
    }

    /// `GetRandomReal(lo, hi)` with the operations of SPEC-0004.
    pub fn real(&mut self, lo: u32, hi: u32) -> u32 {
        let d = sub(hi, lo) & 0x7FFF_FFFF;
        if f32::from_bits(d) < f32::from_bits(MIN_REAL_RANGE) {
            return lo;
        }
        add(mul(self.fraction(), d), lo)
    }

    /// What the generator's state adds to the turn checksum (SPEC-0024):
    /// `bh(a) + bh(b)`.
    pub fn checksum(&self) -> u32 {
        bh(self.a).wrapping_add(bh(self.b))
    }

    /// A real in `0..1` from the low 23 bits of one output, as
    /// `GetRandomReal` takes it.
    pub fn fraction(&mut self) -> u32 {
        let r = self.step();
        add((r & 0x007F_FFFF) | ONE, MINUS_ONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0019: with seed 7 the reaction delay of the first attack order
    /// is 0.0015 s.
    #[test]
    fn reaction_delay() {
        let delay = |seed| {
            let (_, mut game) = Random::reseed(seed);
            f32::from_bits(mul(game[REACTION].fraction(), 0.25f32.to_bits()))
        };
        assert!((delay(7) - 0.0015).abs() < 0.00005, "{}", delay(7));
    }

    /// Every line of `SPEC-0007-vectors.tsv`: the outputs that follow
    /// `SetRandomSeed(seed)`.
    #[test]
    fn reproduces_the_measured_outputs() {
        let source = include_str!("../../../specs/SPEC-0007-vectors.tsv");
        let mut seeds = 0;
        for line in source
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let mut fields = line.split_whitespace();
            let seed: i32 = fields.next().unwrap().parse().unwrap();
            let mut random = Random::seeded(seed as u32);
            for (i, expected) in fields.enumerate() {
                let expected = u32::from_str_radix(expected, 16).unwrap();
                assert_eq!(random.step(), expected, "seed {seed}, output {i}");
            }
            seeds += 1;
        }
        assert_eq!(seeds, 89);
    }

    /// The examples of SPEC-0007 with `SetRandomSeed(777)`.
    #[test]
    fn spec_examples() {
        let mut random = Random::seeded(777);
        let values: Vec<i32> = (0..6).map(|_| random.integer(6, 1)).collect();
        assert_eq!(values, [7, 10, 8, 7, 11, 7]);
        let mut random = Random::seeded(777);
        let values: Vec<i32> = (0..6).map(|_| random.integer(1, 6)).collect();
        assert_eq!(values, [2, 5, 3, 2, 6, 2]);
        let mut random = Random::seeded(777);
        let texts: Vec<String> = (0..6)
            .map(|_| ow3_math::r2sw(random.real(2.5f32.to_bits(), 7.25f32.to_bits()), 1, 9))
            .collect();
        assert_eq!(
            texts,
            [
                "4.846750208",
                "5.005834102",
                "3.415878752",
                "5.382181632",
                "4.171306608",
                "2.869395968"
            ]
        );
    }

    #[test]
    fn equal_bounds_do_not_step() {
        let mut a = Random::seeded(5);
        let mut b = Random::seeded(5);
        assert_eq!(a.integer(3, 3), 3);
        assert_eq!(a.real(ONE, ONE), ONE);
        assert_eq!(a.step(), b.step());
        assert_eq!(a.integer(i32::MIN, i32::MAX), i32::MIN);
    }
}
