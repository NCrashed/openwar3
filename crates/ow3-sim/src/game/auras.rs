//! Hero auras (SPEC-0038): devotion, endurance and unholy aura.
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

/// The aura of hero `h` with its level, if it learned one.
fn aura_of(state: &State, h: u32) -> Option<(i32, i32)> {
    let u = state
        .unit(Some(h))
        .filter(|u| u.alive && is_hero(u.type_id))?;
    u.skills.iter().copied().find(|&(a, level)| {
        level > 0
            && AURAS
                .iter()
                .any(|c| i32::from_be_bytes(*c) == base(state, a))
    })
}

/// The checks of this tick.
pub(super) fn tick(state: &mut State) {
    let tick = state.ticks;
    if tick % REMOVAL == REMOVAL_PHASE {
        // Not found in reach since the removal check before.
        for &o in &state.units.clone() {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                u.buffs.retain(|&(_, _, seen)| seen + REMOVAL > tick);
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
