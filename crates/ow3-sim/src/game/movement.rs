//! Orders of units and their movement on open ground (SPEC-0018, part 1,
//! and SPEC-0019).
//!
//! An ordered unit is updated every 6 ticks from the order on. At an
//! update it stops when it is within 0.49 cells of 32 (15.68) of the
//! target point; otherwise its heading turns towards the target by at most
//! `turnRate`, and once the heading of the update before was within
//! `propWin` of the target's direction it moves along the new heading at
//! `spd` until the next update. Its position advances every tick from the
//! position at the update by the time passed, relative to the world's
//! lower left corner, in the arithmetic of SPEC-0004. Its facing follows
//! the heading at the pace of its `orientInterp` (SPEC-0013), also after
//! it has stopped.
//!
//! Orders (SPEC-0019): `move`, `patrol` (between the target and where the
//! order was given), `attack` and `smart` to a point (like `move`), `attack`
//! on a target (after the reaction delay, walk until the centres are
//! within `rangeN1` and both collision sizes, then turn until the heading
//! is within 0.2 radians of the target and attack), `stop` and
//! `holdposition` (end the motion at once). A new order given to a unit
//! that is moving takes effect at its next update, and its first motion
//! comes a tick late.
//!
//! PROVISIONAL: units go straight at their target (no path finding, no
//! collisions, no small heading wobble, SPEC-0018 open question 2); the
//! arrival test compares squared distances in cells (open question 1);
//! the first update comes 6 ticks after the order; a moving unit that gets
//! an attack order goes on as before during the delay; attackers do not
//! chase targets that move.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::natives::{atan2, cos, sin};
use ow3_math::{add, i2r, mul, sub};

use super::{Object, State, handle, real, string};
use crate::random::REACTION;

/// Order ids that `GetUnitCurrentOrder` gives (SPEC-0019).
pub const ORDER_SMART: i32 = 851_971;
pub const ORDER_ATTACK: i32 = 851_983;
pub const ORDER_MOVE: i32 = 851_986;
pub const ORDER_PATROL: i32 = 851_991;
/// Ticks between updates of a unit.
const UPDATE: u64 = 6;
/// 1/32: distances of the movement are taken in cells of 32 (SPEC-0018).
const CELL: u32 = 0x3D00_0000;
/// The arrival radius of a point move in cells (0.49, 15.68 world units).
const ARRIVAL: u32 = 0x3EFA_E148;
/// A unit attacks once its heading is this close to its target (radians).
const ATTACK_ANGLE: f32 = 0.2;
const PI: u32 = 0x4049_0FDB;
const TWO_PI: u32 = 0x40C9_0FDB;
/// pi / 180.
const DEGREES: u32 = 0x3C8E_FA35;
const SIGN: u32 = 0x8000_0000;

fn real_of(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// `a - b` moved into -pi..pi.
fn difference(a: u32, b: u32) -> u32 {
    let mut d = sub(a, b);
    while real_of(d) > real_of(PI) {
        d = sub(d, TWO_PI);
    }
    while real_of(d) < -real_of(PI) {
        d = add(d, TWO_PI);
    }
    d
}

/// `angle` moved into 0..2 pi.
fn normal(angle: u32) -> u32 {
    let mut a = angle;
    while real_of(a) < 0.0 {
        a = add(a, TWO_PI);
    }
    while real_of(a) >= real_of(TWO_PI) {
        a = sub(a, TWO_PI);
    }
    a
}

/// `from` turned towards `to` by at most `step` (radians), the shorter
/// way.
fn turn_towards(from: u32, to: u32, step: u32) -> u32 {
    let d = difference(to, from);
    if real_of(d).abs() <= real_of(step) {
        normal(to)
    } else if real_of(d) > 0.0 {
        normal(add(from, step))
    } else {
        normal(sub(from, step))
    }
}

/// What a unit was told to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    /// `move`, `smart` and `attack` to a point.
    Point,
    /// Between the target point and `home`, where the order was given.
    Patrol { home: [u32; 2] },
    /// `attack` on a unit, destructable or item.
    Attack { target: u32 },
}

/// A unit following an order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moving {
    order: Order,
    /// The id `GetUnitCurrentOrder` gives.
    order_id: i32,
    /// The point walked to (for an attack, where the target stood at the
    /// last update).
    target: [u32; 2],
    /// The direction of motion, in radians.
    heading: u32,
    /// The tick of the next update.
    next_update: u64,
    /// Moving since the heading came within the propulsion window.
    walking: bool,
    /// The position at the last update, relative to the world's lower
    /// left corner, the velocity since then and the ticks since then.
    origin: [u32; 2],
    velocity: [u32; 2],
    ticks: u32,
    /// The step of the facing (SPEC-0013), which grows to its maximum.
    facing_step: u32,
    /// Done: only the facing still turns towards the heading.
    arrived: bool,
    /// A new order came while the unit was moving: it starts again from
    /// standing at the next update.
    restart: bool,
    /// The first tick of motion after a restart is lost.
    skip: bool,
    /// The tick of an attack order and its reaction delay in seconds: the
    /// unit acts on the order from its first update after the delay.
    reaction: Option<(u64, u32)>,
}

impl Moving {
    /// Whether the order is still on.
    pub fn ordered(&self) -> bool {
        !self.arrived
    }

    /// Whether the unit moves now, for its animation.
    pub fn walking(&self) -> bool {
        !self.arrived && self.walking
    }

    /// The id of the order while it is on.
    pub fn order_id(&self) -> i32 {
        if self.arrived { 0 } else { self.order_id }
    }
}

/// The position `p` relative to the world's lower left corner `min`.
fn relative(p: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [add(p[0], min[0] ^ SIGN), add(p[1], min[1] ^ SIGN)]
}

fn absolute(r: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [sub(r[0], min[0] ^ SIGN), sub(r[1], min[1] ^ SIGN)]
}

/// Where a widget stands.
fn position_of(state: &State, widget: u32) -> Option<[u32; 2]> {
    match state.objects.get(&widget)? {
        Object::Unit(u) => Some([u.x, u.y]),
        Object::Destructable(d) => Some([d.x, d.y]),
        Object::Item(i) => Some([i.x, i.y]),
        _ => None,
    }
}

/// Gives unit `h` an order towards `target`; false for a unit that cannot
/// follow it. A unit that is moving takes the order at its next update.
fn order(state: &mut State, h: u32, order: Order, order_id: i32, target: [u32; 2]) -> bool {
    let tick = state.ticks;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(Object::Unit(u)) = state.objects.get(&h) else {
        return false;
    };
    let speed = state.data.units.get(&u.type_id).map_or(0, |t| t.speed);
    let attack = matches!(order, Order::Attack { .. });
    if !u.alive || (real_of(speed) <= 0.0 && !attack && !state.data.units.is_empty()) {
        return false;
    }
    let busy = u.movement.filter(|m| m.ordered());
    let moving = match busy {
        // Taken at the next update, which the unit keeps.
        Some(m) => Moving {
            order,
            order_id,
            target,
            restart: m.walking || m.restart,
            reaction: None,
            ..m
        },
        None => Moving {
            order,
            order_id,
            target,
            heading: u.facing,
            next_update: tick + UPDATE,
            walking: false,
            origin: relative([u.x, u.y], min),
            velocity: [0, 0],
            ticks: 0,
            facing_step: 0,
            arrived: false,
            restart: false,
            skip: false,
            reaction: None,
        },
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.movement = Some(moving);
        u.attack = None;
        u.turn = None;
    }
    if !state.moving.contains(&h) {
        state.moving.push(h);
    }
    true
}

/// `stop` and `holdposition`: the unit stands where it is now.
fn halt(state: &mut State, h: u32) -> bool {
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return false;
    };
    if !u.alive {
        return false;
    }
    u.attack = None;
    if let Some(m) = u.movement.as_mut() {
        m.arrived = true;
        m.walking = false;
        m.velocity = [0, 0];
    }
    true
}

pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean",
        |vm, state, args| {
            let (Some(h), Some(name)) = (handle(&args[0]), string(&args[1])) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let target = [real(&args[2]), real(&args[3])];
            let mut state = state.borrow_mut();
            let here = state.unit(Some(h)).map(|u| [u.x, u.y]);
            let done = match (name, here) {
                ("move", _) => order(&mut state, h, Order::Point, ORDER_MOVE, target),
                ("attack", _) => order(&mut state, h, Order::Point, ORDER_ATTACK, target),
                ("patrol", Some(home)) => {
                    order(&mut state, h, Order::Patrol { home }, ORDER_PATROL, target)
                }
                // The unit obeys, but the native gives false (SPEC-0019).
                ("smart", _) => {
                    order(&mut state, h, Order::Point, ORDER_SMART, target);
                    false
                }
                _ => false,
            };
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean",
        |vm, state, args| {
            let (Some(h), Some("attack"), Some(target)) =
                (handle(&args[0]), string(&args[1]), handle(&args[2]))
            else {
                return Ok(Some(Value::Boolean(false)));
            };
            let mut state = state.borrow_mut();
            let armed = state
                .unit(Some(h))
                .and_then(|u| state.data.units.get(&u.type_id))
                .is_some_and(|t| t.weapon.is_some());
            let at = position_of(&state, target);
            let done = match at {
                Some(at) if armed && target != h && super::attack::standing(&state, target) => {
                    // The reaction delay (SPEC-0019).
                    let delay = mul(
                        state.game_random[REACTION].fraction(),
                        state.combat.reaction_delay,
                    );
                    let tick = state.ticks;
                    let done = order(&mut state, h, Order::Attack { target }, ORDER_ATTACK, at);
                    if let (true, Some(Object::Unit(u))) = (done, state.objects.get_mut(&h)) {
                        if let Some(m) = u.movement.as_mut() {
                            m.reaction = Some((tick, delay));
                        }
                    }
                    done
                }
                _ => false,
            };
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueImmediateOrder takes unit whichUnit, string order returns boolean",
        |vm, state, args| {
            let done = match (handle(&args[0]), string(&args[1])) {
                (Some(h), Some("stop" | "holdposition")) => halt(&mut state.borrow_mut(), h),
                _ => false,
            };
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native GetUnitCurrentOrder takes unit whichUnit returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let order = match state.unit(handle(&args[0])) {
                Some(u) if u.attack.is_some() => u.attack.map_or(0, |a| a.order_id),
                Some(u) => u.movement.map_or(0, |m| m.order_id()),
                None => 0,
            };
            Ok(Some(Value::Integer(order)))
        }
    );
}

/// One tick of motion of every moving unit, before the timers.
pub(super) fn step(state: &mut State) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    for h in &state.moving {
        let Some(Object::Unit(u)) = state.objects.get_mut(h) else {
            continue;
        };
        let Some(m) = u.movement.as_mut() else {
            continue;
        };
        if m.arrived || !m.walking {
            continue;
        }
        if std::mem::take(&mut m.skip) {
            continue;
        }
        m.ticks += 1;
        let elapsed = mul(i2r(m.ticks as i32), crate::timers::TICK);
        let r = [
            add(m.origin[0], mul(m.velocity[0], elapsed)),
            add(m.origin[1], mul(m.velocity[1], elapsed)),
        ];
        let [x, y] = absolute(r, min);
        u.x = x;
        u.y = y;
    }
}

/// The updates due at this tick, after the timers.
pub(super) fn updates(_vm: &mut Vm, state: &mut State) {
    let tick = state.ticks;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let moving = std::mem::take(&mut state.moving);
    let mut still = Vec::with_capacity(moving.len());
    for h in moving {
        let (type_id, alive) = match state.objects.get(&h) {
            Some(Object::Unit(u)) => (u.type_id, u.alive),
            _ => continue,
        };
        let data = state.data.units.get(&type_id);
        let speed = data.map_or(0, |t| t.speed);
        let turn_rate = data.map_or(PI, |t| t.turn_rate);
        let window = mul(data.map_or(0, |t| t.prop_window), DEGREES);
        let orient = data.map_or(0, |t| t.orient_interp);
        let reach = data.map_or(0, |t| {
            let range = t.weapon.as_ref().map_or(0, |w| w.range);
            add(range, t.collision)
        });
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let Some(mut m) = u.movement else {
            continue;
        };
        if !alive {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.movement = None;
            }
            continue;
        }
        if m.next_update != tick {
            still.push(h);
            continue;
        }
        m.next_update = tick + UPDATE;
        let mut begin_attack = None;
        // Still in the reaction delay: the unit goes on as before.
        let waiting = m.reaction.is_some_and(|(from, delay)| {
            let waited = mul(i2r((tick - from) as i32), crate::timers::TICK);
            real_of(waited) < real_of(delay)
        });
        if !waiting {
            m.reaction = None;
        }
        if !m.arrived && !waiting {
            if std::mem::take(&mut m.restart) {
                m.walking = false;
                m.skip = true;
            }
            // An attacker walks to where its target stands now.
            let mut in_reach = false;
            if let Order::Attack { target } = m.order {
                match position_of(state, target) {
                    Some(at) if super::attack::standing(state, target) => {
                        m.target = at;
                        let other = match state.objects.get(&target) {
                            Some(Object::Unit(t)) => {
                                state.data.units.get(&t.type_id).map_or(0, |d| d.collision)
                            }
                            _ => 0,
                        };
                        let limit = add(reach, other);
                        let here = relative([u.x, u.y], min);
                        let goal = relative(at, min);
                        let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
                        let distance = real_of(add(mul(dx, dx), mul(dy, dy)));
                        in_reach = distance <= real_of(limit) * real_of(limit);
                    }
                    _ => {
                        m.arrived = true;
                        m.velocity = [0, 0];
                    }
                }
            }
            if !m.arrived {
                let here = relative([u.x, u.y], min);
                let goal = relative(m.target, min);
                let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
                // The squared distance in cells.
                let (cx, cy) = (mul(dx, CELL), mul(dy, CELL));
                let distance = real_of(add(mul(cx, cx), mul(cy, cy)));
                let direction = atan2(dy, dx);
                m.origin = here;
                m.ticks = 0;
                if let Order::Attack { target } = m.order {
                    if in_reach {
                        // In range: stand, turn to the target and strike.
                        m.walking = false;
                        m.velocity = [0, 0];
                        if real_of(difference(direction, m.heading)).abs() < ATTACK_ANGLE {
                            m.arrived = true;
                            begin_attack = Some(target);
                        } else {
                            m.heading = turn_towards(m.heading, direction, turn_rate);
                        }
                    } else {
                        walk(&mut m, direction, window, turn_rate, speed);
                    }
                } else if distance <= real_of(mul(ARRIVAL, ARRIVAL)) {
                    if let Order::Patrol { home } = m.order {
                        // Back the other way, turning on the spot.
                        m.order = Order::Patrol { home: m.target };
                        m.target = home;
                        m.walking = false;
                        m.velocity = [0, 0];
                        let goal = relative(home, min);
                        let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
                        walk(&mut m, atan2(dy, dx), window, turn_rate, speed);
                    } else {
                        m.arrived = true;
                        m.walking = false;
                        m.velocity = [0, 0];
                    }
                } else {
                    walk(&mut m, direction, window, turn_rate, speed);
                }
            }
        }
        // The facing follows the heading (SPEC-0018).
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        let (facing, step) = super::facing::follow(u.facing, m.heading, m.facing_step, orient);
        u.facing = facing;
        m.facing_step = step;
        let facing_done = u.facing == m.heading;
        if m.arrived && facing_done && begin_attack.is_none() {
            u.movement = None;
        } else {
            u.movement = Some(m);
            still.push(h);
        }
        if let Some(target) = begin_attack {
            super::attack::begin(state, h, target, tick, m.order_id);
        }
    }
    still.extend(std::mem::take(&mut state.moving));
    state.moving = still;
}

/// The start rule of SPEC-0018 at an update: the heading turns towards
/// `direction`, and the unit walks along the new heading once the heading
/// before was within the propulsion window.
fn walk(m: &mut Moving, direction: u32, window: u32, turn_rate: u32, speed: u32) {
    let within = real_of(difference(direction, m.heading)).abs() < real_of(window);
    m.heading = turn_towards(m.heading, direction, turn_rate);
    m.walking |= within;
    m.velocity = if m.walking {
        [mul(speed, cos(m.heading)), mul(speed, sin(m.heading))]
    } else {
        [0, 0]
    };
}

/// The player that never fights (SPEC-0019: not an enemy of anyone).
const NEUTRAL_PASSIVE: i32 = 15;

/// Whether units of `a` take units of `b` for enemies.
fn enemies(state: &State, a: i32, b: i32) -> bool {
    a != b && a != NEUTRAL_PASSIVE && b != NEUTRAL_PASSIVE && !state.setup.is_ally(a, b)
}

/// Idle units with a weapon look for enemies at their updates (SPEC-0019,
/// "Acquisition"): the nearest enemy unit whose centre is within
/// `acquire` and both collision sizes becomes the target of an attack of
/// their own, which `GetUnitCurrentOrder` shows as 0.
///
/// PROVISIONAL: a unit looks every 6 ticks from its creation on, from the
/// first update (the game's first look comes at a random update from the
/// 1st to the 24th, open question 2); the nearest enemy is taken, the
/// earlier created one on a tie.
pub(super) fn acquisitions(state: &mut State) {
    let tick = state.ticks;
    let idle: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|h| match state.objects.get(h) {
            Some(Object::Unit(u)) => {
                u.alive
                    && u.attack.is_none()
                    && u.movement.is_none_or(|m| !m.ordered())
                    && tick > u.born
                    && (tick - u.born) % UPDATE == 0
            }
            _ => false,
        })
        .collect();
    if idle.is_empty() {
        return;
    }
    // The possible targets, by x: (x, y, collision, owner, creation
    // order, handle).
    let mut targets: Vec<(f32, f32, u32, i32, usize, u32)> = state
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, &h)| match state.objects.get(&h) {
            Some(Object::Unit(o)) if o.alive && !o.hidden => {
                let collision = state.data.units.get(&o.type_id).map_or(0, |d| d.collision);
                Some((real_of(o.x), real_of(o.y), collision, o.owner, i, h))
            }
            _ => None,
        })
        .collect();
    targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.4.cmp(&b.4)));
    let widest = targets.iter().map(|t| real_of(t.2)).fold(0.0f32, f32::max);
    for h in idle {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let (owner, x, y) = (u.owner, real_of(u.x), real_of(u.y));
        let Some(data) = state.data.units.get(&u.type_id) else {
            continue;
        };
        if data.weapon.is_none() || real_of(data.acquire) <= 0.0 {
            continue;
        }
        let reach = add(data.acquire, data.collision);
        let from = targets.partition_point(|t| t.0 < x - real_of(reach) - widest - 1.0);
        let mut best: Option<(f32, usize, u32)> = None;
        for &(ox, oy, collision, other_owner, i, other) in &targets[from..] {
            let limit = real_of(add(reach, collision));
            // Plain reals here: the comparison is the same, and scanning
            // every pair in the engine arithmetic is slow.
            let (dx, dy) = (ox - x, oy - y);
            if dx > real_of(reach) + widest + 1.0 {
                break;
            }
            if dx.abs() > limit || dy.abs() > limit || !enemies(state, owner, other_owner) {
                continue;
            }
            let distance = dx * dx + dy * dy;
            if distance <= limit * limit
                && best.is_none_or(|(d, j, _)| distance < d || (distance == d && i < j))
            {
                best = Some((distance, i, other));
            }
        }
        if let Some((_, _, target)) = best {
            let Some(Object::Unit(o)) = state.objects.get(&target) else {
                continue;
            };
            let at = [o.x, o.y];
            order(state, h, Order::Attack { target }, 0, at);
        }
    }
}
