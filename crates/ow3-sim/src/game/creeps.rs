//! Creeps guarding their place (SPEC-0030): the units of the neutral
//! hostile player.
//!
//! A creep's home is the point where it was created. A creep that
//! acquires a target calls the idle creeps within `CreepCallForHelp` of it
//! to the fight. A creep that has gone farther than `GuardDistance` from
//! home turns back some 5.3 s later, with a `move` order home that attacks
//! do not interrupt, and fights again once home. Creeps of a type with
//! `canSleep` sleep at night: they do not acquire, a hit wakes the creep
//! hit and its camp 7 ticks later, and day wakes them all.
//!
//! PROVISIONAL: the time from passing `GuardDistance` to the return is
//! taken as 1066 ticks (1052 to 1080 measured; what decides is open
//! question 2), and the creeps that chase the same target turn back with
//! the first one; an idle creep away from home goes back the same way;
//! the camp a hit wakes is the sleeping creeps within `CreepCallForHelp`
//! of the creep hit (question 4); creeps fall asleep when night comes if
//! they stand idle.

use ow3_math::{add, mul, sub};

use super::movement;
use super::{Object, State};

/// `Player(12)`, the neutral hostile player.
pub const CREEPS: i32 = 12;

/// Ticks from passing `GuardDistance` to turning back.
const RETURN_TICKS: u64 = 1066;
/// Ticks from a hit to the waking of a sleeping camp.
const WAKE_TICKS: u64 = 7;

/// What a creep keeps beyond other units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Creep {
    /// The tick it went farther than `GuardDistance` from home.
    passed: Option<u64>,
    /// It walks home and does not fight.
    pub(super) returning: bool,
    /// It sleeps, and when a hit wakes it and on whom it turns.
    pub(super) sleeping: bool,
    wake: Option<(u64, u32)>,
}

/// Whether unit `h` is a creep.
fn is_creep(state: &State, h: u32) -> bool {
    state.unit(Some(h)).is_some_and(|u| u.owner == CREEPS)
}

/// Whether unit `h` sleeps (`UnitIsSleeping`).
pub(super) fn sleeping(state: &State, h: u32) -> bool {
    state.unit(Some(h)).is_some_and(|u| u.creep.sleeping)
}

/// A new unit of `owner` and type `type_id`: a creep of a sleeping type
/// sleeps at night.
pub(super) fn born(state: &State, owner: i32, type_id: i32) -> Creep {
    let sleeps = owner == CREEPS
        && state.night
        && state.data.units.get(&type_id).is_some_and(|t| t.can_sleep);
    Creep {
        sleeping: sleeps,
        ..Creep::default()
    }
}

/// Night came (creeps that stand idle fall asleep) or day (all wake).
pub(super) fn night_changed(state: &mut State) {
    let night = state.night;
    for i in 0..state.units.len() {
        let h = state.units[i];
        let can_sleep = match state.objects.get(&h) {
            Some(Object::Unit(u)) => {
                u.alive
                    && u.owner == CREEPS
                    && u.attack.is_none()
                    && u.movement.is_none_or(|m| !m.ordered())
                    && state
                        .data
                        .units
                        .get(&u.type_id)
                        .is_some_and(|t| t.can_sleep)
            }
            _ => false,
        };
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            if !night {
                u.creep.sleeping = false;
                u.creep.wake = None;
            } else if can_sleep {
                u.creep.sleeping = true;
            }
        }
    }
}

/// `victim` was hit by `attacker`: a sleeping creep wakes with its camp a
/// little later. Whether the victim sleeps (and so does not answer now).
pub(super) fn hit_while_asleep(state: &mut State, victim: u32, attacker: u32) -> bool {
    let tick = state.ticks;
    let Some(Object::Unit(u)) = state.objects.get_mut(&victim) else {
        return false;
    };
    if !u.creep.sleeping {
        return false;
    }
    if u.creep.wake.is_none() {
        u.creep.wake = Some((tick + WAKE_TICKS, attacker));
    }
    true
}

/// The idle creeps within `CreepCallForHelp` of creep `caller` that join
/// its fight on `target` (in the order of their creation).
fn camp(state: &State, caller: u32, sleeping: bool) -> Vec<u32> {
    let Some(c) = state.unit(Some(caller)) else {
        return Vec::new();
    };
    let reach = f32::from_bits(state.combat.creep_call_for_help);
    let (x, y) = (f32::from_bits(c.x), f32::from_bits(c.y));
    state
        .units
        .iter()
        .copied()
        .filter(|&h| h != caller)
        .filter(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) => {
                let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
                u.alive
                    && u.owner == CREEPS
                    && u.creep.sleeping == sleeping
                    && !u.creep.returning
                    && u.attack.is_none()
                    && u.movement.is_none_or(|m| !m.ordered())
                    && dx * dx + dy * dy <= reach * reach
            }
            _ => false,
        })
        .collect()
}

/// Creep `caller` acquired `target`: the idle creeps of its camp come too.
pub(super) fn call_for_help(state: &mut State, caller: u32, target: u32) {
    if !is_creep(state, caller) {
        return;
    }
    for h in camp(state, caller, false) {
        movement::join_fight(state, h, target);
    }
}

/// The creeps of this tick: camps waking, chases given up, creeps home.
pub(super) fn tick(state: &mut State) {
    let tick = state.ticks;
    let guard = f32::from_bits(state.combat.guard_distance);
    let units = state.units.clone();
    for h in units {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if !u.alive || u.owner != CREEPS {
            continue;
        }
        let creep = u.creep;
        // A hit wakes the creep and its sleeping camp.
        if let Some((at, attacker)) = creep.wake
            && at <= tick
        {
            let mut woken = camp(state, h, true);
            woken.insert(0, h);
            for &w in &woken {
                if let Some(Object::Unit(u)) = state.objects.get_mut(&w) {
                    u.creep.sleeping = false;
                    u.creep.wake = None;
                }
            }
            for w in woken {
                movement::join_fight(state, w, attacker);
            }
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let ordered = u.movement.is_some_and(|m| m.ordered());
        if creep.returning {
            if !ordered {
                set(state, h, |c| {
                    c.returning = false;
                    c.passed = None;
                });
            }
            continue;
        }
        let (dx, dy) = (sub(u.x, u.home[0]), sub(u.y, u.home[1]));
        let away = f32::from_bits(add(mul(dx, dx), mul(dy, dy))) > guard * guard;
        let fighting = u.attack.is_some() || ordered;
        match creep.passed {
            None if away => set(state, h, |c| c.passed = Some(tick)),
            Some(_) if !away && !fighting => set(state, h, |c| c.passed = None),
            Some(p) if tick >= p + RETURN_TICKS => {
                let target = attack_target(state, h);
                go_home(state, h);
                // The others on the same target turn back with it.
                if let Some(target) = target {
                    let others: Vec<u32> = state
                        .units
                        .iter()
                        .copied()
                        .filter(|&o| {
                            o != h
                                && is_creep(state, o)
                                && attack_target(state, o) == Some(target)
                                && state
                                    .unit(Some(o))
                                    .is_some_and(|u| u.creep.passed.is_some())
                        })
                        .collect();
                    for o in others {
                        go_home(state, o);
                    }
                }
            }
            _ => {}
        }
    }
}

fn set(state: &mut State, h: u32, change: impl FnOnce(&mut Creep)) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        change(&mut u.creep);
    }
}

/// The unit creep `h` chases, if any.
fn attack_target(state: &State, h: u32) -> Option<u32> {
    let u = state.unit(Some(h))?;
    u.attack
        .map(|a| a.target)
        .or_else(|| u.movement.and_then(|m| m.target()))
}

/// Creep `h` gives up its chase and walks home with a `move` order.
fn go_home(state: &mut State, h: u32) {
    let Some(home) = state.unit(Some(h)).map(|u| u.home) else {
        return;
    };
    if movement::move_home(state, h, home) {
        set(state, h, |c| {
            c.returning = true;
            c.passed = None;
        });
    }
}
