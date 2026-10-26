//! Casting abilities (SPEC-0035, SPEC-0037), and stuns.
//!
//! An order of the ability's order string on a target in range (`Rng`
//! of the ability's level plus both collision sizes, the range test of
//! SPEC-0019) starts the cast at once; out of range the caster first
//! walks into range (`game::movement`, `Order::Cast`). An ability without
//! a target starts at once. The effect comes `castpt` after the start:
//! the mana is paid then and the cooldown starts there. The order stays
//! `castbsw` after the effect, then ends. An order while the cooldown
//! runs, or without the mana, does nothing.
//!
//! The effects: Holy Light heals a friendly unit by `DataA`; storm bolt
//! launches a homing missile that does `DataA` of spell damage and stuns
//! for `Dur` (`HeroDur` for heroes); thunder clap does `DataA` to the
//! enemy ground units within `Area` and slows their attacks by `DataD`
//! for `Dur`. A stunned unit stands, does not attack and its order reads
//! 851973; its order comes back 0.01 s after the end of its duration.
//!
//! PROVISIONAL: turning before a cast, the reaction delay and what
//! interrupts a cast are open (SPEC-0035); the slow of movement by
//! thunder clap is not measured and not done.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;
use ow3_math::natives::{cos, sin};
use ow3_math::{add, div, mul, sub};

use super::movement::{self, CELL};
use super::{Object, State};
use crate::data::is_hero;

/// What an ability is cast on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aim {
    Friend,
    Enemy,
    Nothing,
    /// A point on the ground (SPEC-0043).
    Point,
}

/// The abilities that can be cast, by their order string: the base
/// ability, the order id and what it is cast on.
const ABILITY_ORDERS: [(&str, [u8; 4], i32, Aim); 29] = [
    ("blizzard", *b"AHbz", 852_089, Aim::Point),
    ("waterelemental", *b"AHwe", 852_097, Aim::Nothing),
    ("divineshield", *b"AHds", 852_090, Aim::Nothing),
    ("shockwave", *b"AOsh", 852_125, Aim::Enemy),
    ("holybolt", *b"AHhb", 852_092, Aim::Friend),
    ("thunderbolt", *b"AHtb", 852_095, Aim::Enemy),
    ("thunderclap", *b"AHtc", 852_096, Aim::Nothing),
    ("windwalk", *b"AOwk", 852_129, Aim::Nothing),
    ("mirrorimage", *b"AOmi", 852_123, Aim::Nothing),
    ("stomp", *b"AOws", 852_127, Aim::Nothing),
    ("chainlightning", *b"AOcl", 852_119, Aim::Enemy),
    ("healingwave", *b"AOhw", 852_501, Aim::Friend),
    ("frostnova", *b"AUfn", 852_226, Aim::Enemy),
    ("manaburn", *b"AEmb", 852_179, Aim::Enemy),
    ("deathcoil", *b"AUdc", 852_100, Aim::Enemy),
    ("entanglingroots", *b"AEer", 852_171, Aim::Enemy),
    ("flamestrike", *b"AHfs", 852_488, Aim::Point),
    ("impale", *b"AUim", 852_555, Aim::Point),
    ("fanofknives", *b"AEfk", 852_526, Aim::Nothing),
    ("carrionswarm", *b"AUcs", 852_218, Aim::Point),
    ("shadowstrike", *b"AEsh", 852_527, Aim::Enemy),
    ("earthquake", *b"AOeq", 852_121, Aim::Point),
    ("whirlwind", *b"AOww", 852_128, Aim::Nothing),
    ("avatar", *b"AHav", 852_086, Aim::Nothing),
    ("resurrection", *b"AHre", 852_094, Aim::Nothing),
    ("animatedead", *b"AUan", 852_217, Aim::Nothing),
    ("deathanddecay", *b"AUdd", 852_221, Aim::Point),
    ("starfall", *b"AEsf", 852_183, Aim::Nothing),
    ("tranquility", *b"AEtq", 852_184, Aim::Nothing),
];

/// The time from the end of a stun to its order coming back (0.01 s).
const STUN_BACK: u32 = 0x3C23_D70A;

/// Ticks a summon lives over its duration (SPEC-0044).
const SUMMON_OVER: u64 = 6;

/// The order id a stunned unit reads (SPEC-0037).
pub const ORDER_STUNNED: i32 = 851_973;

/// A cast in progress, or the ability a unit walks to cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Casting {
    pub(super) order_id: i32,
    ability: i32,
    level: i32,
    target: Option<u32>,
    /// The ticks of the effect and of the end of the order; `None` while
    /// the caster walks into range.
    effect_at: Option<u64>,
    end_at: u64,
    /// The queue time of the effect: the start's time plus the cast point.
    effect_q: u32,
    /// The point of a point spell.
    pub(super) point: Option<[u32; 2]>,
}

/// The ability of unit `h` that order `name` casts: the ability, its
/// level, the order id and what it is cast on.
fn ability_for(state: &State, h: u32, name: &str) -> Option<(i32, i32, i32, Aim)> {
    let &(_, base, order_id, aim) = ABILITY_ORDERS
        .iter()
        .find(|(n, _, _, _)| n.eq_ignore_ascii_case(name))?;
    let base = i32::from_be_bytes(base);
    let u = state.unit(Some(h))?;
    let data = state.data.units.get(&u.type_id)?;
    data.abilities
        .iter()
        .chain(&data.hero_abilities)
        .copied()
        .filter(|&a| a == base || state.data.abilities.get(&a).is_some_and(|d| d.code == base))
        .map(|a| (a, super::heroes::ability_level(state, u, a)))
        .find(|&(_, level)| level > 0)
        .map(|(a, level)| (a, level, order_id, aim))
}

/// The numbers of level `level` of `ability`.
fn numbers(state: &State, ability: i32, level: i32) -> crate::data::AbilityLevel {
    state
        .data
        .abilities
        .get(&ability)
        .and_then(|a| a.per_level.get((level - 1).max(0) as usize))
        .copied()
        .unwrap_or_default()
}

/// The reach in cells of `h`'s cast on `target`: the ability's range plus
/// both collision sizes.
pub(super) fn reach(state: &State, h: u32, target: u32) -> Option<u32> {
    let c = state.unit(Some(h))?.casting?;
    let range = numbers(state, c.ability, c.level).range;
    let collision = |x: u32| {
        state
            .unit(Some(x))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(0, |t| t.collision)
    };
    Some(mul(add(add(range, collision(h)), collision(target)), CELL))
}

/// Whether `h` may cast `ability` of `level` now: not cooling down, with
/// the mana, not stunned.
fn ready(state: &State, h: u32, ability: i32, level: i32) -> bool {
    let tick = state.ticks;
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let cooling = u.ready.iter().any(|&(a, at)| a == ability && at > tick);
    let mana = f32::from_bits(u.mana.value());
    !cooling && mana >= numbers(state, ability, level).cost as f32 && !stunned(state, h)
}

/// Order `name` of unit `h` on `target`: `None` when it is not the order
/// of an ability, else whether the unit took it.
pub(super) fn order(state: &mut State, h: u32, name: &str, target: u32) -> Option<bool> {
    let known = ABILITY_ORDERS
        .iter()
        .any(|(n, _, _, _)| n.eq_ignore_ascii_case(name));
    let Some((ability, level, order_id, aim)) = ability_for(state, h, name) else {
        return known.then_some(false);
    };
    // The same spell again before the effect of its cast is taken and
    // changes nothing (as for a point spell, SPEC-0043; c-RZJ-1: the
    // Paladin's Holy Light ordered again at 2001, effect at 2033).
    // PROVISIONAL: measured for Blizzard; whatever the new target.
    if state
        .unit(Some(h))
        .and_then(|u| u.casting)
        .is_some_and(|c| c.ability == ability && c.effect_at.is_none_or(|e| e > state.ticks))
    {
        super::table::spell_ordered_again(state);
        return Some(true);
    }
    let owner = state.unit(Some(h))?.owner;
    let fits = state.unit(Some(target)).is_some_and(|t| {
        t.alive
            && match aim {
                Aim::Friend => !movement::enemies(state, owner, t.owner),
                Aim::Enemy => movement::enemies(state, owner, t.owner),
                Aim::Nothing | Aim::Point => false,
            }
    });
    if !fits || !ready(state, h, ability, level) {
        return Some(false);
    }
    end_wind_walk(state, h, ability);
    end_hold(state, h);
    let casting = Casting {
        order_id,
        ability,
        level,
        target: Some(target),
        effect_at: None,
        end_at: 0,
        effect_q: 0,
        point: None,
    };
    let set = |state: &mut State| {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.casting = Some(casting);
        }
    };
    set(state);
    let reach = reach(state, h, target).unwrap_or(0);
    let (here, there) = (
        movement::position_of(state, h)?,
        movement::position_of(state, target)?,
    );
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let tick = state.ticks;
    let in_range = movement::in_range(
        movement::relative(here, min),
        movement::relative(there, min),
        reach,
    );
    // In range and turned within 0.2 to the target, the cast starts at once
    // (SPEC-0035, verification 2); else the unit first stops and turns to
    // it, and the cast starts when turned (c-RZH-1: the Storm Bolt).
    if in_range && movement::turned_to(state, h, there) {
        movement::stop(state, h);
        set(state);
        state.table.cast_at_once = true;
        super::table::spell_ordered(state, h, false);
        start(state, h, tick);
    } else if in_range {
        // A walker stops where it is (c-RZJ-1: the Paladin's Holy Light
        // at 7802, a stop commit in that tick).
        movement::freeze_now(state, h);
        movement::stop(state, h);
        set(state);
        if !movement::cast_order(state, h, target, order_id) {
            return Some(false);
        }
        set(state);
        // Turning to the target is a new goal with a walk (c-SAK-1: 2501).
        super::table::spell_ordered(state, h, true);
    } else {
        // Out of range a walker also stops at the order and walks on from
        // its next update (c-SAK-1: the Mountain King's Storm Bolt at 500,
        // standing 502 to 505).
        movement::freeze_now(state, h);
        if movement::cast_order(state, h, target, order_id) {
            set(state);
            super::table::spell_ordered(state, h, true);
        } else {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.casting = None;
            }
            return Some(false);
        }
    }
    Some(true)
}

/// Order `name` of unit `h` without a target: `None` when it is not the
/// order of such an ability, else whether the unit took it.
pub(super) fn immediate_order(state: &mut State, h: u32, name: &str) -> Option<bool> {
    let (ability, level, order_id, aim) = ability_for(state, h, name)?;
    if aim != Aim::Nothing || !ready(state, h, ability, level) {
        return Some(false);
    }
    // The objects of a cast ordered without a target come before the end
    // of a Wind Walk (c-SAK-1: the Blademaster's Mirror Image at 7101).
    // PROVISIONAL: read for Mirror Image only.
    let code = base_code(state, ability);
    if code != i32::from_be_bytes(*b"AHds") && code != i32::from_be_bytes(*b"AOwk") {
        super::table::immediate_ordered(state, h);
    }
    end_wind_walk(state, h, ability);
    // Divine Shield takes effect in the tick of the order, with no cast
    // time, and the caster is left with no order (SPEC-0045; c-RZJ-1:
    // the Paladin at 4801).
    if base_code(state, ability) == i32::from_be_bytes(*b"AHds") {
        let order = super::table::divine_shield_ordered(state, h);
        movement::stop(state, h);
        let c = Casting {
            order_id,
            ability,
            level,
            target: None,
            effect_at: None,
            end_at: 0,
            effect_q: 0,
            point: None,
        };
        let mut deferred = Vec::new();
        effect(state, h, &c, &mut deferred);
        // No target check of his runs until his next order (SPEC-0045, "The
        // Paladin's own target checks"; c-RZJ-1: none from 4802 to 4901).
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.attack_move = None;
            u.checks_off = true;
        }
        // The order goes after the effect and the attackers' objects.
        state.table.defer(order, "script immediate order");
        return Some(true);
    }
    // Wind Walk too, ending the order the unit had (SPEC-0040; c-SAK-1:
    // the Blademaster's attack-move ended at 400).
    // PROVISIONAL: refused while it lasts (c-SAK-1: the script's orders
    // between 400 and 4400 were not accepted, the next at 4501 was).
    let lasts = state
        .unit(Some(h))
        .and_then(|u| u.wind_walk)
        .is_some_and(|w| w.on(state.ticks));
    if base_code(state, ability) == i32::from_be_bytes(*b"AOwk") && lasts {
        return Some(false);
    }
    if base_code(state, ability) == i32::from_be_bytes(*b"AOwk") {
        // A walker stops with a commit (c-SAK-1: the Blademaster at 4501).
        movement::freeze_now(state, h);
        movement::stop(state, h);
        let c = Casting {
            order_id,
            ability,
            level,
            target: None,
            effect_at: None,
            end_at: 0,
            effect_q: 0,
            point: None,
        };
        let mut deferred = Vec::new();
        effect(state, h, &c, &mut deferred);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.attack_move = None;
        }
        return Some(true);
    }
    movement::stop(state, h);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.casting = Some(Casting {
            order_id,
            ability,
            level,
            target: None,
            effect_at: None,
            end_at: 0,
            effect_q: 0,
            point: None,
        });
    }
    let tick = state.ticks;
    start(state, h, tick);
    Some(true)
}

/// Order `name` of unit `h` on the point `at`: `None` when it is not the
/// order of a point spell, else whether the unit took it. In range and
/// turned within 0.2 to the point the cast starts at once; else the unit
/// walks into range and turns to it (SPEC-0043, as SPEC-0035 for a unit).
///
/// PROVISIONAL: the reach of a point spell is taken as its range (c-RZJ-1:
/// the Archmage 826 from the point walked on at 517).
pub(super) fn point_order(state: &mut State, h: u32, name: &str, at: [u32; 2]) -> Option<bool> {
    let (ability, level, order_id, aim) = ability_for(state, h, name)?;
    if aim != Aim::Point || !ready(state, h, ability, level) {
        return Some(false);
    }
    end_wind_walk(state, h, ability);
    // The same spell again while its cast runs to the effect is taken and
    // changes nothing, whatever its point; during the channel it is
    // refused (SPEC-0043, edge cases; c-RZJ-1: blizzard ordered again at
    // 600).
    if let Some(c) = state
        .unit(Some(h))
        .and_then(|u| u.casting)
        .filter(|c| c.ability == ability)
        && let Some(e) = c.effect_at
    {
        if e > state.ticks {
            // It takes one object, kept (c-RZJ-1: the Blizzard ordered
            // again at 3.0).
            super::table::spell_ordered_again(state);
            return Some(true);
        }
        if state.ticks < c.end_at {
            return Some(false);
        }
    }
    end_hold(state, h);
    let casting = Casting {
        order_id,
        ability,
        level,
        target: None,
        effect_at: None,
        end_at: 0,
        effect_q: 0,
        point: Some(at),
    };
    let set = |state: &mut State| {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.casting = Some(casting);
        }
    };
    let here = movement::position_of(state, h)?;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let in_range = movement::in_range(
        movement::relative(here, min),
        movement::relative(at, min),
        mul(numbers(state, ability, level).range, CELL),
    );
    let tick = state.ticks;
    if in_range && movement::turned_to(state, h, at) {
        movement::stop(state, h);
        set(state);
        // Its objects before the cast's start (SPEC-0024).
        super::table::spell_point_ordered(state, h, true);
        start(state, h, tick);
        flame_mover(state, h, ability);
        return Some(true);
    } else {
        if in_range {
            movement::stop(state, h);
        }
        if !movement::cast_point_order(state, h, order_id, at) {
            return Some(false);
        }
        set(state);
    }
    // Its objects (SPEC-0024; c-RZJ-1: the Blizzard order of 2.5).
    super::table::spell_point_ordered(state, h, false);
    flame_mover(state, h, ability);
    Some(true)
}

/// A Flame Strike's order makes a point mover 0.01 s after it, aimed at
/// the point to arrive in the caster's cast point; its five objects go
/// 5.0 s after the arrival (c-SBH-1: 2.51, freed at 7.81).
/// PROVISIONAL: what the mover is for is not read.
fn flame_mover(state: &mut State, h: u32, ability: i32) {
    if base_code(state, ability) != i32::from_be_bytes(*b"AHfs") {
        return;
    }
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let owner = u.owner;
    let point = state.data.units.get(&u.type_id).map_or(0, |t| t.cast_point);
    let q = add(
        state.event_time.unwrap_or_else(|| state.timers.time()),
        0.01f32.to_bits(),
    );
    let at = tick_of(state, q);
    state.bounces.push(Bounce {
        caster: h,
        owner,
        kind: BounceKind::Mover,
        target: h,
        at,
        amount: 0,
        factor: point,
        left: 1,
        area: 0.0,
        hit: Vec::new(),
        point: [0, 0],
        q,
        times: [0; 3],
    });
}

/// An order of a spell on a unit or a point ends a hold position, one
/// without a target keeps it (c-SBJ-1: the Crypt Lord after Impale runs
/// from his attackers; c-SBK-1: the Warden after Fan of Knives does not).
fn end_hold(state: &mut State, h: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.holding = false;
    }
}

/// The reach in cells of `h`'s cast on a point (`point_order`).
pub(super) fn point_reach(state: &State, h: u32) -> u32 {
    let Some(u) = state.unit(Some(h)) else {
        return 0;
    };
    let range = u
        .casting
        .map_or(0, |c| numbers(state, c.ability, c.level).range);
    mul(range, CELL)
}

/// Blizzard (SPEC-0043): wave `k` of `DataA` falls `180 + 200 * (k - 1)`
/// ticks after the effect; the channel ends 21 ticks after the last one.
const FIRST_WAVE: u64 = 180;
const WAVE_EVERY: u64 = 200;
const CHANNEL_AFTER: u64 = 21;

/// The waves of a channeled point spell of `h`, `since` ticks after its
/// effect: every unit nearer the point than `Area` plus its collision
/// takes `min(DataB, DataF / n)` of the `n` units hit, a building
/// `DataD` of it (SPEC-0043).
///
/// PROVISIONAL: paused units take every wave; whether buildings count in
/// `n` and whether the caster is hit are open.
fn waves(s: &State, h: u32, c: &Casting, since: u64, deferred: &mut Vec<Deferred>) {
    let base = s
        .data
        .abilities
        .get(&c.ability)
        .map_or(c.ability, |a| a.code);
    if &base.to_be_bytes() != b"AHbz" {
        return;
    }
    let n = numbers(s, c.ability, c.level);
    let count = f32::from_bits(n.data[0]) as u64;
    if since < FIRST_WAVE || (since - FIRST_WAVE) % WAVE_EVERY != 0 {
        return;
    }
    if (since - FIRST_WAVE) / WAVE_EVERY >= count {
        return;
    }
    let Some(point) = c.point else {
        return;
    };
    let hit: Vec<(u32, bool)> = s
        .units
        .iter()
        .filter_map(|&o| {
            let Some(Object::Unit(u)) = s.objects.get(&o) else {
                return None;
            };
            let t = s.data.units.get(&u.type_id)?;
            if !u.alive || u.hidden {
                return None;
            }
            let (dx, dy) = (sub(u.x, point[0]), sub(u.y, point[1]));
            let reach = add(n.area, t.collision);
            let near =
                f32::from_bits(add(mul(dx, dx), mul(dy, dy))) < f32::from_bits(mul(reach, reach));
            near.then_some((o, t.building))
        })
        .collect();
    if hit.is_empty() {
        return;
    }
    // In the grid order of the pass (SPEC-0019; c-RZJ-1: the wave of 15.7,
    // footman 5 hit third).
    let order = movement::call_order(s, hit.iter().map(|h| h.0).collect());
    let hit: Vec<(u32, bool)> = order
        .into_iter()
        .filter_map(|o| hit.iter().find(|h| h.0 == o).copied())
        .collect();
    let order_hit: Vec<u32> = hit.iter().map(|h| h.0).collect();
    let share = div(n.data_f, ow3_math::i2r(hit.len() as i32));
    let each = if f32::from_bits(share) < f32::from_bits(n.data[1]) {
        share
    } else {
        n.data[1]
    };
    // The wave's queue time, the effect's plus its ticks (c-RZJ-1:
    // 4.139997, 180 ticks after the effect of 3.239998).
    let wave_q = add(c.effect_q, (since as f32 * 0.005).to_bits());
    let any = !hit.is_empty();
    for (target, building) in hit {
        // Each unit hit takes the wave's short buff, taken off 0.99 s on.
        deferred.push(Deferred::BlizzardHit {
            target,
            end: add(wave_q, BLIZZARD_BUFF),
        });
        deferred.push(Deferred::Damage {
            caster: h,
            target,
            amount: if building { mul(each, n.data_d) } else { each },
            call: Some((point, n.area)),
            slow: None,
            at: Some(wave_q),
            fog: None,
            later_calls: true,
        });
    }
    // A wave that hits makes or restarts the caster's fog modifier (1757).
    if any && let Some(Deferred::Damage { fog, .. }) = deferred.last_mut() {
        *fog = Some((h, wave_q));
    }
    // Each unit hit responds, then one call for the wave (as a
    // Shockwave's step; c-RZJ-1: the wave of 15.699999).
    if any {
        deferred.push(Deferred::Calls {
            caster: h,
            targets: order_hit,
            centre: point,
            area: n.area,
            at: wave_q,
        });
    }
}

/// A Blizzard's shard sets, made at its effect and again every 1.0 s of
/// its timer: a shuffle of the 6 wave indices by 5 draws of the script
/// generator, then 13 of game generator 4 for the shards' offsets
/// (c-RZJ-1 at 3.24, 4.24, ... 8.24: one set per wave, 0.9 s before its
/// hits). PROVISIONAL: the values are not used here, only drawn.
fn blizzard_shards(s: &mut State) {
    for _ in 0..5 {
        s.random.step();
    }
    for _ in 0..13 {
        s.game_random[4].step();
    }
}

/// How long a Blizzard wave's buff stays on a unit it hits (0.99 s).
const BLIZZARD_BUFF: u32 = 0x3F7D_70A4;

/// The cast of `h` starts in tick `tick`: the effect `castpt` later.
pub(super) fn start(state: &mut State, h: u32, tick: u64) {
    super::table::cast_started(state, h);
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let (point, backswing) = state
        .data
        .units
        .get(&u.type_id)
        .map_or((0, 0), |t| (t.cast_point, t.cast_backswing));
    // Fan of Knives has no cast point: its effect comes in the order's
    // tick (SPEC-0041; c-SBK-1: 4.499912).
    let knives = u.casting.is_some_and(|c| {
        state
            .data
            .abilities
            .get(&c.ability)
            .map_or(c.ability, |a| a.code)
            == i32::from_be_bytes(*b"AEfk")
    });
    // Avatar too, and its order ends with the effect (c-SBV-1: 4.499912).
    let avatar = u
        .casting
        .is_some_and(|c| base_code(state, c.ability) == i32::from_be_bytes(*b"AHav"));
    let knives = knives || avatar;
    let point = if knives { 0 } else { point };
    // A cast point between ticks lands in the tick that holds it (SPEC-0041:
    // the death knight's 0.452, 90.4 ticks, at `o + 91`).
    let exact = f64::from(f32::from_bits(point)) * 200.0;
    let effect = if knives {
        tick
    } else if exact - exact.floor() > 0.01 {
        tick + exact.ceil() as u64
    } else {
        tick + super::attack::ticks(point)
    };
    // A channel ends after its last wave (SPEC-0043).
    let channel = u.casting.and_then(|c| {
        let base = state
            .data
            .abilities
            .get(&c.ability)
            .map_or(c.ability, |a| a.code);
        (&base.to_be_bytes() == b"AHbz")
            .then(|| {
                let waves = f32::from_bits(numbers(state, c.ability, c.level).data[0]) as u64;
                FIRST_WAVE + WAVE_EVERY * waves.saturating_sub(1) + CHANNEL_AFTER
            })
            // A Flame Strike's order ends at its landing (SPEC-0039).
            .or_else(|| {
                (&base.to_be_bytes() == b"AHfs")
                    .then(|| super::attack::ticks(numbers(state, c.ability, c.level).cast))
            })
            .or_else(|| avatar.then_some(0))
            // An Earthquake's channel lasts Dur from its effect (c-SBW-1: 25 s).
            .or_else(|| {
                (&base.to_be_bytes() == b"AOeq")
                    .then(|| super::attack::ticks(numbers(state, c.ability, c.level).duration))
            })
            // An Impale's at its wave's time (DataB) + 0.1 after its effect
            // (c-SBJ-1: 0.4 s, 7.299809).
            .or_else(|| {
                (&base.to_be_bytes() == b"AUim").then(|| {
                    let wave = numbers(state, c.ability, c.level).data[1];
                    super::attack::ticks(add(wave, 0.1f32.to_bits()))
                })
            })
    });
    let effect_q = add(
        state.event_time.unwrap_or_else(|| state.timers.time()),
        point,
    );
    let end = effect + channel.unwrap_or_else(|| super::attack::ticks(backswing));
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(c) = u.casting.as_mut()
    {
        c.effect_at = Some(effect);
        c.effect_q = effect_q;
        c.end_at = end;
    }
}

/// Unit `h` is stunned for `duration` seconds from the queue time `at`, or
/// longer if it already is (SPEC-0037): it stops at once. The stun ends
/// on a timer of the queue, and its order comes back on a timer of 0.01 s
/// from that expiry, in the tick whose end that time does not pass.
pub(super) fn stun(state: &mut State, h: u32, source: u32, duration: u32, at: u32, impact: bool) {
    let back = add(add(at, duration), STUN_BACK);
    let (mut tick, mut clock) = (state.ticks, state.move_clock);
    while f32::from_bits(back) > f32::from_bits(add(clock, crate::timers::TICK)) {
        clock = add(clock, crate::timers::TICK);
        tick += 1;
    }
    // The tick its end passes, for the objects of the stun (SPEC-0024).
    let end = add(at, duration);
    let (mut end_tick, mut clock) = (state.ticks, state.move_clock);
    while f32::from_bits(end) > f32::from_bits(add(clock, crate::timers::TICK)) {
        clock = add(clock, crate::timers::TICK);
        end_tick += 1;
    }
    // Dead of the hit: only the objects kept to the stun's end.
    if state.unit(Some(h)).is_some_and(|u| !u.alive) {
        super::table::stunned_dead(state, h, end_tick);
        return;
    }
    // A swing under way is lost: its hit never comes (c-RZH-1: creep 8,
    // swinging at footman 5 when the bolt of 4.715 stunned it; c-SAK-1:
    // gnoll 11 at the bash of 15.748).
    let clock = f32::from_bits(state.move_clock);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.attack.is_some_and(|a| {
            f32::from_bits(a.swing_q) <= clock
                && a.hit_at.is_some_and(|t| f32::from_bits(t) > clock)
        })
    {
        u.stun_dropped_acquired = u.attack.is_some_and(|a| a.order_id == 0);
        u.attack = None;
    }
    // The stun clears an acquired target: the attack record and the chase
    // go, the walk is given up, and the unit is idle at its order's return
    // (c-SBI-1: War Stomp's four creeps at 4.970000; c-SAK-1: kobold 10 at
    // 3.695). Not a target that is the stunner itself (c-SAK-1: creep 1048701
    // on the Mountain King, bashed at 8552, kept him; PROVISIONAL: inferred
    // from the rows).
    let acquired = state.unit(Some(h)).is_some_and(|u| {
        u.attack
            .is_some_and(|a| a.order_id == 0 && a.target != source)
            || u.movement.is_some_and(|m| {
                matches!(m.order, movement::Order::Attack { target } if target != source)
                    && m.order_id() == 0
                    && m.ordered()
            })
    });
    if acquired {
        super::table::walk_dropped(state, h);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.stun_dropped_acquired = true;
            u.attack = None;
            u.movement = None;
        }
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.stunned_until.is_none_or(|t| t <= tick)
    {
        u.stunned_until = Some(tick);
        u.stun_back = Some(back);
        u.stunner = Some(source);
    }
    if state
        .unit(Some(h))
        .is_some_and(|u| u.stunned_until == Some(tick))
    {
        super::table::stun_ticks(state, h, end_tick, tick, end);
    }
    // A missile's impact at the end of the tick stops it where it is then
    // (c-SAK-1: gnoll 16 at the bolt of 2615); a melee hit, at the start of
    // the tick (c-RXD-1: worker 5's bash).
    movement::stand_still(state, h, impact.then_some(at));
    // Its order goes on after the stun with a new path (c-RXD-1: worker 5
    // stunned in 4337, new path at its update of 4741).
    state.drop_path(h);
}

/// A shockwave under way (SPEC-0047): from `origin` along `direction`,
/// started at tick `start`, the units it hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    caster: u32,
    owner: i32,
    origin: [u32; 2],
    direction: [u32; 2],
    start: u64,
    /// The queue time of its start, the effect's.
    start_q: u32,
    /// Its objects, freed as a missile's after its last step.
    objects: Vec<u32>,
    /// The radius of its circle, its first distance and its length.
    area: u32,
    length: u32,
    amount: u32,
    hit: Vec<u32>,
}

/// The wave moves 105 every 20 ticks (1050 a second) and has a last
/// update at its end (SPEC-0047).
const WAVE_EVERY_TICKS: u64 = 20;
/// 0.1 s, the wave's step on its timer.
const WAVE_STEP_TIME: u32 = 0x3DCC_CCCD;
const WAVE_STEP: f32 = 105.0;
const WAVE_SPEED: f32 = 1050.0;

/// The tick of the last update of `w`, counted from its start: its
/// length at its speed (800 -> 152).
fn wave_end(w: &Wave) -> u64 {
    (f32::from_bits(w.length) / WAVE_SPEED * 200.0) as u64
}

/// Creep `h` casts its spell `ability` of `level` on `target` without
/// moving (SPEC-0047): the effect and the end of the order come after its
/// cast point and backswing. Not turned within 0.2 to the target, it first
/// turns and starts at the update at which it is (SPEC-0035; c-RZJ-1: the
/// Ogre Lord ordered at 2423, turned at 2425, started at 2431).
pub(super) fn creep_cast(state: &mut State, h: u32, ability: i32, level: i32, target: u32) {
    let order_id = ABILITY_ORDERS
        .iter()
        .find(|(_, b, _, _)| i32::from_be_bytes(*b) == base_code(state, ability))
        .map_or(0, |o| o.2);
    let casting = Casting {
        order_id,
        ability,
        level,
        target: Some(target),
        effect_at: None,
        end_at: 0,
        effect_q: 0,
        point: None,
    };
    let set = |state: &mut State| {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.casting = Some(casting);
        }
    };
    let turned =
        movement::position_of(state, target).is_some_and(|at| movement::turned_to(state, h, at));
    movement::stop(state, h);
    set(state);
    // Its order's objects (SPEC-0024), after the order's own walk start
    // (the walk is kept to the cast's start).
    if turned {
        super::table::creep_spell_ordered(state, h, true);
        let tick = state.ticks;
        start(state, h, tick);
    } else if movement::cast_order(state, h, target, order_id) {
        set(state);
        super::table::creep_spell_ordered(state, h, false);
    }
}

/// Whether creep spell `ability` of `h` can be cast now (SPEC-0047,
/// "Ready").
pub(super) fn creep_ready(state: &State, h: u32, ability: i32, level: i32) -> bool {
    ready(state, h, ability, level) && state.unit(Some(h)).is_some_and(|u| u.casting.is_none())
}

/// The shockwave of `h` on `target` starts (SPEC-0047, "The wave").
fn shockwave(
    s: &mut State,
    h: u32,
    target: u32,
    n: &crate::data::AbilityLevel,
    start_q: u32,
    objects: Vec<u32>,
) {
    let (Some(u), Some(t)) = (s.unit(Some(h)), s.unit(Some(target))) else {
        return;
    };
    let (dx, dy) = (sub(t.x, u.x), sub(t.y, u.y));
    let length = ow3_math::natives::sqrt(add(mul(dx, dx), mul(dy, dy)));
    if f32::from_bits(length) <= 0.0 {
        return;
    }
    let wave = Wave {
        caster: h,
        owner: u.owner,
        origin: [u.x, u.y],
        direction: [div(dx, length), div(dy, length)],
        start: s.ticks,
        start_q,
        objects,
        area: n.area,
        length: n.data[2],
        amount: n.data[0],
        hit: Vec::new(),
    };
    s.waves.push(wave);
}

/// The updates of the shockwaves in this tick: each enemy nearer the
/// centre than the circle's radius plus its own collision, not hit yet,
/// takes the damage.
fn shockwaves(s: &mut State, deferred: &mut Vec<Deferred>) {
    let tick = s.ticks;
    let mut waves = std::mem::take(&mut s.waves);
    for w in &mut waves {
        let since = tick - w.start;
        let last = wave_end(w);
        let distance = if since == last {
            add(w.area, w.length)
        } else if since % WAVE_EVERY_TICKS == 0 && since < last {
            add(
                w.area,
                (WAVE_STEP * (since / WAVE_EVERY_TICKS) as f32).to_bits(),
            )
        } else {
            continue;
        };
        let centre = [
            add(w.origin[0], mul(w.direction[0], distance)),
            add(w.origin[1], mul(w.direction[1], distance)),
        ];
        // The step's queue time, the effect's plus its ticks (c-RZJ-1:
        // 3.937711, 40 ticks after the effect of 3.737711).
        // (Summed 0.1 s step by step, as the wave's timer: c-RZJ-1's steps
        // at 12.709924, 12.809923, 12.909923, 13.009922, 13.109921.)
        let step_q = if since % WAVE_EVERY_TICKS == 0 {
            (0..since / WAVE_EVERY_TICKS).fold(w.start_q, |q, _| add(q, WAVE_STEP_TIME))
        } else {
            add(w.start_q, (since as f32 * 0.005).to_bits())
        };
        let mut any = false;
        let mut restarts = Vec::new();
        let mut calls = Vec::new();
        let mut fresh = Vec::new();
        for &o in &s.units {
            let Some(Object::Unit(u)) = s.objects.get(&o) else {
                continue;
            };
            // Nor an invulnerable one (c-RZJ-1: the Paladin under Divine
            // Shield at 32.405407).
            if !super::attack::standing(s, o) {
                continue;
            }
            if !u.alive || u.hidden || !movement::enemies(s, w.owner, u.owner) {
                continue;
            }
            let collision = s.data.units.get(&u.type_id).map_or(0, |t| t.collision);
            let (dx, dy) = (sub(u.x, centre[0]), sub(u.y, centre[1]));
            let reach = add(w.area, collision);
            let covered =
                f32::from_bits(add(mul(dx, dx), mul(dy, dy))) < f32::from_bits(mul(reach, reach));
            // A unit hit already is only covered again: its hit buff
            // starts again (c-RZJ-1: the Paladin's, restarted at 4.037711,
            // 4.137711 and 4.237710).
            if covered && w.hit.contains(&o) {
                restarts.push((w.caster, o, step_q));
                continue;
            }
            if covered {
                fresh.push(o);
            }
        }
        // The new hits in the grid order of the pass (SPEC-0019; c-RZJ-1:
        // footman 5, footman 2, the Paladin, footman 4 at 12.709924).
        for o in movement::call_order(s, fresh) {
            w.hit.push(o);
            any = true;
            deferred.push(Deferred::ShockwaveHit {
                caster: w.caster,
                target: o,
                at: step_q,
            });
            // The calls of the step's hits come after all of them
            // (c-RZJ-1: the second Shockwave's first step, 12.708868).
            deferred.push(Deferred::Damage {
                caster: w.caster,
                target: o,
                amount: w.amount,
                call: Some((centre, w.area)),
                slow: None,
                at: Some(step_q),
                fog: None,
                later_calls: true,
            });
            calls.push(o);
        }
        // A step that hits makes or restarts the caster's fog modifier,
        // after its last damage and before the calls of that hit (c-RZJ-1:
        // 1693 at 3.937711).
        if any && let Some(Deferred::Damage { fog, .. }) = deferred.last_mut() {
            *fog = Some((w.caster, step_q));
        }
        if !calls.is_empty() {
            deferred.push(Deferred::Calls {
                caster: w.caster,
                targets: calls,
                centre,
                area: w.area,
                at: step_q,
            });
        }
        // Its end of travel (its length at its speed from the effect, not
        // the step's tick) is its impact as a missile's (c-RZJ-1: the Ogre
        // Lord's first, 3.737711 + 0.761905, its objects freed at
        // 9.504406).
        if since == last {
            let travel = (f32::from_bits(w.length) / WAVE_SPEED).to_bits();
            super::table::missile_hit(s, &w.objects, add(w.start_q, travel));
        }
        // In the order of the grid the area pass visits (SPEC-0019).
        let order = movement::call_order(s, restarts.iter().map(|r| r.1).collect());
        for o in order {
            if let Some(&(caster, _, at)) = restarts.iter().find(|r| r.1 == o) {
                super::table::shockwave_covered(s, caster, o, at);
            }
        }
    }
    waves.retain(|w| tick - w.start < wave_end(w));
    s.waves = waves;
}

/// Whether unit `h` is invulnerable now (SPEC-0045).
/// A Wind Walk (SPEC-0040): the tick its fade ends and the unit is
/// invisible, the clock of that end, the tick it ends, and its speed
/// bonus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindWalk {
    fade: u64,
    fade_clock: u32,
    until: u64,
    bonus: u32,
}

impl WindWalk {
    /// Whether it lasts at `tick`.
    pub(super) fn on(&self, tick: u64) -> bool {
        tick < self.until
    }

    /// Whether the unit is invisible at `tick`.
    pub(super) fn invisible(&self, tick: u64) -> bool {
        self.fade <= tick && tick < self.until
    }
}

/// Whether unit `h` walks invisible (SPEC-0040, Wind Walk).
pub(super) fn wind_walking(state: &State, h: u32) -> bool {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.wind_walk)
        .is_some_and(|w| w.invisible(tick))
}

/// The speed factor of unit `h`'s Wind Walk, while it lasts.
pub(super) fn wind_walk_speed(state: &State, h: u32) -> Option<u32> {
    let tick = state.ticks;
    let w = state.unit(Some(h))?.wind_walk?;
    w.on(tick).then(|| add(0x3F80_0000, w.bonus))
}

/// Unit `h` orders another spell: its Wind Walk ends (c-SAK-1: the
/// Blademaster's at his Mirror Image order of 7102).
fn end_wind_walk(state: &mut State, h: u32, ability: i32) {
    let tick = state.ticks;
    if base_code(state, ability) != i32::from_be_bytes(*b"AOwk")
        && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(w) = u.wind_walk.as_mut()
        && w.on(tick)
    {
        w.until = tick;
    } else {
        return;
    }
    super::table::wind_walk_ended(state, h);
}

pub(super) fn invulnerable(state: &State, h: u32) -> bool {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.invulnerable_until)
        .is_some_and(|t| tick < t)
        || state.unit(Some(h)).is_some_and(|u| u.script_invulnerable)
}

/// The base code of `ability`.
fn base_code(state: &State, ability: i32) -> i32 {
    state
        .data
        .abilities
        .get(&ability)
        .map_or(ability, |a| a.code)
}

/// Whether unit `h` is stunned now.
pub(super) fn stunned(state: &State, h: u32) -> bool {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.stunned_until)
        .is_some_and(|t| tick < t)
        // A rooted unit neither moves nor attacks either (c-SBK-1: the ogre).
        || (state
                .unit(Some(h))
                .and_then(|u| u.rooted_until)
                .is_some_and(|t| tick < t))
}

/// The attack-speed factor of `h` now: `1 - DataD` of a thunder clap that
/// slows it, if one does.
pub(super) fn attack_slow(state: &State, h: u32) -> Option<u32> {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.slowed_until)
        .filter(|&(until, _)| tick < until)
        .map(|(_, factor)| factor)
}

/// The attack-speed factor of a unit hit by frost (1 - 0.25).
const FROST_ATTACK: u32 = 0x3F40_0000;
/// How much frost slows a unit's moves.
const FROST_MOVE: u32 = 0x3F00_0000;

/// What an effect does after the borrow of the state ends.
enum Deferred {
    /// The end of an Avatar's first second: the units round the hero take
    /// it as at the end of an invulnerability (c-SBV-1 at 5.5, hub #9).
    Untargetable { hero: u32, at: u32 },
    /// The hit of a storm bolt launched next to its target.
    SpellHit {
        caster: u32,
        target: u32,
        amount: u32,
        stun: (u32, u32),
        objects: Vec<u32>,
    },
    /// The end of a channel's object, after its last wave's hits.
    ChannelEnd { caster: u32 },
    /// A War Stomp's hit: its damage, then the stun (SPEC-0040).
    StompHit {
        caster: u32,
        target: u32,
        amount: u32,
        stun: (u32, u32),
        /// The effect's queue time, from which the stun counts.
        at: u32,
    },
    /// War Stomp's own object after its hits, kept to queue time `end`.
    StompEnd { end: u32 },
    /// The calls for help of an area pass's hits, after all of them.
    Calls {
        caster: u32,
        targets: Vec<u32>,
        centre: [u32; 2],
        area: u32,
        at: u32,
    },
    /// The object a shockwave's hit takes before its damage: the wave's
    /// caster, the unit and the step's queue time.
    ShockwaveHit { caster: u32, target: u32, at: u32 },
    /// The buff a Blizzard wave puts on a unit it hits, to queue time
    /// `end`.
    BlizzardHit { target: u32, end: u32 },
    /// The object of a thunder clap's area pass, after its hits.
    ClapArea { at: u32, first: Option<u32> },
    /// An object taken now and freed by a queue timer at `end` (`u32::MAX`:
    /// kept).
    Kept { end: u32 },
    /// A unit rooted by Entangling Roots (`table::rooted`).
    Rooted { target: u32, until: u64 },
    /// Damage of a thunder clap.
    Damage {
        caster: u32,
        target: u32,
        amount: u32,
        /// The centre and area of the spell, for its call for help
        /// (SPEC-0019, "Spell damage").
        call: Option<([u32; 2], u32)>,
        /// A thunder clap's slow, put on a target the hit leaves alive:
        /// the queue time it ends.
        slow: Option<u32>,
        /// The queue time of the hit, for what it starts (a thunder
        /// clap's effect; c-RZH-1: gnoll 11's death at 26.400000).
        at: Option<u32>,
        /// The fog modifier of an area pass whose last hit this is: its
        /// caster and the pass's queue time.
        fog: Option<(u32, u32)>,
        /// Its call comes later, with the pass's others.
        later_calls: bool,
    },
}

/// The effects and the ends of the casts of this tick.
pub(super) fn tick(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let mut deferred = Vec::new();
    {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tick = s.ticks;
        // The Wind Walks whose last tick this is end after its script
        // timers (SPEC-0040), freeing their objects (SPEC-0024).
        let ending: Vec<u32> = s
            .units
            .iter()
            .copied()
            .filter(|&h| {
                s.unit(Some(h))
                    .and_then(|u| u.wind_walk)
                    .is_some_and(|w| w.until == tick + 1)
            })
            .collect();
        for h in ending {
            super::table::wind_walk_ended(s, h);
        }
        // Avatars whose time is up end, in the tick of their queue time.
        let next = f32::from_bits(add(s.move_clock, crate::timers::TICK));
        // An Avatar's first second ends on its own timer (hub #9).
        let first: Vec<(u32, u32)> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.avatar).map(|a| (h, a[3])))
            .filter(|&(_, q)| q != 0 && f32::from_bits(q) < next)
            .collect();
        for (hero, at) in first {
            deferred.push(Deferred::Untargetable { hero, at });
            if let Some(Object::Unit(u)) = s.objects.get_mut(&hero)
                && let Some(a) = u.avatar.as_mut()
            {
                a[3] = 0;
            }
        }
        let avatars: Vec<(u32, [u32; 4])> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.avatar).map(|a| (h, a)))
            .filter(|(_, a)| f32::from_bits(a[0]) < next)
            .collect();
        for (h, [end, _, life, _]) in avatars {
            s.table_at(end);
            if let Some((ability, ..)) = ability_for(s, h, "avatar") {
                super::table::avatar_ended(s, h, ability);
            }
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.avatar = None;
                u.life.max = sub(u.life.max, life);
                let now = u.life.value();
                if f32::from_bits(now) > f32::from_bits(u.life.max) {
                    let max = u.life.max;
                    u.life.set(max);
                }
            }
            s.table.now = tick;
        }
        let casting: Vec<(u32, Casting)> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.casting).map(|c| (h, c)))
            .collect();
        bounces(s, &mut deferred);
        quakes(s);
        let early = std::mem::take(&mut s.early_effects);
        let early_ends = std::mem::take(&mut s.early_ends);
        for (h, c) in casting {
            if c.effect_at == Some(tick) && !early.contains(&h) {
                // Its objects count in the tick of its queue time.
                s.table_at(c.effect_q);
                // A spell on a target that died ends the cast at once
                // (SPEC-0035, edge cases; c-RZJ-1: the Paladin's order
                // ended at the effect time 3227 and he acquired at 3229).
                if !effect(s, h, &c, &mut deferred) {
                    fizzled(s, h, &c);
                    continue;
                }
                effect_frees(s, tick);
            }
            if let Some(e) = c.effect_at
                && tick > e
            {
                // The shard sets again at each 1.0 s of the Blizzard's timer
                // (c-RZJ-1: 4.239931).
                let since = tick - e;
                let blizzard = s
                    .data
                    .abilities
                    .get(&c.ability)
                    .is_some_and(|a| a.code == i32::from_be_bytes(*b"AHbz"));
                let count = f32::from_bits(numbers(s, c.ability, c.level).data[0]) as u64;
                if blizzard && since % WAVE_EVERY == 0 && since / WAVE_EVERY < count {
                    blizzard_shards(s);
                }
                waves(s, h, &c, since, &mut deferred);
                // The channel's object goes at its last wave, after the
                // wave's hits (c-RZJ-1: 1621 at 9.139997).
                if blizzard && since == FIRST_WAVE + WAVE_EVERY * count.saturating_sub(1) {
                    deferred.push(Deferred::ChannelEnd { caster: h });
                }
            }
            if c.effect_at.is_some() && tick >= c.end_at && !early_ends.contains(&h) {
                cast_ended(s, h, &c, tick);
            }
        }
        // A wave made at an effect has its first update in that tick
        // (SPEC-0047; c-RZJ-1: the second Shockwave, effect and first hits
        // at 2543).
        shockwaves(s, &mut deferred);
        // Mirror Images: their units and missiles (SPEC-0040).
        mirrors(s);
    }
    // Summons whose time ran out die (SPEC-0044).
    let expired: Vec<u32> = {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let next = f32::from_bits(add(s.move_clock, crate::timers::TICK));
        let (by_q, later_q): (Vec<_>, Vec<_>) = s
            .timed_life_q
            .iter()
            .partition(|(q, _)| f32::from_bits(*q) < next);
        s.timed_life_q = later_q;
        let (now, later): (Vec<_>, Vec<_>) = s.timed_life.iter().partition(|(t, _)| *t <= tick);
        s.timed_life = later;
        now.into_iter()
            .map(|(_, h)| h)
            .chain(by_q.into_iter().map(|(_, h)| h))
            .collect()
    };
    for h in expired {
        state.borrow_mut().table.expiring = Some(h);
        super::widgets::kill_unit(vm, state, Some(h), None);
        state.borrow_mut().table.expiring = None;
    }
    run_deferred(vm, state, deferred);
    let mut s = state.borrow_mut();
    s.table.now = s.ticks;
}

/// What the effects of a tick left for after the state's borrow.
fn run_deferred(vm: &mut Vm, state: &Rc<RefCell<State>>, deferred: Vec<Deferred>) {
    for d in deferred {
        match d {
            Deferred::SpellHit {
                caster,
                target,
                amount,
                stun,
                objects,
            } => super::attack::spell_hit(vm, state, caster, target, amount, stun, objects),
            Deferred::StompHit {
                caster,
                target,
                amount,
                stun,
                at,
            } => super::attack::stomp_hit(vm, state, caster, target, amount, stun, at),
            Deferred::Untargetable { hero, at } => {
                let acquired = {
                    let mut s = state.borrow_mut();
                    s.table_at(at);
                    let was = s.event_time.replace(at);
                    let acquired = super::movement::acquire_vulnerable(&mut s, hero);
                    s.event_time = was;
                    acquired
                };
                super::widgets::acquired_with(vm, state, &acquired, true);
                let mut s = state.borrow_mut();
                s.table.now = s.ticks;
            }
            Deferred::StompEnd { end } => {
                let mut s = state.borrow_mut();
                let tick = s.table.now;
                let o = s.table.take(tick, "war stomp: effect");
                s.table.free_at_q(end, o, "war stomp: effect");
            }
            Deferred::Calls {
                caster,
                targets,
                centre,
                area,
                at,
            } => {
                // Each unit hit responds, in the hit order, then one call
                // for the pass, in which a unit that took the caster in its
                // response answers again (c-RZJ-1: the second Shockwave's
                // third step, footman 7 at 12.909923).
                let mut s = state.borrow_mut();
                let previous = s.event_time.replace(at);
                let impact = Some((centre, f32::from_bits(area)));
                let alive = |s: &State, t: u32| matches!(s.objects.get(&t), Some(Object::Unit(u)) if u.alive);
                let mut took = Vec::new();
                s.victim_only = true;
                for &target in &targets {
                    if alive(&s, target) {
                        let before = movement::attack_target(&s, target);
                        movement::retaliate(&mut s, target, caster, impact);
                        if before != Some(caster)
                            && movement::attack_target(&s, target) == Some(caster)
                        {
                            took.push(target);
                        }
                    }
                }
                s.victim_only = false;
                if let Some(&first) = targets.iter().find(|&&t| alive(&s, t)) {
                    s.call_only = true;
                    s.reacquire = took;
                    movement::retaliate(&mut s, first, caster, impact);
                    s.call_only = false;
                    s.reacquire.clear();
                }
                s.event_time = previous;
            }
            Deferred::ChannelEnd { caster } => {
                super::table::channel_ended(&mut state.borrow_mut(), caster);
            }
            Deferred::ShockwaveHit { caster, target, at } => {
                super::table::shockwave_hit(&mut state.borrow_mut(), caster, target, at);
            }
            Deferred::BlizzardHit { target, end } => {
                super::table::blizzard_hit(&mut state.borrow_mut(), target, end);
            }
            Deferred::Rooted { target, until } => {
                if let Some(Object::Unit(u)) = state.borrow_mut().objects.get_mut(&target) {
                    u.rooted_until = Some(until);
                }
                super::table::rooted(&mut state.borrow_mut(), target);
            }
            Deferred::Kept { end } => {
                let mut s = state.borrow_mut();
                let tick = s.table.now;
                let o = s.table.take(tick, "spell: effect");
                // (`u32::MAX`: kept.)
                if end != u32::MAX {
                    s.table.free_at_q(end, o, "spell: effect");
                }
            }
            Deferred::ClapArea { at, first } => {
                super::table::clap_area(&mut state.borrow_mut(), at, first);
            }
            Deferred::Damage {
                caster,
                target,
                amount,
                call,
                slow,
                at,
                fog,
                later_calls,
            } => {
                let previous = state.borrow().event_time;
                if at.is_some() {
                    state.borrow_mut().event_time = at;
                }
                super::widgets::damage_widget(vm, state, Some(caster), Some(target), amount, 0, 0);
                let mut s = state.borrow_mut();
                s.event_time = previous;
                // The slow's objects after the hit's (SPEC-0024).
                if let Some(end) = slow
                    && matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive)
                {
                    super::table::clap_slowed(&mut s, target, end);
                }
                if let Some((caster, q)) = fog {
                    super::table::fog_pass(&mut s, caster, q, true);
                }
                // The victim and its friends answer the hit (SPEC-0019);
                // a Shockwave step's calls come after all its hits
                // (`Deferred::Calls`).
                if !later_calls
                    && matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive)
                {
                    movement::retaliate(
                        &mut s,
                        target,
                        caster,
                        call.map(|(p, a)| (p, f32::from_bits(a))),
                    );
                }
            }
        }
    }
}

/// The cast of `h` ends at an effect on a dead target: its order ends
/// with it, the object kept for the impact first (c-RZJ-1: the Paladin's
/// Holy Light, 2850 before the order's objects at 16.128393).
fn fizzled(s: &mut State, h: u32, c: &Casting) {
    // Its check 0.01 s after the effect's queue time (c-RZJ-1: 3229).
    movement::end_cast_at(s, h, c.effect_q);
    super::table::impact_unused(s, h);
    super::table::order_ended(s, h);
}

/// The order of cast `c` of `h` ends in tick `tick` (SPEC-0024).
fn cast_ended(s: &mut State, h: u32, c: &Casting, tick: u64) {
    super::table::channel_ended(s, h);
    // Its order ends (SPEC-0024), counted by the game in the
    // tick of the end's queue time, the effect's plus the
    // ticks after it; the unit's check comes 0.01 s after that
    // time (c-RZJ-1: the Ogre Lord, end 4.247710, check
    // 4.257710).
    let after = (tick - c.effect_at.unwrap_or(tick)) as f32 * 0.005;
    let end_q = add(c.effect_q, after.to_bits());
    movement::end_cast_at(s, h, end_q);
    s.table_at(end_q);
    // A spell without a missile kept its order's sixth for an
    // impact: it goes first (c-RZJ-1: the Holy Light's 2221 at
    // 11.829115).
    let missile = s
        .data
        .abilities
        .get(&c.ability)
        .is_some_and(|a| a.code == i32::from_be_bytes(*b"AHtb"));
    if !missile {
        super::table::impact_unused(s, h);
    }
    let from = s.table.deferred_count();
    super::table::order_ended(s, h);
    // A Flame Strike's by its cast timer at the landing, after the
    // landing's hits (c-SBH-1: 4.134939).
    if base_code(s, c.ability) == i32::from_be_bytes(*b"AHfs") {
        s.table.defer_to_q(from, end_q);
    }
    s.table.now = tick;
}

/// The effect of `h`'s cast if it is due at the order itself (no cast point).
pub(super) fn effect_at_order(vm: &mut Vm, state: &Rc<RefCell<State>>, h: u32) {
    let q = {
        let s = state.borrow();
        let tick = s.ticks;
        s.unit(Some(h))
            .and_then(|u| u.casting)
            .filter(|c| c.effect_at == Some(tick))
            .map(|c| c.effect_q)
    };
    // (Just above the effect's queue time.)
    if let Some(q) = q {
        effects_before(vm, state, q + 1);
    }
}

/// The effects due in this tick whose queue times come before `limit`,
/// the time of the tick's updates: they run first, as timers of the queue
/// ahead of the updates (c-RZJ-1: the Ogre Lord's Shockwave at 3.737711,
/// before the updates of 3.738891).
pub(super) fn effects_before(vm: &mut Vm, state: &Rc<RefCell<State>>, limit: u32) {
    let mut deferred = Vec::new();
    {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tick = s.ticks;
        let due: Vec<(u32, Casting)> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.casting).map(|c| (h, c)))
            .filter(|(_, c)| {
                c.effect_at == Some(tick) && f32::from_bits(c.effect_q) < f32::from_bits(limit)
            })
            .collect();
        for (h, c) in due {
            s.table_at(c.effect_q);
            s.early_effects.push(h);
            if !effect(s, h, &c, &mut deferred) {
                fizzled(s, h, &c);
            }
            effect_frees(s, tick);
            s.table.now = tick;
        }
        // So do the ends of casts (c-RZJ-1: the Ogre Lord's second
        // Shockwave, its order's end before a reaction end in 2645).
        let ending: Vec<(u32, Casting)> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.casting).map(|c| (h, c)))
            .filter(|(h, c)| {
                c.effect_at.is_some_and(|e| {
                    let after = (tick.saturating_sub(e)) as f32 * 0.005;
                    tick >= c.end_at
                        && !s.early_ends.contains(h)
                        && f32::from_bits(add(c.effect_q, after.to_bits())) < f32::from_bits(limit)
                })
            })
            .collect();
        for (h, c) in ending {
            s.early_ends.push(h);
            cast_ended(s, h, &c, tick);
        }
    }
    run_deferred(vm, state, deferred);
    let mut s = state.borrow_mut();
    s.table.now = s.ticks;
}

/// An effect counted in the tick before `tick` frees its 0.0001 s objects
/// in that tick (c-SBJ-1: Death Coil's at 2.952, freed at 2.950074).
fn effect_frees(s: &mut State, tick: u64) {
    if s.table.now < tick {
        let now = s.table.now;
        s.table.flush(now);
    }
}

/// The effect of cast `c` of `h`; the mana is paid and the cooldown starts
/// now. `false` when the spell fizzles on a dead target.
fn effect(s: &mut State, h: u32, c: &Casting, deferred: &mut Vec<Deferred>) -> bool {
    // A spell on a unit that died before the effect does nothing and costs
    // nothing: no mana, no cooldown (c-RZJ-1: the Paladin's Holy Light on
    // footman 5, dead at 3142, effect time 3227; his next cast at 3301).
    // PROVISIONAL: measured for Holy Light only.
    if let Some(t) = c.target
        && !s.unit(Some(t)).is_some_and(|u| u.alive)
    {
        return false;
    }
    let tick = s.ticks;
    let n = numbers(s, c.ability, c.level);
    let base = s
        .data
        .abilities
        .get(&c.ability)
        .map_or(c.ability, |a| a.code);
    let seconds = |hero: bool| if hero { n.hero_duration } else { n.duration };
    let duration = |hero: bool| super::attack::ticks(seconds(hero));
    match &base.to_be_bytes() {
        // The summon stands Area in front of the caster, placed as by
        // `CreateUnit`, and dies 12006 ticks after the effect
        // (SPEC-0044).
        // PROVISIONAL: the 6 ticks over the duration are read with a script
        // timer (SPEC-0044, open question 4).
        b"AHwe" => {
            if let Some(u) = s.unit(Some(h)) {
                let (x, y, f, owner) = (u.x, u.y, u.facing, u.owner);
                let at = [
                    add(x, mul(n.area, cos(f))),
                    add(y, mul(n.area, sin(f))),
                    super::facing::to_degrees(f),
                ];
                if let Some(e) = super::widgets::create_unit_by_script(s, owner, n.unit_id, at) {
                    // Its first acquisition check counts from the effect's queue
                    // time (SPEC-0019; c-RZJ-1: elemental 16, effect at 2.3).
                    if let Some(Object::Unit(w)) = s.objects.get_mut(&e)
                        && w.acquire_phase != super::NO_PHASE
                    {
                        w.next_acquire = add(c.effect_q, mul(w.acquire_phase, 0x3F00_0000));
                    }
                    super::table::summon_made(s, h, e, n.cost);
                    // It dies in the tick that holds the effect's queue time
                    // plus its duration (c-RZJ-1: the first, effect at
                    // 2.299991, dead at 62.298977).
                    s.timed_life_q.push((add(c.effect_q, seconds(false)), e));
                    s.summons.push((h, e));
                }
            }
        }
        // Invulnerable from the next tick for its duration (SPEC-0045); the
        // units attacking it drop it.
        // Invisible and faster for its duration (SPEC-0040, Wind Walk;
        // c-SAK-1: the Blademaster at 400, 20 s at level 1, speed 300 to
        // 330). It ends after the script timers of its last tick, whose
        // periodic timers were started earlier (SPEC-0002; c-SAK-1: the
        // order of 4401 refused, at the end's queue time 22.0).
        // PROVISIONAL: the bonus damage and the end at an attack or cast
        // are not done (no hit of his in c-SAK-1).
        // Mirror Image (SPEC-0040).
        b"AOmi" => mirror_image(s, h, c.ability, &n, c.effect_q),
        b"AOwk" => {
            let hero = s.unit(Some(h)).is_some_and(|u| is_hero(u.type_id));
            let until = tick + 1 + duration(hero);
            // The fade is a timer of the world clock's queue, of DataA
            // less a thousandth, firing in the first tick that reaches it
            // (SPEC-0040; c-SAK-1: 0.5994 after the casts at 2.0 and
            // 22.497225).
            // PROVISIONAL: read for DataA 0.6 only, where 0.6 * 0.999 and
            // 0.6 - 0.0006 are the same float. It fires at the end of the
            // tick before the one counted here, its frees before the turn
            // checksum of 2.6 s (c-SAK-1.parts.tsv, C = 1097).
            let fade = mul(n.data[0], 0.999f32.to_bits());
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.wind_walk = Some(WindWalk {
                    fade: tick + super::attack::ticks(fade) - 1,
                    fade_clock: add(s.move_clock, fade),
                    until,
                    bonus: n.data[1],
                });
            }
            super::table::wind_walk_cast(s, h);
            movement::target_died(s, h);
        }
        b"AHds" => {
            let hero = s.unit(Some(h)).is_some_and(|u| is_hero(u.type_id));
            let until = tick + 1 + duration(hero);
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.invulnerable_until = Some(until);
            }
            let at = s.timers.time();
            super::table::divine_shield_effect(s, h, at, seconds(hero));
            movement::target_died(s, h);
        }
        // Avatar (c-SBV-1): DataB more life, DataC more damage for Dur.
        // PROVISIONAL: the armor (DataA) and the spell immunity are not
        // done.
        b"AHav" => {
            super::table::avatar_effect(s, h, c.ability, c.effect_q);
            end_hold(s, h);
            let end = add(c.effect_q, seconds(true));
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.life.max = add(u.life.max, n.data[1]);
                let life = add(u.life.value(), n.data[1]);
                u.life.set(life);
                u.avatar = Some([end, n.data[2], n.data[1], add(c.effect_q, 1.0f32.to_bits())]);
            }
        }
        // Animate Dead: its effect event, then the dead it raises for Dur
        // (c-SBX-1 at 3.449962).
        b"AUan" => {
            super::table::spell_effect_event(s, h);
            resurrection(s, h, &n, c.effect_q, true);
        }
        // Resurrection: its effect event, then the dead it brings back.
        b"AHre" => {
            super::table::spell_effect_event(s, h);
            // Its cost's watch before the dead it brings back (c-SBV-1: 1028,
            // the old 1057 freed after the effect's).
            super::table::mana_watches(s, h, n.cost);
            resurrection(s, h, &n, c.effect_q, false);
        }
        // Earthquake: its spell effect event, then its area acts from DataA
        // after the effect, every 1.0 s, to DataA before the channel's end
        // (c-SBW-1 at 3.3, hub #10).
        b"AOeq" => {
            super::table::spell_effect_event(s, h);
            if let Some(point) = c.point {
                s.quakes.push(Quake {
                    caster: h,
                    point,
                    area: n.area,
                    slow: n.data[2],
                    next: add(c.effect_q, n.data[0]),
                    stop: sub(add(c.effect_q, n.duration), n.data[0]),
                    objects: Vec::new(),
                    inside: Vec::new(),
                    occupied: None,
                    empty: false,
                });
            }
        }
        // The creep Shockwave (SPEC-0047).
        b"AOsh" => {
            if let Some(target) = c.target {
                let objects = super::table::shockwave_effect(s);
                shockwave(s, h, target, &n, c.effect_q, objects);
            }
        }
        b"AHhb" => {
            // Its spell effect event (c-RZJ-1: the Paladin's at 10.159946).
            super::table::spell_effect_event(s, h);
            if let Some(Object::Unit(t)) = c.target.and_then(|t| s.objects.get_mut(&t))
                && t.alive
            {
                let life = add(t.life.value(), n.data[0]);
                let life = if f32::from_bits(life) > f32::from_bits(t.life.max) {
                    t.life.max
                } else {
                    life
                };
                t.life.set(life);
            }
        }
        b"AHtb" => {
            if let Some(target) = c.target {
                let speed = s
                    .data
                    .abilities
                    .get(&c.ability)
                    .map_or(0, |a| a.missile_speed);
                let stun = (seconds(false), seconds(true));
                // Its effect event and missile objects (SPEC-0024).
                let objects = super::table::bolt_effect(s, h, n.cost);
                match super::attack::launch_spell(s, h, target, n.data[0], stun, speed, c.effect_q)
                {
                    Some(mut missile) => {
                        missile.objects = objects;
                        s.missiles.push(missile);
                    }
                    None => deferred.push(Deferred::SpellHit {
                        caster: h,
                        target,
                        amount: n.data[0],
                        stun,
                        objects,
                    }),
                }
            }
        }
        // The channel's objects; its waves come by `waves` (SPEC-0043).
        b"AHbz" => {
            super::table::blizzard_effect(s, h, n.cost);
            blizzard_shards(s);
        }
        b"AHtc" => {
            let Some(u) = s.unit(Some(h)) else {
                return true;
            };
            let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
            let (u_x, u_y) = (u.x, u.y);
            let area = f32::from_bits(n.area);
            // 1 - DataD: the attack-speed factor of the slowed units.
            let speed = ow3_math::sub(1.0f32.to_bits(), n.data_d);
            let now = c.effect_q;
            let hit: Vec<u32> = s
                .units
                .iter()
                .copied()
                .filter(|&o| {
                    s.unit(Some(o)).is_some_and(|t| {
                        let ground = s
                            .data
                            .units
                            .get(&t.type_id)
                            .is_some_and(|d| d.target_type.iter().any(|c| c == "ground"));
                        let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
                        t.alive
                            && !t.hidden
                            && ground
                            && movement::enemies(s, owner, t.owner)
                            && dx * dx + dy * dy <= area * area
                    })
                })
                .collect();
            // The spell effect's event and the watches of its cost first
            // (SPEC-0024).
            super::table::clap_effect(s, h, n.cost);
            // The units in the order of the grid of calls for help
            // (SPEC-0019; c-SBG-1: paladin 2, ogre 3, ogre 1).
            let hit = movement::call_order(s, hit);
            let centre = [u_x, u_y];
            for o in hit {
                let hero = s.unit(Some(o)).is_some_and(|t| is_hero(t.type_id));
                let until = tick + duration(hero);
                let fresh = attack_slow(s, o).is_none();
                let cooldown = super::attack::swing_cooldown(s, o);
                if let Some(Object::Unit(t)) = s.objects.get_mut(&o) {
                    t.slowed_until = Some((until, speed));
                    t.move_slow = Some(n.data[2]);
                    if fresh {
                        super::attack::slow_cooldown(t, now, speed, cooldown);
                    }
                }
                deferred.push(Deferred::Damage {
                    caster: h,
                    target: o,
                    amount: n.data[0],
                    call: Some((centre, n.area)),
                    slow: Some(ow3_math::add(now, seconds(hero))),
                    at: Some(now),
                    fog: None,
                    later_calls: false,
                });
            }
            deferred.push(Deferred::ClapArea {
                at: now,
                first: Some(h),
            });
        }
        // Every enemy ground unit within Area takes DataA and is stunned for
        // Dur (heroes HeroDur) at the cast point (SPEC-0040).
        // PROVISIONAL: the order of the hits taken as Thunder Clap's, and
        // their objects as Storm Bolt's stun; neither is read.
        // The first unit is reached 0.25 s after the cast point (SPEC-0040).
        b"AOcl" => bounce(s, h, c, &n, BounceKind::Lightning),
        b"AOhw" => bounce(s, h, c, &n, BounceKind::Heal),
        // At the cast point the target takes 0, DataA and DataB, the other
        // enemies within Area of it DataA (SPEC-0041).
        // PROVISIONAL: the others' order taken as Thunder Clap's; the slow
        // is not built.
        // A homing missile launched at the cast point; an enemy takes DataA / 2
        // (SPEC-0041). PROVISIONAL: the heal of an undead friend is not
        // built.
        // A missile as Death Coil's (one timer from the launch point), its hit
        // 0.00 then DataE (SPEC-0041; c-SBO-1: 75 at 5.944808).
        // PROVISIONAL: the damage over time and the slow are not built.
        b"AEsh" => {
            if let Some(target) = c.target {
                let speed = s
                    .data
                    .abilities
                    .get(&c.ability)
                    .map_or(0, |a| a.missile_speed);
                let amount = n.data_e;
                let objects = super::table::bolt_effect(s, h, n.cost);
                match super::attack::launch_spell_missile(
                    s, h, target, amount, speed, c.effect_q, false,
                ) {
                    Some(mut missile) => {
                        missile.objects = objects;
                        s.missiles.push(missile);
                    }
                    None => deferred.push(Deferred::SpellHit {
                        caster: h,
                        target,
                        amount,
                        stun: (0, 0),
                        objects,
                    }),
                }
            }
        }
        b"AUdc" => {
            if let Some(target) = c.target {
                let speed = s
                    .data
                    .abilities
                    .get(&c.ability)
                    .map_or(0, |a| a.missile_speed);
                let amount = ow3_math::mul(n.data[0], 0.5f32.to_bits());
                // Its effect event and missile objects, as Storm Bolt's (c-SBJ-1:
                // 2.949975, freed 5.0 s after the hit).
                let objects = super::table::bolt_effect(s, h, n.cost);
                match super::attack::launch_spell_missile(
                    s, h, target, amount, speed, c.effect_q, false,
                ) {
                    Some(mut missile) => {
                        missile.objects = objects;
                        s.missiles.push(missile);
                    }
                    None => deferred.push(Deferred::SpellHit {
                        caster: h,
                        target,
                        amount,
                        stun: (0, 0),
                        objects,
                    }),
                }
            }
        }
        // PROVISIONAL: the hold of the roots is not built.
        // The effect event and the 0 hit at once, then an object kept with
        // the roots; DataA from 2 ticks on (c-SBK-1: 7.239974, kept 1413,
        // 15.00 from 1451). PROVISIONAL: the walk and the object (1414 ..
        // 1417) made after it are not built.
        b"AEer" => {
            if let Some(t) = c.target {
                super::table::clap_effect(s, h, n.cost);
                deferred.push(Deferred::Damage {
                    caster: h,
                    target: t,
                    amount: 0,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
                let hero = s.unit(Some(t)).is_some_and(|u| is_hero(u.type_id));
                deferred.push(Deferred::Rooted {
                    target: t,
                    until: s.ticks + duration(hero),
                });
            }
            bounce(s, h, c, &n, BounceKind::Roots);
        }
        b"AHfs" => flame_strike(s, h, c, &n),
        // A wave from the caster towards the point, DataA long and Area
        // wide: the enemy ground units in it are thrown up, stunned for
        // DataD and then Dur (heroes HeroDur), and take DataC when they
        // land DataD later (SPEC-0041; c-SBJ-1: the effect at 6.9, three
        // units, landing at 7.9, order back at 9.91).
        // PROVISIONAL: every unit in the wave is reached at the effect;
        // the wave's travel time is not built.
        b"AUim" => impale(s, h, c, &n),
        b"AEfk" => fan_of_knives(s, h, c, &n, deferred),
        b"AUcs" => carrion_swarm(s, h, c, &n, deferred),
        b"AUfn" => {
            let (Some(t), Some(owner)) = (c.target, s.unit(Some(h)).map(|u| u.owner)) else {
                return true;
            };
            let Some([x, y]) = s
                .unit(Some(t))
                .map(|u| [f32::from_bits(u.x), f32::from_bits(u.y)])
            else {
                return true;
            };
            let area = f32::from_bits(n.area);
            let near: Vec<u32> = s
                .units
                .iter()
                .copied()
                .filter(|&o| {
                    s.unit(Some(o)).is_some_and(|u| {
                        let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
                        u.alive
                            && !u.hidden
                            && movement::enemies(s, owner, u.owner)
                            && dx * dx + dy * dy <= area * area
                    })
                })
                .collect();
            // The target's 0 first, then DataA on every unit of the area,
            // the target too, each slowed after its hit as by a thunder
            // clap; the area's fog modifier, and the target's DataB last
            // (c-SBJ-1: 4.959991, slows to 8.959991, the modifier to
            // 7.964661).
            // PROVISIONAL: the slow's speeds are not built.
            let now = c.effect_q;
            super::table::clap_effect(s, h, n.cost);
            let damage = |target, amount, slow| Deferred::Damage {
                caster: h,
                target,
                amount,
                call: None,
                slow,
                at: None,
                fog: None,
                later_calls: false,
            };
            deferred.push(damage(t, 0, None));
            for o in movement::call_order(s, near) {
                let hero = s.unit(Some(o)).is_some_and(|u| is_hero(u.type_id));
                // The frost of `Units\MiscData.txt`: attacks 0.25 slower
                // (`FrostAttackSpeedDecrease`), moves 0.5 slower
                // (`FrostMoveSpeedDecrease`) (c-SBJ-1: gnoll 4's swing
                // at 5.22486).
                let fresh = attack_slow(s, o).is_none();
                let cooldown = super::attack::swing_cooldown(s, o);
                if let Some(Object::Unit(u)) = s.objects.get_mut(&o) {
                    u.slowed_until = Some((tick + duration(hero), FROST_ATTACK));
                    u.move_slow = Some(FROST_MOVE);
                    if fresh {
                        super::attack::slow_cooldown(u, now, FROST_ATTACK, cooldown);
                    }
                }
                deferred.push(damage(o, n.data[0], Some(add(now, seconds(hero)))));
            }
            deferred.push(Deferred::ClapArea {
                at: now,
                first: Some(h),
            });
            deferred.push(damage(t, n.data[1], None));
        }
        // A damage event of 0 at the cast point, the burn DataB later
        // (SPEC-0041).
        // Its effect event, the 0 hit, then an object kept 1 s (c-SBK-1:
        // 2.909998, freed at 3.914951).
        b"AEmb" => {
            if let Some(t) = c.target {
                super::table::spell_effect_event(s, h);
                deferred.push(Deferred::Damage {
                    caster: h,
                    target: t,
                    amount: 0,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
                deferred.push(Deferred::Kept {
                    end: add(c.effect_q, 1.0f32.to_bits()),
                });
            }
            bounce(s, h, c, &n, BounceKind::ManaBurn);
        }
        b"AOws" => {
            let Some(u) = s.unit(Some(h)) else {
                return true;
            };
            let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
            let area = f32::from_bits(n.area);
            let hit: Vec<u32> = s
                .units
                .iter()
                .copied()
                .filter(|&o| {
                    s.unit(Some(o)).is_some_and(|t| {
                        let ground = s
                            .data
                            .units
                            .get(&t.type_id)
                            .is_some_and(|d| d.target_type.iter().any(|c| c == "ground"));
                        let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
                        // The area reaches the unit's edge (c-SBI-1: kobold 6 at 273,
                        // collision 31, hit by the area 250).
                        let reach = area
                            + s.data
                                .units
                                .get(&t.type_id)
                                .map_or(0.0, |d| f32::from_bits(d.collision));
                        t.alive
                            && !t.hidden
                            && ground
                            && movement::enemies(s, owner, t.owner)
                            && dx * dx + dy * dy <= reach * reach
                    })
                })
                .collect();
            let stun = (seconds(false), seconds(true));
            // Its effect's event first, its own object after the hits, kept
            // Dur (c-SBI-1: 4.969878 to 7.969760).
            super::table::spell_effect_event(s, h);
            for o in movement::call_order(s, hit) {
                deferred.push(Deferred::StompHit {
                    caster: h,
                    target: o,
                    amount: n.data[0],
                    stun,
                    at: c.effect_q,
                });
            }
            deferred.push(Deferred::StompEnd {
                end: add(c.effect_q, n.duration),
            });
        }
        _ => {}
    }
    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
        let mana = ow3_math::sub(u.mana.value(), ow3_math::i2r(n.cost));
        u.mana.set(mana);
        u.ready.retain(|&(a, _)| a != c.ability);
        u.ready
            .push((c.ability, tick + super::attack::ticks(n.cooldown)));
    }
    true
}

/// A Mirror Image under way (SPEC-0040): the hero, the tick its units
/// appear, the numbers of its level, and once they are made, each unit
/// with the tick its missile arrives, the hero's place among them.
#[derive(Clone, Debug, PartialEq)]
pub struct Mirror {
    hero: u32,
    appear: u64,
    /// The queue time of the timer that makes the units: the effect's
    /// plus DataD; their checks count from it (SPEC-0019; c-SAK-1:
    /// 36.299999 at 7262).
    appear_q: u32,
    numbers: crate::data::AbilityLevel,
    missile_speed: u32,
    made: Vec<(u32, u64)>,
    hero_place: usize,
}

/// 1 degree in radians.
const DEGREE: u32 = 0x3C8E_FA35;

/// The places of Mirror Image round `(x, y)` with facing `f` (radians):
/// `n` places at `distance`, place `k` at `f + 90 - k * 360 / n` degrees,
/// snapped to the cell grid as SPEC-0040 gives.
fn mirror_places(x: u32, y: u32, f: u32, n: u32, distance: u32) -> Vec<[u32; 2]> {
    let i2r = ow3_math::i2r;
    let floor = ow3_math::floor;
    let thirty_two = i2r(32);
    (0..n)
        .map(|k| {
            let degrees = sub(i2r(90), div(i2r(k as i32 * 360), i2r(n as i32)));
            let t = add(f, mul(degrees, DEGREE));
            let (px, py) = (add(x, mul(distance, cos(t))), add(y, mul(distance, sin(t))));
            let snap = |v: u32, low: i32| {
                add(
                    mul(thirty_two, floor(div(sub(v, i2r(low)), thirty_two))),
                    i2r(16),
                )
            };
            [snap(px, 32), snap(py, 64)]
        })
        .collect()
}

/// Mirror Image takes effect for hero `h`: hidden at once, its target
/// checks stopped, its attackers dropping it; the units come DataD later
/// (SPEC-0040; c-SAK-1: effect 7162, units 7262).
fn mirror_image(s: &mut State, h: u32, ability: i32, n: &crate::data::AbilityLevel, effect_q: u32) {
    let tick = s.ticks;
    super::table::mirror_effect(s, h, n.cost);
    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
        u.hidden = true;
        u.checks_off = true;
        // Its regeneration from the attributes stops while it is away
        // (SPEC-0040).
        u.mana.hide(crate::data::attribute_regen(u.intelligence));
        u.life.hide(crate::data::attribute_regen(u.strength));
    }
    movement::stop(s, h);
    movement::target_died(s, h);
    let missile_speed = s
        .data
        .abilities
        .get(&ability)
        .map_or(0, |a| a.missile_speed);
    s.mirrors.push(Mirror {
        hero: h,
        appear: tick + super::attack::ticks(n.data_d),
        appear_q: add(effect_q, n.data_d),
        numbers: *n,
        missile_speed,
        made: Vec::new(),
        hero_place: 0,
    });
}

/// The Mirror Images of this tick: their units appear, and the missiles
/// to their places arrive (SPEC-0040).
fn mirrors(s: &mut State) {
    let tick = s.ticks;
    let mut list = std::mem::take(&mut s.mirrors);
    for m in &mut list {
        if m.appear == tick {
            mirror_appear(s, m);
        }
        let arrived: Vec<(usize, u32)> = m
            .made
            .iter()
            .enumerate()
            .filter(|(_, (_, at))| *at == tick)
            .map(|(k, (unit, _))| (k, *unit))
            .collect();
        for (k, unit) in arrived {
            if k == m.hero_place {
                super::table::mirror_hero_back(s, m.hero, unit);
                mirror_hero_back(s, m.hero, unit);
                s.summons.push((m.hero, unit));
            } else {
                if let Some(Object::Unit(u)) = s.objects.get_mut(&unit) {
                    u.hidden = false;
                }
                s.summons.push((m.hero, unit));
            }
        }
    }
    let last = |m: &Mirror| m.made.iter().map(|&(_, at)| at).max().unwrap_or(m.appear);
    list.retain(|m| m.appear > tick || last(m) > tick);
    s.mirrors.extend(list);
}

/// The units of Mirror Image `m` appear, all hidden: a draw of game
/// generator 12 chooses the hero's place, then for each place in order an
/// illusion of the hero is made (its own draws of generator 2 and of the
/// script generator); the one at the hero's place stands in for him until
/// his missile arrives. Each lasts `Dur` from now (SPEC-0040; c-SAK-1).
///
/// The missiles fly straight from the hero's launch point at the
/// ability's missile speed (c-SAK-1: arrivals at 7288 and 7297).
fn mirror_appear(s: &mut State, m: &mut Mirror) {
    let tick = s.ticks;
    let Some(hero) = s.unit(Some(m.hero)).filter(|u| u.alive).cloned() else {
        return;
    };
    let images = f32::from_bits(m.numbers.data[0]) as u32;
    let n = images + 1;
    let places = mirror_places(hero.x, hero.y, hero.facing, n, m.numbers.range);
    // A die of `n` sides on the raw output (SPEC-0040; c-SAK-1: 0xEA8CF3FD
    // at 7262, place 1).
    let r = s.game_random[MIRROR_PLACE].step();
    m.hero_place = ((u64::from(r) * u64::from(n)) >> 32) as usize;
    let lasts = super::attack::ticks(m.numbers.duration);
    let facing = super::facing::to_degrees(hero.facing);
    // The missiles leave from the hero's launch point (`launchY` ahead,
    // `launchX` to the right), floored to whole units (c-SAK-1: from
    // (1227, -1)).
    let launch = {
        let [side, ahead] = s.data.units.get(&hero.type_id).map_or([0, 0], |t| t.launch);
        let (fx, fy) = (cos(hero.facing), sin(hero.facing));
        [
            ow3_math::floor(add(hero.x, add(mul(fx, ahead), mul(fy, side)))),
            ow3_math::floor(add(hero.y, sub(mul(fy, ahead), mul(fx, side)))),
        ]
    };
    for place in places {
        let Some(e) =
            super::widgets::create_unit(s, hero.owner, hero.type_id, [place[0], place[1], facing])
        else {
            continue;
        };
        // The units enter their regions where they appear, at the queue
        // time of the ability's timer (SPEC-0049). The one at the hero's
        // place too (c-SAK-1: the image 20 and the stand-in 21 at 7262).
        let now = s.event_time.replace(m.appear_q);
        super::regions::test_unit(s, e);
        s.event_time = now;
        // A copy is in no record of the game sum (SPEC-0024).
        if let Some(list) = s.type_records.get_mut(&hero.owner)
            && let Some((_, q)) = list.iter_mut().find(|(t, _)| *t == hero.type_id)
        {
            q[1] -= 1;
            q[4] -= 1;
        }
        if let Some(Object::Unit(u)) = s.objects.get_mut(&e) {
            u.hidden = true;
            u.next_acquire = add(m.appear_q, mul(u.acquire_phase, 0x3F00_0000));
            u.illusion = Some(Illusion {
                dealt: m.numbers.data[1],
                taken: m.numbers.data[2],
            });
            u.level = 0;
            u.xp = 0;
            u.life.max = hero.life.max;
            u.life.set(hero.life.value());
            u.mana.max = hero.mana.max;
            u.mana.set(hero.mana.value());
            u.skills = hero.skills.clone();
            u.strength = hero.strength;
            u.agility = hero.agility;
            u.intelligence = hero.intelligence;
        }
        s.timed_life.push((tick + lasts + SUMMON_OVER, e));
        let at = s.table.now;
        s.table_at(m.appear_q);
        super::table::mirror_copy(s, e);
        s.table.now = at;
        let (dx, dy) = (
            f32::from_bits(sub(place[0], launch[0])),
            f32::from_bits(sub(place[1], launch[1])),
        );
        // Its missile moves speed x 0.005 a tick and arrives in the tick
        // it reaches the place, counted from this one.
        let step = f32::from_bits(m.missile_speed).max(1.0) * f32::from_bits(crate::timers::TICK);
        let steps = ((dx * dx + dy * dy).sqrt() / step).ceil().max(1.0) as u64;
        m.made.push((e, tick + steps));
    }
    let at = s.table.now;
    s.table_at(m.appear_q);
    for &(e, _) in &m.made {
        super::table::mirror_missile(s, e);
    }
    s.table.now = at;
}

/// The missile to the hero's place arrives: the hero stands there and
/// shows again, the stand-in goes, and his target checks start again with
/// 0.01 s (SPEC-0040; c-SAK-1: 7298).
fn mirror_hero_back(s: &mut State, hero: u32, stand_in: u32) {
    let at = s.unit(Some(stand_in)).map(|u| [u.x, u.y]);
    let now = s.move_clock;
    if let (Some(at), Some(Object::Unit(u))) = (at, s.objects.get_mut(&hero)) {
        u.x = at[0];
        u.y = at[1];
        u.hidden = false;
        u.checks_off = false;
        u.mana.show(crate::data::attribute_regen(u.intelligence));
        u.life.show(crate::data::attribute_regen(u.strength));
        if u.acquire_phase != super::NO_PHASE {
            u.next_acquire = add(now, movement::AFTER_DEATH);
        }
    }
    // PROVISIONAL: the hero is tested at his new place as a unit
    // moved by `SetUnitPosition` (SPEC-0049 does not cover it).
    super::regions::test_unit(s, hero);
    super::obstacles::died(s, stand_in);
    s.objects.remove(&stand_in);
    s.units.retain(|&u| u != stand_in);
}

/// The game generator of the hero's place among Mirror Image's places
/// (SPEC-0040).
const MIRROR_PLACE: usize = 12;

/// What makes a unit an illusion (SPEC-0040): the factors of the damage
/// it deals and takes (`DataB`, `DataC` of Mirror Image).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Illusion {
    pub dealt: u32,
    pub taken: u32,
}

/// The Wind Walk fades that end in this tick, at the start of the tick
/// before the unit updates, read at the fade timer's expiry (SPEC-0040):
/// the unit's target checks stop until its next order, and an attack it
/// has is reset and taken up again, the unit stopping where it is and
/// searching towards its target's position at its update (c-SAK-1: the
/// Blademaster at 520 and 4621).
pub(super) fn fades(state: &mut State) {
    let tick = state.ticks;
    let faded: Vec<(u32, u32)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let w = state.unit(Some(h))?.wind_walk?;
            (w.fade == tick && w.on(tick)).then_some((h, w.fade_clock))
        })
        .collect();
    for (h, clock) in faded {
        // An attack under way ends and is taken up again as a new goal with
        // its walk, before the state parts of the fade go (SPEC-0024;
        // c-SAK-1: the Blademaster at 4620).
        let attacking = movement::attacks(state, h);
        if attacking {
            super::table::attack_ended_at_fade(state, h);
        }
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.checks_off = true;
        }
        state.table.at_fade = true;
        movement::regive_attack(state, h, clock);
        state.table.at_fade = false;
        if attacking {
            super::table::walk_made_at_order(state, h);
        }
        super::table::wind_walk_faded(state, h);
    }
}

/// A Chain Lightning or Healing Wave on its way (SPEC-0040): it reaches
/// `target` at tick `at` with `amount`, then jumps every 0.25 s to
/// another unit within `area` of it, each time with `factor` of the
/// amount, until `left` units are reached.
#[derive(Clone, Debug, PartialEq)]
pub struct Bounce {
    caster: u32,
    owner: i32,
    kind: BounceKind,
    target: u32,
    at: u64,
    amount: u32,
    factor: u32,
    left: u32,
    area: f32,
    hit: Vec<u32>,
    /// A Flame Strike's point and the queue time of its next hit.
    point: [u32; 2],
    q: u32,
    /// An Impale wave's time in the air, and its stun after the landing
    /// for units and heroes.
    times: [u32; 3],
}

/// What a [`Bounce`] does where it arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BounceKind {
    /// Chain Lightning's damage.
    Lightning,
    /// Healing Wave's heal.
    Heal,
    /// Mana Burn's loss of mana and as much damage, once.
    ManaBurn,
    /// Impale's damage when a thrown unit lands, once.
    Land,
    /// Impale's wave: its next point, `left` of them to come.
    Wave,
    /// Entangling Roots' damage: a 0 event, nine hits a second apart, a
    /// last 0 event.
    Roots,
    /// Flame Strike's hits on the units round its point.
    Flame,
    /// The point mover a Flame Strike's order makes 0.01 s after it.
    Mover,
    /// A Flame Strike area's timer of random delays (game generator 23).
    Flames,
}

/// How long the objects of a bounce's effect and jumps are kept: 1 s for
/// Chain Lightning, 2 s for Healing Wave (c-SBI-1).
fn bounce_life(kind: BounceKind) -> u32 {
    if kind == BounceKind::Heal {
        2.0f32
    } else {
        1.0f32
    }
    .to_bits()
}

/// Fan of Knives' effect (`b"AEfk"` in `effect`): a knife at each enemy
/// within Area, the nearest first, at most DataB / DataA of them (DataB 0:
/// no limit). Each draws on game generator 25 (its look) and 26 (its
/// speed, Missilespeed + floor(out * 100 / 2^32)), and flies from the
/// caster's centre as one timer, its objects freed 5.0 s after its hit
/// (c-SBK-1: knives at kobolds 7, 5, 6 and ogre 4, speeds 717, 788, 798,
/// 768).
/// PROVISIONAL: the 100 is a field of the missile's data not yet found;
/// whether the search sorts by distance or only came out so is not read.
fn fan_of_knives(
    s: &mut State,
    h: u32,
    c: &Casting,
    n: &crate::data::AbilityLevel,
    deferred: &mut Vec<Deferred>,
) {
    let Some(u) = s.unit(Some(h)) else {
        return;
    };
    let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
    let area = f32::from_bits(n.area);
    let mut near: Vec<(u32, f32)> = s
        .units
        .iter()
        .copied()
        .filter_map(|o| {
            let t = s.unit(Some(o))?;
            let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
            let d = dx * dx + dy * dy;
            (t.alive && !t.hidden && movement::enemies(s, owner, t.owner) && d <= area * area)
                .then_some((o, d))
        })
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (amount, cap) = (f32::from_bits(n.data[0]), f32::from_bits(n.data[1]));
    if cap > 0.0 && amount > 0.0 {
        near.truncate((cap / amount) as usize);
    }
    super::table::clap_effect(s, h, n.cost);
    let base = s
        .data
        .abilities
        .get(&c.ability)
        .map_or(0, |a| f32::from_bits(a.missile_speed) as u32);
    for (target, _) in near {
        s.game_random[KNIFE_LOOK].step();
        let out = s.game_random[KNIFE_SPEED].step();
        let speed = base + ((u64::from(out) * 100) >> 32) as u32;
        let objects = super::table::missile_launched(s);
        let speed = (speed as f32).to_bits();
        match super::attack::launch_spell_missile(s, h, target, n.data[0], speed, c.effect_q, false)
        {
            Some(mut missile) => {
                missile.spell = Some(super::attack::KNIFE);
                missile.objects = objects;
                s.missiles.push(missile);
            }
            None => deferred.push(Deferred::SpellHit {
                caster: h,
                target,
                amount: n.data[0],
                stun: super::attack::KNIFE,
                objects,
            }),
        }
    }
}

/// Carrion Swarm's effect (`b"AUcs"` in `effect`): a wave (Missilespeed)
/// DataC long towards the point, Area wide at the caster and DataD at its
/// end; each enemy in it takes an object, then DataA, at most DataB in
/// all, the nearest along the line first; then the caster's area fog
/// modifier. The wave's five missile objects go 5.0 s after its end
/// (c-SBJ-1: the effect at 9.8, kobolds 6 and 5, objects freed at
/// 10.124302, 12.80 and 15.533394).
/// PROVISIONAL: every unit in the wave is taken at the effect, and the
/// unit's object kept 0.32 s; the wave's travel is not read.
fn carrion_swarm(
    s: &mut State,
    h: u32,
    c: &Casting,
    n: &crate::data::AbilityLevel,
    deferred: &mut Vec<Deferred>,
) {
    let (Some(point), Some(u)) = (c.point, s.unit(Some(h))) else {
        return;
    };
    let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
    let (dx, dy) = (f32::from_bits(point[0]) - x, f32::from_bits(point[1]) - y);
    let length = f32::from_bits(ow3_math::natives::sqrt(add(
        (dx * dx).to_bits(),
        (dy * dy).to_bits(),
    )));
    let (reach, start, end) = (
        f32::from_bits(n.data[2]),
        f32::from_bits(n.area),
        f32::from_bits(n.data_d),
    );
    let mut hit: Vec<(u32, f32)> = s
        .units
        .iter()
        .copied()
        .filter_map(|o| {
            let t = s.unit(Some(o))?;
            let (px, py) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
            let (along, across) = if length > 0.0 {
                (
                    (px * dx + py * dy) / length,
                    (px * dy - py * dx).abs() / length,
                )
            } else {
                (0.0, (px * px + py * py).sqrt())
            };
            let half = (start + (end - start) * along / reach) / 2.0;
            (t.alive
                && !t.hidden
                && movement::enemies(s, owner, t.owner)
                && (0.0..=reach).contains(&along)
                && across <= half)
                .then_some((o, along))
        })
        .collect();
    hit.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (amount, cap) = (f32::from_bits(n.data[0]), f32::from_bits(n.data[1]));
    if cap > 0.0 && amount > 0.0 {
        hit.truncate((cap / amount) as usize);
    }
    super::table::clap_effect(s, h, n.cost);
    let objects = super::table::missile_launched(s);
    let speed = s
        .data
        .abilities
        .get(&c.ability)
        .map_or(0, |a| a.missile_speed);
    if f32::from_bits(speed) > 0.0 {
        let gone = add(c.effect_q, ow3_math::div(n.data[2], speed));
        super::table::missile_hit(s, &objects, gone);
    }
    for (target, _) in hit {
        deferred.push(Deferred::Kept {
            end: add(c.effect_q, 0.32f32.to_bits()),
        });
        deferred.push(Deferred::Damage {
            caster: h,
            target,
            amount: n.data[0],
            call: None,
            slow: None,
            at: None,
            fog: None,
            later_calls: false,
        });
    }
    deferred.push(Deferred::ClapArea {
        at: c.effect_q,
        first: Some(h),
    });
}

/// The game generators of a knife's look and speed (c-SBK-1).
const KNIFE_LOOK: usize = 25;
const KNIFE_SPEED: usize = 26;

/// Impale's effect (`b"AUim"` in `effect`).
fn impale(s: &mut State, h: u32, c: &Casting, n: &crate::data::AbilityLevel) {
    let (Some(point), Some(owner)) = (c.point, s.unit(Some(h)).map(|u| u.owner)) else {
        return;
    };
    super::table::clap_effect(s, h, n.cost);
    let mut wave = Bounce {
        caster: h,
        owner,
        kind: BounceKind::Wave,
        target: 0,
        at: s.ticks,
        amount: n.data[2],
        factor: 0,
        left: WAVE_STEPS + 1,
        area: f32::from_bits(n.data[0]),
        hit: Vec::new(),
        point,
        q: c.effect_q,
        times: [n.data_d, n.duration, n.hero_duration],
    };
    // Its first point at the effect itself.
    wave_step(s, &mut wave);
    if wave.left > 0 {
        s.bounces.push(wave);
    }
}

/// The steps of an Impale wave after its first point, 0.06 s apart, each
/// DataA / 5 further on (c-SBJ-1).
/// PROVISIONAL: the 5 steps, the 0.06 s and the first point's 125 are
/// read in the game, their data fields are not found.
const WAVE_STEPS: u32 = 5;
const IMPALE_STEP: f32 = 0.06;
const WAVE_FIRST: f32 = 125.0;

/// The next point of Impale wave `b`: the enemy ground units within 125
/// plus their collision size of it, not reached before, are thrown up:
/// stunned for the time in the air and then Dur (heroes HeroDur), taking
/// DataC when they land (SPEC-0041; c-SBJ-1: three at the effect, 6.9,
/// landing at 7.9, order back at 9.91).
fn wave_step(s: &mut State, b: &mut Bounce) {
    let k = WAVE_STEPS + 1 - b.left;
    let Some([x, y]) = s
        .unit(Some(b.caster))
        .map(|u| [f32::from_bits(u.x), f32::from_bits(u.y)])
    else {
        b.left = 0;
        return;
    };
    let (dx, dy) = (
        f32::from_bits(b.point[0]) - x,
        f32::from_bits(b.point[1]) - y,
    );
    let length = f32::from_bits(ow3_math::natives::sqrt(add(
        (dx * dx).to_bits(),
        (dy * dy).to_bits(),
    )));
    let along = WAVE_FIRST + b.area / WAVE_STEPS as f32 * k as f32;
    let (px, py) = if length > 0.0 {
        (x + dx / length * along, y + dy / length * along)
    } else {
        (x, y)
    };
    let reached: Vec<u32> = s
        .units
        .iter()
        .copied()
        .filter(|&o| {
            !b.hit.contains(&o)
                && s.unit(Some(o)).is_some_and(|t| {
                    let data = s.data.units.get(&t.type_id);
                    let ground = data.is_some_and(|d| d.target_type.iter().any(|c| c == "ground"));
                    let reach = WAVE_FIRST + data.map_or(0.0, |d| f32::from_bits(d.collision));
                    let (ex, ey) = (f32::from_bits(t.x) - px, f32::from_bits(t.y) - py);
                    t.alive
                        && !t.hidden
                        && ground
                        && movement::enemies(s, b.owner, t.owner)
                        && ex * ex + ey * ey <= reach * reach
                })
        })
        .collect();
    let now = add(b.q, (IMPALE_STEP * k as f32).to_bits());
    let ahead = movement::angle(dx.to_bits(), dy.to_bits());
    let back = add(ahead, std::f32::consts::PI.to_bits());
    let back = if f32::from_bits(back) >= std::f32::consts::TAU {
        ow3_math::sub(back, std::f32::consts::TAU.to_bits())
    } else {
        back
    };
    let [air, dur, hero_dur] = b.times;
    let landing = s.ticks + super::attack::ticks(air);
    for o in movement::call_order(s, reached) {
        b.hit.push(o);
        let hero = s.unit(Some(o)).is_some_and(|t| is_hero(t.type_id));
        super::table::stun_objects(s, b.caster, o, Vec::new());
        stun(
            s,
            o,
            b.caster,
            add(air, if hero { hero_dur } else { dur }),
            now,
            true,
        );
        if let Some(Object::Unit(t)) = s.objects.get_mut(&o) {
            t.entries.impale = true;
            // It is turned to face back along the wave: its facing and its
            // walk heading the wave's direction + pi (c-SBJ-1: kobolds 7 and 8
            // at pi after the wave of direction 0).
            t.facing = back;
            if let Some(m) = t.movement.as_mut() {
                m.heading = back;
            }
        }
        s.bounces.push(Bounce {
            caster: b.caster,
            owner: b.owner,
            kind: BounceKind::Land,
            target: o,
            at: landing,
            amount: b.amount,
            factor: 0,
            left: 1,
            area: 0.0,
            hit: Vec::new(),
            point: [0, 0],
            q: 0,
            times: [0; 3],
        });
    }
    b.left -= 1;
    let next = add(b.q, (IMPALE_STEP * (k + 1) as f32).to_bits());
    b.at = tick_of(s, next);
}

/// The next unit bounce `b` jumps to from `[x, y]`: for the lightning the
/// nearest enemy not hit yet, for the wave the most wounded friend not
/// healed yet, within its area; not an invulnerable one (c-SBM-1: the
/// paused creeps).
fn next_bounce(s: &State, b: &Bounce, [x, y]: [f32; 2]) -> Option<(u32, f32)> {
    let mut best: Option<(u32, f32)> = None;
    for &o in &s.units {
        let Some(t) = s.unit(Some(o)) else {
            continue;
        };
        if b.hit.contains(&o) || o == b.target || !t.alive || t.hidden || invulnerable(s, o) {
            continue;
        }
        let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
        let d2 = dx * dx + dy * dy;
        if d2 > b.area * b.area {
            continue;
        }
        let enemy = movement::enemies(s, b.owner, t.owner);
        let key = if b.kind == BounceKind::Heal {
            let missing = f32::from_bits(t.life.max) - f32::from_bits(t.life.value());
            if enemy || missing <= 0.0 {
                continue;
            }
            -missing
        } else {
            if !enemy {
                continue;
            }
            d2
        };
        if best.is_none_or(|(_, k)| key < k) {
            best = Some((o, key));
        }
    }
    best
}

/// The time between two jumps, 0.25 s.
const BOUNCE_TICKS: u64 = 50;

/// A new bounce from the effect of `c` (Chain Lightning or Healing Wave).
fn bounce(s: &mut State, h: u32, c: &Casting, n: &crate::data::AbilityLevel, kind: BounceKind) {
    // Chain Lightning's effect: its event's objects and one kept 1 s (c-SBI-1:
    // 2.879976, freed at 3.880051). PROVISIONAL: the other spells' objects.
    // Healing Wave's alike, kept 2 s (c-SBI-1: 6.899738 to 8.899607).
    if kind != BounceKind::ManaBurn && kind != BounceKind::Roots {
        super::table::spell_effect_event(s, h);
        let tick = s.table.now;
        let o = s.table.take(tick, "bounce: effect");
        let end = ow3_math::add(c.effect_q, bounce_life(kind));
        s.table.free_at_q(end, o, "bounce: effect");
    }
    let (Some(target), Some(owner)) = (c.target, s.unit(Some(h)).map(|u| u.owner)) else {
        return;
    };
    // Mana Burn reaches its unit DataB after the cast point, once.
    let (left, at) = if kind == BounceKind::ManaBurn {
        (1, s.ticks + super::attack::ticks(n.data[1]))
    } else if kind == BounceKind::Roots {
        (10, s.ticks + 2)
    } else {
        (
            f32::from_bits(n.data[1]).max(1.0) as u32,
            s.ticks + BOUNCE_TICKS,
        )
    };
    s.bounces.push(Bounce {
        caster: h,
        owner,
        kind,
        target,
        at,
        amount: n.data[0],
        factor: ow3_math::sub(1.0f32.to_bits(), n.data[2]),
        left,
        area: f32::from_bits(n.area),
        hit: Vec::new(),
        point: [0, 0],
        q: 0,
        times: [0; 3],
    });
}

/// The tick whose clock, at its end, reaches queue time `q`.
pub(super) fn tick_of(s: &State, q: u32) -> u64 {
    let (mut tick, mut clock) = (s.ticks, s.move_clock);
    while f32::from_bits(q) > f32::from_bits(add(clock, crate::timers::TICK)) {
        clock = add(clock, crate::timers::TICK);
        tick += 1;
    }
    tick
}

/// A Flame Strike from `h` at `point`: it lands `Cast` after the cast
/// point and hits the enemies within Area of it with DataA then and every
/// DataB after, `HeroDur / DataB + 1` times (SPEC-0039: 9).
///
/// PROVISIONAL: the steps are exact 0.33 s on the clock, the game's drift
/// to 67 ticks once (657) not read; the later DataC a second, the hits'
/// order and the spell's objects are not built.
fn flame_strike(s: &mut State, h: u32, c: &Casting, n: &crate::data::AbilityLevel) {
    let (Some(point), Some(owner)) = (c.point, s.unit(Some(h)).map(|u| u.owner)) else {
        return;
    };
    super::table::clap_effect(s, h, n.cost);
    let q = add(c.effect_q, n.cast);
    let at = tick_of(s, q);
    s.bounces.push(Bounce {
        caster: h,
        owner,
        kind: BounceKind::Flame,
        target: h,
        at,
        amount: n.data[0],
        factor: n.data[1],
        left: u32::MAX,
        area: f32::from_bits(n.area),
        hit: Vec::new(),
        point,
        q,
        // The area's start (set at the landing), first phase and life;
        // the second phase's step and amount.
        times: [q, n.hero_duration, n.duration],
    });
    s.flame_second.push((h, n.data[2], n.data_d));
}

/// The game generator of a Flame Strike area's timer of random delays.
const FLAME_GENERATOR: usize = 23;

/// A firing of a Flame Strike area's timer of random delays (`Flames`):
/// its draws place flames only; the timer starts again with a delay
/// drawn last, and stops with the area (c-SBH-1, 02:10: 2n + 4 steps for
/// a count n).
fn flames_fire(s: &mut State, b: &mut Bounce) {
    // At its firing: n = 2 + two draws of 0 .. 9; past 5 one flame near
    // the centre (two draws) and n - 1; a start angle; two draws a flame;
    // then the next delay, a uniform draw.
    let g = &mut s.game_random[FLAME_GENERATOR];
    let die = |g: &mut crate::random::Random| ((u64::from(g.step()) * 10) >> 32) as u32;
    let mut n = 2 + die(g) + die(g);
    if n > 5 {
        g.step();
        g.step();
        n -= 1;
    }
    g.step();
    for _ in 0..n {
        g.step();
        g.step();
    }
    let out = g.step();
    let delay = (f32::from_bits(0x3F80_0000 | (out & 0x7F_FFFF)) - 1.0).to_bits();
    b.q = add(b.q, delay);
    // It stops with the area (`factor` holds the area's end).
    if f32::from_bits(b.q) > f32::from_bits(b.factor) {
        b.left = 0;
    } else {
        b.at = tick_of(s, b.q);
    }
}

/// The bounces that reach their units in this tick: a Chain Lightning's
/// damage, a Healing Wave's heal (never above the maximum), then the
/// next unit: for the lightning the nearest enemy not hit yet, for the
/// wave the most wounded friend not healed yet, within the area of the
/// unit just reached (SPEC-0040).
///
/// PROVISIONAL: ties keep the first unit made; "most wounded" taken as
/// the most life missing; the objects of both spells are not read.
fn bounces(s: &mut State, deferred: &mut Vec<Deferred>) {
    let tick = s.ticks;
    let mut all = std::mem::take(&mut s.bounces);
    let mut more = Vec::new();
    for b in &mut all {
        if b.at != tick {
            continue;
        }
        // Entangling Roots: 0 at 30 ticks after the cast point, DataA 2 ticks
        // later and every second, nine times, then 0 (SPEC-0041).
        if b.kind == BounceKind::Wave {
            wave_step(s, b);
            continue;
        }
        if b.kind == BounceKind::Mover {
            let objects = super::table::missile_launched(s);
            super::table::missile_hit(s, &objects, add(b.q, b.factor));
            b.left = 0;
            continue;
        }
        if b.kind == BounceKind::Flames {
            flames_fire(s, b);
            continue;
        }
        if b.kind == BounceKind::Flame {
            let [start, first, life] = b.times;
            let end = add(add(start, life), 0.01f32.to_bits());
            // At the landing the area's objects, kept, and its timer of
            // random delays (c-SBH-1: 1021 .. 1024 at 4.13).
            if b.q == start {
                let tick = s.table.now;
                for _ in 0..4 {
                    let o = s.table.take(tick, "flame strike: area");
                    s.table
                        .free_at_q(add(end, 5.0f32.to_bits()), o, "flame strike: area");
                }
                let flames = Bounce {
                    caster: b.caster,
                    owner: b.owner,
                    kind: BounceKind::Flames,
                    target: b.caster,
                    at: tick,
                    amount: 0,
                    factor: end,
                    left: 1,
                    area: 0.0,
                    hit: Vec::new(),
                    point: [0, 0],
                    q: start,
                    times: [0; 3],
                };
                let mut flames = flames;
                flames_fire(s, &mut flames);
                if flames.left > 0 {
                    more.push(flames);
                }
            }
            let [x, y] = b.point.map(f32::from_bits);
            let hit: Vec<u32> = s
                .units
                .iter()
                .copied()
                .filter(|&o| {
                    s.unit(Some(o)).is_some_and(|t| {
                        let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
                        t.alive
                            && !t.hidden
                            && movement::enemies(s, b.owner, t.owner)
                            && dx * dx + dy * dy <= b.area * b.area
                    })
                })
                .collect();
            // The step's timer again, DataB on the clock; at the first
            // phase's end (HeroDur) it is started anew with DataD, the hits
            // then DataC (c-SBH-1: 4.00 from +3.67 to +8.67).
            let second = add(start, first);
            let mut next = add(b.q, b.factor);
            let mut switch = None;
            if f32::from_bits(b.q) < f32::from_bits(second)
                && f32::from_bits(next) > f32::from_bits(second)
                && let Some(&(_, amount, step)) =
                    s.flame_second.iter().find(|(c, _, _)| *c == b.caster)
            {
                next = add(second, step);
                switch = Some((amount, step));
            }
            // Per unit the step's object, kept the step's time less 0.01 s
            // (0.32 s, then 0.99 s), then the hit; the area's fog modifier after them
            // (c-SBH-1: footmen 3, 1, 4, 2; 1037 freed 3 s after the last
            // step).
            let kept = add(b.q, ow3_math::sub(b.factor, 0.01f32.to_bits()));
            for o in movement::call_order(s, hit) {
                deferred.push(Deferred::Kept { end: kept });
                deferred.push(Deferred::Damage {
                    caster: b.caster,
                    target: o,
                    amount: b.amount,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
            }
            deferred.push(Deferred::ClapArea {
                at: b.q,
                first: Some(b.caster),
            });
            if let Some((amount, step)) = switch {
                b.factor = step;
                b.amount = amount;
            }
            if f32::from_bits(next) > f32::from_bits(end) {
                b.left = 0;
                s.flame_second.retain(|(c, _, _)| *c != b.caster);
            } else {
                b.q = next;
                b.at = tick_of(s, next);
            }
            continue;
        }
        if b.kind == BounceKind::Roots {
            let amount = if b.left == 11 || b.left == 1 {
                0
            } else {
                b.amount
            };
            if s.unit(Some(b.target)).is_some_and(|u| u.alive) {
                deferred.push(Deferred::Damage {
                    caster: b.caster,
                    target: b.target,
                    amount,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
            } else {
                b.left = 1;
            }
            b.at = tick
                + match b.left {
                    11 => 2,
                    2 => 199,
                    _ => 200,
                };
            b.left -= 1;
            continue;
        }
        let alive = s.unit(Some(b.target)).is_some_and(|u| u.alive);
        if alive {
            // The object of the jump on from this unit, kept as the effect's,
            // made before the hit or heal; none at the last (c-SBI-1: the
            // lightning at 627, 677 and 727, not 777; the wave at 1431 and
            // 1481). PROVISIONAL: a jump taken as due while units are left.
            let from = s
                .unit(Some(b.target))
                .map(|u| [f32::from_bits(u.x), f32::from_bits(u.y)]);
            let next = from.is_some_and(|p| next_bounce(s, b, p).is_some());
            if (b.kind == BounceKind::Lightning || b.kind == BounceKind::Heal) && b.left > 1 && next
            {
                let now = s.table.now;
                let o = s.table.take(now, "bounce: jump");
                let q = s.event_time.unwrap_or_else(|| s.timers.time());
                let end = ow3_math::add(q, bounce_life(b.kind));
                s.table.free_at_q(end, o, "bounce: jump");
            }
            if b.kind == BounceKind::ManaBurn {
                let mut burnt = 0;
                if let Some(Object::Unit(t)) = s.objects.get_mut(&b.target) {
                    let mana = t.mana.value();
                    burnt = if f32::from_bits(mana) < f32::from_bits(b.amount) {
                        mana
                    } else {
                        b.amount
                    };
                    t.mana.set(ow3_math::sub(mana, burnt));
                }
                deferred.push(Deferred::Damage {
                    caster: b.caster,
                    target: b.target,
                    amount: burnt,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
            } else if b.kind == BounceKind::Heal {
                if let Some(Object::Unit(t)) = s.objects.get_mut(&b.target) {
                    let life = add(t.life.value(), b.amount);
                    let life = if f32::from_bits(life) > f32::from_bits(t.life.max) {
                        t.life.max
                    } else {
                        life
                    };
                    t.life.set(life);
                }
            } else {
                deferred.push(Deferred::Damage {
                    caster: b.caster,
                    target: b.target,
                    amount: b.amount,
                    call: None,
                    slow: None,
                    at: None,
                    fog: None,
                    later_calls: false,
                });
            }
        }
        b.hit.push(b.target);
        b.left = b.left.saturating_sub(1);
        let Some([x, y]) = s
            .unit(Some(b.target))
            .map(|u| [f32::from_bits(u.x), f32::from_bits(u.y)])
        else {
            b.left = 0;
            continue;
        };
        let best = next_bounce(s, b, [x, y]);
        match best {
            Some((o, _)) if b.left > 0 => {
                b.target = o;
                b.at = tick + BOUNCE_TICKS;
                b.amount = ow3_math::mul(b.amount, b.factor);
            }
            _ => b.left = 0,
        }
    }
    all.retain(|b| b.left > 0);
    s.bounces.extend(all);
    s.bounces.extend(more);
}

/// A Resurrection of `h` (c-SBV-1 at 3.5): up to DataA dead units of its
/// owner, not heroes nor buildings, within Area of it, the nearest first,
/// stand up again where they lie with full life and no order.
///
/// PROVISIONAL: the order of the units is read from one case (all three
/// footmen nearest first); a tie, other players' dead and a decayed corpse
/// are not read.
fn resurrection(s: &mut State, h: u32, n: &crate::data::AbilityLevel, q: u32, animate: bool) {
    let Some(u) = s.unit(Some(h)) else {
        return;
    };
    let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
    let area = f32::from_bits(n.area);
    let mut dead: Vec<(f32, u32)> = s
        .units
        .iter()
        .filter_map(|&o| {
            let Some(Object::Unit(t)) = s.objects.get(&o) else {
                return None;
            };
            let d = s.data.units.get(&t.type_id)?;
            let (dx, dy) = (f32::from_bits(t.x) - x, f32::from_bits(t.y) - y);
            let far = dx * dx + dy * dy;
            (!t.alive
                && (animate || t.owner == owner)
                && !is_hero(t.type_id)
                && !d.building
                && t.entries.dead_corpse.is_some()
                && far <= area * area)
                .then_some((far, o))
        })
        .collect();
    dead.sort_by(|a, b| a.0.total_cmp(&b.0));
    let count = f32::from_bits(n.data[0]) as usize;
    for (_, t) in dead.into_iter().take(count) {
        if animate {
            super::table::animated(s, t);
            s.timed_life_q.push((add(q, n.duration), t));
        } else {
            super::table::resurrected(s, t);
        }
        let Some(Object::Unit(u)) = s.objects.get_mut(&t) else {
            continue;
        };
        // Raised, it is the caster's, cannot be hurt and looks for a target
        // 0.01 s after the effect (c-SBX-1: the three kobolds at 3.459962).
        // PROVISIONAL: the invulnerability is read from the log (no hits on
        // them).
        // Brought back, it checks for a target 0.01 s after the effect, on a
        // new phase (c-SBV-1: the footmen at 3.51, then every 0.5 s, hub #9).
        u.next_acquire = add(q, 0.01f32.to_bits());
        if animate {
            u.owner = owner;
            u.script_invulnerable = true;
        }
        if animate {
            for g in &mut s.creep_groups {
                g.retain(|&m| m != t);
            }
        }
        u.alive = true;
        u.died_at = None;
        u.attack = None;
        u.movement = None;
        u.holding = false;
        let max = u.life.max;
        u.life.set(max);
        let type_id = u.type_id;
        super::widgets::type_record(s, owner, type_id, |q| q[4] += 1);
    }
}

/// An Earthquake's area (c-SBW-1, hub #10): its caster and point, Area,
/// the slow (DataC), the queue time of its next action and of its last,
/// its own five objects once it acts, and the units in it with their
/// objects.
#[derive(Clone, Debug, PartialEq)]
pub struct Quake {
    caster: u32,
    point: [u32; 2],
    area: u32,
    slow: u32,
    next: u32,
    stop: u32,
    objects: Vec<u32>,
    inside: Vec<(u32, u32)>,
    /// An object it holds while units are in it, and whether it found none
    /// at its last action.
    occupied: Option<u32>,
    empty: bool,
}

/// How often an Earthquake's area acts after its first action.
const QUAKE_EVERY: f32 = 1.0;

/// The Earthquakes acting in this tick: each takes every living ground
/// unit in its area, of either side, in the grid order of calls: a unit
/// coming in is slowed by DataC, its walk made again at the new speed,
/// and takes an object; a unit gone out is no longer slowed and its object
/// goes. At its last action's time the area's objects and its units' go
/// (c-SBW-1: 3.8 to 27.8, hub #10).
///
/// PROVISIONAL: a unit counts within Area of its centre; its speed
/// back at its leaving is taken as at a slow's end; the damage to
/// buildings (DataB) is not done.
fn quakes(s: &mut State) {
    let tick = s.ticks;
    let next_clock = f32::from_bits(add(s.move_clock, crate::timers::TICK));
    let mut quakes = std::mem::take(&mut s.quakes);
    quakes.retain_mut(|q| {
        // Units dead since its last action took their objects with them.
        q.inside
            .retain(|&(o, _)| s.unit(Some(o)).is_some_and(|u| u.alive));
        while f32::from_bits(q.next) < next_clock {
            s.table_at(q.next);
            if f32::from_bits(q.next) > f32::from_bits(q.stop) {
                super::table::quake_ended(s, &q.objects, &q.inside);
                if let Some(o) = q.occupied.take() {
                    s.table.defer(o, "Earthquake: occupied");
                }
                for &(o, _) in &q.inside {
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&o) {
                        u.move_slow = None;
                    }
                }
                s.table.now = tick;
                return false;
            }
            if q.objects.is_empty() {
                q.objects = super::table::quake_area(s);
            }
            let area = f32::from_bits(q.area);
            let (px, py) = (f32::from_bits(q.point[0]), f32::from_bits(q.point[1]));
            let now: Vec<u32> = s
                .units
                .iter()
                .copied()
                .filter(|&o| {
                    let Some(Object::Unit(u)) = s.objects.get(&o) else {
                        return false;
                    };
                    let Some(t) = s.data.units.get(&u.type_id) else {
                        return false;
                    };
                    let (dx, dy) = (f32::from_bits(u.x) - px, f32::from_bits(u.y) - py);
                    u.alive && !u.hidden && !t.flying && dx * dx + dy * dy <= area * area
                })
                .collect();
            let gone: Vec<(u32, u32)> = q
                .inside
                .iter()
                .copied()
                .filter(|(o, _)| !now.contains(o))
                .collect();
            for (o, obj) in gone {
                super::table::quake_left(s, o, obj);
                if let Some(Object::Unit(u)) = s.objects.get_mut(&o) {
                    u.move_slow = None;
                }
                q.inside.retain(|i| i.0 != o);
            }
            let fresh: Vec<u32> = now
                .into_iter()
                .filter(|o| q.inside.iter().all(|i| i.0 != *o))
                .collect();
            for o in movement::call_order(s, fresh) {
                if let Some(Object::Unit(u)) = s.objects.get_mut(&o) {
                    u.move_slow = Some(q.slow);
                }
                let obj = super::table::quake_entered(s, o);
                q.inside.push((o, obj));
            }
            // Its object while units are in it: made after theirs, freed at the
            // second action in a row that finds none (c-SBW-1: 1215, made at
            // 3.8, freed at 10.8, the Blademaster gone at 9.8).
            // PROVISIONAL: inferred from one area.
            if !q.inside.is_empty() {
                q.empty = false;
                if q.occupied.is_none() {
                    q.occupied = Some(super::table::quake_occupied(s));
                }
            } else if q.empty {
                if let Some(o) = q.occupied.take() {
                    s.table.defer(o, "Earthquake: occupied");
                }
            } else {
                q.empty = true;
            }
            q.next = add(q.next, QUAKE_EVERY.to_bits());
        }
        s.table.now = tick;
        let _ = q.caster;
        true
    });
    s.quakes.extend(quakes);
}
