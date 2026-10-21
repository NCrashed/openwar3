//! Casting an ability on a unit (SPEC-0035, measured on Holy Light).
//!
//! An order of the ability's order string on a target in range (`Rng`
//! of the ability's level plus both collision sizes, the range test of
//! SPEC-0019) starts the cast at once; out of range the caster first
//! walks into range (`game::movement`, `Order::Cast`). The effect comes
//! `castpt` after the start: the mana is paid then and the cooldown starts
//! there. The order stays `castbsw` after the effect, then ends. An order
//! while the cooldown runs, or without the mana, does nothing.
//!
//! PROVISIONAL: only Holy Light on a friendly unit (a heal of `DataA`);
//! turning before a cast, the reaction delay and what interrupts a cast
//! are open (SPEC-0035, open questions 1 to 3).

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;
use ow3_math::{add, mul};

use super::movement::{self, CELL};
use super::{Object, State};

/// The abilities that can be cast, by their order string: the base
/// ability and the order id.
const ABILITY_ORDERS: [(&str, [u8; 4], i32); 1] = [("holybolt", *b"AHhb", 852_092)];

/// A cast in progress, or the ability a unit walks to cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Casting {
    pub(super) order_id: i32,
    ability: i32,
    level: i32,
    target: u32,
    /// The ticks of the effect and of the end of the order; `None` while
    /// the caster walks into range.
    effect_at: Option<u64>,
    end_at: u64,
}

/// The ability of unit `h` that order `name` casts, with its level.
fn ability_for(state: &State, h: u32, name: &str) -> Option<(i32, i32, i32)> {
    let &(_, base, order_id) = ABILITY_ORDERS
        .iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(name))?;
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
        .map(|(a, level)| (a, level, order_id))
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

/// Order `name` of unit `h` on `target`: `None` when it is not the order
/// of an ability, else whether the unit took it.
pub(super) fn order(state: &mut State, h: u32, name: &str, target: u32) -> Option<bool> {
    let (ability, level, order_id) = match ability_for(state, h, name) {
        Some(found) => found,
        None if ABILITY_ORDERS
            .iter()
            .any(|(n, _, _)| n.eq_ignore_ascii_case(name)) =>
        {
            return Some(false);
        }
        None => return None,
    };
    let tick = state.ticks;
    let u = state.unit(Some(h))?;
    let cooling = u.ready.iter().any(|&(a, at)| a == ability && at > tick);
    let mana = f32::from_bits(u.mana.value());
    let cost = numbers(state, ability, level).cost;
    let friendly = state
        .unit(Some(target))
        .is_some_and(|t| t.alive && !movement::enemies(state, u.owner, t.owner));
    if cooling || mana < cost as f32 || !friendly {
        return Some(false);
    }
    let casting = Casting {
        order_id,
        ability,
        level,
        target,
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
    if movement::in_range(
        movement::relative(here, min),
        movement::relative(there, min),
        reach,
    ) {
        movement::stop(state, h);
        set(state);
        start(state, h, tick);
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

/// The effects and the ends of the casts of this tick.
pub(super) fn tick(_vm: &mut Vm, state: &Rc<RefCell<State>>) {
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
            let n = numbers(s, c.ability, c.level);
            // Holy Light: the target heals by DataA; the mana is paid and
            // the cooldown starts now.
            if let Some(Object::Unit(t)) = s.objects.get_mut(&c.target)
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
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                let mana = ow3_math::sub(u.mana.value(), ow3_math::i2r(n.cost));
                u.mana.set(mana);
                u.ready.retain(|&(a, _)| a != c.ability);
                u.ready
                    .push((c.ability, tick + super::attack::ticks(n.cooldown)));
            }
        }
        if c.effect_at.is_some() && tick >= c.end_at {
            if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.casting = None;
            }
        }
    }
}
