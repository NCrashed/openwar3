//! Hero experience, levels and attributes by level (SPEC-0033, SPEC-0034).
//!
//! A hero's level follows its experience by `NeedHeroXP`. At level `L`
//! each attribute is `floor(base + plus * (L - 1))`; a change made by
//! `SetHeroStr` and the others is kept on top of that. When a unit dies,
//! each player for whom it is an enemy shares its experience among its
//! heroes within `HeroExpRange`; creeps give less to heroes of higher
//! levels (`HeroFactorXP`).
//!
//! PROVISIONAL: experience shared between allies, summoned units,
//! buildings and deaths without a killer are not measured (SPEC-0033,
//! open questions 1 and 2); every enemy death counts here, buildings too.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::{add, floor, i2r, mul, r2i};

use super::widgets::{Attribute, set_attribute};
use super::{Object, State, handle, integer};
use crate::data::is_hero;

/// The player of the creeps (`Player(12)`, neutral hostile).
const CREEPS: i32 = 12;

/// Attribute `which` (0 strength, 1 agility, 2 intelligence) of a hero of
/// `type_id` at `level` without changes by script.
fn attribute_at(state: &State, type_id: i32, which: usize, level: i64) -> i32 {
    let Some(t) = state.data.units.get(&type_id) else {
        return 0;
    };
    let base = match which {
        0 => i2r(t.strength),
        1 => t.agility,
        _ => i2r(t.intelligence),
    };
    let gained = mul(t.attribute_plus[which], i2r((level - 1).max(0) as i32));
    r2i(floor(add(base, gained)))
}

/// Hero `h` goes to `level`: its attributes grow by what the levels give,
/// and with them its life and mana (SPEC-0034).
fn set_level(state: &mut State, h: u32, level: i64) {
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let (type_id, old) = (u.type_id, u.level);
    if level <= old {
        return;
    }
    let now = [u.strength, u.agility, u.intelligence];
    for (which, attribute) in [
        Attribute::Strength,
        Attribute::Agility,
        Attribute::Intelligence,
    ]
    .into_iter()
    .enumerate()
    {
        let gain =
            attribute_at(state, type_id, which, level) - attribute_at(state, type_id, which, old);
        if gain != 0 {
            set_attribute(state, Some(h), attribute, now[which] + gain);
        }
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.level = level;
    }
    // Its level events, one a level (SPEC-0034).
    // PROVISIONAL: one event a level when it gains several at once.
    for _ in old..level {
        state.leveled.push(h);
    }
}

/// Hero `h` has `xp` experience, no less than it had and no more than the
/// highest level needs; its level follows.
fn set_xp(state: &mut State, h: u32, xp: i64) {
    let rules = &state.combat.heroes;
    let cap = rules.xp_for_level(rules.max_level);
    let Some(u) = state.unit(Some(h)).filter(|u| is_hero(u.type_id)) else {
        return;
    };
    let xp = xp.min(cap).max(u.xp);
    let level = rules.level_of(xp);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.xp = xp;
    }
    set_level(state, h, level);
}

/// The experience of the death of unit `dying` for the heroes near it
/// (SPEC-0033, "Experience from a death").
pub(super) fn death(state: &mut State, dying: u32) {
    let Some(d) = state.unit(Some(dying)) else {
        return;
    };
    // An illusion's death gives no experience (SPEC-0040; probe c-SAO).
    if d.illusion.is_some() {
        return;
    }
    let (owner, at, type_id) = (d.owner, [d.x, d.y], d.type_id);
    let base = if is_hero(type_id) {
        state.combat.heroes.grant_hero_xp.value(d.level)
    } else {
        let level = state.data.units.get(&type_id).map_or(0, |t| t.level);
        state.combat.heroes.grant_normal_xp.value(i64::from(level))
    };
    if base <= 0 {
        return;
    }
    let range = f32::from_bits(state.combat.heroes.exp_range);
    let near = |u: &super::Unit| {
        let (dx, dy) = (
            f32::from_bits(u.x) - f32::from_bits(at[0]),
            f32::from_bits(u.y) - f32::from_bits(at[1]),
        );
        dx * dx + dy * dy <= range * range
    };
    // The heroes in range of each player for whom it is an enemy.
    let mut by_player: std::collections::BTreeMap<i32, Vec<(u32, i64)>> = Default::default();
    for &h in &state.units {
        if h == dying {
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if !u.alive || !is_hero(u.type_id) || u.illusion.is_some() || !near(u) {
            continue;
        }
        if !super::movement::enemies(state, u.owner, owner) {
            continue;
        }
        by_player.entry(u.owner).or_default().push((h, u.level));
    }
    for heroes in by_player.values() {
        let share = base as f64 / heroes.len() as f64;
        for &(h, level) in heroes {
            let factor = if owner == CREEPS {
                let list = &state.combat.heroes.factor_xp;
                let i = (level - 1).max(0) as usize;
                list.get(i).or(list.last()).copied().unwrap_or(100) as f64 / 100.0
            } else {
                1.0
            };
            // Halves up.
            let gain = (share * factor + 0.5).floor() as i64;
            let xp = state.unit(Some(h)).map_or(0, |u| u.xp);
            set_xp(state, h, xp + gain);
        }
    }
}

/// The skill points hero `u` has left: one a level, the first only for a
/// type with hero data (SPEC-0009, SPEC-0034), less those it spent.
pub(super) fn skill_points(state: &State, u: &super::Unit) -> i32 {
    if !is_hero(u.type_id) {
        return 0;
    }
    let first = i32::from(
        state
            .data
            .units
            .get(&u.type_id)
            .is_some_and(|t| t.hero_data),
    );
    first + (u.level as i32 - 1).max(0) - u.spent
}

/// The level of `ability` of unit `u`: what a hero learned, else 1 for an
/// ability of its type, else 0.
pub(super) fn ability_level(state: &State, u: &super::Unit, ability: i32) -> i32 {
    // The buff of an aura (SPEC-0038).
    if super::auras::has_buff(state, u, ability) {
        return 1;
    }
    if let Some(&(_, level)) = u.skills.iter().find(|(a, _)| *a == ability) {
        return level;
    }
    let own = state
        .data
        .units
        .get(&u.type_id)
        .is_some_and(|t| t.abilities.contains(&ability));
    i32::from(own)
}

/// `SelectHeroSkill`: hero `h` spends a skill point on the next level of
/// `ability` if it has a point, the ability is one of its type's and its
/// level allows it: level `k` needs hero level `reqLevel + s * (k - 1)`,
/// `s` the ability's `levelSkip` or else `HeroAbilityLevelSkip`
/// (SPEC-0035).
fn select_skill(state: &mut State, h: u32, ability: i32) {
    let Some(u) = state
        .unit(Some(h))
        .filter(|u| u.alive && is_hero(u.type_id))
    else {
        return;
    };
    let learnable = state
        .data
        .units
        .get(&u.type_id)
        .is_some_and(|t| t.hero_abilities.contains(&ability));
    let Some(data) = state.data.abilities.get(&ability).filter(|_| learnable) else {
        return;
    };
    let next = ability_level(state, u, ability) + 1;
    let skip = if data.level_skip != 0 {
        i64::from(data.level_skip)
    } else {
        state.combat.heroes.ability_level_skip
    };
    let needs = i64::from(data.req_level) + skip * i64::from(next - 1);
    if skill_points(state, u) <= 0 || next > data.levels || u.level < needs {
        return;
    }
    let tick = state.ticks;
    let brilliance = data.code == i32::from_be_bytes(*b"AHab") && next == 1;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        match u.skills.iter_mut().find(|(a, _)| *a == ability) {
            Some(entry) => entry.1 = next,
            None => u.skills.push((ability, next)),
        }
        if brilliance {
            u.aura_learned = Some(tick);
        }
        u.spent += 1;
    }
}
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native SelectHeroSkill takes unit whichHero, integer abilcode returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                select_skill(&mut state.borrow_mut(), h, integer(&args[1]));
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer",
        |vm, state, args| {
            let s = state.borrow();
            let level = s
                .unit(handle(&args[0]))
                .map_or(0, |u| ability_level(&s, u, integer(&args[1])));
            Ok(Some(Value::Integer(level)))
        }
    );
    native!(
        add,
        state,
        "constant native GetHeroLevel takes unit whichHero returns integer",
        |vm, state, args| {
            let level = state.borrow().unit(handle(&args[0])).map_or(0, |u| u.level);
            Ok(Some(Value::Integer(level as i32)))
        }
    );
    native!(
        add,
        state,
        "native GetHeroXP takes unit whichHero returns integer",
        |vm, state, args| {
            let xp = state.borrow().unit(handle(&args[0])).map_or(0, |u| u.xp);
            Ok(Some(Value::Integer(xp as i32)))
        }
    );
    native!(
        add,
        state,
        "native SetHeroXP takes unit whichHero, integer newXpVal, boolean showEyeCandy returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                set_xp(&mut state.borrow_mut(), h, i64::from(integer(&args[1])));
                super::widgets::hero_levels(vm, state);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native AddHeroXP takes unit whichHero, integer xpToAdd, boolean showEyeCandy returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                let mut s = state.borrow_mut();
                let xp = s.unit(Some(h)).map_or(0, |u| u.xp);
                set_xp(&mut s, h, xp + i64::from(integer(&args[1])));
                drop(s);
                super::widgets::hero_levels(vm, state);
            }
            Ok(None)
        }
    );
    // A level not below the hero's: the experience that level needs.
    native!(
        add,
        state,
        "native SetHeroLevel takes unit whichHero, integer level, boolean showEyeCandy returns nothing",
        |vm, state, args| {
            if let Some(h) = handle(&args[0]) {
                let mut s = state.borrow_mut();
                let level = i64::from(integer(&args[1])).min(s.combat.heroes.max_level);
                let xp = s.combat.heroes.xp_for_level(level);
                set_xp(&mut s, h, xp);
                drop(s);
                super::widgets::hero_levels(vm, state);
            }
            Ok(None)
        }
    );
}

#[cfg(test)]
mod tests {
    /// SPEC-0033, rounding: halves up.
    #[test]
    fn halves_up() {
        for (x, rounded) in [(17.5f64, 18i64), (42.5, 43), (12.5, 13), (12.4, 12)] {
            assert_eq!((x + 0.5).floor() as i64, rounded);
        }
    }
}
