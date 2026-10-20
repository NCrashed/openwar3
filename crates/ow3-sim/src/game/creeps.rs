//! Creeps guarding their place (SPEC-0030): the units of the neutral
//! hostile player.
//!
//! A creep's home is the point where it was created. A creep that
//! acquires a target calls the idle creeps within `CreepCallForHelp` of it
//! to the fight. Each creep in a fight has its own guard check, at once
//! and then every 2 s: the first that finds it outside its zone
//! (`GuardDistance` plus its collision size round its home) starts a timer
//! of `GuardReturnTime`, at the end of which it walks home with a `move`
//! order that attacks do not interrupt, and fights again once home.
//! Creeps of a type with `canSleep` sleep from dusk to dawn: they do not
//! acquire, and a hit wakes the creep hit and every sleeping creep within
//! about 832 of it in the same tick.
//!
//! PROVISIONAL: the guard checks go on while the creep is away from home
//! after its fight (a creep that changes its target does not start them
//! again, open question 2); woken creeps do not wake others (question 4);
//! only idle creeps fall asleep at dusk.

use ow3_math::{add, mul, sub};

use super::movement;
use super::{Object, State};

/// `Player(12)`, the neutral hostile player.
pub const CREEPS: i32 = 12;

/// Ticks between two guard checks (2.0 s) and of the return timer
/// (`GuardReturnTime`, 5.0 s, as ticks of 0.005 s).
const CHECK_TICKS: u64 = 400;
/// The radius in which a hit wakes sleeping creeps (831.5 woke, 833 did
/// not; SPEC-0030).
const WAKE_RADIUS: f32 = 832.0;

/// What a creep keeps beyond other units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Creep {
    /// The tick its guard checks count from, while they go on.
    checks_from: Option<u64>,
    /// The tick its return timer ends.
    return_at: Option<u64>,
    /// It walks home and does not fight.
    pub(super) returning: bool,
    /// It sleeps.
    pub(super) sleeping: bool,
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

/// Dusk came (creeps that stand idle fall asleep) or dawn (all wake), in
/// one tick (the night of SPEC-0013 turns at 18:00 and 6:00).
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
            } else if can_sleep {
                u.creep.sleeping = true;
            }
        }
    }
}

/// Creep `h` enters a fight: its guard checks start, the first in this
/// tick, unless they go on already or it is on its way home.
pub(super) fn enter_fight(state: &mut State, h: u32) {
    let tick = state.ticks;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.owner == CREEPS
        && !u.creep.returning
        && u.creep.checks_from.is_none()
        && u.creep.return_at.is_none()
    {
        u.creep.checks_from = Some(tick);
    }
}

/// `victim` was hit by `attacker`: a sleeping victim wakes, and with it,
/// in this tick, every sleeping creep within the wake radius, which join
/// the fight. The victim then answers the hit as an idle unit.
pub(super) fn wake_on_hit(state: &mut State, victim: u32, attacker: u32) {
    let Some(v) = state.unit(Some(victim)) else {
        return;
    };
    if !v.creep.sleeping {
        return;
    }
    let (x, y) = (f32::from_bits(v.x), f32::from_bits(v.y));
    let others: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| h != victim)
        .filter(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) => {
                let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
                u.alive
                    && u.owner == CREEPS
                    && u.creep.sleeping
                    && dx * dx + dy * dy <= WAKE_RADIUS * WAKE_RADIUS
            }
            _ => false,
        })
        .collect();
    for &h in std::iter::once(&victim).chain(&others) {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.creep.sleeping = false;
        }
    }
    for h in others {
        movement::join_fight(state, h, attacker, None);
        enter_fight(state, h);
    }
}

/// The idle creeps within `CreepCallForHelp` of creep `caller`, in the
/// order of their creation.
fn camp(state: &State, caller: u32) -> Vec<u32> {
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
                    && !u.creep.sleeping
                    && !u.creep.returning
                    && u.attack.is_none()
                    && u.movement.is_none_or(|m| !m.ordered())
                    && dx * dx + dy * dy <= reach * reach
            }
            _ => false,
        })
        .collect()
}

/// Creep `caller` acquired `target`: it enters the fight, and the idle
/// creeps of its camp come too, in the same tick, in the order of the
/// grid of SPEC-0019 ("Order of the responders"); each that comes calls
/// in turn, so a chain reaches creeps beyond the caller, after all of the
/// caller's. Their reaction delays count from `from` on the timer queue
/// (the expiry of the caller's check), or from this tick. Gives the creeps
/// that came, in that order.
pub(super) fn call_for_help(
    state: &mut State,
    caller: u32,
    target: u32,
    from: Option<u32>,
) -> Vec<u32> {
    if !is_creep(state, caller) {
        return Vec::new();
    }
    enter_fight(state, caller);
    let mut came = Vec::new();
    let mut callers = std::collections::VecDeque::from([caller]);
    while let Some(c) = callers.pop_front() {
        let near: Vec<u32> = camp(state, c)
            .into_iter()
            .filter(|h| *h != caller && !came.contains(h))
            .collect();
        for h in movement::call_order(state, near) {
            if movement::join_fight(state, h, target, from) {
                enter_fight(state, h);
                came.push(h);
                callers.push_back(h);
            }
        }
    }
    came
}

/// The creeps of this tick: guard checks, return timers, creeps home.
pub(super) fn tick(state: &mut State) {
    let tick = state.ticks;
    let guard = f32::from_bits(state.combat.guard_distance);
    let return_ticks =
        (f64::from(f32::from_bits(state.combat.guard_return_time)) * 200.0).round() as u64;
    let units = state.units.clone();
    for h in units {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if !u.alive || u.owner != CREEPS {
            continue;
        }
        let creep = u.creep;
        let ordered = u.movement.is_some_and(|m| m.ordered());
        if creep.returning {
            if !ordered {
                set(state, h, |c| c.returning = false);
            }
            continue;
        }
        if creep.return_at == Some(tick) {
            go_home(state, h);
            continue;
        }
        // A guard check: outside the zone, the return timer starts.
        if let Some(from) = creep.checks_from
            && (tick - from) % CHECK_TICKS == 0
        {
            let collision = state
                .data
                .units
                .get(&u.type_id)
                .map_or(0.0, |t| f32::from_bits(t.collision));
            let zone = guard + collision;
            let (dx, dy) = (sub(u.x, u.home[0]), sub(u.y, u.home[1]));
            let away = f32::from_bits(add(mul(dx, dx), mul(dy, dy))) > zone * zone;
            if away {
                set(state, h, |c| {
                    c.checks_from = None;
                    c.return_at = Some(tick + return_ticks);
                });
            }
        }
    }
}

fn set(state: &mut State, h: u32, change: impl FnOnce(&mut Creep)) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        change(&mut u.creep);
    }
}

/// Creep `h` gives up its chase and walks home with a `move` order.
fn go_home(state: &mut State, h: u32) {
    let Some(home) = state.unit(Some(h)).map(|u| u.home) else {
        return;
    };
    let walks = movement::move_home(state, h, home);
    set(state, h, |c| {
        c.return_at = None;
        c.returning = walks;
    });
}
