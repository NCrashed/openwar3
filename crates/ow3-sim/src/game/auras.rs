//! Auras (SPEC-0038, SPEC-0046): devotion, endurance and unholy aura of
//! heroes and of units that have them, and brilliance aura.
//!
//! At checks every 0.5 s each hero with an aura gives its buff to the
//! friendly ground units within `Area` of it, itself included. Buffs are
//! taken away only at checks every 2 s, at the second one after the last
//! check that found the unit in reach; a pass in the tick of a check comes
//! after it. A dead unit's aura passes no more (c-SBP-1).
//! Devotion adds `DataA` to the armor; endurance and unholy aura add
//! `DataA` to the movement speed as a share, from the unit's next update.
//!
//! PROVISIONAL: the phase of the checks (ticks 1 mod 100 and 202 mod 400)
//! is that of the test maps (SPEC-0038); attack speed and
//! regeneration are not done.

use std::collections::BTreeMap;

use ow3_math::{add, mul, sub};

use super::{Object, State};
use crate::data::is_hero;

/// The auras by their base codes.
const AURAS: [[u8; 4]; 4] = [*b"AHad", *b"AOae", *b"AUau", *b"AOac"];
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

/// The order an aura's pass gives its units: a call's grid order
/// (SPEC-0019) read backwards (rows from the top, cells from the right,
/// in a cell the unit that entered first).
fn pass_order(state: &State, units: Vec<u32>) -> Vec<u32> {
    let mut order = super::movement::call_order(state, units);
    order.reverse();
    order
}

/// The checks of this tick.
pub(super) fn tick(state: &mut State) {
    let tick = state.ticks;
    // The buffs gained: (the aura's setup order, unit, aura).
    let mut gains = brilliance(state);
    if tick % REMOVAL == REMOVAL_PHASE {
        // Not found in reach since the removal check before. The pass of the
        // tick before a check (q 7.005) comes after it (q 7.0001), so it
        // counts for the next one (c-SBL-1: the ogre lord, last found by the
        // taskmaster's pass of 7.005, lost its buff at 11.0).
        // Their objects go in the order they were made (c-SBP-1 at 9.0).
        let mut all: Vec<(u32, u32, i32)> = Vec::new();
        for &o in &state.units.clone() {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                let before: Vec<i32> = u.buffs.iter().map(|b| b.0).collect();
                u.buffs.retain(|&(a, _, seen)| {
                    seen + REMOVAL + 1 >= tick
                        || base_of(&state.data, a) == i32::from_be_bytes(BRILLIANCE)
                });
                let lost: Vec<i32> = before
                    .into_iter()
                    .filter(|a| !u.buffs.iter().any(|b| b.0 == *a))
                    .collect();
                for a in lost {
                    all.push((super::table::aura_buff_creation(state, o, a), o, a));
                }
            }
        }
        all.sort_unstable();
        for (_, o, a) in all {
            super::table::aura_buff_lost(state, o, a);
        }
    }
    // Each aura passes every 0.5 s from its setup, its making or its
    // learning (c-SBN-1: the Death Knight's Unholy Aura learned at the
    // level-up of 11.689039, first given at 12.189039).
    let passes = |u: &super::Unit| {
        u.aura_setup
            .map_or(tick % PERIOD == 1, |s| tick > s && (tick - s) % PERIOD == 0)
    };
    let passing = state
        .units
        .iter()
        .any(|&h| aura_of(state, h).is_some() && state.unit(Some(h)).is_some_and(passes));
    if !passing {
        buff_objects(state, gains);
        return;
    }
    // A learned aura's pass is a queue timer: the after-swing walks due
    // before it come first, also those at its expiry, their timers older
    // (c-SBN-1: the footman's backswing end at 12.189039, its walk back
    // before the pass of 12.189039, the aura learned at the kill).
    let first_q = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| passes(u))?;
            aura_of(state, h)?;
            let (setup, mut q) = (u.aura_setup?, u.aura_q?);
            for _ in 0..(tick - setup) / PERIOD {
                q = add(q, 0.5f32.to_bits());
            }
            Some(q)
        })
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)));
    if let Some(q) = first_q {
        // (One ulp past it.)
        super::attack::after_swing_walks(state, Some(q + 1));
    }
    // The 0.0001 s frees of the hits ahead of the tick's script timers
    // (`ObjectTable::mark_early`), before the step, go first (c-RZJ-1:
    // the hit of 22.99714 freed at 22.99724, the buff at 22.999998).
    state.table.flush_early(tick);
    let mut given: Vec<(u32, i32, i32, u64)> = Vec::new();
    for &h in &state.units {
        let Some((ability, level)) = aura_of(state, h) else {
            continue;
        };
        // (c-RZJ-1: the heroes' and the Ogre Lord's auras of tick 201,
        // first given at 1.505.)
        if !state.unit(Some(h)).is_some_and(passes) {
            continue;
        }
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
        let order = hero.aura_order;
        let mut near = Vec::new();
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
                near.push(o);
            }
        }
        // In the reverse of a call's grid order (SPEC-0038; c-RZJ-1: the
        // Ogre Lord's creeps 15, 13, 10, 14, 8, 9, 12 at 1.505).
        for o in pass_order(state, near) {
            given.push((o, ability, level, order));
        }
    }
    for (o, ability, level, order) in given {
        let fresh = state
            .unit(Some(o))
            .is_some_and(|u| !u.buffs.iter().any(|&(a, _, _)| a == ability));
        if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
            u.buffs.retain(|&(a, _, _)| a != ability);
            u.buffs.push((ability, level, tick));
        }
        if fresh {
            gains.push((order, o, ability));
        }
    }
    buff_objects(state, gains);
}

/// A new buff takes its object, aura by aura in the order of their setup
/// (SPEC-0024, SPEC-0038; c-RZJ-1 at 1.505: the Ogre Lord's 7,
/// Brilliance's 1, the Paladin's Devotion 7).
fn buff_objects(state: &mut State, mut gains: Vec<(u64, u32, i32)>) {
    gains.sort_by_key(|g| g.0);
    for (_, o, ability) in gains {
        super::table::aura_buff_gained(state, o, ability);
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

/// Brilliance Aura (SPEC-0046): pulses at the auras' steps every `PERIOD`
/// ticks from the first after the hero learned it; every fourth pulse first takes
/// the buff from the units no pulse found since the fourth before, then
/// each pulse gives it to the friendly units nearer the hero than `Area`
/// plus their collision. A buffed unit regenerates `DataA` more mana a
/// second.
///
/// PROVISIONAL: the buff goes in the tick of the pulse.
const BRILLIANCE: [u8; 4] = *b"AHab";
const PULSES_PER_REMOVAL: u64 = 4;

fn brilliance(state: &mut State) -> Vec<(u64, u32, i32)> {
    let tick = state.ticks;
    let code = i32::from_be_bytes(BRILLIANCE);
    let mut gains = Vec::new();
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
        // The first step of the auras' 0.5 s steps after the learning
        // (c-RZJ-1: learned in 201, first pulse at the step of 301; the
        // tests' learning 1 or 2 ticks before a step, SPEC-0046).
        let first = {
            let step = from - from % PERIOD + 1;
            if step > from { step } else { step + PERIOD }
        };
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
        let (owner, x, y, order) = (hero.owner, hero.x, hero.y, hero.aura_order);
        let hero_setup = hero.aura_setup;
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
        for o in pass_order(state, near) {
            note(state, o, &mut before);
            // Only a unit with mana takes the buff's object (c-RZJ-1: the
            // Paladin at 1.505, not the footmen).
            let fresh = state.unit(Some(o)).is_some_and(|u| {
                !u.buffs.iter().any(|&(a, _, _)| a == ability) && f32::from_bits(u.mana.max) > 0.0
            });
            if let Some(Object::Unit(u)) = state.objects.get_mut(&o) {
                u.buffs.retain(|&(a, _, _)| a != ability);
                u.buffs.push((ability, level, tick));
            }
            // Not in the tick of its setup (as the other auras).
            if fresh && hero_setup != Some(tick) {
                gains.push((order, o, ability));
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
    gains
}
