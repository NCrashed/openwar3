//! Auras (SPEC-0038, SPEC-0046): devotion, endurance and unholy aura of
//! heroes and of units that have them, and brilliance aura.
//!
//! At checks every 0.5 s each hero with an aura gives its buff to the
//! friendly ground units within `Area` of it, itself included. Buffs are
//! taken away only at checks every 2 s, at the second one after the last
//! check that found the unit in reach.
//! Devotion adds `DataA` to the armor; endurance and unholy aura add
//! `DataA` to the movement speed as a share, from the unit's next update.
//!
//! PROVISIONAL: the phase of the checks (ticks 1 mod 100 and 202 mod 400)
//! is that of the test maps (SPEC-0038); a walking hero leaving a
//! standing unit took one check more in two cases; attack speed and
//! regeneration are not done.

use std::collections::BTreeMap;

use ow3_math::{add, mul, sub};

use super::{Object, State};
use crate::data::is_hero;

/// The auras by their base codes.
const AURAS: [[u8; 4]; 3] = [*b"AHad", *b"AOae", *b"AUau"];
/// Ticks between the checks that give the buffs, and between those that
/// take them away (c-RYR: script ticks 402, 802, 1202, ...).
const PERIOD: u64 = 100;
const REMOVAL: u64 = 400;
/// Script tick 402 of the tests (3.0 s) is game tick 602.
const REMOVAL_PHASE: u64 = 202;

/// The base code of `ability`.
fn base(state: &State, ability: i32) -> i32 {
    state
        .data
        .abilities
        .get(&ability)
        .map_or(ability, |a| a.code)
}

/// The base code of `ability` in `data`.
fn base_of(data: &crate::data::ObjectData, ability: i32) -> i32 {
    data.abilities.get(&ability).map_or(ability, |a| a.code)
}

/// The aura of hero `h` with its level, if it learned one.
fn aura_of(state: &State, h: u32) -> Option<(i32, i32)> {
    let u = state.unit(Some(h)).filter(|u| u.alive)?;
    let is_aura = |a: i32| {
        AURAS
            .iter()
            .any(|c| i32::from_be_bytes(*c) == base(state, a))
    };
    if is_hero(u.type_id) {
        return u
            .skills
            .iter()
            .copied()
            .find(|&(a, level)| level > 0 && is_aura(a));
    }
    // A unit with an aura of its type, at level 1 (c-RZJ-1: the Ogre
    // Lord's ACav, a devotion aura).
    state
        .data
        .units
        .get(&u.type_id)?
        .abilities
        .iter()
        .copied()
        .find(|&a| is_aura(a))
        .map(|a| (a, 1))
}

/// The checks of this tick.
pub(super) fn tick(state: &mut State) {
    let tick = state.ticks;
    brilliance(state);
    if tick % REMOVAL == REMOVAL_PHASE {
        // Not found in reach since the removal check before.
        for &o in &state.units.clone() {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                u.buffs.retain(|&(a, _, seen)| {
                    seen + REMOVAL > tick
                        || base_of(&state.data, a) == i32::from_be_bytes(BRILLIANCE)
                });
            }
        }
    }
    if tick % PERIOD != 1 {
        return;
    }
    let mut given: Vec<(u32, i32, i32)> = Vec::new();
    for &h in &state.units {
        let Some((ability, level)) = aura_of(state, h) else {
            continue;
        };
        let Some(hero) = state.unit(Some(h)) else {
            continue;
        };
        let area = state
            .data
            .abilities
            .get(&ability)
            .and_then(|a| a.per_level.get((level - 1).max(0) as usize))
            .map_or(0.0, |n| f32::from_bits(n.area));
        let (owner, x, y) = (hero.owner, f32::from_bits(hero.x), f32::from_bits(hero.y));
        for &o in &state.units {
            let Some(u) = state.unit(Some(o)) else {
                continue;
            };
            let ground = state
                .data
                .units
                .get(&u.type_id)
                .is_some_and(|t| t.target_type.iter().any(|c| c == "ground"));
            let friend = u.owner == owner || state.setup.is_ally(owner, u.owner);
            let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
            if u.alive && !u.hidden && ground && friend && dx * dx + dy * dy <= area * area {
                given.push((o, ability, level));
            }
        }
    }
    for (o, ability, level) in given {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
            u.buffs.retain(|&(a, _, _)| a != ability);
            u.buffs.push((ability, level, tick));
        }
    }
}

/// The `DataA` of the buffs of `u` with an aura of base `code`, summed.
fn bonus(state: &State, u: &super::Unit, code: &[u8; 4]) -> f32 {
    u.buffs
        .iter()
        .filter(|&&(a, _, _)| base(state, a) == i32::from_be_bytes(*code))
        .filter_map(|&(a, level, _)| {
            state
                .data
                .abilities
                .get(&a)
                .and_then(|d| d.per_level.get((level - 1).max(0) as usize))
                .map(|n| f32::from_bits(n.data[0]))
        })
        .sum()
}

/// The armor `u` gets from auras (devotion).
pub(super) fn armor(state: &State, u: &super::Unit) -> u32 {
    bonus(state, u, b"AHad").to_bits()
}

/// The factor of `u`'s movement speed from auras (endurance, unholy).
pub(super) fn speed_factor(state: &State, u: &super::Unit) -> u32 {
    (1.0 + bonus(state, u, b"AOae") + bonus(state, u, b"AUau")).to_bits()
}

/// Whether `u` has the buff `buff` of an aura (for `GetUnitAbilityLevel`).
pub(super) fn has_buff(state: &State, u: &super::Unit, buff: i32) -> bool {
    u.buffs
        .iter()
        .any(|&(a, _, _)| state.data.abilities.get(&a).is_some_and(|d| d.buff == buff))
}

/// Brilliance Aura (SPEC-0046): pulses every `PERIOD` ticks from
/// `FIRST_PULSE` after the hero learned it; every fourth pulse first takes
/// the buff from the units no pulse found since the fourth before, then
/// each pulse gives it to the friendly units nearer the hero than `Area`
/// plus their collision. A buffed unit regenerates `DataA` more mana a
/// second.
///
/// PROVISIONAL: the first pulse is taken 2 ticks after the learning (1 or
/// 2 in the tests, SPEC-0046 open question 1); the buff goes in the tick
/// of the pulse.
const BRILLIANCE: [u8; 4] = *b"AHab";
const FIRST_PULSE: u64 = 2;
const PULSES_PER_REMOVAL: u64 = 4;

fn brilliance(state: &mut State) {
    let tick = state.ticks;
    let code = i32::from_be_bytes(BRILLIANCE);
    let heroes: Vec<(u32, i32, i32, u64)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            let from = u.aura_learned?;
            let (a, l) = u
                .skills
                .iter()
                .copied()
                .find(|&(a, l)| l > 0 && base(state, a) == code)?;
            Some((h, a, l, from))
        })
        .collect();
    let mut before: BTreeMap<u32, f32> = BTreeMap::new();
    let note = |state: &State, o: u32, before: &mut BTreeMap<u32, f32>| {
        if let Some(u) = state.unit(Some(o)) {
            before
                .entry(o)
                .or_insert_with(|| bonus(state, u, &BRILLIANCE));
        }
    };
    for (h, ability, level, from) in heroes {
        let first = from + FIRST_PULSE;
        if tick < first || (tick - first) % PERIOD != 0 {
            continue;
        }
        if ((tick - first) / PERIOD) % PULSES_PER_REMOVAL == 0 {
            let since = tick.saturating_sub(PERIOD * PULSES_PER_REMOVAL);
            for o in state.units.clone() {
                let stale = state.unit(Some(o)).is_some_and(|u| {
                    u.buffs
                        .iter()
                        .any(|&(a, _, seen)| a == ability && seen < since)
                });
                if stale {
                    note(state, o, &mut before);
                    if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                        u.buffs
                            .retain(|&(a, _, seen)| a != ability || seen >= since);
                    }
                }
            }
        }
        let Some(hero) = state.unit(Some(h)) else {
            continue;
        };
        let area = state
            .data
            .abilities
            .get(&ability)
            .and_then(|a| a.per_level.get((level - 1).max(0) as usize))
            .map_or(0, |n| n.area);
        let (owner, x, y) = (hero.owner, hero.x, hero.y);
        let near: Vec<u32> = state
            .units
            .iter()
            .copied()
            .filter(|&o| {
                let Some(u) = state.unit(Some(o)) else {
                    return false;
                };
                let collision = state.data.units.get(&u.type_id).map_or(0, |t| t.collision);
                let friend = u.owner == owner || state.setup.is_ally(owner, u.owner);
                let (dx, dy) = (sub(u.x, x), sub(u.y, y));
                let reach = add(area, collision);
                u.alive
                    && !u.hidden
                    && friend
                    && f32::from_bits(add(mul(dx, dx), mul(dy, dy)))
                        < f32::from_bits(mul(reach, reach))
            })
            .collect();
        for o in near {
            note(state, o, &mut before);
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                u.buffs.retain(|&(a, _, _)| a != ability);
                u.buffs.push((ability, level, tick));
            }
        }
    }
    // The units whose bonus changed go on at their new rate.
    for (o, old) in before {
        let Some(u) = state.unit(Some(o)) else {
            continue;
        };
        let new = bonus(state, u, &BRILLIANCE);
        if new != old {
            let rate = add(sub(u.mana.rate, old.to_bits()), new.to_bits());
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                u.mana.retarget(rate);
            }
        }
    }
}

/// The mana a second `u` regenerates more from auras (brilliance).
pub(super) fn mana_bonus(state: &State, u: &super::Unit) -> u32 {
    bonus(state, u, &BRILLIANCE).to_bits()
}
