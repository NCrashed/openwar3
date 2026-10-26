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
    pub(super) return_time: Option<u32>,
    /// The order in which the creeps' timers were started: timers with
    /// equal expiries fire in that order (c-RXD-1: 7, 8, 6, 9).
    pub(super) started: u64,
    /// It walks home and does not fight.
    pub(super) returning: bool,
    /// Its move order home has not ended (it may be answering a call):
    /// its acquisition check looks for no target (SPEC-0030).
    pub(super) homebound: bool,
    /// It answered a call on its way home: the walk home lies below the
    /// attack in its order queue, after a move to the target (SPEC-0030).
    pub(super) queued_home: bool,
    /// Where its last walk home ended: it walks back there when it loses
    /// its target.
    pub(super) guard: Option<[u32; 2]>,
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

/// Creep `h` enters a fight at queue time `at`: its guard checks start
/// again, the first at `at`, also when they already run (c-RXD-1: creep 8,
/// its checks from its acquisition of 6667, its return timer from 7869,
/// giving up at 8869), unless it is on its way home.
pub(super) fn enter_fight(state: &mut State, h: u32, at: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.owner == CREEPS
        && !u.creep.returning
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

/// The idle creeps within `CreepCallForHelp` and their collision size of
/// creep `caller`, the caller among them, in the order of their creation.
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
        .filter(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) => {
                let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
                u.alive
                    && u.owner == CREEPS
                    && !u.creep.sleeping
                    && u.attack.is_none()
                    // Nor one stunned (c-SAK-1: kobold 10, stunned by the
                    // bolt of 740, took no target at 978).
                    && u.stunned_until.is_none_or(|t| state.ticks >= t)
                    // Creeps on their way home come too, also while their
                    // move order home lasts in a chase (c-RXD-1: creeps 6
                    // and 7 answered creep 8 at 6667).
                    && (u.creep.returning
                        || u.creep.homebound
                        || u.movement.is_none_or(|m| !m.ordered()))
                    && may_answer(state, u)
                    // Within the call's radius plus its own collision size
                    // (c-SAK-1: ogre 13's call took kobolds 14 and 15,
                    // 618.5 away).
                    && {
                        let r = reach
                            + state
                                .data
                                .units
                                .get(&u.type_id)
                                .map_or(0.0, |t| f32::from_bits(t.collision));
                        dx * dx + dy * dy <= r * r
                    }
            }
            _ => false,
        })
        .collect()
}

/// Creep `caller` acquired `target`: it enters the fight, and the other
/// living members of its group, in their order of joining, each make a
/// call of their own unless their 0.5 s call timer runs; the idle creeps
/// a call reaches, the member among them, take the target in the order
/// of the grid of SPEC-0019 ("Order of the responders"). Those that come
/// make no call of their own (SPEC-0030, "Creep groups"; c-SAK-1: kobold
/// 8 at 706, both camps in the game's order). Their reaction delays count from `from` on the timer queue
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
    restart_returns(state, caller, at);
    enter_fight(state, caller, at);
    let mut came = Vec::new();
    // Those that came and those that only recorded the target, in order.
    let mut answered = Vec::new();
    let callers: Vec<u32> = group_of(state, caller)
        .into_iter()
        .filter(|&m| m != caller)
        .collect();
    let mut reached = Vec::new();
    let now_q = f32::from_bits(state.event_time.unwrap_or(at));
    for c in callers {
        let Some(cu) = state.unit(Some(c)).filter(|u| u.alive) else {
            continue;
        };
        if now_q < f32::from_bits(cu.call_until) {
            continue;
        }
        if let Some(Object::Unit(cu)) = state.objects.get_mut(&c) {
            cu.call_until = (now_q + movement::CREEP_CALL_COOLDOWN).to_bits();
        }
        let near: Vec<u32> = camp(state, c)
            .into_iter()
            .filter(|h| *h != caller && !came.contains(h))
            .collect();
        for h in movement::call_order(state, near) {
            reached.push(h);
            // A creep on its way home comes too (SPEC-0030; c-RXD-1: 6, 7
            // and 9 at 5919).
            set(state, h, |c| c.returning = false);
            // One already chasing takes the new target at once, as a
            // switch (c-RXD-1: creeps 6 and 7 at 6667, turning at 6673).
            // One standing in the backswing of the kill of its target takes
            // nothing from the call (SPEC-0030, "Losing the target in a
            // backswing"; c-RXD-1: creep 9 at 6667). Its acquired-target
            // event fires all the same (SPEC-0019, "During such a
            // backswing"; c-SAK-1: gnoll 17 at 4766).
            let now = state.move_clock;
            if state.unit(Some(h)).is_some_and(|u| {
                f32::from_bits(u.backswing_end) > f32::from_bits(now) && u.guard_return.is_some()
            }) {
                state.record_only.push(h);
                answered.push(h);
                continue;
            }
            // One already attacking that target goes on as it is (c-RXD-1:
            // creep 7 at 6787, on unit 1 since 6667).
            if movement::attack_target(state, h) == Some(target) {
                continue;
            }
            let switching = movement::attacking(state, h);
            let joined = if switching {
                movement::switch_target(state, h, target)
            } else {
                movement::join_fight(state, h, target, from)
            };
            if joined {
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.entries.called = true;
                }
                // A switch while attacking keeps the guard checks running
                // (SPEC-0030, "What restarts the guard checks").
                if !switching {
                    enter_fight(state, h, at);
                }
                came.push(h);
                answered.push(h);
            }
        }
    }
    // The creeps the call reached may cast their spells, after they took
    // the target (SPEC-0047).
    for h in reached {
        consider_spell(state, h, caller, target, from);
    }
    answered
}

/// A call by `caller` reaches every creep of its camp: one whose return is
/// in progress and that stands within `MaxGuardDistance` of its home gets
/// its return timer restarted (SPEC-0030, "Calls restart the return
/// timer"; c-RXD-1: creeps 6 and 7 at 6667, 6763 and 6787, giving up at
/// 7788).
/// A call or a hit reached creep `h` at queue time `at`: a return in
/// progress restarts, within `MaxGuardDistance` of its home (SPEC-0030;
/// c-RZJ-1: ogre 14, restarted by the hits on it, the calls of creeps 9
/// and 10 when hit, and the area calls of Blizzard and of missiles).
pub(super) fn restart_return(state: &mut State, h: u32, at: u32) {
    let far = f32::from_bits(state.combat.max_guard_distance);
    let back = state.combat.guard_return_time;
    // Plus its collision size, as the walk back's distance test (SPEC-0030;
    // c-SAK-1: kobold 19 at 2105).
    let collision = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .map_or(0.0, |t| f32::from_bits(t.collision));
    let far = far + collision;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        let (hx, hy) = (
            f32::from_bits(u.x) - f32::from_bits(u.home[0]),
            f32::from_bits(u.y) - f32::from_bits(u.home[1]),
        );
        if u.alive
            && u.owner == CREEPS
            && (u.creep.homebound || u.creep.return_time.is_some())
            && hx * hx + hy * hy <= far * far
        {
            u.creep.return_time = Some(add(at, back));
        }
    }
}

fn restart_returns(state: &mut State, caller: u32, at: u32) {
    let Some(c) = state.unit(Some(caller)) else {
        return;
    };
    let reach = f32::from_bits(state.combat.creep_call_for_help);
    let far = f32::from_bits(state.combat.max_guard_distance);
    let back = state.combat.guard_return_time;
    let (x, y) = (f32::from_bits(c.x), f32::from_bits(c.y));
    for h in state.units.clone() {
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
        let (hx, hy) = (
            f32::from_bits(u.x) - f32::from_bits(u.home[0]),
            f32::from_bits(u.y) - f32::from_bits(u.home[1]),
        );
        if u.alive
            && u.owner == CREEPS
            // The return is in progress from the start of its timer until
            // the walk home ends (SPEC-0030; c-RZJ-1: ogre 14, timer from
            // 1436, restarted by the calls after).
            && (u.creep.homebound || u.creep.return_time.is_some())
            && dx * dx + dy * dy <= reach * reach
            && hx * hx + hy * hy <= far * far
        {
            u.creep.return_time = Some(add(at, back));
        }
    }
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
    let mut ended = Vec::new();
    for h in units {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if !u.alive || u.owner != CREEPS {
            continue;
        }
        let creep = u.creep;
        let ordered = u.movement.is_some_and(|m| m.ordered());
        // The move order home ends when the creep stands idle again.
        let idle = !ordered && u.attack.is_none();
        if creep.homebound && idle {
            ended.push(h);
        }
        // The move to its dead target done, the walk home below it comes
        // next. PROVISIONAL: not reached in c-SAK-1.
        if creep.queued_home && idle {
            go_home(state, h);
            continue;
        }
        if creep.returning {
            if !ordered {
                let here = [u.x, u.y];
                set(state, h, |c| {
                    c.returning = false;
                    c.guard = Some(here);
                });
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
    for h in ended {
        set(state, h, |c| c.homebound = false);
    }
}

fn set(state: &mut State, h: u32, change: impl FnOnce(&mut Creep)) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        change(&mut u.creep);
    }
}

/// Creep `h` gives up its chase and walks home with a `move` order.
fn go_home(state: &mut State, h: u32) {
    let Some((home, at)) = state.unit(Some(h)).map(|u| (u.home, u.creep.return_time)) else {
        return;
    };
    // The walk home restarts its acquisition check 0.01 s after the
    // return timer (c-RXD-1: creep 6, return at 4557, check at 4559, then
    // every 0.5 s).
    if let Some(t) = at
        && let Some(super::Object::Unit(u)) = state.objects.get_mut(&h)
    {
        u.next_acquire = add(t, movement::AFTER_DEATH);
    }
    // The give-up stops it with a commit first.
    if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) {
        u.checks_off = false;
    }
    movement::stop_commit(state, h);
    let walks = movement::move_home(state, h, home);
    set(state, h, |c| {
        c.queued_home = false;
        c.return_time = None;
        c.returning = walks;
        c.homebound = walks;
    });
}

/// A creep idle after a stun at queue time `now`: its guard checks start
/// again, and outside its zone its return timer starts (SPEC-0037; c-RZH-1: creep 8 at
/// 1946).
pub(super) fn after_stun(state: &mut State, h: u32, now: u32) {
    let back = state.combat.guard_return_time;
    let guard = f32::from_bits(state.combat.guard_distance);
    let collision = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .map_or(0.0, |t| f32::from_bits(t.collision));
    let zone = guard + collision;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && u.owner == CREEPS
    {
        let (dx, dy) = (sub(u.x, u.home[0]), sub(u.y, u.home[1]));
        let away = f32::from_bits(add(mul(dx, dx), mul(dy, dy))) > zone * zone;
        // Only outside the zone, as at a guard check (c-SBJ-1: kobolds 7
        // and 8 after Impale at 9.92, inside: no return timer; analyst
        // 11-15 06:35).
        if away {
            u.creep.return_time = Some(add(now, back));
        }
        u.creep.checks_next = Some(add(now, CHECK_PERIOD));
    }
}

/// The timer from a creep spell's decision to its order, 0.001 s
/// (SPEC-0047).
const CREEP_SPELL_DELAY: u32 = 0x3A83_126F;

/// How long a creep's spell decision holds the spell, 0.5 s (SPEC-0047).
const DECISION_HOLD: u32 = 0x3F00_0000;

/// Creep spells (SPEC-0047): at a hit on creep `h` or a call for help
/// that reaches it, with `victim` the creep hit or calling and `attacker`
/// the enemy, a creep with Shockwave casts it on the attacker when the
/// spell is ready, no other creep of its player is casting, and at least
/// 3 enemies (2 when the victim is the creep itself, below 400 life)
/// stand nearer the attacker than the spell's area plus their collision,
/// the attacker counted.
///
/// PROVISIONAL: the per-player limit is taken as "no other unit of the
/// player casting" (SPEC-0047, open question 1); the cast range is not
/// checked.
pub(super) fn consider_spell(
    state: &mut State,
    h: u32,
    victim: u32,
    attacker: u32,
    call_q: Option<u32>,
) {
    if !is_creep(state, h) {
        return;
    }
    // A paused creep decides nothing (c-SBL-1: the paused ogre lord, hit to
    // 1.46 life at 10.979 with the hero and the footman near, cast nothing).
    let Some(u) = state.unit(Some(h)).filter(|u| u.alive && !u.paused) else {
        return;
    };
    let owner = u.owner;
    let life = f32::from_bits(u.life.value());
    let shockwave = i32::from_be_bytes(*b"AOsh");
    let Some(ability) = state.data.units.get(&u.type_id).and_then(|t| {
        t.abilities.iter().copied().find(|a| {
            state
                .data
                .abilities
                .get(a)
                .is_some_and(|d| d.code == shockwave)
        })
    }) else {
        return;
    };
    if !super::cast::creep_ready(state, h, ability, 1) {
        return;
    }
    // A decision past readiness puts the spell on a 0.5 s cooldown, cast
    // or not (SPEC-0047, "The decision"; c-RZJ-1: 6029 decided, 6117
    // refused).
    let hold = state.ticks + super::attack::ticks(DECISION_HOLD);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.ready.retain(|&(a, _)| a != ability);
        u.ready.push((ability, hold));
    }
    let busy = state.units.iter().any(|&o| {
        o != h
            && state
                .unit(Some(o))
                .is_some_and(|o| o.alive && o.owner == owner && o.casting.is_some())
    });
    if busy {
        return;
    }
    let Some(at) = state.unit(Some(attacker)).map(|a| [a.x, a.y]) else {
        return;
    };
    let area = state
        .data
        .abilities
        .get(&ability)
        .and_then(|a| a.per_level.first())
        .map_or(0, |n| n.area);
    let count = state
        .units
        .iter()
        .filter(|&&o| {
            let Some(e) = state.unit(Some(o)) else {
                return false;
            };
            // Invulnerable units do not count (SPEC-0047; c-RZJ-1: the
            // shielded Paladin at 6029).
            if !e.alive
                || e.hidden
                || !movement::enemies(state, owner, e.owner)
                || super::cast::invulnerable(state, o)
            {
                return false;
            }
            let collision = state.data.units.get(&e.type_id).map_or(0, |t| t.collision);
            let (dx, dy) = (sub(e.x, at[0]), sub(e.y, at[1]));
            let reach = add(area, collision);
            f32::from_bits(add(mul(dx, dx), mul(dy, dy))) < f32::from_bits(mul(reach, reach))
        })
        .count();
    let threshold = if victim == h && life < 400.0 { 2 } else { 3 };
    if count >= threshold {
        // Ordered by a 0.001 s timer of the creep's computer player from
        // the call's queue time (c-RZJ-1: 3.176711 -> 3.177711).
        let call_q =
            call_q.unwrap_or_else(|| state.event_time.unwrap_or_else(|| state.timers.time()));
        let order_q = add(call_q, CREEP_SPELL_DELAY);
        state.creep_casts.push((h, ability, attacker, order_q));
    }
}

/// The most creeps that join one group (SPEC-0030, "Groups").
const GROUP_SIZE: usize = 12;
/// How near the first living member of a group a creep must be to join it:
/// 26 path cells, CreepCampPathingCellDistance, the 676 of SPEC-0030 being
/// its square; the distance must be below it (c-SAK-1: kobold 14, 800 from
/// kobold 8, joined its group; kobold 15, 854, started another).
const GROUP_REACH: f32 = 26.0 * 32.0;

/// New unit `h` joins a group if it is a creep (SPEC-0030, "Groups"): the
/// nearest group that has had fewer than 12 joiners and whose first living
/// member is within 676 of it, else a new group of its own.
pub(super) fn join_group(state: &mut State, h: u32) {
    let Some(u) = state.unit(Some(h)).filter(|u| u.owner == CREEPS) else {
        return;
    };
    let (x, y) = (f32::from_bits(u.x), f32::from_bits(u.y));
    let mut best: Option<(f32, usize)> = None;
    for (i, group) in state.creep_groups.iter().enumerate() {
        if group.len() >= GROUP_SIZE {
            continue;
        }
        let Some(first) = group
            .iter()
            .find_map(|&m| state.unit(Some(m)).filter(|m| m.alive))
        else {
            continue;
        };
        let (dx, dy) = (f32::from_bits(first.x) - x, f32::from_bits(first.y) - y);
        let distance = dx * dx + dy * dy;
        if distance < GROUP_REACH * GROUP_REACH && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, i));
        }
    }
    match best {
        Some((_, i)) => state.creep_groups[i].push(h),
        None => state.creep_groups.push(vec![h]),
    }
}

/// The members of creep `h`'s group, in the order they joined, the dead
/// included.
pub(super) fn group_of(state: &State, h: u32) -> Vec<u32> {
    state
        .creep_groups
        .iter()
        .find(|g| g.contains(&h))
        .cloned()
        .unwrap_or_default()
}

/// Whether creep `u` may answer a call: one whose return is in progress
/// answers only within MaxGuardDistance plus its collision of its home,
/// and not when walking back at 0.6 of its life or less (SPEC-0030;
/// c-SAK-1: gnoll 16, 1089 from home, refused at 2587; ogre 18 answered
/// at 2915). Its return timer running counts as a return in progress
/// (c-SAK-1: ogre 18, 1130 from home on its move to footman 7's point,
/// refused kobold 19's call at 4817).
pub(super) fn may_answer(state: &State, u: &super::Unit) -> bool {
    if !(u.creep.returning || u.creep.homebound || u.creep.return_time.is_some()) {
        return true;
    }
    let home = u.home;
    let collision = state
        .data
        .units
        .get(&u.type_id)
        .map_or(0.0, |t| f32::from_bits(t.collision));
    // From its path agent's position, its last commit (c-SAK-1: kobold
    // 19 at 3557, 1034.5 from home there).
    let at = match u.movement {
        Some(m) => movement::absolute(m.origin, [state.world_bounds[0], state.world_bounds[1]]),
        _ => [u.x, u.y],
    };
    let far = f32::from_bits(state.combat.max_guard_distance) + collision;
    let (hx, hy) = (
        f32::from_bits(at[0]) - f32::from_bits(home[0]),
        f32::from_bits(at[1]) - f32::from_bits(home[1]),
    );
    let weak =
        u.creep.returning && f32::from_bits(u.life.value()) <= 0.6 * f32::from_bits(u.life.max);
    !weak && hx * hx + hy * hy < far * far
}
