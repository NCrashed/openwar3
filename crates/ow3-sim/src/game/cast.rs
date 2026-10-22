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
use ow3_math::{add, mul};

use super::movement::{self, CELL};
use super::{Object, State};
use crate::data::is_hero;

/// What an ability is cast on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aim {
    Friend,
    Enemy,
    Nothing,
}

/// The abilities that can be cast, by their order string: the base
/// ability, the order id and what it is cast on.
const ABILITY_ORDERS: [(&str, [u8; 4], i32, Aim); 3] = [
    ("holybolt", *b"AHhb", 852_092, Aim::Friend),
    ("thunderbolt", *b"AHtb", 852_095, Aim::Enemy),
    ("thunderclap", *b"AHtc", 852_096, Aim::Nothing),
];

/// The time from the end of a stun to its order coming back (0.01 s).
const STUN_BACK: u32 = 0x3C23_D70A;

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
    let owner = state.unit(Some(h))?.owner;
    let fits = state.unit(Some(target)).is_some_and(|t| {
        t.alive
            && match aim {
                Aim::Friend => !movement::enemies(state, owner, t.owner),
                Aim::Enemy => movement::enemies(state, owner, t.owner),
                Aim::Nothing => false,
            }
    });
    if !fits || !ready(state, h, ability, level) {
        return Some(false);
    }
    let casting = Casting {
        order_id,
        ability,
        level,
        target: Some(target),
        effect_at: None,
        end_at: 0,
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
        movement::stop(state, h);
        set(state);
        if !movement::cast_order(state, h, target, order_id) {
            return Some(false);
        }
        set(state);
    } else if movement::cast_order(state, h, target, order_id) {
        set(state);
    } else {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.casting = None;
        }
        return Some(false);
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
    movement::stop(state, h);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.casting = Some(Casting {
            order_id,
            ability,
            level,
            target: None,
            effect_at: None,
            end_at: 0,
        });
    }
    let tick = state.ticks;
    start(state, h, tick);
    Some(true)
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
    let end = effect + super::attack::ticks(backswing);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(c) = u.casting.as_mut()
    {
        c.effect_at = Some(effect);
        c.end_at = end;
    }
}

/// Unit `h` is stunned for `duration` seconds from the queue time `at`, or
/// longer if it already is (SPEC-0037): it stops at once. The stun ends
/// on a timer of the queue, and its order comes back on a timer of 0.01 s
/// from that expiry, in the tick whose end that time does not pass.
pub(super) fn stun(state: &mut State, h: u32, duration: u32, at: u32) {
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
    }
    movement::stand_still(state, h);
    // Its order goes on after the stun with a new path (c-RXD-1: worker 5
    // stunned in 4337, new path at its update of 4741).
    state.paths.remove(&h);
}

/// Whether unit `h` is stunned now.
pub(super) fn stunned(state: &State, h: u32) -> bool {
    let tick = state.ticks;
    state
        .unit(Some(h))
        .and_then(|u| u.stunned_until)
        .is_some_and(|t| tick < t)
}

/// The factor by which `h`'s time between hits grows now: `1 / (1 - DataD)`
/// of a thunder clap that slows it, if one does.
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
                effect(s, h, &c, &mut deferred);
            }
            if c.effect_at.is_some() && tick >= c.end_at {
                movement::end_cast(s, h);
            }
        }
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
            } => {
                super::widgets::damage_widget(vm, state, Some(caster), Some(target), amount, 0, 0);
            }
        }
    }
}

/// The effect of cast `c` of `h`; the mana is paid and the cooldown starts
/// now.
fn effect(s: &mut State, h: u32, c: &Casting, deferred: &mut Vec<Deferred>) {
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
                return;
            };
            let (owner, x, y) = (u.owner, f32::from_bits(u.x), f32::from_bits(u.y));
            let area = f32::from_bits(n.area);
            // 1 / (1 - DataD): the factor of the time between hits.
            let factor = ow3_math::div(1.0f32.to_bits(), ow3_math::sub(1.0f32.to_bits(), n.data_d));
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
            for o in hit {
                let hero = s.unit(Some(o)).is_some_and(|t| is_hero(t.type_id));
                let until = tick + duration(hero);
                if let Some(Object::Unit(t)) = s.objects.get_mut(&o) {
                    t.slowed_until = Some((until, factor));
                }
                deferred.push(Deferred::Damage {
                    caster: h,
                    target: o,
                    amount: n.data[0],
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
}
