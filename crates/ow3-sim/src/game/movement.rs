//! The `move` order of a unit on open ground (SPEC-0018, part 1).
//!
//! The unit is updated every 6 ticks from the order on. At an update it
//! stops when it is within 15.6 of the target; otherwise its heading turns
//! towards the target by at most `turnRate`, and once the heading of the
//! update before was within `propWin` of the target's direction it moves
//! along the new heading at `spd` until the next update. Its position
//! advances every tick from the position at the update by the time
//! passed, relative to the world's lower left corner, in the arithmetic of
//! SPEC-0004. Its facing follows the heading at the pace of its
//! `orientInterp` (SPEC-0013), also after it has stopped.
//!
//! PROVISIONAL: units go straight at their target (no path finding, no
//! collisions, no small heading wobble, open question 2); the arrival
//! test is the plain distance below 15.6 (open question 1); the first
//! update comes 6 ticks after the order.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::natives::{atan2, cos, sin};
use ow3_math::{add, i2r, mul, sub};

use super::{Object, State, handle, real, string};

/// The order id of `move` that `GetUnitCurrentOrder` gives.
pub const ORDER_MOVE: i32 = 851_986;
/// Ticks between updates of a unit.
const UPDATE: u64 = 6;
/// 15.6 squared: a unit closer than 15.6 to its target has arrived.
const ARRIVED: f32 = 15.6 * 15.6;
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

/// A unit going somewhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moving {
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
    /// Stopped: only the facing still turns towards the heading.
    arrived: bool,
}

impl Moving {
    /// Whether the move order is still on.
    pub fn ordered(&self) -> bool {
        !self.arrived
    }

    /// Whether the unit moves now, for its animation.
    pub fn walking(&self) -> bool {
        !self.arrived && self.walking
    }
}

/// The position `p` relative to the world's lower left corner `min`.
fn relative(p: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [add(p[0], min[0] ^ SIGN), add(p[1], min[1] ^ SIGN)]
}

fn absolute(r: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [sub(r[0], min[0] ^ SIGN), sub(r[1], min[1] ^ SIGN)]
}

/// Gives unit `h` a move order to `target`; false for a unit that cannot
/// move.
fn order(state: &mut State, h: u32, target: [u32; 2]) -> bool {
    let tick = state.ticks;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(Object::Unit(u)) = state.objects.get(&h) else {
        return false;
    };
    let speed = state.data.units.get(&u.type_id).map_or(0, |t| t.speed);
    if !u.alive || real_of(speed) <= 0.0 && !state.data.units.is_empty() {
        return false;
    }
    let moving = Moving {
        target,
        heading: u.facing,
        next_update: tick + UPDATE,
        walking: false,
        origin: relative([u.x, u.y], min),
        velocity: [0, 0],
        ticks: 0,
        facing_step: 0,
        arrived: false,
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

pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean",
        |vm, state, args| {
            let (Some(h), Some("move")) = (handle(&args[0]), string(&args[1])) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let target = [real(&args[2]), real(&args[3])];
            let done = order(&mut state.borrow_mut(), h, target);
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
                Some(u) if u.movement.is_some_and(|m| m.ordered()) => ORDER_MOVE,
                _ => 0,
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
pub(super) fn updates(state: &mut State) {
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
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        let Some(mut m) = u.movement else {
            continue;
        };
        if !alive {
            u.movement = None;
            continue;
        }
        if m.next_update != tick {
            still.push(h);
            continue;
        }
        m.next_update = tick + UPDATE;
        if !m.arrived {
            // Where the unit is now.
            let here = relative([u.x, u.y], min);
            let goal = relative(m.target, min);
            let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
            let distance = real_of(add(mul(dx, dx), mul(dy, dy)));
            if distance < ARRIVED {
                m.arrived = true;
                m.velocity = [0, 0];
            } else {
                let direction = atan2(dy, dx);
                // The heading of the update before decides whether the
                // unit moves from this update on.
                let within = real_of(difference(direction, m.heading)).abs() < real_of(window);
                m.heading = turn_towards(m.heading, direction, turn_rate);
                m.walking |= within;
                m.origin = here;
                m.ticks = 0;
                m.velocity = if m.walking {
                    [mul(speed, cos(m.heading)), mul(speed, sin(m.heading))]
                } else {
                    [0, 0]
                };
            }
        }
        // The facing follows the heading.
        let (first, largest) = super::facing::steps(orient);
        m.facing_step = add(m.facing_step, first);
        if real_of(m.facing_step) > real_of(largest) {
            m.facing_step = largest;
        }
        u.facing = turn_towards(u.facing, m.heading, m.facing_step);
        let facing_done = u.facing == m.heading;
        if m.arrived && facing_done {
            u.movement = None;
        } else {
            u.movement = Some(m);
            still.push(h);
        }
    }
    still.extend(std::mem::take(&mut state.moving));
    state.moving = still;
}
