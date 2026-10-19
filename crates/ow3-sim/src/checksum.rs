//! The turn checksum of SPEC-0024, which a replay records after every
//! time slot (block 0x22).
//!
//! The simulation does not keep the engine's object table, so the parts
//! that come from it (`C`, `D` and the ids in `G`) are not reproduced.
//! [`turns`] gives the parts that follow from the replay alone, and
//! [`Turn::remainder`] the rest of each recorded checksum, which changes
//! only at turns where the game's state changed (SPEC-0024, verification
//! 4).

use ow3_formats::replay::{Event, Replay, fold};

use crate::random::Random;

/// The build number of 1.27b, the first value of the checksum.
pub const BUILD: u32 = 6059;

/// The static values of the system sum: `S7` (slots 19 down to 7
/// folded), `c5`, `c4` and the cheat flags `c0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statics {
    pub s7: u32,
    pub c5: u32,
    pub c4: u32,
    pub c0: u32,
}

/// The static values measured on the test maps made from `(12)IceCrown`
/// without changed object data, and no cheats.
pub const ICECROWN_TEST: Statics = Statics {
    s7: 0xEA73_D958,
    c5: 0xA996_F046,
    c4: 0x414C_0D79,
    c0: 0,
};

/// The changing parts of a turn's checksum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    /// The game time when the checksum is taken, `25 * floor(t / 25)` ms.
    pub b: u32,
    /// The network sum.
    pub n: u32,
    /// The next entry and the next creation number of the object table.
    pub c: u32,
    pub d: u32,
    /// The script generator's sum.
    pub e: u32,
    /// The game generators' sum.
    pub r: u32,
    /// The game sum.
    pub g: u32,
}

/// The turn checksum of `parts` with the static values `statics`.
pub fn turn_checksum(statics: &Statics, parts: &Parts) -> u32 {
    let mut s = fold(statics.s7, parts.g);
    s = fold(s, statics.c5);
    s = fold(s, statics.c4);
    s = fold(s, parts.r);
    s = fold(s, parts.n);
    s = fold(s, statics.c0);
    let mut v = (s ^ BUILD).rotate_left(3);
    v = fold(v, parts.b);
    v = fold(v, parts.c);
    v = fold(v, parts.d);
    fold(v, parts.e)
}

/// `R`: the sum of the game generators.
pub fn generators_sum(generators: &[Random]) -> u32 {
    generators
        .iter()
        .map(Random::checksum)
        .fold(0, u32::wrapping_add)
}

/// `B` for a checksum taken after `ms` of game time.
pub fn time_part(ms: u64) -> u32 {
    (ms / 25 * 25) as u32
}

/// A turn of a replay: its time slot and the checksum recorded after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    /// The game time before the turn's slot (`t_(k-1)`) and after it, ms.
    pub before: u64,
    pub after: u64,
    /// The network sum including the turn's slot.
    pub n: u32,
    /// The recorded checksum.
    pub recorded: u32,
}

impl Turn {
    /// The recorded checksum without the parts the replay gives (`B` and
    /// `N`): a value made of the other parts and the static values only,
    /// equal between turns in which they did not change.
    pub fn remainder(&self) -> u32 {
        let known = Parts {
            b: time_part(self.before),
            n: self.n,
            ..Parts::default()
        };
        self.recorded ^ turn_checksum(&Statics::default(), &known)
    }
}

/// The turns of `replay`: every time slot followed by a checksum.
///
/// The checksum after a slot of 0 ms (the slots that carry the player's
/// actions in single player) repeats the one before, so such a turn takes
/// the time and network sum of the turn before; its block still adds to
/// the network sum of the turns after it (seen in `c-RSL-2.w3g`).
///
/// PROVISIONAL: whether the repeat goes with the slot's length of 0 or
/// with its actions is not known; in the replays they always go together.
pub fn turns(replay: &Replay) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut n = 0u32;
    let mut ms = 0;
    let mut slot = None;
    for event in &replay.events {
        match event {
            Event::TimeSlot {
                ms: step, network, ..
            } => {
                n = n.wrapping_add(*network);
                slot = Some((ms, ms + u64::from(*step), *step == 0));
                ms += u64::from(*step);
            }
            Event::Checksum(recorded) => {
                if let Some((before, after, repeat)) = slot.take() {
                    let turn = match turns.last() {
                        Some(last) if repeat => Turn {
                            after,
                            recorded: *recorded,
                            ..*last
                        },
                        _ => Turn {
                            before,
                            after,
                            n,
                            recorded: *recorded,
                        },
                    };
                    turns.push(turn);
                }
            }
            _ => {}
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> u32 {
        u32::from_str_radix(s, 16).unwrap()
    }

    /// The rows of `SPEC-0024-vectors.tsv`: turn, `t`, then the parts and
    /// the checksum.
    fn vectors() -> Vec<(u64, u64, Parts, u32)> {
        include_str!("../../../specs/SPEC-0024-vectors.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .map(|l| {
                let w: Vec<&str> = l.split('\t').collect();
                let parts = Parts {
                    b: w[2].parse().unwrap(),
                    n: hex(w[3]),
                    c: w[4].parse().unwrap(),
                    d: w[5].parse().unwrap(),
                    e: hex(w[6]),
                    r: hex(w[7]),
                    g: hex(w[8]),
                };
                (
                    w[0].parse().unwrap(),
                    w[1].parse().unwrap(),
                    parts,
                    hex(w[9]),
                )
            })
            .collect()
    }

    /// SPEC-0024, verification 3.
    #[test]
    fn checksums_of_the_vectors() {
        let rows = vectors();
        assert_eq!(rows.len(), 195);
        for (turn, _, parts, checksum) in rows {
            assert_eq!(
                turn_checksum(&ICECROWN_TEST, &parts),
                checksum,
                "turn {turn}"
            );
        }
    }

    /// `B` is the time of the previous turn's end rounded down to 25 ms.
    #[test]
    fn time_of_the_vectors() {
        let rows = vectors();
        for pair in rows.windows(2) {
            let (turn, _, parts, _) = pair[1];
            assert_eq!(parts.b, time_part(pair[0].1), "turn {turn}");
        }
    }
}
