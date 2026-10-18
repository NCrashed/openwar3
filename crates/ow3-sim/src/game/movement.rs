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
//! on a target (stand for the reaction delay, then walk until the target
//! passes the range test, then turn until the heading is within 0.2
//! radians of the target and attack), `stop` and `holdposition` (end the
//! motion at once). Other new orders given to a unit that is moving take
//! effect at its next update, and its first motion comes a tick late.
//!
//! On a map with a pathing map, point orders and patrols go round its
//! obstacles and other units by the paths of SPEC-0020 and SPEC-0021
//! (`game::follow`). Positions, headings and velocities follow the
//! movement arithmetic of SPEC-0018, and all units share one update cycle.
//!
//! Attackers walk to their goal points by the same paths, a new one at
//! every new goal, searched to the target up to the attack range
//! (SPEC-0019).
//!
//! PROVISIONAL: the extra commits between updates (SPEC-0018 open
//! question 2) are left out; the arrival test compares squared distances
//! in cells (open question 1).

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::natives::{arc_cosine, cos, sin, sqrt};
use ow3_math::{add, div, floor, i2r, mul, sub};

use super::{Object, State, follow, handle, integer, real, string};
use crate::random::REACTION;

/// Order ids that `GetUnitCurrentOrder` gives (SPEC-0019).
pub const ORDER_SMART: i32 = 851_971;
pub const ORDER_ATTACK: i32 = 851_983;
pub const ORDER_MOVE: i32 = 851_986;
pub const ORDER_PATROL: i32 = 851_991;
/// Ticks between updates of a unit.
const UPDATE: u64 = 6;
/// The ticks of the update cycle, the same for all units, are those equal
/// to `PHASE` modulo `UPDATE` (SPEC-0018, "Phase"; the game counts them
/// one lower, from its own start).
const PHASE: u64 = 5;

/// The first update after `tick`: updates come before the script's timers
/// in a tick, so an order given at an update tick waits for the next.
pub(super) fn next_update_after(tick: u64) -> u64 {
    tick + UPDATE - (tick + UPDATE - PHASE) % UPDATE
}

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
    pub(super) heading: u32,
    /// The tick of the next update.
    next_update: u64,
    /// Moving since the heading came within the propulsion window.
    walking: bool,
    /// The committed position, relative to the world's lower left corner,
    /// the velocity in world units per second and the movement clock at
    /// the commit (SPEC-0018, "Movement arithmetic").
    pub(super) origin: [u32; 2],
    pub(super) velocity: [u32; 2],
    since: u32,
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
    /// Updates until an attacker renews its goal point; `None` before
    /// the first goal of a chase.
    chase: Option<u32>,
    /// The next goal of a resumed chase comes with an update standing.
    renew_stands: bool,
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

    /// The target of the attack order while it is on.
    pub fn target(&self) -> Option<u32> {
        match self.order {
            Order::Attack { target } if !self.arrived => Some(target),
            _ => None,
        }
    }
}

/// The position `p` relative to the world's lower left corner `min`.
pub(super) fn relative(p: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [add(p[0], min[0] ^ SIGN), add(p[1], min[1] ^ SIGN)]
}

fn absolute(r: [u32; 2], min: [u32; 2]) -> [u32; 2] {
    [sub(r[0], min[0] ^ SIGN), sub(r[1], min[1] ^ SIGN)]
}

/// 0.0001: shorter clock differences count as none.
const NO_TIME: u32 = 0x38D1_B717;
/// Vectors and angles this small count as 0 (0.00001).
const NO_LENGTH: u32 = 0x3727_C5AC;
/// Below this speed the stored heading is kept (0.0000004).
const STILL: u32 = 0x34D6_BF95;
/// Turns this small count as none (0.0000002).
const NO_TURN: u32 = 0x3456_BF95;

/// The movement clock time from `since` to `clock`, 0 below 0.0001.
fn elapsed(clock: u32, since: u32) -> u32 {
    let e = sub(clock, since);
    if real_of(e).abs() < real_of(NO_TIME) {
        0
    } else {
        e
    }
}

impl Moving {
    /// The position at movement clock `clock`: the committed one moved on
    /// by the velocity.
    fn position(&self, clock: u32) -> [u32; 2] {
        let e = elapsed(clock, self.since);
        [
            add(self.origin[0], mul(self.velocity[0], e)),
            add(self.origin[1], mul(self.velocity[1], e)),
        ]
    }

    /// Commits the position at `clock`.
    fn commit(&mut self, clock: u32) {
        self.origin = self.position(clock);
        self.since = clock;
    }
}

/// The angle of vector (`x`, `y`) in `0..2 pi`, through `acos` and `sqrt`
/// of the engine (SPEC-0018).
fn angle(x: u32, y: u32) -> u32 {
    let len = sqrt(add(mul(x, x), mul(y, y)));
    if real_of(len).abs() <= real_of(NO_LENGTH) {
        return 0;
    }
    let a = arc_cosine(div(x, len));
    if real_of(a).abs() <= real_of(NO_LENGTH) {
        0
    } else if real_of(y) < 0.0 {
        sub(TWO_PI, a)
    } else {
        a
    }
}

/// The heading of a unit at an update, and the turn `d` towards `target`
/// (SPEC-0018, steps 1 and 2).
fn heading_and_turn(m: &Moving, target: [u32; 2]) -> (u32, u32) {
    let [vx, vy] = m.velocity;
    let speed = sqrt(add(mul(vx, vx), mul(vy, vy)));
    let h = if real_of(speed) < real_of(STILL) {
        m.heading
    } else {
        angle(vx, vy)
    };
    let a = angle(sub(target[0], m.origin[0]), sub(target[1], m.origin[1]));
    let mut d = sub(a, h);
    if real_of(d) >= 0.0 && real_of(d) > real_of(PI) {
        d = sub(d, TWO_PI);
    } else if real_of(d) < 0.0 && -real_of(d) > real_of(PI) {
        d = add(d, TWO_PI);
    }
    if real_of(d).abs() < real_of(NO_TURN) {
        d = 0;
    }
    (h, d)
}

/// An update of a unit's motion towards `target` (SPEC-0018, steps 1 to
/// 5): the heading turns by at most `turn_rate`, and the unit moves at
/// `speed` when it may and the turn was within the propulsion window
/// `window`. Whether it moves.
fn motion(
    m: &mut Moving,
    target: [u32; 2],
    may_move: bool,
    speed: u32,
    turn_rate: u32,
    window: u32,
) -> bool {
    let (h, d) = heading_and_turn(m, target);
    let moves = may_move && real_of(d).abs() < real_of(window) && real_of(speed) > 0.0;
    m.heading = if real_of(d).abs() <= real_of(turn_rate) {
        add(h, d)
    } else if real_of(d) > 0.0 {
        add(h, turn_rate)
    } else {
        add(h, turn_rate ^ SIGN)
    };
    let [vx, vy] = m.velocity;
    m.velocity = if moves {
        let w = [mul(speed, cos(m.heading)), mul(speed, sin(m.heading))];
        [add(vx, sub(w[0], vx)), add(vy, sub(w[1], vy))]
    } else {
        [0, 0]
    };
    m.walking = moves;
    moves
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
    let clock = state.move_clock;
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
            chase: None,
            renew_stands: false,
            ..m
        },
        None => Moving {
            order,
            order_id,
            target,
            heading: u.facing,
            next_update: next_update_after(tick),
            walking: false,
            origin: relative([u.x, u.y], min),
            velocity: [0, 0],
            since: clock,
            facing_step: 0,
            arrived: false,
            restart: false,
            skip: false,
            reaction: None,
            chase: None,
            renew_stands: false,
        },
    };
    // A new path is searched at the next update (SPEC-0020).
    state.paths.remove(&h);
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

/// `IssuePointOrder(h, name, target)`: what the native returns.
pub(super) fn point_order(state: &mut State, h: u32, name: &str, target: [u32; 2]) -> bool {
    let here = state.unit(Some(h)).map(|u| [u.x, u.y]);
    match (name, here) {
        ("move", _) => order(state, h, Order::Point, ORDER_MOVE, target),
        ("attack", _) => order(state, h, Order::Point, ORDER_ATTACK, target),
        ("patrol", Some(home)) => order(state, h, Order::Patrol { home }, ORDER_PATROL, target),
        // The unit obeys, but the native gives false (SPEC-0019).
        ("smart", _) => {
            order(state, h, Order::Point, ORDER_SMART, target);
            false
        }
        _ => false,
    }
}

/// `IssueTargetOrder(h, name, target)`: only `attack` so far.
pub(super) fn target_order(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    if name == "move" {
        // PROVISIONAL: the game takes it with the current order `move`
        // (SPEC-0019); how a unit follows its target is not specified, so
        // it walks to where the target is now.
        return match position_of(state, target) {
            Some(at) if target != h => order(state, h, Order::Point, ORDER_MOVE, at),
            _ => false,
        };
    }
    if name != "attack" {
        return false;
    }
    let armed = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| t.weapon.is_some());
    match position_of(state, target) {
        Some(at) if armed && target != h && super::attack::standing(state, target) => {
            // The reaction delay (SPEC-0019).
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            let tick = state.ticks;
            let done = order(state, h, Order::Attack { target }, ORDER_ATTACK, at);
            if done {
                react(state, h, tick, delay);
            }
            done
        }
        _ => false,
    }
}

/// The unit `h` waits `delay` seconds counted from `from`: a walking unit
/// stops at once and stands during the delay (SPEC-0019).
fn react(state: &mut State, h: u32, from: u64, delay: u32) {
    let clock = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        if let Some(m) = u.movement.as_mut() {
            m.reaction = Some((from, delay));
            m.walking = false;
            m.restart = false;
            m.skip = false;
            m.commit(clock);
            m.velocity = [0, 0];
        }
    }
}

/// `IssueImmediateOrder(h, name)`: `stop` and `holdposition`.
pub(super) fn immediate_order(state: &mut State, h: u32, name: &str) -> bool {
    match name {
        "stop" | "holdposition" => halt(state, h),
        // PROVISIONAL: the game takes it and the current order reads 0
        // after it (SPEC-0019); here it stops the unit like `stop`.
        "move" => halt(state, h),
        _ => false,
    }
}

/// The ids of the order names (SPEC-0019, "Order ids").
const ORDER_IDS: [(&str, i32); 7] = [
    ("smart", 851_971),
    ("stop", 851_972),
    ("attack", 851_983),
    ("attackground", 851_984),
    ("move", 851_986),
    ("patrol", 851_990),
    ("holdposition", 851_993),
];

/// `OrderId`: the id of `name` in any case, 0 for an unknown name.
fn order_id(name: &str) -> i32 {
    ORDER_IDS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map_or(0, |&(_, id)| id)
}

/// `OrderId2String`: the name of `id`, none for 0 and unknown ids.
fn order_name(id: i32) -> Option<&'static str> {
    ORDER_IDS.iter().find(|&&(_, i)| i == id).map(|&(n, _)| n)
}

/// Whether units of `a` and `b` are enemies of each other.
pub(super) fn hostile(state: &State, a: u32, b: u32) -> bool {
    match (state.unit(Some(a)), state.unit(Some(b))) {
        (Some(a), Some(b)) => enemies(state, a.owner, b.owner),
        _ => false,
    }
}

/// `stop` and `holdposition`: the unit stands where it is now.
fn halt(state: &mut State, h: u32) -> bool {
    state.paths.remove(&h);
    let clock = state.move_clock;
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
        m.commit(clock);
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
            let done = point_order(&mut state.borrow_mut(), h, name, target);
            Ok(Some(Value::Boolean(done)))
        }
    );
    // The same order to the point of a location (`IssuePointOrderLocBJ`).
    native!(
        add,
        state,
        "native IssuePointOrderLoc takes unit whichUnit, string order, location whichLocation returns boolean",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let at = super::enumerate::location(&state, handle(&args[2]));
            let (Some(h), Some(name), Some((x, y))) = (handle(&args[0]), string(&args[1]), at)
            else {
                return Ok(Some(Value::Boolean(false)));
            };
            let done = point_order(&mut state, h, name, [x, y]);
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean",
        |vm, state, args| {
            let (Some(h), Some(name), Some(target)) =
                (handle(&args[0]), string(&args[1]), handle(&args[2]))
            else {
                return Ok(Some(Value::Boolean(false)));
            };
            let done = target_order(&mut state.borrow_mut(), h, name, target);
            Ok(Some(Value::Boolean(done)))
        }
    );
    // Order ids (SPEC-0019): the `...ById` natives act as the named ones,
    // and an id without a name is refused, the order staying as it was.
    native!(
        add,
        state,
        "native OrderId takes string orderIdString returns integer",
        |vm, state, args| Ok(Some(Value::Integer(string(&args[0]).map_or(0, order_id))))
    );
    native!(
        add,
        state,
        "native OrderId2String takes integer orderId returns string",
        |vm, state, args| {
            Ok(Some(match order_name(integer(&args[0])) {
                Some(name) => Value::string(name),
                None => Value::String(None),
            }))
        }
    );
    native!(
        add,
        state,
        "native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean",
        |vm, state, args| {
            let (Some(h), Some(name)) = (handle(&args[0]), order_name(integer(&args[1]))) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let target = [real(&args[2]), real(&args[3])];
            let done = point_order(&mut state.borrow_mut(), h, name, target);
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueTargetOrderById takes unit whichUnit, integer order, widget targetWidget returns boolean",
        |vm, state, args| {
            let (Some(h), Some(name), Some(target)) = (
                handle(&args[0]),
                order_name(integer(&args[1])),
                handle(&args[2]),
            ) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let done = target_order(&mut state.borrow_mut(), h, name, target);
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean",
        |vm, state, args| {
            let done = match (handle(&args[0]), order_name(integer(&args[1]))) {
                (Some(h), Some(name)) => immediate_order(&mut state.borrow_mut(), h, name),
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
                (Some(h), Some(name)) => immediate_order(&mut state.borrow_mut(), h, name),
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

/// One tick of motion of every moving unit, before the timers: the
/// movement clock advances and each unit stands at its committed position
/// moved on by its velocity (SPEC-0018).
pub(super) fn step(state: &mut State) {
    state.move_clock = add(state.move_clock, crate::timers::TICK);
    let (min, clock) = (
        [state.world_bounds[0], state.world_bounds[1]],
        state.move_clock,
    );
    for h in &state.moving {
        let Some(Object::Unit(u)) = state.objects.get_mut(h) else {
            continue;
        };
        let Some(m) = u.movement.as_ref() else {
            continue;
        };
        if m.velocity == [0, 0] {
            continue;
        }
        let [x, y] = absolute(m.position(clock), min);
        u.x = x;
        u.y = y;
    }
}

/// The updates due at this tick, after the timers.
pub(super) fn updates(_vm: &mut Vm, state: &mut State) {
    let tick = state.ticks;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    // Units updating in the same tick go from the last created (SPEC-0021).
    let mut moving = std::mem::take(&mut state.moving);
    let created: std::collections::HashMap<u32, usize> = state
        .units
        .iter()
        .enumerate()
        .map(|(i, &h)| (h, i))
        .collect();
    moving.sort_by_key(|h| std::cmp::Reverse(created.get(h).copied()));
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
        // Range and collision size in cells.
        let reach = data.map_or(0, |t| {
            let range = t.weapon.as_ref().map_or(0, |w| w.range);
            add(mul(range, CELL), mul(t.collision, CELL))
        });
        let range_cells = mul(
            data.and_then(|t| t.weapon.as_ref()).map_or(0, |w| w.range),
            CELL,
        );
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let Some(mut m) = u.movement else {
            continue;
        };
        let class = crate::path::size_class(data.map_or(0, |t| t.collision));
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
        // The position is committed first (SPEC-0018).
        m.commit(state.move_clock);
        let here = m.origin;
        let mut begin_attack = None;
        // Still in the reaction delay: the unit stands.
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
                m.velocity = [0, 0];
                m.skip = true;
            }
            // An attacker walks to its goal point, where the target stood
            // when the goal was last renewed, and is in reach when the
            // goal is.
            let mut in_reach = false;
            let mut renewed = false;
            let mut target_now = [0, 0];
            let mut full_reach = reach;
            if let Order::Attack { target } = m.order {
                match position_of(state, target) {
                    Some(at) if super::attack::standing(state, target) => {
                        target_now = relative(at, min);
                        match m.chase {
                            Some(left) if left > 0 => m.chase = Some(left - 1),
                            _ => {
                                // A new goal; the paths follow it
                                // (`game::follow`).
                                m.target = at;
                                m.chase = Some(countdown(here, relative(at, min)));
                                renewed = true;
                            }
                        }
                        let other = match state.objects.get(&target) {
                            Some(Object::Unit(t)) => {
                                state.data.units.get(&t.type_id).map_or(0, |d| d.collision)
                            }
                            _ => 0,
                        };
                        // The swing starts with the target or the goal in
                        // range (SPEC-0019).
                        full_reach = add(reach, mul(other, CELL));
                        in_reach = in_range(here, target_now, full_reach)
                            || in_range(here, relative(m.target, min), full_reach);
                    }
                    _ => {
                        m.arrived = true;
                        m.velocity = [0, 0];
                    }
                }
            }
            if !m.arrived {
                let goal = relative(m.target, min);
                let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
                // The squared distance in cells.
                let (cx, cy) = (mul(dx, CELL), mul(dy, CELL));
                let distance = real_of(add(mul(cx, cx), mul(cy, cy)));
                if let Order::Attack { target } = m.order {
                    if in_reach {
                        // In range: stand, turn to the target and strike
                        // once the turn left is below 0.2 radians.
                        let (h, d) = heading_and_turn(&m, goal);
                        if real_of(d).abs() < ATTACK_ANGLE {
                            m.heading = h;
                            m.walking = false;
                            m.velocity = [0, 0];
                            m.arrived = true;
                            begin_attack = Some(target);
                        } else {
                            motion(&mut m, goal, false, speed, turn_rate, window);
                        }
                    } else if state.grid.is_some() {
                        // To the goal point by the paths of SPEC-0020 and
                        // SPEC-0021, as a point order.
                        let walker = follow::Walker {
                            handle: h,
                            here,
                            order: goal,
                            class,
                            squared_speed: follow::squared_speed(m.velocity),
                            chase: Some(follow::Chase {
                                target: target_now,
                                range: range_cells,
                                reach: full_reach,
                            }),
                        };
                        let steer = follow::steer(state, &walker, tick);
                        // The goal taken 66 ticks after a chase resumes
                        // comes with an update standing (SPEC-0019).
                        let steer = if renewed && std::mem::take(&mut m.renew_stands) {
                            follow::Steer::Stand(goal)
                        } else {
                            steer
                        };
                        match steer {
                            follow::Steer::Walk(to) => {
                                motion(&mut m, to, true, speed, turn_rate, window);
                            }
                            follow::Steer::Stand(to) => {
                                motion(&mut m, to, false, speed, turn_rate, window);
                            }
                            // At the goal but not in range of the target:
                            // it waits for the next goal.
                            follow::Steer::Arrived => {
                                motion(&mut m, goal, false, speed, turn_rate, window);
                            }
                        }
                    } else {
                        motion(&mut m, goal, true, speed, turn_rate, window);
                    }
                } else {
                    let squared_speed = follow::squared_speed(m.velocity);
                    let walker = |order| follow::Walker {
                        handle: h,
                        here,
                        order,
                        class,
                        squared_speed,
                        chase: None,
                    };
                    // Round the obstacles of the pathing map (SPEC-0020), or
                    // straight without one.
                    let mut steer = if state.grid.is_some() {
                        follow::steer(state, &walker(goal), tick)
                    } else if distance <= real_of(mul(ARRIVAL, ARRIVAL)) {
                        follow::Steer::Arrived
                    } else {
                        follow::Steer::Walk(goal)
                    };
                    if steer == follow::Steer::Arrived {
                        if let Order::Patrol { home } = m.order {
                            // Back the other way, turning on the spot.
                            m.order = Order::Patrol { home: m.target };
                            m.target = home;
                            m.walking = false;
                            m.velocity = [0, 0];
                            state.paths.remove(&h);
                            let goal = relative(home, min);
                            steer = if state.grid.is_some() {
                                follow::steer(state, &walker(goal), tick)
                            } else {
                                follow::Steer::Walk(goal)
                            };
                        } else {
                            m.arrived = true;
                            m.walking = false;
                            m.velocity = [0, 0];
                            state.paths.remove(&h);
                        }
                    }
                    match steer {
                        follow::Steer::Walk(to) => {
                            motion(&mut m, to, true, speed, turn_rate, window);
                        }
                        // No motion, but the heading turns (SPEC-0020).
                        follow::Steer::Stand(to) => {
                            motion(&mut m, to, false, speed, turn_rate, window);
                        }
                        follow::Steer::Arrived => {}
                    }
                }
            }
        }
        // After a new order the first tick of motion is lost (SPEC-0019):
        // the clock of the commit is one tick ahead.
        if m.skip && m.walking {
            m.skip = false;
            m.since = add(state.move_clock, crate::timers::TICK);
        }
        // The facing follows the heading (SPEC-0018).
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        let heading = normal(m.heading);
        let (facing, step) = super::facing::follow(u.facing, heading, m.facing_step, orient);
        u.facing = facing;
        m.facing_step = step;
        let facing_done = u.facing == heading;
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

/// The range test of SPEC-0019 between the points `a` and `b`, with
/// `reach` the range and both collision sizes in cells: the squared
/// distance in cells below `R * R` or within 0.001 of it, with `R` at
/// least 0.49.
pub(super) fn in_range(a: [u32; 2], b: [u32; 2], reach: u32) -> bool {
    let r = if real_of(reach) < real_of(ARRIVAL) {
        ARRIVAL
    } else {
        reach
    };
    let (dx, dy) = (mul(sub(b[0], a[0]), CELL), mul(sub(b[1], a[1]), CELL));
    let d2 = add(mul(dx, dx), mul(dy, dy));
    let r2 = mul(r, r);
    real_of(d2) < real_of(r2) || real_of(sub(d2, r2)).abs() < real_of(RANGE_SLACK)
}

/// The updates until a chase renews its goal after taking the goal `b`
/// from `a` (SPEC-0019): `round(0.33 d)` with `d` in cells, clamped to
/// 16..132. The square root is the correctly rounded one of IEEE 754.
fn countdown(a: [u32; 2], b: [u32; 2]) -> u32 {
    let (dx, dy) = (mul(sub(b[0], a[0]), CELL), mul(sub(b[1], a[1]), CELL));
    let d = real_of(add(mul(dx, dx), mul(dy, dy))).sqrt();
    let x = real_of(mul(ow3_math::s2r("0.33"), d.to_bits()));
    ((x + 0.5).floor() as u32).clamp(16, 132)
}

/// Whether unit `h` reaches `target` from where both stand now.
pub(super) fn reaches(state: &State, h: u32, target: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let collision = |u: &super::Unit| state.data.units.get(&u.type_id).map(|t| (t, t.collision));
    let (Some(u), Some(at)) = (state.unit(Some(h)), position_of(state, target)) else {
        return false;
    };
    let Some((data, own)) = collision(u) else {
        return false;
    };
    let other = state
        .unit(Some(target))
        .and_then(collision)
        .map_or(0, |c| c.1);
    let range = data.weapon.as_ref().map_or(0, |w| w.range);
    let reach = add(add(mul(range, CELL), mul(own, CELL)), mul(other, CELL));
    in_range(relative([u.x, u.y], min), relative(at, min), reach)
}

/// Unit `h` takes up its attack on `target` again after its swing
/// (SPEC-0019): the goal is where the target is now, its path is searched
/// at once, the unit moves from its next update on, and at its 12th
/// update it takes a new goal with an update standing.
pub(super) fn chase(state: &mut State, h: u32, target: u32, order_id: i32) {
    let Some(at) = position_of(state, target) else {
        return;
    };
    if !order(state, h, Order::Attack { target }, order_id, at) {
        return;
    }
    let (tick, min) = (state.ticks, [state.world_bounds[0], state.world_bounds[1]]);
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    let here = relative([u.x, u.y], min);
    if let Some(m) = u.movement.as_mut() {
        m.chase = Some(11);
        m.renew_stands = true;
    }
    let data = state.data.units.get(&u.type_id);
    let own = data.map_or(0, |t| t.collision);
    let class = crate::path::size_class(own);
    let range = data.and_then(|t| t.weapon.as_ref()).map_or(0, |w| w.range);
    let other = state
        .unit(Some(target))
        .and_then(|t| state.data.units.get(&t.type_id))
        .map_or(0, |t| t.collision);
    let reach = add(add(mul(range, CELL), mul(own, CELL)), mul(other, CELL));
    if state.grid.is_some() {
        let goal = relative(at, min);
        let walker = follow::Walker {
            handle: h,
            here,
            order: goal,
            class,
            squared_speed: 0,
            chase: Some(follow::Chase {
                target: goal,
                range: mul(range, CELL),
                reach,
            }),
        };
        follow::steer(state, &walker, tick);
    }
}

/// 0.001, the slack of the range test.
const RANGE_SLACK: u32 = 0x3A83_126F;

/// The player that never fights (SPEC-0019: not an enemy of anyone).
const NEUTRAL_PASSIVE: i32 = 15;

/// Whether units of `a` take units of `b` for enemies.
pub(super) fn enemies(state: &State, a: i32, b: i32) -> bool {
    a != b && a != NEUTRAL_PASSIVE && b != NEUTRAL_PASSIVE && !state.setup.is_ally(a, b)
}

/// 100 ticks (0.5 s) between the acquisition checks of a unit.
const ACQUIRE_PERIOD: u64 = 100;
/// The tick counted from a unit's creation that SPEC-0019 calls 0, as the
/// timers count it (a 0.5 s timer of the map initialisation fires at our
/// tick 101).
const ACQUIRE_OFFSET: u64 = 1;
const HUNDRED: u32 = 0x42C8_0000;
const HALF: u32 = 0x3F00_0000;

/// Idle units with a weapon look for enemies on their own schedule
/// (SPEC-0019, "Acquisition by idle units"): `floor(100 * x)` ticks after
/// their creation and then every 100 ticks, with `x` drawn when they were
/// created. Of the enemies whose centres are within `acquire` and both
/// collision sizes a unit takes the nearest unit, a structure only when
/// there is no unit, and on equal distances the one with the smaller y,
/// then x. The target becomes that of an attack of its own, which
/// `GetUnitCurrentOrder` shows as 0, after the reaction delay counted
/// from the exact time of the check. Gives the units that acquired a
/// target, in that order.
pub(super) fn acquisitions(state: &mut State) -> Vec<u32> {
    let tick = state.ticks;
    // (handle, x, start of the schedule).
    let due: Vec<(u32, u32, u64)> = state
        .units
        .iter()
        .filter_map(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u))
                if u.alive
                    && !u.paused
                    && u.attack.is_none()
                    && u.movement.is_none_or(|m| !m.ordered()) =>
            {
                let x = Some(u.acquire_phase).filter(|&x| x != super::NO_PHASE)?;
                let start = u.born + ACQUIRE_OFFSET;
                let first = start + real_of(floor(mul(x, HUNDRED))) as u64;
                (tick >= first && (tick - first) % ACQUIRE_PERIOD == 0).then_some((h, x, start))
            }
            _ => None,
        })
        .collect();
    if due.is_empty() {
        return Vec::new();
    }
    // The possible targets, by x: (x, y, collision, owner, structure,
    // handle).
    let mut targets: Vec<(f32, f32, u32, i32, bool, u32)> = state
        .units
        .iter()
        .filter_map(|&h| match state.objects.get(&h) {
            Some(Object::Unit(o)) if o.alive && !o.hidden => {
                let data = state.data.units.get(&o.type_id);
                let collision = data.map_or(0, |d| d.collision);
                let structure = data.is_some_and(|d| d.building);
                Some((real_of(o.x), real_of(o.y), collision, o.owner, structure, h))
            }
            _ => None,
        })
        .collect();
    targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let widest = targets.iter().map(|t| real_of(t.2)).fold(0.0f32, f32::max);
    let mut acquired = Vec::new();
    for (h, phase, start) in due {
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
        // (structure, squared distance, y, x, handle).
        let mut best: Option<(bool, f32, f32, f32, u32)> = None;
        for &(ox, oy, collision, other_owner, structure, other) in &targets[from..] {
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
            let key = (structure, distance, oy, ox, other);
            if distance <= limit * limit
                && best.is_none_or(|b| (key.0, key.1, key.2, key.3) < (b.0, b.1, b.2, b.3))
            {
                best = Some(key);
            }
        }
        let Some((.., target)) = best else {
            continue;
        };
        let Some(Object::Unit(o)) = state.objects.get(&target) else {
            continue;
        };
        let at = [o.x, o.y];
        if order(state, h, Order::Attack { target }, 0, at) {
            // The delay runs from the exact time of the check, `0.5 * x`
            // seconds after the start of the schedule.
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            react(state, h, start, add(delay, mul(phase, HALF)));
            acquired.push(h);
        }
    }
    acquired
}
