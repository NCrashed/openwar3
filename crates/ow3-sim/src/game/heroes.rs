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
    // Its parts of the level are made anew with its level events
    // (`widgets::hero_levels`).
    if !state.level_parts.contains(&h) {
        state.level_parts.push(h);
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
    // The last made first, whatever their places (SPEC-0033; c-SAK-1: the
    // Blademaster before the Mountain King at 6052; probe c-SBF).
    let mut by_player: std::collections::BTreeMap<i32, Vec<(u32, i64)>> = Default::default();
    for &h in state.units.iter().rev() {
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
            let rules = &state.combat.heroes;
            let factor = |level: i64| {
                if owner != CREEPS {
                    return 1.0;
                }
                let list = &rules.factor_xp;
                let i = (level - 1).max(0) as usize;
                list.get(i).or(list.last()).copied().unwrap_or(100) as f64 / 100.0
            };
            let xp = state.unit(Some(h)).map_or(0, |u| u.xp);
            // Only the part up to the next level's threshold takes this
            // level's factor; the rest, back as unreduced experience, takes
            // the next level's, and so on. The new total is rounded once,
            // halves up (SPEC-0033; c-SAK-1: 486 + 30 from ogre 13 at
            // level 2 -> 506).
            let total = award(xp, share, level, rules.max_level, factor, |l| {
                rules.xp_for_level(l)
            });
            set_xp(state, h, total);
        }
    }
}

/// The new experience of a hero with `xp` at `level` given `share` of
/// unreduced experience: each level's `factor` applies up to the next
/// level's threshold (`need`), and the total is rounded once, halves up
/// (SPEC-0033).
fn award(
    xp: i64,
    share: f64,
    level: i64,
    max_level: i64,
    factor: impl Fn(i64) -> f64,
    need: impl Fn(i64) -> i64,
) -> i64 {
    let (mut total, mut raw, mut level) = (xp as f64, share, level);
    loop {
        let f = factor(level);
        let next = need(level + 1) as f64;
        if level < max_level && f > 0.0 && total + raw * f >= next {
            raw -= (next - total) / f;
            total = next;
            level += 1;
        } else {
            total += raw * f;
            break;
        }
    }
    (total + 0.5).floor() as i64
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
        // (Its aura, if this is one, starts with the learning.)
        u.aura_setup = Some(tick);
        state.aura_setups += 1;
        u.aura_order = state.aura_setups;
        u.spent += 1;
    }
    // Its objects of the engine's table (SPEC-0024): 2 for an ability
    // with a mana cost, 1 for one without (c-SAK-1: AHtb and AOwk 2, AHbh
    // and AOcr 1; c-SBM-1: Far Sight, cost 75 and no cooldown, 2).
    // PROVISIONAL: read for learning the first level only.
    let cooled = data.per_level.first().is_some_and(|l| l.cost > 0);
    // An aura takes its ability object, the aura's object and the aura's
    // buff on the hero, and Brilliance a rate part for that buff's mana
    // regeneration, Unholy Aura one for its life's (c-RZJ-1: the Archmage's
    // Brilliance 4, the Paladin's Devotion 3; c-SBN-1: the Death Knight's
    // Unholy Aura 4). PROVISIONAL: a second level is not read.
    let aura = [*b"AHab", *b"AHad", *b"AOae", *b"AUau"].contains(&data.code.to_be_bytes());
    let count = if aura {
        3 + usize::from([*b"AHab", *b"AUau"].contains(&data.code.to_be_bytes()))
    } else if cooled {
        2
    } else {
        1
    };
    let objects = state.table.take_n(state.table.now, count, "hero skill");
    // An aura's second goes first at the hero's death, its third with its
    // buffs (c-SBP-1: the paladin at 6.2, 1313 and 1314).
    if aura && let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.entries.death_first.push(objects[1]);
        u.entries.death_late.push(objects[2]);
    }
    // The second is a watch on the mana at the ability's cost (c-SAK-1).
    if cooled {
        let cost = data.per_level.first().map_or(0, |l| l.cost);
        let mana = state
            .unit(Some(h))
            .map_or(0.0, |u| f32::from_bits(u.mana.value()));
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries
                .mana_watches
                .push((cost, objects[1], mana >= cost as f32));
        }
        if mana < cost as f32 {
            super::table::watch_scheduled(state, h, objects[1]);
        }
    }
}
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean",
        |vm, state, args| {
            // Only the attack ability so far (c-SBG-1).
            let removed = handle(&args[0]).is_some_and(|h| {
                let mut s = state.borrow_mut();
                let attack = i32::from_be_bytes(*b"Aatk");
                let has = integer(&args[1]) == attack
                    && s.unit(Some(h))
                        .is_some_and(|u| u.alive && !u.attack_removed);
                if has {
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        u.attack_removed = true;
                    }
                    super::table::attack_removed(&mut s, h);
                }
                has
            });
            Ok(Some(Value::Boolean(removed)))
        }
    );
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
    /// SPEC-0033: a share that passes a level threshold (test map c-SAZ).
    #[test]
    fn award_across_levels() {
        let factor = |l: i64| [0.8, 0.7, 0.6, 0.5][(l - 1) as usize];
        let need = |l: i64| [0, 0, 200, 500, 900, 1400][l as usize];
        for (xp, share, level, total) in [
            (490, 25.0, 2, 506),
            (495, 25.0, 2, 511),
            (190, 60.0, 1, 233),
            (486, 60.0, 2, 524),
            (1190, 25.0, 4, 1203),
            (486, 30.0, 2, 506),
        ] {
            assert_eq!(super::award(xp, share, level, 4, factor, need), total);
        }
    }

    #[test]
    fn halves_up() {
        for (x, rounded) in [(17.5f64, 18i64), (42.5, 43), (12.5, 13), (12.4, 12)] {
            assert_eq!((x + 0.5).floor() as i64, rounded);
        }
    }
}
