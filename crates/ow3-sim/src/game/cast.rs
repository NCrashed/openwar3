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
const ABILITY_ORDERS: [(&str, [u8; 4], i32, Aim); 9] = [
    ("blizzard", *b"AHbz", 852_089, Aim::Point),
    ("waterelemental", *b"AHwe", 852_097, Aim::Nothing),
    ("divineshield", *b"AHds", 852_090, Aim::Nothing),
    ("shockwave", *b"AOsh", 852_125, Aim::Enemy),
    ("holybolt", *b"AHhb", 852_092, Aim::Friend),
    ("thunderbolt", *b"AHtb", 852_095, Aim::Enemy),
    ("thunderclap", *b"AHtc", 852_096, Aim::Nothing),
    ("windwalk", *b"AOwk", 852_129, Aim::Nothing),
    ("mirrorimage", *b"AOmi", 852_123, Aim::Nothing),
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
    point: Option<[u32; 2]>,
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
    } else {
        // Out of range a walker also stops at the order and walks on from
        // its next update (c-SAK-1: the Mountain King's Storm Bolt at 500,
        // standing 502 to 505).
        movement::freeze_now(state, h);
        if movement::cast_order(state, h, target, order_id) {
            set(state);
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
    end_wind_walk(state, h, ability);
    // Divine Shield takes effect in the tick of the order, with no cast
    // time, and the caster is left with no order (SPEC-0045; c-RZJ-1:
    // the Paladin at 4801).
    if base_code(state, ability) == i32::from_be_bytes(*b"AHds") {
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
            return Some(true);
        }
        if state.ticks < c.end_at {
            return Some(false);
        }
    }
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
        start(state, h, tick);
    } else {
        if in_range {
            movement::stop(state, h);
        }
        if !movement::cast_point_order(state, h, order_id, at) {
            return Some(false);
        }
        set(state);
    }
    Some(true)
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
    let share = div(n.data_f, ow3_math::i2r(hit.len() as i32));
    let each = if f32::from_bits(share) < f32::from_bits(n.data[1]) {
        share
    } else {
        n.data[1]
    };
    for (target, building) in hit {
        deferred.push(Deferred::Damage {
            caster: h,
            target,
            amount: if building { mul(each, n.data_d) } else { each },
            call: Some((point, n.area)),
        });
    }
}

/// The cast of `h` starts in tick `tick`: the effect `castpt` later.
pub(super) fn start(state: &mut State, h: u32, tick: u64) {
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let (point, backswing) = state
        .data
        .units
        .get(&u.type_id)
        .map_or((0, 0), |t| (t.cast_point, t.cast_backswing));
    let effect = tick + super::attack::ticks(point);
    // A channel ends after its last wave (SPEC-0043).
    let channel = u.casting.and_then(|c| {
        let base = state
            .data
            .abilities
            .get(&c.ability)
            .map_or(c.ability, |a| a.code);
        (&base.to_be_bytes() == b"AHbz").then(|| {
            let waves = f32::from_bits(numbers(state, c.ability, c.level).data[0]) as u64;
            FIRST_WAVE + WAVE_EVERY * waves.saturating_sub(1) + CHANNEL_AFTER
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
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.stunned_until.is_none_or(|t| t <= tick)
    {
        u.stunned_until = Some(tick);
        u.stun_back = Some(back);
        u.stunner = Some(source);
    }
    // A missile's impact at the end of the tick stops it where it is then
    // (c-SAK-1: gnoll 16 at the bolt of 2615); a melee hit, at the start of
    // the tick (c-RXD-1: worker 5's bash).
    movement::stand_still(state, h, impact.then_some(at));
    // Its order goes on after the stun with a new path (c-RXD-1: worker 5
    // stunned in 4337, new path at its update of 4741).
    state.paths.remove(&h);
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
    /// The radius of its circle, its first distance and its length.
    area: u32,
    length: u32,
    amount: u32,
    hit: Vec<u32>,
}

/// The wave moves 105 every 20 ticks (1050 a second) and has a last
/// update at its end (SPEC-0047).
const WAVE_EVERY_TICKS: u64 = 20;
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
    if turned {
        let tick = state.ticks;
        start(state, h, tick);
    } else if movement::cast_order(state, h, target, order_id) {
        set(state);
    }
}

/// Whether creep spell `ability` of `h` can be cast now (SPEC-0047,
/// "Ready").
pub(super) fn creep_ready(state: &State, h: u32, ability: i32, level: i32) -> bool {
    ready(state, h, ability, level) && state.unit(Some(h)).is_some_and(|u| u.casting.is_none())
}

/// The shockwave of `h` on `target` starts (SPEC-0047, "The wave").
fn shockwave(s: &mut State, h: u32, target: u32, n: &crate::data::AbilityLevel) {
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
        for &o in &s.units {
            let Some(Object::Unit(u)) = s.objects.get(&o) else {
                continue;
            };
            if !u.alive || u.hidden || w.hit.contains(&o) || !movement::enemies(s, w.owner, u.owner)
            {
                continue;
            }
            let collision = s.data.units.get(&u.type_id).map_or(0, |t| t.collision);
            let (dx, dy) = (sub(u.x, centre[0]), sub(u.y, centre[1]));
            let reach = add(w.area, collision);
            if f32::from_bits(add(mul(dx, dx), mul(dy, dy))) < f32::from_bits(mul(reach, reach)) {
                w.hit.push(o);
                deferred.push(Deferred::Damage {
                    caster: w.caster,
                    target: o,
                    amount: w.amount,
                    call: Some((centre, w.area)),
                });
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
    }
}

pub(super) fn invulnerable(state: &State, h: u32) -> bool {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.invulnerable_until)
        .is_some_and(|t| tick < t)
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

/// What an effect does after the borrow of the state ends.
enum Deferred {
    /// The hit of a storm bolt launched next to its target.
    SpellHit {
        caster: u32,
        target: u32,
        amount: u32,
        stun: (u32, u32),
    },
    /// Damage of a thunder clap.
    Damage {
        caster: u32,
        target: u32,
        amount: u32,
        /// The centre and area of the spell, for its call for help
        /// (SPEC-0019, "Spell damage").
        call: Option<([u32; 2], u32)>,
    },
}

/// The effects and the ends of the casts of this tick.
pub(super) fn tick(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let mut deferred = Vec::new();
    {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tick = s.ticks;
        let casting: Vec<(u32, Casting)> = s
            .units
            .iter()
            .filter_map(|&h| s.unit(Some(h)).and_then(|u| u.casting).map(|c| (h, c)))
            .collect();
        for (h, c) in casting {
            if c.effect_at == Some(tick) {
                // A spell on a target that died ends the cast at once
                // (SPEC-0035, edge cases; c-RZJ-1: the Paladin's order
                // ended at the effect time 3227 and he acquired at 3229).
                if !effect(s, h, &c, &mut deferred) {
                    movement::end_cast(s, h);
                    continue;
                }
            }
            if let Some(e) = c.effect_at
                && tick > e
            {
                waves(s, h, &c, tick - e, &mut deferred);
            }
            if c.effect_at.is_some() && tick >= c.end_at {
                movement::end_cast(s, h);
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
        let (now, later): (Vec<_>, Vec<_>) = s.timed_life.iter().partition(|(t, _)| *t <= tick);
        s.timed_life = later;
        now.into_iter().map(|(_, h)| h).collect()
    };
    for h in expired {
        super::widgets::kill_unit(vm, state, Some(h), None);
    }
    for d in deferred {
        match d {
            Deferred::SpellHit {
                caster,
                target,
                amount,
                stun,
            } => super::attack::spell_hit(vm, state, caster, target, amount, stun),
            Deferred::Damage {
                caster,
                target,
                amount,
                call,
            } => {
                super::widgets::damage_widget(vm, state, Some(caster), Some(target), amount, 0, 0);
                // The victim and its friends answer the hit (SPEC-0019).
                let mut s = state.borrow_mut();
                if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
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
                    s.timed_life.push((tick + duration(false) + SUMMON_OVER, e));
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
            // 0.6 - 0.0006 are the same float.
            let fade = mul(n.data[0], 0.999f32.to_bits());
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.wind_walk = Some(WindWalk {
                    fade: tick + super::attack::ticks(fade),
                    fade_clock: add(s.move_clock, fade),
                    until,
                    bonus: n.data[1],
                });
            }
            movement::target_died(s, h);
        }
        b"AHds" => {
            let hero = s.unit(Some(h)).is_some_and(|u| is_hero(u.type_id));
            let until = tick + 1 + duration(hero);
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.invulnerable_until = Some(until);
            }
            movement::target_died(s, h);
        }
        // The creep Shockwave (SPEC-0047).
        b"AOsh" => {
            if let Some(target) = c.target {
                shockwave(s, h, target, &n);
            }
        }
        b"AHhb" => {
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
                match super::attack::launch_spell(s, h, target, n.data[0], stun, speed) {
                    Some(missile) => s.missiles.push(missile),
                    None => deferred.push(Deferred::SpellHit {
                        caster: h,
                        target,
                        amount: n.data[0],
                        stun,
                    }),
                }
            }
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
            let centre = [u_x, u_y];
            for o in hit {
                let hero = s.unit(Some(o)).is_some_and(|t| is_hero(t.type_id));
                let until = tick + duration(hero);
                let fresh = attack_slow(s, o).is_none();
                if let Some(Object::Unit(t)) = s.objects.get_mut(&o) {
                    t.slowed_until = Some((until, speed));
                    if fresh {
                        super::attack::slow_cooldown(t, now, speed);
                    }
                }
                deferred.push(Deferred::Damage {
                    caster: h,
                    target: o,
                    amount: n.data[0],
                    call: Some((centre, n.area)),
                });
            }
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
                mirror_hero_back(s, m.hero, unit);
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
        s.random.fraction();
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
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.checks_off = true;
        }
        movement::regive_attack(state, h, clock);
    }
}
