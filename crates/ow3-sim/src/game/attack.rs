//! Hits of attacking units (SPEC-0016).
//!
//! An attack begins when the unit stands in range of its target and faces
//! it (SPEC-0019, `game::movement`); then each hit does the damage of
//! SPEC-0015 for the amount of [`crate::combat::hit_amount`], with the dice
//! from game generator 2, the first one `dmgpt1` after the start and the
//! next ones `cool1` apart, until the target dies or the unit gets another
//! order.
//!
//! One tick after the end of a swing (`backSw1` after the damage) an
//! attacker whose target has left its reach chases it again (SPEC-0019);
//! its next hit comes no earlier than `cool1` after the last one.
//!
//! PROVISIONAL: hits come after the timers of their tick.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;

use super::{Object, State};
use crate::combat::{DAMAGE_TYPE_NORMAL, hit_amount};
use crate::data::{Area, Weapon, WeaponKind, is_hero};
use crate::random::ATTACK_DICE;
use ow3_math::natives::{cos, sin, sqrt};
use ow3_math::{add, div, floor, mul, sub};

/// A unit's attack on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attacking {
    pub(super) target: u32,
    /// The tick of the next hit.
    next: u64,
    /// The order id `GetUnitCurrentOrder` gives: 0 for an attack the unit
    /// began on its own (SPEC-0019).
    pub(super) order_id: i32,
    /// The tick after the end of the last swing, when a target out of
    /// reach is chased again (SPEC-0019).
    pub(super) resume: Option<u64>,
    /// The attacker swings again, or takes a new goal and stands, at its
    /// first update from this tick on: the cooldown less the damage point
    /// after the last hit (SPEC-0019, "After the swing").
    pub(super) renew_at: Option<u64>,
    /// The expiry of the damage-point timer of a swing started at an
    /// update: its queue time plus `dmgpt1` (SPEC-0016). The hit comes in
    /// the tick that holds it.
    hit_at: Option<u32>,
    /// The queue time at which that swing started.
    swing_q: u32,
}

impl Attacking {
    /// The attack as a viewer sees it, for a unit of type `type_id`.
    pub(super) fn view(&self, state: &State, type_id: i32) -> super::AttackView {
        let weapon = weapon_for(state, type_id, Some(self.target)).unwrap_or_default();
        super::AttackView {
            target: self.target,
            next_hit: self.next,
            cooldown: ticks(weapon.cooldown),
            damage_point: ticks(weapon.damage_point),
        }
    }
}

/// Seconds as a whole number of ticks of 0.005 s, at least one.
pub(super) fn ticks(seconds: u32) -> u64 {
    (f64::from(f32::from_bits(seconds)) * 200.0)
        .round()
        .max(1.0) as u64
}

/// Whether `target` can still be hit.
pub(super) fn standing(state: &State, target: u32) -> bool {
    match state.objects.get(&target) {
        Some(Object::Unit(u)) => u.alive,
        Some(Object::Destructable(d)) => d.alive,
        Some(Object::Item(i)) => f32::from_bits(i.life) > 0.0,
        _ => false,
    }
}

/// Unit `h` starts hitting `target` at tick `tick`: the first hit comes its
/// weapon's damage point later (SPEC-0019).
pub(super) fn begin(state: &mut State, h: u32, target: u32, tick: u64, order_id: i32) {
    let point = match state.objects.get(&h) {
        Some(Object::Unit(u)) => weapon_for(state, u.type_id, Some(target)).map_or(0, |w| {
            slowed(state, h, hero_cooldown(state, h, w.damage_point))
        }),
        _ => return,
    };
    let damage_point = ticks(point);
    let now = state.timers.time();
    // A cooldown expiring in this tick, after the start of the tick, also
    // times the swing (`movement::cooldown_expiries`).
    let start = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        // A swing waits for the cooldown of the last one, also on another
        // target (c-RXR-1: grunt 15, whose target died, swung at the
        // expiry of that cooldown).
        let swing_q = if f32::from_bits(u.cooldown_end) > f32::from_bits(start) {
            u.cooldown_end
        } else {
            now
        };
        let hit_at = add(swing_q, point);
        let wait = ticks(sub(swing_q, now));
        u.attack = Some(Attacking {
            target,
            next: (tick + wait + damage_point).max(u.weapon_ready),
            order_id,
            resume: None,
            renew_at: None,
            hit_at: Some(hit_at),
            swing_q,
        });
    }
    if !state.attacking.contains(&h) {
        state.attacking.push(h);
    }
}

/// The hits due at this tick, in the order the attacks began.
pub(super) fn hits(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    fly(vm, state);
    hits_before(vm, state, None);
}

/// The hits due in this tick that expire before `limit` (all with `None`):
/// the timers of the main queue before the acquisition checks of the tick
/// (c-RXR-1: rifleman 7's hit and the call for help it made came before
/// grunt 14's check in 5489).
pub(super) fn hits_before(vm: &mut Vm, state: &Rc<RefCell<State>>, limit: Option<u32>) {
    // The hits are timers of the queue: those due in this tick come in the
    // order of their expiries, before the rest (c-RXR-1, 1979: footman 1
    // before rifleman 6).
    let mut attackers = state.borrow().attacking.clone();
    {
        let s = state.borrow();
        // Equal expiries go by the start of their timers, the swings.
        let key = |h: &u32| {
            s.unit(Some(*h))
                .and_then(|u| u.attack)
                .filter(|a| due(&s, a))
                .and_then(|a| {
                    a.hit_at
                        .map(|e| (f32::from_bits(e), f32::from_bits(a.swing_q)))
                })
                .unwrap_or((f32::INFINITY, 0.0))
        };
        attackers.sort_by(|a, b| {
            let (a, b) = (key(a), key(b));
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
        });
    }
    for h in attackers {
        if let Some(limit) = limit {
            let s = state.borrow();
            let early = s
                .unit(Some(h))
                .and_then(|u| u.attack)
                .filter(|a| due(&s, a))
                .and_then(|a| a.hit_at)
                .is_some_and(|e| f32::from_bits(e) < f32::from_bits(limit));
            if !early {
                continue;
            }
        }
        // A stunned unit does not attack: its hit waits (SPEC-0037).
        // PROVISIONAL: what a stun does to a swing under way is not
        // measured.
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            if super::cast::stunned(s, h) {
                let tick = s.ticks;
                if let Some(Object::Unit(u)) = s.objects.get_mut(&h)
                    && let Some(a) = u.attack.as_mut()
                    && a.next <= tick
                {
                    a.next = tick + 1;
                }
                continue;
            }
        }
        // A swing after the first starts only with the target in reach;
        // else the attacker chases it (SPEC-0019, "Start of the attack":
        // a started swing always hits, but a new one needs the range).
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let next_swing = match s.objects.get(&h) {
                Some(Object::Unit(u)) if u.alive => {
                    u.attack.filter(|a| a.resume.is_some()).map(|a| {
                        let point = weapon_for(s, u.type_id, Some(a.target)).map_or(1, |w| {
                            ticks(slowed(s, h, hero_cooldown(s, h, w.damage_point)))
                        });
                        (a, a.next.saturating_sub(point))
                    })
                }
                _ => None,
            };
            // A target out of reach when the next swing would start, or at
            // the new goal after the swing: chased from the new goal on,
            // the attacker standing until then (SPEC-0019).
            if let Some((a, start)) = next_swing
                && (start == tick || (s.update_now && a.renew_at.is_some_and(|r| tick >= r)))
                && a.renew_at.is_none_or(|r| tick >= r)
                && standing(s, a.target)
                && !super::movement::reaches(s, h, a.target)
            {
                if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                    u.attack = None;
                }
                // A chase that took its new goal at this update searches
                // it at the next one (SPEC-0019, "After the swing").
                if !super::movement::chasing(s, h, a.target) {
                    super::movement::chase(s, h, a.target, a.order_id, None);
                }
                continue;
            }
        }
        // A swing starts: the roll of a chance-on-attack ability
        // (SPEC-0036).
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let starts = s.unit(Some(h)).and_then(|u| u.attack).is_some_and(|a| {
                let point = s
                    .unit(Some(h))
                    .and_then(|u| weapon_for(s, u.type_id, Some(a.target)))
                    .map_or(1, |w| {
                        ticks(slowed(s, h, hero_cooldown(s, h, w.damage_point)))
                    });
                a.next.saturating_sub(point) == tick
            });
            if starts {
                roll_proc(s, h);
            }
        }
        // The hit, if one is due: target, amount and attack type.
        let hit = {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let (type_id, attack) = match s.objects.get(&h) {
                Some(Object::Unit(u)) if u.alive => (u.type_id, u.attack),
                _ => (0, None),
            };
            match attack {
                Some(a) if due(s, &a) && standing(s, a.target) => {
                    let weapon = weapon_for(s, type_id, Some(a.target)).unwrap_or_default();
                    let primary = hero_primary(s, h);
                    // A thunder clap slows the attacks (SPEC-0037).
                    let cooldown = slowed(s, h, hero_cooldown(s, h, weapon.cooldown));
                    let backswing = slowed(s, h, weapon.backswing);
                    let amount = hit_amount(&weapon, primary, &mut s.game_random[ATTACK_DICE]);
                    // A launch draws one more output for the missile
                    // (SPEC-0016).
                    if weapon.kind != WeaponKind::Normal {
                        s.game_random[ATTACK_DICE].fraction();
                    }
                    let (amount, bash) = proc_effect(s, h, amount);
                    let point = slowed(s, h, hero_cooldown(s, h, weapon.damage_point));
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        let next = a.next + ticks(cooldown);
                        let end = tick + ticks(backswing);
                        let resume = Some(end + 1);
                        // The next swing, and with it the new goal of a
                        // chase, at the first update from `D + cool -
                        // dmgpt` on (SPEC-0019).
                        let renew_at = Some((tick + ticks(cooldown)).saturating_sub(ticks(point)));
                        // The next swing starts at the expiry of this one's
                        // cooldown (SPEC-0016).
                        let swing_q = add(a.swing_q, cooldown);
                        u.cooldown_end = swing_q;
                        u.cooldown_target = a.target;
                        u.backswing_end = add(a.hit_at.unwrap_or(0), backswing);
                        u.attack = Some(Attacking {
                            next,
                            resume,
                            renew_at,
                            hit_at: Some(add(swing_q, point)),
                            swing_q,
                            ..a
                        });
                        u.weapon_ready = next;
                    }
                    // A missile weapon launches at the damage point; the
                    // damage comes when the missile arrives (SPEC-0016).
                    if weapon.kind != WeaponKind::Normal
                        && let Some(missile) = launch(
                            s,
                            h,
                            a.target,
                            amount,
                            weapon.attack_type,
                            weapon.area.clone(),
                            weapon.kind == WeaponKind::Artillery,
                            // The flight counts from the start of the tick
                            // (c-RXR-1: troll 16, launch 4131).
                            s.move_clock,
                        )
                    {
                        s.missiles.push(missile);
                        None
                    } else {
                        if let (Some(trace), Some(at)) = (s.hit_trace.as_mut(), a.hit_at) {
                            trace.push((tick, at, s.timers.time(), a.target));
                        }
                        // Evasion rolls after the dice; an evaded hit does
                        // nothing (SPEC-0036).
                        // PROVISIONAL: measured for melee hits only.
                        if evades(s, a.target) {
                            None
                        } else {
                            s.event_time = a.hit_at;
                            Some((a.target, amount, weapon.attack_type, weapon.area, bash))
                        }
                    }
                }
                Some(a) if due(s, &a) => {
                    // The target is gone.
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        u.attack = None;
                    }
                    None
                }
                _ => None,
            }
        };
        if let Some((target, amount, attack_type, area, bash)) = hit {
            // Bash: its damage comes before the hit, armor not counted
            // (SPEC-0036, SPEC-0037), and the target is stunned.
            if let Some((extra, unit_stun, hero_stun)) = bash {
                super::widgets::damage_widget(vm, state, Some(h), Some(target), extra, 0, 0);
                let mut s = state.borrow_mut();
                if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
                    let hero = s.unit(Some(target)).is_some_and(|u| is_hero(u.type_id));
                    let at = s.event_time.unwrap_or_else(|| s.timers.time());
                    super::cast::stun(&mut s, target, if hero { hero_stun } else { unit_stun }, at);
                }
            }
            strike(vm, state, h, target, amount, attack_type, area, None);
            state.borrow_mut().event_time = None;
        }
        // One tick after the swing the attacker takes a goal again, also
        // with its target in reach, where it stands and turns to its path
        // until its cooldown is over (SPEC-0019, "After the swing").
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let resumed = match s.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive => u.attack.filter(|a| a.resume == Some(tick)),
            _ => None,
        };
        if let Some(a) = resumed
            && standing(&s, a.target)
        {
            // A target in reach keeps the swing timed by the cooldown.
            let in_reach = super::movement::reaches(&s, h, a.target);
            super::movement::chase(&mut s, h, a.target, a.order_id, a.renew_at);
            if in_reach && let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.attack = Some(a);
            }
        }
    }
    let mut s = state.borrow_mut();
    let s = &mut *s;
    s.attacking.retain(
        |h| matches!(s.objects.get(h), Some(Object::Unit(u)) if u.alive && u.attack.is_some()),
    );
}

/// The weapon a unit of `type_id` uses against `target` (SPEC-0016,
/// "Second weapon"): by the target's classes. PROVISIONAL: destructables
/// count as `debris` and items as `item`.
pub(super) fn weapon_for(state: &State, type_id: i32, target: Option<u32>) -> Option<Weapon> {
    let data = state.data.units.get(&type_id)?;
    let classes = match target.and_then(|t| state.objects.get(&t)) {
        Some(Object::Unit(o)) => state
            .data
            .units
            .get(&o.type_id)
            .map(|t| t.target_type.clone())
            .unwrap_or_default(),
        Some(Object::Destructable(_)) => vec!["debris".to_owned()],
        Some(Object::Item(_)) => vec!["item".to_owned()],
        _ => Vec::new(),
    };
    data.weapon_against(&classes).cloned()
}

/// The units that the area of a hit of `h` on `target` damages besides the
/// target, in the order of their creation, with the amounts before armor
/// (SPEC-0016, "Area damage"): by the distance from the target's centre
/// to theirs, `amount` up to `full`, `half_factor` of it up to `half`,
/// `quarter_factor` up to `quarter`, for units of the classes the area
/// allows, allies and neutral ones too unless it names `enemy`.
///
/// PROVISIONAL: the limits are the fields themselves (the game's were
/// about one unit farther, centred a little beyond the target, open
/// question 6); only units are damaged, not destructables or items.
fn splash(state: &State, h: u32, target: u32, area: &Area, amount: u32) -> Vec<(u32, u32)> {
    let Some(t) = state.unit(Some(target)) else {
        return Vec::new();
    };
    splash_at(state, h, [t.x, t.y], Some(target), area, amount)
}

/// The units round point `at` that an area hit of `h` reaches, but
/// `except`, with their shares of `amount`.
fn splash_at(
    state: &State,
    h: u32,
    at: [u32; 2],
    except: Option<u32>,
    area: &Area,
    amount: u32,
) -> Vec<(u32, u32)> {
    let Some(attacker) = state.unit(Some(h)) else {
        return Vec::new();
    };
    let (x, y) = (f32::from_bits(at[0]), f32::from_bits(at[1]));
    let enemies_only = area.targets.iter().any(|c| c == "enemy");
    let mut out = Vec::new();
    for &other in &state.units {
        if Some(other) == except {
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&other) else {
            continue;
        };
        if !u.alive || u.hidden {
            continue;
        }
        let classes = state
            .data
            .units
            .get(&u.type_id)
            .map_or(&[][..], |d| &d.target_type[..]);
        if !classes.iter().any(|c| area.targets.contains(c)) {
            continue;
        }
        if enemies_only && !super::movement::enemies(state, attacker.owner, u.owner) {
            continue;
        }
        let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
        let d = (dx * dx + dy * dy).sqrt();
        let share = if d <= area.full {
            amount
        } else if d <= area.half {
            ow3_math::mul(amount, area.half_factor)
        } else if d <= area.quarter {
            ow3_math::mul(amount, area.quarter_factor)
        } else {
            continue;
        };
        out.push((other, share));
    }
    out
}

/// The damage of a hit of `h` on `target`, by its swing or its missile:
/// the units round the target first, then the target, and the answers
/// to it (SPEC-0016, SPEC-0019).
#[allow(clippy::too_many_arguments)]
fn strike(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    impact: Option<[u32; 2]>,
) {
    // The units round the target, before the target takes its hit.
    let splashed = area
        .as_ref()
        .map(|area| splash(&state.borrow(), h, target, area, amount))
        .unwrap_or_default();
    super::widgets::damage_widget(
        vm,
        state,
        Some(h),
        Some(target),
        amount,
        attack_type,
        DAMAGE_TYPE_NORMAL,
    );
    for (victim, share) in splashed {
        super::widgets::damage_widget(
            vm,
            state,
            Some(h),
            Some(victim),
            share,
            attack_type,
            DAMAGE_TYPE_NORMAL,
        );
    }
    // A kill on an attack-move: the unit walks on after its
    // backswing (SPEC-0019).
    let mut s = state.borrow_mut();
    // The victim and its friends answer the hit (SPEC-0019).
    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
        super::movement::retaliate(&mut s, target, h, impact);
    }
    if !standing(&s, target) {
        let tick = s.ticks;
        let backswing = match s.objects.get(&h) {
            Some(Object::Unit(u)) => weapon_for(&s, u.type_id, Some(target))
                .map_or(1, |w| ticks(slowed(&s, h, w.backswing))),
            _ => 1,
        };
        super::movement::killed(&mut s, h, tick, backswing);
    }
}
/// A missile in flight (SPEC-0016, "Missiles"). One without homing hits
/// its target wherever it is when a timer of `L / Missilespeed` from the
/// launch expires (`L` the plane distance at the launch); a homing one
/// moves `Missilespeed * 0.1` towards the target's centre every 0.1 s
/// and hits once it is nearer than that; a shell of artillery goes to the
/// target's point at the launch and hits the units round it.
#[derive(Clone, Debug, PartialEq)]
pub struct Missile {
    attacker: u32,
    target: u32,
    /// Where it is, for a homing missile, or where it started.
    at: [u32; 2],
    /// The target's point at the launch, for artillery and for drawing.
    point: [u32; 2],
    /// The movement clock at the launch and the expiry of its flight.
    launched: u32,
    expiry: u32,
    /// A homing missile: its step per 0.1 s and the tick of its next step.
    homing: Option<(u32, u64)>,
    artillery: bool,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    /// A missile of an ability: the stun of its hit, for units and for
    /// heroes, in seconds (SPEC-0037).
    spell: Option<(u32, u32)>,
}

/// 1/32: world units to path cells.
const CELLS: u32 = 0x3D00_0000;
/// 0.1 s, the step of a homing missile, in ticks and as a real.
const HOMING_TICKS: u64 = 20;
/// The least delay of a timer, 0.0001 s (SPEC-0002).
const MIN_DELAY: u32 = 0x38D1_B717;
const HOMING_STEP: u32 = 0x3DCC_CCCD;

/// The plane distance from `a` to `b` (world units, engine reals).
fn distance(a: [u32; 2], b: [u32; 2]) -> u32 {
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    sqrt(add(mul(dx, dx), mul(dy, dy)))
}

/// The missile of `h` launched now at `target`, if its type has a missile
/// speed and the target is not nearer than `Missilespeed * 0.1` to the
/// launch point (then it hits at once). The launch point is `launchY`
/// ahead and `launchX` to the side.
///
/// PROVISIONAL: "ahead" is taken as the direction to the target (the
/// attacker faces it within 0.2 radians when it swings) and the side as
/// its right (SPEC-0016 measured only targets straight ahead).
///
/// `launched` is the movement clock at the start of the launch tick.
#[allow(clippy::too_many_arguments)]
fn launch(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    artillery: bool,
    launched: u32,
) -> Option<Missile> {
    let u = state.unit(Some(h))?;
    let data = state.data.units.get(&u.type_id)?;
    let speed = data.missile_speed;
    if f32::from_bits(speed) <= 0.0 {
        return None;
    }
    let to = super::movement::position_of(state, target)?;
    // Along the facing the unit shows, `launchY` ahead and `launchX` to
    // the right, floored to quarters (c-RXR-1, five troll missiles).
    let (fx, fy) = (cos(u.facing), sin(u.facing));
    let [side, ahead] = data.launch;
    let quarter = |v: u32| mul(floor(mul(v, 4.0f32.to_bits())), 0.25f32.to_bits());
    let at = [
        quarter(add(u.x, add(mul(fx, ahead), mul(fy, side)))),
        quarter(add(u.y, sub(mul(fy, ahead), mul(fx, side)))),
    ];
    let flight = distance(at, to);
    let step = mul(speed, HOMING_STEP);
    if f32::from_bits(flight) < f32::from_bits(step) {
        return None;
    }
    let homing = (data.missile_homing && !artillery).then(|| {
        // The first step is the move of the launch.
        (step, state.ticks + HOMING_TICKS)
    });
    let at = match homing {
        Some(_) => {
            let k = div(step, flight);
            [
                add(at[0], mul(sub(to[0], at[0]), k)),
                add(at[1], mul(sub(to[1], at[1]), k)),
            ]
        }
        None => at,
    };
    Some(Missile {
        attacker: h,
        target,
        at,
        point: to,
        launched,
        // The timer of the movers adds its least delay, 0.0001 (c-RXR-1:
        // troll 18, impact 2715).
        expiry: add(add(launched, div(flight, speed)), MIN_DELAY),
        homing,
        artillery,
        amount,
        attack_type,
        area,
        spell: None,
    })
}

/// The missiles of this tick: those that arrive strike, in the order of
/// their launch. A missile whose target died or is gone is lost; a shell
/// of artillery goes on to its point.
fn fly(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let arrived = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tick = s.ticks;
        // The flights are timers of the movers' scheduler: they end in the
        // tick whose clock, at its end, is at or after the expiry, after
        // the main queue (c-RXR-1, troll 19: expiry 4.297861, impact in
        // tick 860, after the script timer of that tick).
        let next_clock = add(s.move_clock, crate::timers::TICK);
        let mut arrived = Vec::new();
        let mut flying = Vec::with_capacity(s.missiles.len());
        for mut m in std::mem::take(&mut s.missiles) {
            if !m.artillery && !standing(s, m.target) {
                continue;
            }
            match m.homing {
                None => {
                    if f32::from_bits(m.expiry) <= f32::from_bits(next_clock) {
                        arrived.push(m);
                    } else {
                        flying.push(m);
                    }
                }
                Some((step, next)) if next == tick => {
                    let Some(to) = super::movement::position_of(s, m.target) else {
                        continue;
                    };
                    let left = distance(m.at, to);
                    if f32::from_bits(left) < f32::from_bits(step) {
                        arrived.push(m);
                        continue;
                    }
                    let k = div(step, left);
                    m.at = [
                        add(m.at[0], mul(sub(to[0], m.at[0]), k)),
                        add(m.at[1], mul(sub(to[1], m.at[1]), k)),
                    ];
                    m.homing = Some((step, next + HOMING_TICKS));
                    flying.push(m);
                }
                Some(_) => flying.push(m),
            }
        }
        s.missiles = flying;
        // The impacts come at the end of the tick.
        s.event_time = Some(next_clock);
        arrived
    };
    for m in arrived {
        if let Some(stun) = m.spell {
            spell_hit(vm, state, m.attacker, m.target, m.amount, stun);
            continue;
        }
        match (m.artillery, &m.area) {
            // A shell hits the units round its point by their distances.
            (true, Some(area)) => {
                let hit = splash_at(&state.borrow(), m.attacker, m.point, None, area, m.amount);
                for (victim, share) in hit {
                    strike(
                        vm,
                        state,
                        m.attacker,
                        victim,
                        share,
                        m.attack_type,
                        None,
                        Some(m.point),
                    );
                }
            }
            (true, None) => {}
            (false, _) => {
                // The impact calls for help round where the missile hits.
                let impact = super::movement::position_of(&state.borrow(), m.target);
                strike(
                    vm,
                    state,
                    m.attacker,
                    m.target,
                    m.amount,
                    m.attack_type,
                    m.area,
                    impact,
                )
            }
        }
    }
    state.borrow_mut().event_time = None;
}

/// A missile in flight as a viewer sees it: the type of the unit that
/// launched it, where it is (world units) and where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissileView {
    pub type_id: i32,
    pub at: [f32; 2],
    pub towards: [f32; 2],
}

/// The missiles in flight; one on a timer is drawn on the line from its
/// launch point to the target by the share of its flight time gone.
pub(super) fn views(state: &State) -> Vec<MissileView> {
    let clock = f32::from_bits(state.move_clock);
    state
        .missiles
        .iter()
        .filter_map(|m| {
            let type_id = state.unit(Some(m.attacker)).map_or(0, |u| u.type_id);
            let to = if m.artillery {
                m.point
            } else {
                super::movement::position_of(state, m.target)?
            };
            let to = to.map(f32::from_bits);
            let from = m.at.map(f32::from_bits);
            let at = match m.homing {
                Some(_) => from,
                None => {
                    let (start, end) = (f32::from_bits(m.launched), f32::from_bits(m.expiry));
                    let share = ((clock - start) / (end - start).max(1e-6)).clamp(0.0, 1.0);
                    [
                        from[0] + (to[0] - from[0]) * share,
                        from[1] + (to[1] - from[1]) * share,
                    ]
                }
            };
            Some(MissileView {
                type_id,
                at,
                towards: to,
            })
        })
        .collect()
}

/// The chance-on-attack abilities by their base codes: critical strike,
/// bash, drunken brawler (SPEC-0036).
const PROC_ABILITIES: [[u8; 4]; 3] = [*b"AOcr", *b"AHbh", *b"ANdb"];

/// The chance factors of the pseudo-random distribution by the chance in
/// steps of 5 percent (SPEC-0036, table `C`).
const PROC_TABLE: [f32; 21] = [
    0.0, 0.0038, 0.0147, 0.0320, 0.0547, 0.0819, 0.1126, 0.1461, 0.1815, 0.2184, 0.2562, 0.2953,
    0.3333, 0.3781, 0.4203, 0.4607, 0.5, 0.5799, 0.6666, 0.7706, 1.0,
];

/// The chance-on-attack ability of unit `h`, its base code and level.
fn proc_ability(state: &State, h: u32) -> Option<(i32, i32, i32)> {
    let u = state.unit(Some(h))?;
    let t = state.data.units.get(&u.type_id)?;
    t.abilities
        .iter()
        .chain(&t.hero_abilities)
        .copied()
        .find_map(|a| {
            let base = state.data.abilities.get(&a).map_or(a, |d| d.code);
            let known = PROC_ABILITIES
                .iter()
                .any(|p| i32::from_be_bytes(*p) == base);
            let level = super::heroes::ability_level(state, u, a);
            (known && level > 0).then_some((a, base, level))
        })
}

/// The roll of unit `h`'s chance-on-attack ability at the start of a
/// swing (SPEC-0036): `C[i] * N` against one draw of game generator 3.
fn roll_proc(state: &mut State, h: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.procced = None;
    }
    let Some((ability, _, level)) = proc_ability(state, h) else {
        return;
    };
    let chance = state
        .data
        .abilities
        .get(&ability)
        .and_then(|a| a.per_level.get((level - 1).max(0) as usize))
        .map_or(0, |l| l.data[0]);
    let i = ow3_math::r2i(add(mul(chance, 0.2f32.to_bits()), 0.5f32.to_bits())).clamp(0, 20);
    let n = state
        .unit(Some(h))
        .and_then(|u| u.proc_counters.iter().find(|(a, _)| *a == ability))
        .map_or(1, |&(_, n)| n);
    let c = mul(PROC_TABLE[i as usize].to_bits(), ow3_math::i2r(n as i32));
    let x = state.game_random[crate::random::ATTACK_PROC].fraction();
    let procs = f32::from_bits(c) >= f32::from_bits(x);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        let next = if procs { 1 } else { n + 1 };
        match u.proc_counters.iter_mut().find(|(a, _)| *a == ability) {
            Some(entry) => entry.1 = next,
            None => u.proc_counters.push((ability, next)),
        }
        if procs {
            u.procced = Some((ability, level));
        }
    }
}

/// What the proc of `h`'s swing does to its hit of `amount`: the amount
/// times `DataB` for a critical strike, and a damage of `DataC` before
/// the hit for bash (SPEC-0036).
///
/// PROVISIONAL: the stun of bash and the effects of drunken brawler are
/// not measured; the extra damage of bash takes the hit's attack type.
fn proc_effect(state: &State, h: u32, amount: u32) -> (u32, Option<(u32, u32, u32)>) {
    let Some((ability, level)) = state.unit(Some(h)).and_then(|u| u.procced) else {
        return (amount, None);
    };
    let Some(data) = state.data.abilities.get(&ability) else {
        return (amount, None);
    };
    let numbers = data
        .per_level
        .get((level - 1).max(0) as usize)
        .copied()
        .unwrap_or_default();
    if data.code == i32::from_be_bytes(*b"AOcr") {
        (mul(amount, numbers.data[1]), None)
    } else if data.code == i32::from_be_bytes(*b"AHbh") {
        (
            amount,
            Some((numbers.data[2], numbers.duration, numbers.hero_duration)),
        )
    } else {
        (amount, None)
    }
}

/// The primary attribute of hero `h` times `StrAttackBonus`, added to the
/// damage of its hits (SPEC-0034); 0 for other units.
fn hero_primary(state: &State, h: u32) -> u32 {
    let Some(u) = state.unit(Some(h)).filter(|u| is_hero(u.type_id)) else {
        return 0;
    };
    let points = match state
        .data
        .units
        .get(&u.type_id)
        .map_or(0, |t| t.primary_attribute)
    {
        1 => u.strength,
        2 => u.agility,
        3 => u.intelligence,
        _ => 0,
    };
    mul(ow3_math::i2r(points), state.combat.heroes.str_attack)
}

/// The time `seconds` of a damage point, backswing or cooldown of `h`'s
/// swing divided by its attack-speed factor while a thunder clap slows it
/// (SPEC-0037).
fn slowed(state: &State, h: u32, seconds: u32) -> u32 {
    super::cast::attack_slow(state, h).map_or(seconds, |speed| div(seconds, speed))
}

/// A slow of attack-speed factor `speed` starting at the queue time `now`:
/// the time left of the running cooldown of `u` is divided by it, and a
/// swing waiting for that cooldown starts at the new end with its damage
/// point divided too (SPEC-0037; c-RZH-1, creep 10 at 5281).
///
/// PROVISIONAL: the end of a slow is not measured; the times stay.
pub(super) fn slow_cooldown(u: &mut super::Unit, now: u32, speed: u32) {
    let old = u.cooldown_end;
    if f32::from_bits(old) <= f32::from_bits(now) {
        return;
    }
    let end = add(now, div(sub(old, now), speed));
    let later = ticks(sub(end, old));
    u.cooldown_end = end;
    u.weapon_ready += later;
    if let Some(a) = u.attack.as_mut().filter(|a| a.swing_q == old) {
        let point = a.hit_at.map_or(0, |h| sub(h, old));
        let slowed = div(point, speed);
        a.swing_q = end;
        a.hit_at = Some(add(end, slowed));
        a.next += later + ticks(sub(slowed, point));
    }
}

/// The seconds between the hits of `h` with a weapon of `cooldown`: for a
/// hero `cooldown / (1 + AgiAttackSpeedBonus * AGI)` (SPEC-0034). Its
/// damage point shrinks the same way.
fn hero_cooldown(state: &State, h: u32, cooldown: u32) -> u32 {
    match state.unit(Some(h)).filter(|u| is_hero(u.type_id)) {
        Some(u) => {
            let speed = mul(
                state.combat.heroes.agi_attack_speed,
                ow3_math::i2r(u.agility),
            );
            div(cooldown, add(1.0f32.to_bits(), speed))
        }
        None => cooldown,
    }
}

/// The missile of an ability of `h` launched now at `target` from the
/// caster's centre: homing at `speed`, `amount` of spell damage and a stun
/// of `stun` seconds (for units, for heroes) on its hit (SPEC-0037). `None`
/// when the target is nearer than `speed * 0.1`: it is hit at once.
pub(super) fn launch_spell(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    stun: (u32, u32),
    speed: u32,
) -> Option<Missile> {
    // It flies straight at the target's position at the launch, from the
    // caster's position floored to 1/128 cell, at its speed per tick, and
    // hits in the first tick in which it has flown the distance (SPEC-0037,
    // "Flight"; c-RZH-1: launch 891, 8.1708 cells, 53 ticks, damage in 943).
    let u = state.unit(Some(h))?;
    let from = [u.x, u.y];
    let to = super::movement::position_of(state, target)?;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells = |p: [u32; 2]| super::movement::relative(p, min).map(|c| mul(c, CELLS));
    let floor128 = |c: u32| (f32::from_bits(mul(c, 0x4300_0000)).floor() / 128.0).to_bits();
    let (a, b) = (cells(from).map(floor128), cells(to));
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    let d = ow3_math::natives::sqrt(add(mul(dx, dx), mul(dy, dy)));
    let per_tick = mul(mul(speed, CELLS), crate::timers::TICK);
    if f32::from_bits(per_tick) <= 0.0 {
        return None;
    }
    let n = (f32::from_bits(div(d, per_tick))).ceil().max(1.0) as u64;
    // The impact tick's own clock: due at the end of the tick before.
    let expiry = (0..n).fold(state.move_clock, |c, _| add(c, crate::timers::TICK));
    Some(Missile {
        attacker: h,
        target,
        at: from,
        point: to,
        launched: state.move_clock,
        expiry,
        homing: None,
        artillery: false,
        amount,
        attack_type: 0,
        area: None,
        spell: Some(stun),
    })
}

/// The hit of a spell of `h` on `target`: a damage event of 0 first, then
/// `amount` of the spells attack type, which armor does not reduce, and
/// the stun (SPEC-0037).
pub(super) fn spell_hit(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    stun: (u32, u32),
) {
    for damage in [0, amount] {
        super::widgets::damage_widget(vm, state, Some(h), Some(target), damage, 0, 0);
    }
    let mut s = state.borrow_mut();
    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
        let hero = s.unit(Some(target)).is_some_and(|u| is_hero(u.type_id));
        // The impact comes at the end of the tick, and the stun counts from
        // there (c-RZH-1: bolt 943, order back at 1946).
        let at = s.event_time.unwrap_or_else(|| s.timers.time());
        super::cast::stun(&mut s, target, if hero { stun.1 } else { stun.0 }, at);
    }
}

/// Whether the hit of attack `a` is due in this tick: by the expiry of
/// its damage-point timer against the queue's time at the end of the tick,
/// else by its tick.
fn due(state: &State, a: &Attacking) -> bool {
    match a.hit_at {
        Some(at) => f32::from_bits(at) <= f32::from_bits(state.timers.time()),
        None => a.next <= state.ticks,
    }
}

/// Whether unit `target`, with evasion (`AEev`), evades a hit: one more
/// output of game generator 2, after the attacker's dice, below `DataA`
/// of its level (SPEC-0036). A paused unit does not roll.
fn evades(state: &mut State, target: u32) -> bool {
    let chance = {
        let Some(u) = state.unit(Some(target)).filter(|u| !u.paused) else {
            return false;
        };
        let evasion = i32::from_be_bytes(*b"AEev");
        let data = &state.data;
        let skill = u
            .skills
            .iter()
            .copied()
            .chain(
                data.units
                    .get(&u.type_id)
                    .into_iter()
                    .flat_map(|t| t.abilities.iter().map(|&a| (a, 1))),
            )
            .find(|&(a, level)| {
                level > 0 && data.abilities.get(&a).is_some_and(|d| d.code == evasion)
            });
        let Some((ability, level)) = skill else {
            return false;
        };
        data.abilities
            .get(&ability)
            .and_then(|d| d.per_level.get((level - 1).max(0) as usize))
            .map(|n| n.data[0])
    };
    let Some(chance) = chance else {
        return false;
    };
    let x = state.game_random[ATTACK_DICE].fraction();
    f32::from_bits(x) < f32::from_bits(chance)
}
