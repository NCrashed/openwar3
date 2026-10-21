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

/// The period of the guard checks (2.0 s).
const CHECK_PERIOD: u32 = 0x4000_0000;
/// The radius in which a hit wakes sleeping creeps (831.5 woke, 833 did
/// not; SPEC-0030).
const WAKE_RADIUS: f32 = 832.0;

/// What a creep keeps beyond other units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Creep {
    /// The queue time of its next guard check, while they go on.
    checks_next: Option<u32>,
    /// The queue time its return timer expires.
    return_time: Option<u32>,
    /// The order in which the creeps' timers were started: timers with
    /// equal expiries fire in that order (c-RXD-1: 7, 8, 6, 9).
    started: u64,
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

/// Creep `h` enters a fight at queue time `at`: its guard checks start,
/// the first at `at`, unless they go on already or it is on its way home.
pub(super) fn enter_fight(state: &mut State, h: u32, at: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.owner == CREEPS
        && !u.creep.returning
        && u.creep.checks_next.is_none()
        && u.creep.return_time.is_none()
    {
        u.creep.checks_next = Some(at);
        state.creep_timers += 1;
        u.creep.started = state.creep_timers;
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
        enter_fight(state, h, state.move_clock);
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
                    && u.attack.is_none()
                    // Creeps on their way home come too (c-RXD-1).
                    && (u.creep.returning || u.movement.is_none_or(|m| !m.ordered()))
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
    // The caller's check, or this tick.
    let at = from.unwrap_or(state.move_clock);
    enter_fight(state, caller, at);
    let mut came = Vec::new();
    let mut callers = std::collections::VecDeque::from([caller]);
    while let Some(c) = callers.pop_front() {
        let near: Vec<u32> = camp(state, c)
            .into_iter()
            .filter(|h| *h != caller && !came.contains(h))
            .collect();
        for h in movement::call_order(state, near) {
            // A creep on its way home comes too (SPEC-0030; c-RXD-1: 6, 7
            // and 9 at 5919).
            set(state, h, |c| c.returning = false);
            if movement::join_fight(state, h, target, from) {
                enter_fight(state, h, at);
                came.push(h);
                callers.push_back(h);
            }
        }
    }
    came
}

/// The creeps of this tick: guard checks, return timers, creeps home.
pub(super) fn tick(state: &mut State) {
    let guard = f32::from_bits(state.combat.guard_distance);
    // Timers of the queue: they fire in the tick whose interval holds
    // their expiry (c-RXD-1: the return of tick 4557), equal ones in the
    // order they were started.
    let end = f32::from_bits(add(state.move_clock, crate::timers::TICK));
    let mut units = state.units.clone();
    units.sort_by_key(|&h| {
        state.unit(Some(h)).map_or((u32::MAX, 0), |u| {
            let next = u.creep.return_time.or(u.creep.checks_next);
            (next.unwrap_or(u32::MAX), u.creep.started)
        })
    });
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
                // Home again: its acquisition check restarts 0.01 s later
                // (c-RXD-1: creep 8 home in 5917, check in 5919).
                // From the update cycle of its arrival.
                let at = add(state.update_time, movement::AFTER_DEATH);
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.next_acquire = at;
                }
            }
            continue;
        }
        if creep.return_time.is_some_and(|t| f32::from_bits(t) <= end) {
            go_home(state, h);
            continue;
        }
        // A guard check: outside the zone, the return timer starts.
        if let Some(e) = creep.checks_next
            && f32::from_bits(e) <= end
        {
            let collision = state
                .data
                .units
                .get(&u.type_id)
                .map_or(0.0, |t| f32::from_bits(t.collision));
            let zone = guard + collision;
            let (dx, dy) = (sub(u.x, u.home[0]), sub(u.y, u.home[1]));
            let away = f32::from_bits(add(mul(dx, dx), mul(dy, dy))) > zone * zone;
            let back = state.combat.guard_return_time;
            set(state, h, |c| {
                if away {
                    c.checks_next = None;
                    c.return_time = Some(add(e, back));
                } else {
                    c.checks_next = Some(add(e, CHECK_PERIOD));
                }
            });
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
        c.return_time = None;
        c.returning = walks;
    });
}
