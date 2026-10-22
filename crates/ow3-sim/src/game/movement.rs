//! Orders of units and their                 at_order,ovement on open ground (SPEC-0018, part 1,
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
//! PROVISIONAL: the arrival test compares squared distances in cells
//! (SPEC-0018, open question 1).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
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
/// A unit acts on an order by a script no earlier than this many ticks
/// after it (SPEC-0019, "When it starts").
const ORDER_FIRST_ACT: u64 = 2;
/// Ticks between updates of a unit.
const UPDATE: u64 = 6;
/// The ticks of the update cycle, the same for all units, are those equal
/// to `PHASE` modulo `UPDATE` from `FIRST_UPDATE` on (SPEC-0018, "Phase
/// from the start of the game": 7, 13, 19, ...; tick 1 is the first tick
/// after `main`).
const PHASE: u64 = 1;
const FIRST_UPDATE: u64 = 7;

/// 0.0300000012 (one unit in the last place above the nearest float to
/// 0.03): the time of the first update of the cycle and its period
/// (SPEC-0018, "Order within an update tick").
const UPDATE_STEP: u32 = 0x3CF5_C290;

/// At an update tick, the time `U` of the update cycle for it, else
/// `None`. `U` is 0.03 for the first update and 0.03 more for each next;
/// an update runs in the tick whose queue interval holds its `U`
/// (`clock(k) < U <= clock(k + 1)`), so as `U` moves ahead of the clock
/// the updates fall a tick later now and then (SPEC-0018).
pub(super) fn update_time(state: &mut State) -> Option<u32> {
    state.update_now = false;
    if state.ticks < FIRST_UPDATE {
        return None;
    }
    let next = if state.update_time == 0 {
        UPDATE_STEP
    } else {
        add(state.update_time, UPDATE_STEP)
    };
    let end = add(state.move_clock, crate::timers::TICK);
    if real_of(next) > real_of(end) {
        return None;
    }
    state.update_time = next;
    state.update_now = true;
    // The update cycles are numbered from 1 (SPEC-0018).
    state.update_cycle += 1;
    let cycle = state.update_cycle;
    state.budget.cycle(cycle);
    Some(next)
}

/// The first update after `tick`: updates come before the script's timers
/// in a tick, so an order given at an update tick waits for the next.
pub(super) fn next_update_after(tick: u64) -> u64 {
    let next = tick + UPDATE - (tick + UPDATE - PHASE) % UPDATE;
    next.max(FIRST_UPDATE)
}

/// 1/32: distances of the movement are taken in cells of 32 (SPEC-0018).
pub(super) const CELL: u32 = 0x3D00_0000;
/// The arrival radius of a point move in cells (0.49, 15.68 world units).
const ARRIVAL: u32 = 0x3EFA_E148;
/// A unit attacks once its heading is this close to its target (radians).
const ATTACK_ANGLE: f32 = 0.2;
const PI: u32 = 0x4049_0FDB;
const TWO_PI: u32 = 0x40C9_0FDB;
const ONE: u32 = 0x3F80_0000;
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

/// The stored heading of an update, wrapped into `0..2 pi` when outside
/// it by the `fmod` of SPEC-0004 (`c-RXI-1`: peasant 0 at 1069, 6.684936 -> 3ecdb265).
fn wrapped(angle: u32) -> u32 {
    if (0.0..real_of(TWO_PI)).contains(&real_of(angle)) {
        return angle;
    }
    if real_of(angle) < 0.0 {
        return add(angle, TWO_PI);
    }
    ow3_math::fmod(angle, TWO_PI)
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
    /// Build a building of `type_id` at the target point (SPEC-0026).
    Build { type_id: i32 },
    /// Build the building just started: the worker stays.
    Repair { building: u32 },
    /// Walk to a building under construction and go on building it.
    Resume { building: u32 },
    /// Follow an allied unit (SPEC-0019): `from` is where the follower
    /// stood when ordered (relative to the world's lower left corner) and
    /// `start_gap` the gap between the two then, edge to edge.
    Follow {
        leader: u32,
        from: [u32; 2],
        start_gap: u32,
        /// It stands in the follow range (its order goes on).
        standing: bool,
    },
    /// Walk to a gold mine or tree to gather from it (SPEC-0029).
    Gather { target: u32 },
    /// Carry a load to a building that takes it.
    Return { building: u32 },
    /// Stand at a mine or tree and gather (or wait at the mine).
    Work { target: u32 },
    /// Walk into the range of an ability on `target` to cast it
    /// (SPEC-0035).
    Cast { target: u32 },
}

/// The order id of a worker building (`repair`, SPEC-0026).
pub const ORDER_REPAIR: i32 = 852_024;
/// A worker starts its building within this squared distance of the
/// building's position, in world units (100, SPEC-0026).
const BUILD_REACH_SQUARED: f32 = 10_000.0;
/// A worker starts its building facing it within this angle (0.2).
const FACING_SITE: u32 = 0x3E4C_CCCD;

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
    /// A point order of a player stopped it: it stops after the motion
    /// of the next tick (SPEC-0019).
    stop_after_step: bool,
    /// The tick the reaction delay counts from, the delay in seconds and
    /// the earliest tick of the update that may act: the unit acts on the
    /// order from its first update after both.
    reaction: Option<(u64, u32, u64)>,
    /// The end of the reaction delay of an attack taken at an acquisition
    /// or a call for help, on the timer queue (SPEC-0019): the unit takes
    /// its goal in the tick that holds it.
    reaction_end: Option<u32>,
    /// The goal was taken at the end of the reaction delay: the first
    /// update keeps it and starts the countdown of its renewal from there.
    goal_taken: bool,
    /// The goal is the place a backswing-end check faced the target at:
    /// the first update keeps it (SPEC-0019).
    faced: bool,
    /// The new goal after a swing comes at the first update from this tick
    /// on (SPEC-0019).
    renew_at: Option<u64>,
    /// That renewal is the expiry of a cooldown from before this attack.
    renew_cooldown: bool,
    /// An attack ordered by a script: the update at which the unit acts
    /// cannot start its swing yet (SPEC-0019, "When it starts").
    swing_waits: bool,
    /// Updates until an attacker renews its goal point; `None` before
    /// the first goal of a chase.
    chase: Option<u32>,
    /// The earliest tick of the first swing of an attack ordered by a
    /// script: the update after the one at which the unit acted.
    swing_gate: Option<u64>,
    /// The next goal of a resumed chase comes with an update standing.
    renew_stands: bool,
    /// The point the last update steered to, for the trace.
    aim: [u32; 2],
    /// The number of the order, counted over the game: units are handled
    /// newest order first (SPEC-0021).
    given: u64,
}

impl Moving {
    /// It walks back to its guard point (`attack_moves`): as idle for
    /// acquisitions and calls for help.
    fn going_back(&self) -> bool {
        self.order == Order::Point && self.order_id == 0
    }

    /// Whether the order is still on.
    pub fn ordered(&self) -> bool {
        !self.arrived
    }

    /// Whether the unit gathers (SPEC-0029): walks to its mine or tree,
    /// works there or carries a load back.
    pub(super) fn gathering(&self) -> bool {
        !self.arrived
            && matches!(
                self.order,
                Order::Gather { .. } | Order::Return { .. } | Order::Work { .. }
            )
    }

    /// Whether the unit moves now, for its animation.
    pub fn walking(&self) -> bool {
        !self.arrived && self.walking
    }

    /// The point walked to.
    pub fn target_point(&self) -> [u32; 2] {
        self.target
    }

    /// The id of the order while it is on.
    /// Whether it follows a unit standing in the follow range (SPEC-0019).
    pub fn follower_stands(&self) -> bool {
        !self.arrived && matches!(self.order, Order::Follow { standing: true, .. })
    }

    /// The id of its order, also once it ended.
    pub(super) fn last_order_id(&self) -> i32 {
        self.order_id
    }

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
pub(super) fn angle(x: u32, y: u32) -> u32 {
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
    let mut d = super::facing::angle_difference(a, h);
    if real_of(d).abs() < real_of(NO_TURN) {
        d = 0;
    }
    (h, d)
}

/// An update of a unit's motion towards `target` (SPEC-0018, steps 1 to
/// 5): the heading turns by at most `turn_rate`, and the unit moves at
/// `speed` when it may and the turn was within the propulsion window
/// `window`, its velocity limited to `limit`. Whether it moves.
fn motion(
    m: &mut Moving,
    target: [u32; 2],
    may_move: bool,
    speed: u32,
    limit: u32,
    turn_rate: u32,
    window: u32,
) -> bool {
    let (h, d) = heading_and_turn(m, target);
    m.aim = target;
    let moves = may_move && real_of(d).abs() < real_of(window) && real_of(speed) > 0.0;
    let h = if real_of(d).abs() <= real_of(turn_rate) {
        add(h, d)
    } else if real_of(d) > 0.0 {
        add(h, turn_rate)
    } else {
        add(h, turn_rate ^ SIGN)
    };
    let wanted = m.velocity_before(h, speed);
    m.velocity = [0, 0];
    m.heading = wrapped(h);
    if moves {
        let [vx, vy] = wanted;
        let q = add(mul(vx, vx), mul(vy, vy));
        if real_of(q).abs() >= real_of(NO_TURN) {
            // Limited to the unit's own speed (SPEC-0018, step 5; a group's
            // speed does not change the limit, SPEC-0025): the approximate
            // cosine and sine make it a little longer.
            let len = sqrt(q);
            m.velocity = if real_of(len) > real_of(limit) {
                let k = mul(limit, div(ONE, len));
                [mul(vx, k), mul(vy, k)]
            } else {
                [vx, vy]
            };
            // The stored heading is the angle of the limited velocity,
            // with its own length (SPEC-0018, verification 5).
            m.heading = angle(m.velocity[0], m.velocity[1]);
        }
    }
    m.walking = moves;
    moves
}

impl Moving {
    /// The velocity wanted along heading `h` at `speed`, from the current
    /// one, before the limit.
    fn velocity_before(&self, h: u32, speed: u32) -> [u32; 2] {
        let [vx, vy] = self.velocity;
        let w = [mul(speed, cos(h)), mul(speed, sin(h))];
        [add(vx, sub(w[0], vx)), add(vy, sub(w[1], vy))]
    }
}

/// Where unit `h` is at the start of this tick: its last commit moved on by
/// its velocity to the movement clock (SPEC-0019, the goal of a chase).
fn tick_start_position(state: &State, h: u32) -> Option<[u32; 2]> {
    match state.objects.get(&h)? {
        Object::Unit(u) => {
            let min = [state.world_bounds[0], state.world_bounds[1]];
            Some(
                u.movement
                    .map_or([u.x, u.y], |m| absolute(m.position(state.move_clock), min)),
            )
        }
        _ => position_of(state, h),
    }
}

/// Where a widget stands.
pub(super) fn position_of(state: &State, widget: u32) -> Option<[u32; 2]> {
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
    let given = state.orders_given + 1;
    let moving = match busy {
        // Taken at the next update, which the unit keeps.
        Some(m) => Moving {
            order,
            order_id,
            target,
            restart: (m.walking && !state.regrouping) || m.restart,
            reaction: None,
            reaction_end: None,
            goal_taken: false,
            faced: false,
            renew_at: None,
            renew_cooldown: false,
            chase: None,
            renew_stands: false,
            given,
            swing_gate: None,
            swing_waits: false,
            ..m
        },
        None => Moving {
            order,
            order_id,
            target,
            // The stored heading of a unit whose facing still turns to it
            // (c-RXR-1: troll 16 kept 2.535097 through its attacks).
            heading: u.movement.map_or(u.facing, |m| m.heading),
            next_update: next_update_after(tick),
            walking: false,
            origin: relative([u.x, u.y], min),
            velocity: [0, 0],
            since: clock,
            facing_step: 0,
            arrived: false,
            restart: false,
            skip: false,
            stop_after_step: false,
            reaction: None,
            reaction_end: None,
            goal_taken: false,
            faced: false,
            renew_at: None,
            renew_cooldown: false,
            chase: None,
            renew_stands: false,
            given,
            swing_gate: None,
            swing_waits: false,
            aim: [0, 0],
        },
    };
    state.orders_given = given;
    // A new order takes the unit out of its group.
    for cluster in &mut state.clusters {
        cluster.keep(|m| m != h);
    }
    // A new path is searched at the next update (SPEC-0020); in the new
    // layout of a queued group the members keep theirs until their places
    // are laid out (SPEC-0025, "When a member searches again").
    if !state.regrouping {
        state.paths.remove(&h);
        state.member_places.remove(&h);
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.movement = Some(moving);
        u.attack = None;
        u.turn = None;
        u.casting = None;
    }
    if !state.moving.contains(&h) {
        state.moving.push(h);
    }
    true
}

/// Units ordered to a point together (SPEC-0025, "Clusters"), walking in
/// formation along one path.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    /// In the order of the units in the order.
    members: Vec<u32>,
    /// The order point.
    goal: [u32; 2],
    /// The tick of the first update.
    first: u64,
    /// The members' positions at the order, in cells: the first update
    /// lays the cluster out from them (c-RTW-3 tick 5497).
    at_order: Vec<[u32; 2]>,
    /// The waypoints in cells relative to the world corner, from the
    /// first update on, and the current one.
    waypoints: Vec<[u32; 2]>,
    current: usize,
    /// The formation angle and the members' offsets from the waypoint,
    /// in cells.
    angle: u32,
    offsets: Vec<[u32; 2]>,
    /// Updates since a member arrived while others were not near.
    stall: u32,
    /// Updates of the cluster so far, and the members that have walked
    /// (SPEC-0025, "Members in each other's searches").
    updates: u32,
    walked: std::collections::BTreeSet<u32>,
}

impl Cluster {
    /// Keeps the members `keep` says, each with its offset (the offsets,
    /// once laid out, go by the members' places in the list).
    fn keep(&mut self, keep: impl Fn(u32) -> bool) {
        let kept: Vec<bool> = self.members.iter().map(|&m| keep(m)).collect();
        if self.offsets.len() == kept.len() {
            let mut flags = kept.iter();
            self.offsets.retain(|_| *flags.next().unwrap_or(&true));
        }
        let mut flags = kept.iter();
        self.members.retain(|_| *flags.next().unwrap_or(&true));
    }
}

/// What a cluster tells a member at an update: whether it waits, the
/// group's speed if there is one, whether its place is at the last
/// waypoint, and the number of members.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Control {
    held: bool,
    speed: Option<u32>,
    last: bool,
    size: usize,
}

/// Units whose distance in cells, taken to its integer part, is at most
/// this many are in one cluster.
const CLUSTER_DISTANCE: f32 = 40.0;

/// A point order to `units` together (SPEC-0025): each takes it as a point
/// order, and those that did are split into clusters that walk in
/// formation. Whether any took it.
///
/// PROVISIONAL: the distance of the clusters is the straight one (units
/// with hierarchical paths use a path distance, open question 4), each
/// member keeps its own speed (part 2 of the spec) and finds its own
/// path, and the members walk to the order point plus their offsets (the
/// cluster's waypoints are not followed).
pub(super) fn group_point_order(
    state: &mut State,
    units: &[u32],
    name: &str,
    target: [u32; 2],
) -> bool {
    for &h in units {
        super::queue::clear(state, h);
    }
    regroup(state, units, name, target, true)
}

/// The group order of [`group_point_order`] without touching the members'
/// queues: also the new layout of a queued group order each time one more
/// member starts it (SPEC-0031, "Groups"), where the members walking on
/// do not stop (`stop` false).
pub(super) fn regroup(
    state: &mut State,
    units: &[u32],
    name: &str,
    target: [u32; 2],
    stop: bool,
) -> bool {
    state.regrouping = !stop;
    let taken: Vec<u32> = units
        .iter()
        .copied()
        .filter(|&h| point_order_taken(state, h, name, target, stop).0)
        .collect();
    state.regrouping = false;
    let positions: Vec<[u32; 2]> = taken
        .iter()
        .map(|&h| position_of(state, h).unwrap_or([0, 0]))
        .collect();
    // Connected by chains of near units.
    let mut cluster_of: Vec<usize> = (0..taken.len()).collect();
    for i in 0..taken.len() {
        for j in 0..i {
            let (dx, dy) = (
                mul(sub(positions[i][0], positions[j][0]), CELL),
                mul(sub(positions[i][1], positions[j][1]), CELL),
            );
            let d = real_of(floor(sqrt(add(mul(dx, dx), mul(dy, dy)))));
            if d <= CLUSTER_DISTANCE && cluster_of[i] != cluster_of[j] {
                let (from, to) = (cluster_of[i], cluster_of[j]);
                for c in cluster_of.iter_mut() {
                    if *c == from {
                        *c = to;
                    }
                }
            }
        }
    }
    let first = next_update_after(state.ticks);
    let mut seen = Vec::new();
    for &c in &cluster_of {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        let members: Vec<u32> = (0..taken.len())
            .filter(|&i| cluster_of[i] == c)
            .map(|i| taken[i])
            .collect();
        if members.len() > 1 {
            let at_order: Vec<[u32; 2]> = cluster_members(
                state,
                &members,
                [state.world_bounds[0], state.world_bounds[1]],
            )
            .0
            .iter()
            .map(|m| m.position)
            .collect();
            state.clusters.push(Cluster {
                members,
                goal: target,
                first,
                at_order,
                waypoints: Vec::new(),
                current: 0,
                angle: 0,
                offsets: Vec::new(),
                stall: 0,
                updates: 0,
                walked: std::collections::BTreeSet::new(),
            });
        }
    }
    // The group counts as one order, the newest (SPEC-0025).
    let given = state.orders_given;
    state.group_orders.insert(given);
    for &h in &taken {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
        {
            m.given = given;
        }
    }
    !taken.is_empty()
}

/// The clusters updating at `tick` (SPEC-0025).
///
/// At the first update the leader is the member nearest to the goal, the
/// cluster's path is searched from it, and the formation angle points
/// from the leader to the first waypoint ("Formation angle and leader").
/// Each member walks to the current waypoint plus its offset ("Layout").
/// At every update the members at their places are marked arrived; the
/// cluster goes on to the next waypoint, turning the formation and
/// laying it out again, when one has arrived and the others are near or
/// have waited too long ("Following the cluster's path"); at the last
/// waypoint arrived members leave. Members that would walk ahead of a
/// front rank are held, and while none is held and none has arrived all
/// walk at the slowest member's speed ("Group speed").
///
/// PROVISIONAL: the hold rule is never suspended and one arrived member
/// is not enough to go on (the member states of the 66-update countdown
/// are open question 2).
fn update_clusters(state: &mut State, tick: u64) -> BTreeMap<u32, Control> {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let mut control = BTreeMap::new();
    let mut clusters = std::mem::take(&mut state.clusters);
    for cluster in &mut clusters {
        if tick < cluster.first || !state.update_now {
            continue;
        }
        cluster.keep(|h| state.unit(Some(h)).is_some_and(|u| u.alive));
        let (members, ordered) = cluster_members(state, &cluster.members, min);
        if members.len() < 2 {
            cluster.members.clear();
            continue;
        }
        if cluster.waypoints.is_empty() {
            let mut members = members.clone();
            if cluster.at_order.len() == members.len() {
                for (m, &p) in members.iter_mut().zip(&cluster.at_order) {
                    m.position = p;
                }
            }
            let goal = mul_both(relative(cluster.goal, min), CELL);
            let leader = nearest(&members, goal);
            let class = cluster
                .members
                .get(leader)
                .and_then(|&h| state.unit(Some(h)))
                .map_or(0, |u| {
                    crate::path::size_class(
                        state.data.units.get(&u.type_id).map_or(0, |t| t.collision),
                    )
                });
            cluster.waypoints = cluster_path(state, members[leader].position, goal, class);
            let w = cluster.waypoints[0];
            let lead = members[leader].position;
            cluster.angle = angle(sub(w[0], lead[0]), sub(w[1], lead[1]));
            cluster.offsets = super::formation::offsets(&members, cluster.angle);
            send_to_places(state, cluster, min);
        }
        // Who is at its place.
        let w = cluster.waypoints[cluster.current];
        let distances: Vec<u32> = members
            .iter()
            .zip(&cluster.offsets)
            .map(|(m, o)| {
                let place = [add(w[0], o[0]), add(w[1], o[1])];
                let (dx, dy) = (sub(place[0], m.position[0]), sub(place[1], m.position[1]));
                add(mul(dx, dx), mul(dy, dy))
            })
            .collect();
        let mut arrived: Vec<bool> = distances
            .iter()
            .zip(&ordered)
            .map(|(&d, &o)| !o || real_of(d) <= real_of(mul(ARRIVAL, ARRIVAL)))
            .collect();
        let last = cluster.current + 1 == cluster.waypoints.len();
        if !last && arrived.iter().any(|&a| a) {
            let near = arrived
                .iter()
                .zip(&distances)
                .all(|(&a, &d)| a || real_of(d) < NEAR_PLACE);
            cluster.stall += 1;
            if near || cluster.stall > STALL {
                let next = cluster.waypoints[cluster.current + 1];
                cluster.angle = angle(sub(next[0], w[0]), sub(next[1], w[1]));
                cluster.current += 1;
                cluster.stall = 0;
                cluster.offsets = super::formation::offsets(&members, cluster.angle);
                send_to_places(state, cluster, min);
                arrived = ordered.iter().map(|&o| !o).collect();
            }
        }
        let held = super::formation::holds(&members, &arrived, cluster.angle);
        // Members neither held nor walking yet are marked for the steps of
        // this update: local searches pass them by (SPEC-0025, "Members in
        // each other's searches").
        cluster.updates += 1;
        for &h in &cluster.members {
            let v = state
                .unit(Some(h))
                .and_then(|u| u.movement)
                .map_or([0, 0], |m| m.velocity);
            if real_of(follow::squared_speed(v)) > WALKED_SQUARED {
                cluster.walked.insert(h);
            }
        }
        let all_walk = cluster.current > 0 || cluster.updates > ALL_WALK_AFTER;
        for (i, &h) in cluster.members.iter().enumerate() {
            if !held[i] && !all_walk && !cluster.walked.contains(&h) {
                state.marked.insert(h);
            }
        }
        let speed = (!held.iter().any(|&h| h) && !arrived.iter().any(|&a| a)).then(|| {
            let speeds: Vec<u32> = cluster
                .members
                .iter()
                .filter_map(|&h| state.unit(Some(h)))
                .map(|u| state.data.units.get(&u.type_id).map_or(0, |t| t.speed))
                .collect();
            speeds[1..].iter().fold(
                speeds[0],
                |a, &s| {
                    if real_of(s) < real_of(a) { s } else { a }
                },
            )
        });
        let last = cluster.current + 1 == cluster.waypoints.len();
        let size = cluster.members.len();
        for (i, &h) in cluster.members.iter().enumerate() {
            control.insert(
                h,
                Control {
                    held: held[i],
                    speed,
                    last,
                    size,
                },
            );
        }
        // At the last waypoint the arrived members leave.
        if last {
            let leaving: std::collections::BTreeSet<u32> = cluster
                .members
                .iter()
                .zip(&arrived)
                .filter(|(_, a)| **a)
                .map(|(&m, _)| m)
                .collect();
            cluster.keep(|m| !leaving.contains(&m));
        }
    }
    clusters.retain(|c| c.members.len() > 1);
    state.clusters.extend(clusters);
    control
}

/// A member has walked once its speed was above 0.01 cells per second
/// (squared, in world units per second), and all have after this many
/// updates on the first path (SPEC-0025).
const WALKED_SQUARED: f32 = 0.1024;
const ALL_WALK_AFTER: u32 = 65;

/// Whether unit `h` is a member of a cluster or follows a group order.
pub(super) fn grouped(state: &State, h: u32) -> bool {
    let group_order = matches!(state.objects.get(&h), Some(Object::Unit(u))
        if u.movement.is_some_and(|m| state.group_orders.contains(&m.given)));
    group_order || state.clusters.iter().any(|c| c.members.contains(&h))
}

/// Squared distances in cells below this are near a place (16 cells).
const NEAR_PLACE: f32 = 256.0;
/// Updates a cluster waits for its members before it goes on anyway.
const STALL: u32 = 99;

/// The members of a cluster as the layout takes them, in cells, and
/// whether each still follows its order.
fn cluster_members(
    state: &State,
    handles: &[u32],
    min: [u32; 2],
) -> (Vec<super::formation::Member>, Vec<bool>) {
    handles
        .iter()
        .filter_map(|&h| state.unit(Some(h)))
        .map(|u| {
            let data = state.data.units.get(&u.type_id);
            (
                super::formation::Member {
                    position: mul_both(relative([u.x, u.y], min), CELL),
                    radius: mul(data.map_or(0, |t| t.collision), CELL),
                    rank: data.map_or(0, |t| t.formation),
                },
                u.movement.is_some_and(|m| m.ordered()),
            )
        })
        .unzip()
}

/// The member nearest to `goal` (squared distance, the first of equals).
fn nearest(members: &[super::formation::Member], goal: [u32; 2]) -> usize {
    let squared = |p: [u32; 2]| {
        let (dx, dy) = (sub(goal[0], p[0]), sub(goal[1], p[1]));
        add(mul(dx, dx), mul(dy, dy))
    };
    let mut best = 0;
    for i in 1..members.len() {
        if real_of(squared(members[i].position)) < real_of(squared(members[best].position)) {
            best = i;
        }
    }
    best
}

/// The waypoints of a cluster from `from` to `goal`, in cells: those of
/// the segment goals of its path, as for a single unit (SPEC-0025).
fn cluster_path(state: &State, from: [u32; 2], goal: [u32; 2], class: u8) -> Vec<[u32; 2]> {
    match state.grid.as_ref() {
        Some(grid) => follow::segment_goals(grid, from, goal, class),
        None => vec![goal],
    }
}

/// Sends each member of `cluster` to its place: the current waypoint plus
/// its offset.
fn send_to_places(state: &mut State, cluster: &Cluster, min: [u32; 2]) {
    let w = cluster.waypoints[cluster.current];
    // The square of 2 x 2 cells of a place.
    let square = |p: [u32; 2]| p.map(|c| (real_of(c) as i32) >> 1);
    for (&h, o) in cluster.members.iter().zip(&cluster.offsets) {
        let place = [add(w[0], o[0]), add(w[1], o[1])];
        state.member_now.insert(h, place);
        // A member whose new place lies in the square of the place of its
        // last search keeps its path and that place (SPEC-0025, "When a
        // member searches again"; c-RTW-3 tick 5539).
        if state
            .member_places
            .get(&h)
            .is_some_and(|&old| square(old) == square(place))
        {
            continue;
        }
        state.member_places.insert(h, place);
        let to = absolute(mul_both(place, CELLS_TO_WORLD), min);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
        {
            m.target = to;
        }
        state.paths.remove(&h);
    }
}

/// 32: cells to world units.
const CELLS_TO_WORLD: u32 = 0x4200_0000;

fn mul_both(p: [u32; 2], k: u32) -> [u32; 2] {
    [mul(p[0], k), mul(p[1], k)]
}

/// The player states of gold and lumber (`PLAYER_STATE_RESOURCE_GOLD`,
/// `PLAYER_STATE_RESOURCE_LUMBER`).
const GOLD: i32 = 1;
const LUMBER: i32 = 2;

/// An order to worker `h` to build a building of `type_id` at `target`
/// (SPEC-0026): the costs are paid at once and the worker walks to the
/// site. Whether it took the order.
///
/// PROVISIONAL: the checks that reject a build order are not specified
/// (open question 3); an order the player cannot pay for, to a type that
/// is not a building or to a unit that is not a worker is not taken.
pub(super) fn build_order(state: &mut State, h: u32, type_id: i32, target: [u32; 2]) -> bool {
    super::queue::clear(state, h);
    let Some(owner) = state.unit(Some(h)).filter(|u| u.alive).map(|u| u.owner) else {
        return false;
    };
    let worker = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| t.worker);
    let Some(building) = state.data.units.get(&type_id).filter(|t| t.building) else {
        return false;
    };
    let (gold, lumber) = (building.gold_cost, building.lumber_cost);
    let states = &state.setup.player_states;
    let has = |s: i32| states.get(&(owner, s)).copied().unwrap_or(0);
    if !worker || has(GOLD) < gold || has(LUMBER) < lumber {
        return false;
    }
    if !place_allowed(state, owner, type_id, target) {
        return false;
    }
    if !order(state, h, Order::Build { type_id }, type_id, target) {
        return false;
    }
    set_attack_move(state, h, None);
    let states = &mut state.setup.player_states;
    *states.entry((owner, GOLD)).or_default() -= gold;
    *states.entry((owner, LUMBER)).or_default() -= lumber;
    true
}

/// A town hall may not stand this near a gold mine, centre to centre
/// (SPEC-0026 rule 4: the limit lies above 749 and at most 768).
/// PROVISIONAL: 768.
const MINE_DISTANCE: f32 = 768.0;

/// Whether player `owner` may place a building of `type_id` at `target`
/// (SPEC-0026, "Where a building may be placed"): the cells of its
/// pathing texture there, snapped to its grid, are all buildable, meet no
/// texture of another building or destructable and no footprint of a
/// unit of another player, and a town hall keeps away from gold mines.
pub(super) fn place_allowed(state: &State, owner: i32, type_id: i32, target: [u32; 2]) -> bool {
    let facing = super::facing::from_degrees(270f32.to_bits());
    let (x, y) = super::obstacles::site(state, type_id, target[0], target[1], facing);
    let Some(rect) = super::obstacles::texture_rect(state, type_id, x, y, facing) else {
        return true;
    };
    let (x0, x1, y0, y1) = rect;
    // 1. Buildable ground.
    if let Some(grid) = state.grid.as_ref()
        && !(y0..=y1).all(|cy| (x0..=x1).all(|cx| grid.buildable(cx, cy)))
    {
        return false;
    }
    // 2. Other buildings and destructables.
    if super::obstacles::overlaps_objects(state, rect) {
        return false;
    }
    // 3. Units of other players.
    let min = [state.world_bounds[0], state.world_bounds[1]];
    for h in &state.units {
        let Some(Object::Unit(u)) = state.objects.get(h) else {
            continue;
        };
        let Some(t) = state.data.units.get(&u.type_id) else {
            continue;
        };
        if !u.alive || u.owner == owner || t.building {
            continue;
        }
        let at = mul_both(relative([u.x, u.y], min), CELL);
        let class = crate::path::size_class(t.collision);
        let on = crate::path::occupied_cells(at, class)
            .iter()
            .any(|&(cx, cy)| (x0..=x1).contains(&cx) && (y0..=y1).contains(&cy));
        if on {
            return false;
        }
    }
    // 4. Town halls away from gold mines.
    if type_id == i32::from_be_bytes(*b"htow") {
        let mine = i32::from_be_bytes(*b"ngol");
        let limit = mul(MINE_DISTANCE.to_bits(), MINE_DISTANCE.to_bits());
        for h in &state.units {
            let Some(Object::Unit(u)) = state.objects.get(h) else {
                continue;
            };
            if !u.alive || u.type_id != mine {
                continue;
            }
            let (dx, dy) = (sub(u.x, x), sub(u.y, y));
            if real_of(add(mul(dx, dx), mul(dy, dy))) < real_of(limit) {
                return false;
            }
        }
    }
    true
}

/// The workers that came near their sites at the updates of this tick
/// start their buildings after the script's timers of the tick, as in
/// c-RSR-1 (SPEC-0026). The worker then stands with order `repair` until
/// the building is finished.
///
/// PROVISIONAL: in c-RTI-1 two buildings were started before the timers
/// of their tick; what decides is SPEC-0026 open question 6.
pub(super) fn start_buildings(state: &mut State) {
    for (h, type_id, site) in std::mem::take(&mut state.starting) {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if u.movement
            .is_none_or(|m| m.order != (Order::Build { type_id }) || m.target != site)
        {
            continue;
        }
        let Some(building) = start_construction(state, h, type_id, site) else {
            continue;
        };
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
        {
            m.order = Order::Repair { building };
            m.order_id = ORDER_REPAIR;
        }
        state.paths.remove(&h);
    }
}

/// Whether `h` is a worker and `building` a building of its player under
/// construction.
fn unfinished_of_own(state: &State, h: u32, building: u32) -> bool {
    let worker = state
        .unit(Some(h))
        .filter(|u| state.data.units.get(&u.type_id).is_some_and(|t| t.worker));
    match (worker, state.objects.get(&building)) {
        (Some(w), Some(Object::Unit(b))) => {
            b.alive && b.owner == w.owner && b.construction.is_some()
        }
        _ => false,
    }
}

/// Whether `target` is a destructable (a tree to gather).
fn is_tree_target(state: &State, target: u32) -> bool {
    matches!(state.objects.get(&target), Some(Object::Destructable(_)))
}

/// Unit `m` turns to `target`: its heading points at its position, which
/// its facing follows.
fn face_target(state: &State, m: &mut Moving, target: u32, min: [u32; 2]) {
    if let Some(at) = position_of(state, target) {
        let at = relative(at, min);
        m.heading = angle(sub(at[0], m.origin[0]), sub(at[1], m.origin[1]));
    }
}

/// The buildings a living worker builds now (order `repair`, standing).
pub(super) fn worked(state: &State) -> std::collections::BTreeSet<u32> {
    state
        .units
        .iter()
        .filter_map(|h| match state.objects.get(h) {
            Some(Object::Unit(u)) if u.alive => match u.movement.map(|m| m.order) {
                Some(Order::Repair { building }) => Some(building),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Ticks from the start of a building to the tick work begins, with the
/// worker moved off its footprint (SPEC-0026: usually two updates; 18
/// ticks were also seen and the rule is not known). Without the move work
/// begins in the tick of the start.
const WORK_DELAY_MOVED: u64 = 12;

/// Worker `h` starts a building of `type_id` at `site` (SPEC-0026): the
/// building appears there with a tenth of its life, its alarm draws game
/// generator 21 once, and a worker that stands on its footprint moves to
/// a free place next to it. The building.
///
/// PROVISIONAL: the worker's free place is the nearest cell centre whose
/// cells are all passable (open question 2).
fn start_construction(state: &mut State, h: u32, type_id: i32, site: [u32; 2]) -> Option<u32> {
    let owner = state.unit(Some(h)).map(|u| u.owner)?;
    let facing = 270f32.to_bits();
    let building = super::widgets::create_unit(state, owner, type_id, [site[0], site[1], facing])?;
    // Its food comes when it is finished.
    let food = state.data.units.get(&type_id).map_or(0, |t| t.food_made);
    *state.setup.player_states.entry((owner, 4)).or_default() -= food;
    let moved = move_off_footprint(state, h);
    let data = state.data.units.get(&type_id);
    let (max, build_time) = data.map_or((0, 0), |t| (t.hp, t.build_time));
    let base = mul(max, TENTH);
    let rate = div(mul(max, NINE_TENTHS), build_time);
    // Work begins at once, or later with the worker moved; `Tb` is the
    // clock of that tick plus 0.0001 (`game::construction`).
    let delay = if moved { WORK_DELAY_MOVED } else { 0 };
    let begin = state.ticks + delay;
    let mut construction = super::Construction::new(base, rate, max, build_time, begin);
    let end = (delay == 0).then(|| construction.work(add(state.move_clock, GROWTH_OFFSET)));
    if let Some(Object::Unit(b)) = state.objects.get_mut(&building) {
        b.life = super::Regenerating::full(max, 0);
        b.life.set(base);
        b.construction = Some(Box::new(construction));
    }
    state.object_timers.retain(|&(_, of)| of != building);
    if let Some(end) = end {
        super::training::start_timer(
            state,
            end,
            super::training::EngineTimer::Construction(building),
        );
    }
    state.constructions.push((false, building));
    Some(building)
}

/// 0.1 and 0.9.
const TENTH: u32 = 0x3DCC_CCCD;
const NINE_TENTHS: u32 = 0x3F66_6666;
/// 0.0001 (SPEC-0026, `Tb`).
const GROWTH_OFFSET: u32 = 0x38D1_B717;

/// Moves worker `h` off a footprint it stands on; whether it was moved.
fn move_off_footprint(state: &mut State, h: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let class =
        crate::path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
    let here = mul_both(relative([u.x, u.y], min), CELL);
    let Some(grid) = state.grid.as_ref() else {
        return false;
    };
    let free = |p: [u32; 2]| {
        crate::path::occupied_cells(p, class)
            .iter()
            .all(|&(x, y)| grid.passable(x, y))
    };
    if free(here) {
        return false;
    }
    let (cx, cy) = crate::path::cell_of(here);
    let mut best: Option<([u32; 2], u32)> = None;
    for dy in -8..=8 {
        for dx in -8..=8 {
            let p = [cx + dx, cy + dy].map(|c| add(i2r(c), 0x3F00_0000));
            if !free(p) {
                continue;
            }
            let (ex, ey) = (sub(p[0], here[0]), sub(p[1], here[1]));
            let d = add(mul(ex, ex), mul(ey, ey));
            if best.is_none_or(|(_, b)| real_of(d) < real_of(b)) {
                best = Some((p, d));
            }
        }
    }
    let Some((p, _)) = best else {
        return false;
    };
    let [x, y] = absolute(mul_both(p, CELLS_TO_WORLD), min);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.x = x;
        u.y = y;
        if let Some(m) = u.movement.as_mut() {
            m.origin = relative([x, y], min);
        }
    }
    true
}

/// A gathering order of `game::harvest` for worker `h`: walking to its
/// mine, tree or drop-off at `target`.
pub(super) fn harvest_order(
    state: &mut State,
    h: u32,
    gather: Order,
    order_id: i32,
    target: [u32; 2],
) -> bool {
    let done = order(state, h, gather, order_id, target);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

/// Unit `h` joins a fight on `target` as an idle unit answering a call
/// for help (SPEC-0019): an attack with order id 0 after its reaction
/// delay from this tick, or from the time `from` on the timer queue.
pub(super) fn join_fight(state: &mut State, h: u32, target: u32, from: Option<u32>) -> bool {
    let tick = state.ticks;
    let Some(at) = position_of(state, target) else {
        return false;
    };
    let taken = order(state, h, Order::Attack { target }, 0, at);
    if taken {
        let delay = mul(
            state.game_random[REACTION].fraction(),
            state.combat.reaction_delay,
        );
        match from {
            // From the expiry of the caller's check (SPEC-0019).
            Some(time) => react_until(state, h, add(time, delay)),
            None => react(state, h, tick, delay, tick),
        }
    }
    taken
}

/// The side of a cell of the grid a call for help walks (SPEC-0019).
const CALL_CELL: f32 = 256.0;

/// `units` in the order a call for help visits them (SPEC-0019, "Order of
/// the responders"): by cells of 256 from the map's lower left corner,
/// the rows from the bottom up, in a row from left to right, in a cell
/// the unit created later first; a unit lies in the cell of its position
/// less its collision size.
pub(super) fn call_order(state: &State, mut units: Vec<u32>) -> Vec<u32> {
    let [x0, y0] = [state.world_bounds[0], state.world_bounds[1]].map(real_of);
    let created: BTreeMap<u32, usize> = state
        .units
        .iter()
        .enumerate()
        .map(|(i, &h)| (h, i))
        .collect();
    let key = |h: &u32| {
        let Some(u) = state.unit(Some(*h)) else {
            return (i64::MAX, i64::MAX, 0);
        };
        let c = state
            .data
            .units
            .get(&u.type_id)
            .map_or(0.0, |t| real_of(t.collision));
        // The first cell of its box in the scan, rows from the bottom and
        // each row from the left, and in it the unit that entered last
        // first (c-RXD-1: creeps 8 before 6 at 1557, 7 before 9 at 5919).
        match u.call_cells.iter().min_by_key(|((x, y), _)| (*y, *x)) {
            Some(&((x, y), seq)) => (y, x, -(seq as i64)),
            None => {
                let cell =
                    |v: u32, origin: f32| ((real_of(v) - c - origin) / CALL_CELL).floor() as i64;
                let index = created.get(h).copied().unwrap_or(0) as i64;
                (cell(u.y, y0), cell(u.x, x0), -index)
            }
        }
    };
    units.sort_by_key(key);
    units
}

/// Keeps the cells of the grid of calls for help up to date: each unit is
/// entered in every cell of 256 its box (its position, at its last commit
/// when it walks, plus or minus its collision size) covers, newest first
/// (SPEC-0019, "Order of the responders").
pub(super) fn refresh_call_cells(state: &mut State) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let units = state.units.clone();
    for h in units {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let c = state
            .data
            .units
            .get(&u.type_id)
            .map_or(0.0, |t| real_of(t.collision));
        let [x, y] = u.movement.map_or(
            [
                real_of(u.x) - real_of(min[0]),
                real_of(u.y) - real_of(min[1]),
            ],
            |m| [real_of(m.origin[0]), real_of(m.origin[1])],
        );
        let span = |p: f32| {
            (((p - c) / CALL_CELL).floor() as i64)..=(((p + c) / CALL_CELL).floor() as i64)
        };
        let cells: Vec<(i64, i64)> = span(y)
            .flat_map(|cy| span(x).map(move |cx| (cx, cy)))
            .collect();
        let old = u.call_cells.clone();
        if old.len() == cells.len() && old.iter().all(|(c, _)| cells.contains(c)) {
            continue;
        }
        let mut next = state.call_entries;
        let new: Vec<((i64, i64), u64)> = cells
            .into_iter()
            .map(|cell| match old.iter().find(|(c, _)| *c == cell) {
                Some(&kept) => kept,
                None => {
                    next += 1;
                    (cell, next)
                }
            })
            .collect();
        state.call_entries = next;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.call_cells = new;
        }
    }
}

/// Creep `h` walks home to `home` with a `move` order (SPEC-0030).
pub(super) fn move_home(state: &mut State, h: u32, home: [u32; 2]) -> bool {
    let done = order(state, h, Order::Point, ORDER_MOVE, home);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

/// Unit `h` stops where it is, its order over.
pub(super) fn stop(state: &mut State, h: u32) {
    halt(state, h);
}

/// Unit `h` walks into the range of its cast on `target` (SPEC-0035).
pub(super) fn cast_order(state: &mut State, h: u32, target: u32, order_id: i32) -> bool {
    match position_of(state, target) {
        Some(at) => order(state, h, Order::Cast { target }, order_id, at),
        None => false,
    }
}

/// A unit just trained walks to the rally point of its building, by the
/// rules of `move` with the order id 851970 (SPEC-0027).
pub(super) fn rally_order(state: &mut State, h: u32, target: [u32; 2]) -> bool {
    order(state, h, Order::Point, super::training::ORDER_RALLY, target)
}

/// Whether unit `h` is a building.
fn is_building(state: &State, h: u32) -> bool {
    state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| t.building)
}

/// `IssuePointOrder(h, name, target)`: what the native returns.
pub(super) fn point_order(state: &mut State, h: u32, name: &str, target: [u32; 2]) -> bool {
    // An order without Shift empties the queue (SPEC-0031).
    super::queue::clear(state, h);
    point_order_taken(state, h, name, target, true).1
}

/// A point order: whether the unit took it, and what the native returns.
fn point_order_taken(
    state: &mut State,
    h: u32,
    name: &str,
    target: [u32; 2],
    stop: bool,
) -> (bool, bool) {
    // A building takes `setrally` and `smart` on the ground as its rally
    // point (SPEC-0027).
    if (name == "setrally" || name == "smart") && is_building(state, h) {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.rally = Some(target);
        }
        return (true, true);
    }
    let here = state.unit(Some(h)).map(|u| [u.x, u.y]);
    if stop && matches!(name, "move" | "attack" | "patrol" | "smart") {
        freeze(state, h);
    }
    let (taken, done) = match (name, here) {
        ("move", _) => {
            let done = order(state, h, Order::Point, ORDER_MOVE, target);
            (done, done)
        }
        ("attack", _) => {
            let done = order(state, h, Order::Point, ORDER_ATTACK, target);
            // The order, its attack ability's entries and the path
            // objects are made, and some freed, in this tick (SPEC-0024;
            // c-RXR-1, tick 301).
            if done {
                state.objects_change();
            }
            (done, done)
        }
        ("patrol", Some(home)) => {
            let done = order(state, h, Order::Patrol { home }, ORDER_PATROL, target);
            (done, done)
        }
        // The unit obeys, but the native gives false (SPEC-0019).
        ("smart", _) => (order(state, h, Order::Point, ORDER_SMART, target), false),
        _ => (false, false),
    };
    if taken {
        // An attack to a point checks for enemies on the way, the first
        // time 102 ticks later (SPEC-0019, "Attack-move with enemies").
        let attack_move = (name == "attack").then(|| AttackMove {
            point: target,
            next_check: add(state.timers.time(), ATTACK_MOVE_FIRST_TIME),
            resume_at: None,
        });
        set_attack_move(state, h, attack_move);
    }
    (taken, done)
}

/// `IssueTargetOrder(h, name, target)`: `attack` and `move`. A new order
/// ends an attack-move.
pub(super) fn target_order(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    super::queue::clear(state, h);
    let done = target_order_taken(state, h, name, target);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

/// `IssueImmediateOrder(h, name)`: `stop`, `holdposition` and `move`. A
/// new order ends an attack-move.
pub(super) fn immediate_order(state: &mut State, h: u32, name: &str) -> bool {
    super::queue::clear(state, h);
    let done = immediate_order_taken(state, h, name);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

fn target_order_taken(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    // The order of an ability (SPEC-0035).
    if let Some(taken) = super::cast::order(state, h, name, target) {
        return taken;
    }
    if name == "move" {
        return follow_order(state, h, target, ORDER_MOVE);
    }
    if matches!(name, "smart" | "repair") && unfinished_of_own(state, h, target) {
        // PROVISIONAL (not in SPEC-0026): a worker sent to its player's
        // building under construction goes on building it.
        return match position_of(state, target) {
            Some(at) => order(
                state,
                h,
                Order::Resume { building: target },
                ORDER_REPAIR,
                at,
            ),
            None => false,
        };
    }
    // A worker on a gold mine or tree gathers from it; with a load on a
    // building that takes it, it delivers there (SPEC-0029).
    if matches!(name, "smart" | "harvest") && super::harvest::can_harvest(state, h, target) {
        return super::harvest::order_harvest(state, h, target);
    }
    if name == "smart" && super::harvest::can_deliver(state, h, target) {
        return super::harvest::order_deliver(state, h, target);
    }
    if name == "smart" {
        // On an enemy an attack, on anything else following it.
        if hostile(state, h, target) {
            return target_order_taken(state, h, "attack", target);
        }
        return follow_order(state, h, target, ORDER_SMART);
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
                // It acts at its first update at or after the order plus
                // the delay, not before the order plus 2 ticks; an order
                // of `main` counts as given in tick 1. It swings one update
                // later at the earliest (SPEC-0019, "When it starts").
                let given = tick.max(1);
                react(state, h, given, delay, given + ORDER_FIRST_ACT);
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                    && let Some(m) = u.movement.as_mut()
                {
                    m.swing_waits = true;
                }
            }
            done
        }
        _ => false,
    }
}

/// The unit `h` waits `delay` seconds counted from `from`, and acts no
/// earlier than tick `earliest`: a walking unit stops at once and stands
/// during the delay (SPEC-0019).
fn react(state: &mut State, h: u32, from: u64, delay: u32, earliest: u64) {
    let clock = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        if let Some(m) = u.movement.as_mut() {
            m.reaction = Some((from, delay, earliest));
            m.walking = false;
            m.restart = false;
            m.skip = false;
            m.commit(clock);
            m.velocity = [0, 0];
        }
    }
}

/// `react` for an attack taken at an acquisition or a call for help: the
/// delay ends at `end` on the timer queue (SPEC-0019).
fn react_until(state: &mut State, h: u32, end: u32) {
    let tick = state.ticks;
    react(state, h, tick, 0, tick);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        m.reaction = None;
        m.reaction_end = Some(end);
    }
}

fn immediate_order_taken(state: &mut State, h: u32, name: &str) -> bool {
    // An ability without a target (SPEC-0037).
    if let Some(taken) = super::cast::immediate_order(state, h, name) {
        return taken;
    }
    match name {
        "stop" | "holdposition" => halt(state, h),
        // PROVISIONAL: the game takes it and the current order reads 0
        // after it (SPEC-0019); here it stops the unit like `stop`.
        "move" => halt(state, h),
        // A worker takes its load back (SPEC-0029, not studied).
        "returnresources" => super::harvest::order_back(state, h),
        _ => false,
    }
}

/// The ids of the order names (SPEC-0019, "Order ids").
const ORDER_IDS: [(&str, i32); 13] = [
    ("smart", 851_971),
    ("stop", 851_972),
    ("cancel", 851_976),
    ("setrally", 851_980),
    ("attack", 851_983),
    ("attackground", 851_984),
    ("move", 851_986),
    ("patrol", 851_990),
    ("holdposition", 851_993),
    ("resumeharvesting", 852_017),
    ("harvest", 852_018),
    ("returnresources", 852_020),
    ("holybolt", 852_092),
];

/// `OrderId`: the id of `name` in any case, 0 for an unknown name.
fn order_id(name: &str) -> i32 {
    ORDER_IDS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map_or(0, |&(_, id)| id)
}

/// The name of the orders as the game knows it, for an order given by
/// name.
pub(super) fn known_order(name: &str) -> Option<&'static str> {
    order_name(order_id(name))
}

/// `OrderId2String`: the name of `id`, none for 0 and unknown ids.
pub(super) fn order_name(id: i32) -> Option<&'static str> {
    ORDER_IDS.iter().find(|&&(_, i)| i == id).map(|&(n, _)| n)
}

/// Whether units of `a` and `b` are enemies of each other.
pub(super) fn hostile(state: &State, a: u32, b: u32) -> bool {
    match (state.unit(Some(a)), state.unit(Some(b))) {
        (Some(a), Some(b)) => enemies(state, a.owner, b.owner),
        _ => false,
    }
}

/// A new point order stops a walking unit at once; it starts again as
/// from standing at its next update (SPEC-0019, "Stop, hold position and
/// new orders while walking").
fn freeze(state: &mut State, h: u32) {
    let (clock, later) = (state.move_clock, state.player_orders);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
        && m.walking
    {
        // A player's order, given between two ticks, comes after the
        // motion of the next one.
        if later {
            m.stop_after_step = true;
            return;
        }
        m.commit(clock);
        m.velocity = [0, 0];
        m.walking = false;
        let record = (state.ticks, h, m.origin, m.velocity, m.heading, [0, 0]);
        if let Some(trace) = state.agent_trace.as_mut() {
            trace.push(record);
        }
    }
}

/// Unit `h` stands where it is now, keeping its order (a stun,
/// SPEC-0037).
pub(super) fn stand_still(state: &mut State, h: u32) {
    let clock = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        m.commit(clock);
        m.velocity = [0, 0];
        m.walking = false;
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
    u.casting = None;
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
    // A worker builds (SPEC-0026).
    native!(
        add,
        state,
        "native IssueBuildOrderById takes unit whichPeon, integer unitId, real x, real y returns boolean",
        |vm, state, args| {
            let Some(h) = handle(&args[0]) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let target = [real(&args[2]), real(&args[3])];
            let done = build_order(&mut state.borrow_mut(), h, integer(&args[1]), target);
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
            let Some(h) = handle(&args[0]) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let id = integer(&args[1]);
            // Unit types train, `cancel` takes back the last (SPEC-0027).
            let done = match order_name(id) {
                Some("cancel") => super::training::cancel(vm, state, h),
                Some(name) => immediate_order(&mut state.borrow_mut(), h, name),
                None => super::training::train(vm, state, h, id),
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
                (Some(h), Some("cancel")) => super::training::cancel(vm, state, h),
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
                // An attack-move reads `attack` all along (SPEC-0019).
                Some(u) if u.shown_order.is_some_and(|(_, until)| until >= state.ticks) => {
                    u.shown_order.map_or(0, |(id, _)| id)
                }
                Some(u) if u.attack_move.is_some() => ORDER_ATTACK,
                Some(u) if u.stunned_until.is_some_and(|t| state.ticks < t) => {
                    super::cast::ORDER_STUNNED
                }
                Some(u) if u.casting.is_some() => u.casting.map_or(0, |c| c.order_id),
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
    // In tick k the clock is the sum of k - 1 ticks (SPEC-0018).
    if state.ticks > 1 {
        state.move_clock = add(state.move_clock, crate::timers::TICK);
    }
    let (min, clock) = (
        [state.world_bounds[0], state.world_bounds[1]],
        state.move_clock,
    );
    for h in &state.moving {
        let Some(Object::Unit(u)) = state.objects.get_mut(h) else {
            continue;
        };
        let Some(m) = u.movement.as_mut() else {
            continue;
        };
        if m.velocity == [0, 0] {
            continue;
        }
        let [x, y] = absolute(m.position(clock), min);
        u.x = x;
        u.y = y;
        if std::mem::take(&mut m.stop_after_step) {
            m.commit(clock);
            m.velocity = [0, 0];
            m.walking = false;
            // It starts as from standing.
            m.restart = false;
        }
    }
}

/// The updates due at this tick, after the timers.
pub(super) fn updates(_vm: &mut Vm, state: &mut State) {
    refresh_call_cells(state);
    let tick = state.ticks;
    let control = update_clusters(state, tick);
    let min = [state.world_bounds[0], state.world_bounds[1]];
    // Units updating in the same tick go by their orders, the newest first,
    // and the members of a group in the order of its cluster (SPEC-0021,
    // "Searches of many units in the same tick"; SPEC-0025).
    state.start_velocities = state
        .moving
        .iter()
        .filter_map(|&h| {
            state
                .unit(Some(h))
                .and_then(|u| u.movement)
                .map(|m| (h, m.velocity))
        })
        .collect();
    state.start_origins = state
        .moving
        .iter()
        .filter_map(|&h| {
            state
                .unit(Some(h))
                .and_then(|u| u.movement)
                // Only the units due to update in this tick.
                .filter(|m| state.update_now && m.next_update <= tick)
                .map(|m| (h, m.origin))
        })
        .collect();
    attack_checks(state);
    state.updated.clear();
    let mut moving = std::mem::take(&mut state.moving);
    let key = |h: &u32| {
        let given = match state.objects.get(h) {
            Some(Object::Unit(u)) => u.movement.map_or(0, |m| m.given),
            _ => 0,
        };
        let member = state
            .clusters
            .iter()
            .find_map(|c| c.members.iter().position(|m| m == h))
            .unwrap_or(0);
        (std::cmp::Reverse(given), member)
    };
    moving.sort_by_key(key);
    let mut still = Vec::with_capacity(moving.len());
    for h in moving {
        let (type_id, alive) = match state.objects.get(&h) {
            Some(Object::Unit(u)) => (u.type_id, u.alive),
            _ => continue,
        };
        let data = state.data.units.get(&type_id);
        // A member of a group may be held or slowed (SPEC-0025); its own
        // limit stays.
        let limit = data.map_or(0, |t| t.speed);
        // Endurance and unholy aura (SPEC-0038).
        let limit = match state.unit(Some(h)) {
            Some(u) if !u.buffs.is_empty() => mul(limit, super::auras::speed_factor(state, u)),
            _ => limit,
        };
        // The pace of the gold cycle (SPEC-0029).
        let limit = super::harvest::pace_factor(state, h).map_or(limit, |k| mul(limit, k));
        let cluster = control.get(&h).map_or(1, |c| c.size);
        // A member at its place short of the last waypoint waits there.
        let place_on_way = control.get(&h).is_some_and(|c| !c.last);
        let held = control.get(&h).is_some_and(|c| c.held);
        let speed = match control.get(&h) {
            Some(c) if c.held => 0,
            Some(Control { speed: Some(g), .. }) if real_of(*g) < real_of(limit) => *g,
            _ => limit,
        };
        let turn_rate = data.map_or(PI, |t| t.turn_rate);
        let window = mul(data.map_or(0, |t| t.prop_window), DEGREES);
        let orient = data.map_or(0, |t| t.orient_interp);
        // Range and collision size in cells, with the weapon for the
        // target of an attack (SPEC-0016).
        let attack_target = match state.objects.get(&h) {
            Some(Object::Unit(u)) => u.movement.and_then(|m| match m.order {
                Order::Attack { target } => Some(target),
                _ => None,
            }),
            _ => None,
        };
        let range = super::attack::weapon_for(state, type_id, attack_target).map_or(0, |w| w.range);
        let reach = data.map_or(0, |t| add(mul(range, CELL), mul(t.collision, CELL)));
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
        // Units update at the update ticks, from the one they are due at
        // (a tick later when the updates moved, SPEC-0018).
        if !state.update_now || m.next_update > tick {
            still.push(h);
            continue;
        }
        m.next_update = tick + UPDATE;
        // The position is committed first (SPEC-0018).
        m.commit(state.move_clock);
        // A stunned unit stands and does nothing (SPEC-0037).
        if super::cast::stunned(state, h) {
            m.velocity = [0, 0];
            m.walking = false;
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.movement = Some(m);
            }
            still.push(h);
            continue;
        }
        let here = m.origin;
        let mut begin_attack = None;
        // Still in the reaction delay: the unit stands.
        let waiting = m.reaction_end.is_some()
            || m.reaction.is_some_and(|(from, delay, earliest)| {
                let waited = mul(i2r(tick as i32 - from as i32), crate::timers::TICK);
                tick < earliest || real_of(waited) < real_of(delay)
            });
        if !waiting {
            m.reaction = None;
            // An attack ordered by a script swings from the next update on.
            if std::mem::take(&mut m.swing_waits) {
                m.swing_gate = Some(tick + UPDATE);
            }
        }
        // A worker near the site of its building and facing it starts it
        // at the end of this tick's updates (SPEC-0026; `start_buildings`);
        // one near but facing away turns on the spot first.
        let mut turning_to_site = false;
        if let Order::Build { type_id } = m.order
            && !m.arrived
            && !waiting
        {
            let site = relative(m.target, min);
            let (dx, dy) = (sub(site[0], here[0]), sub(site[1], here[1]));
            if real_of(add(mul(dx, dx), mul(dy, dy))) <= BUILD_REACH_SQUARED {
                let off = super::facing::angle_difference(angle(dx, dy), normal(m.heading));
                if real_of(off).abs() <= real_of(FACING_SITE) {
                    state.starting.push((h, type_id, m.target));
                    m.walking = false;
                    m.velocity = [0, 0];
                } else {
                    motion(&mut m, site, false, speed, limit, turn_rate, window);
                }
                turning_to_site = true;
            }
        }
        // A worker reaching its mine, tree or drop-off stands there; what it
        // does is up to `harvest::tick` after the updates.
        // A building is reached at the update after the one at which the
        // walk came to the end of its path, touching it (SPEC-0029); a
        // tree by the distance.
        let building_target = |target: u32| {
            state
                .unit(Some(target))
                .is_some_and(|b| b.footprint.is_some())
        };
        if let Order::Gather { target } | Order::Return { building: target } = m.order
            && !waiting
            // A worker reaches its tree by the arrival test of its walk
            // (SPEC-0019, SPEC-0029; c-RYP-1: peasant 5 turned at 1339 and
            // 1345 and arrived at 1351, peasant 0 turned at 991 and
            // arrived at 997).
            && if building_target(target) || is_tree_target(state, target) {
                m.arrived
            } else {
                super::harvest::reached(state, h, here, target)
            }
        {
            let delivering = matches!(m.order, Order::Return { .. });
            state.harvest_reached.push((h, target, delivering));
            m.order = Order::Work { target };
            m.arrived = false;
        }
        if let Order::Work { .. } = m.order {
            m.walking = false;
            m.velocity = [0, 0];
        }
        // A worker sent back to a building under construction takes up the
        // work once it touches the building, and turns to it.
        //
        // PROVISIONAL: the reach of SPEC-0029 for a mine or a drop-off (the
        // footprint touching a red cell); SPEC-0026 does not give one.
        if let Order::Resume { building } = m.order
            && !waiting
        {
            if !unfinished_of_own(state, h, building) {
                m.arrived = true;
            } else if super::harvest::reached(state, h, here, building) {
                m.order = Order::Repair { building };
                m.arrived = false;
                m.walking = false;
                m.velocity = [0, 0];
                face_target(state, &mut m, building, min);
            }
        }
        // A caster walks to its target and starts its cast at the update at
        // which it is in range (SPEC-0035).
        // PROVISIONAL: the effect came 99 ticks after the last position
        // change, taken as a start one tick before this update.
        if let Order::Cast { target } = m.order
            && !waiting
            && !m.arrived
        {
            match position_of(state, target).filter(|_| super::attack::standing(state, target)) {
                Some(at) => {
                    m.target = at;
                    let reach = super::cast::reach(state, h, target).unwrap_or(0);
                    if in_range(here, relative(at, min), reach) {
                        m.arrived = true;
                        m.walking = false;
                        m.velocity = [0, 0];
                        super::cast::start(state, h, tick.saturating_sub(1));
                    }
                }
                None => m.arrived = true,
            }
        }
        // A worker building stays until the building is finished.
        if let Order::Repair { building } = m.order {
            m.walking = false;
            m.velocity = [0, 0];
            let building_now = matches!(
                state.objects.get(&building),
                Some(Object::Unit(b)) if b.alive && b.construction.is_some()
            );
            if !building_now {
                m.arrived = true;
            }
        }
        let repairing = matches!(m.order, Order::Repair { .. } | Order::Work { .. });
        if !m.arrived && !waiting && !repairing && !turning_to_site {
            if std::mem::take(&mut m.restart) {
                m.walking = false;
                m.velocity = [0, 0];
                m.skip = true;
            }
            // An attacker walks to its goal point, where the target stood
            // when the goal was last renewed, and is in reach when the
            // goal is.
            let mut in_reach = false;
            // The goal of R after a swing (SPEC-0019).
            let mut after_swing = false;
            // The goal before a renewal after a swing (SPEC-0019).
            let old_goal = m.target;
            let mut full_reach = reach;
            if let Order::Attack { target } = m.order {
                // The target where it is at the start of this tick (SPEC-0019).
                match tick_start_position(state, target) {
                    Some(at) if super::attack::standing(state, target) => {
                        let renew_now = m.renew_at.is_some_and(|r| tick >= r);
                        match m.chase {
                            // The new goal after a swing (SPEC-0019).
                            _ if renew_now => {
                                m.renew_at = None;
                                after_swing = true;
                                m.target = at;
                                // A goal of a timer: the countdown starts at
                                // the search, the next update, which does not
                                // count (SPEC-0019, "Schedule").
                                m.chase = Some(countdown(here, relative(at, min)) + 1);
                            }
                            // Swinging at its target the unit has left its
                            // cluster: no countdown and no renewal until the
                            // next backswing ends (c-RXR-1: rifleman 9 from
                            // 2695 to 5053).
                            Some(_)
                                if state.unit(Some(h)).is_some_and(|u| {
                                    u.attack.is_some_and(|a| a.target == target)
                                }) => {}
                            Some(left) if left > 0 => m.chase = Some(left - 1),
                            // The first search after the reaction delay;
                            // the renewal counts from here.
                            _ if std::mem::take(&mut m.goal_taken) => {
                                // At the first update, a renewal: the 64 grid rule
                                // (c-RXR-1: grunt 11 renewed at 595, grunt 15 kept
                                // its goal at 625); a goal faced by the attack's
                                // check stays.
                                let faced = std::mem::take(&mut m.faced);
                                if !faced
                                    && grid64(relative(at, min)) != grid64(relative(m.target, min))
                                {
                                    m.target = at;
                                }
                                m.chase = Some(countdown(here, relative(m.target, min)));
                            }
                            _ => {
                                // A new goal, if the target is in another cell of
                                // the 64 grid than the old goal; the paths follow
                                // it (`game::follow`). In the same cell the old
                                // goal stays (c-RXR-1: grunt 11 at 799; test
                                // c-RYV).
                                if grid64(relative(at, min)) != grid64(relative(m.target, min)) {
                                    m.target = at;
                                }
                                m.chase = Some(countdown(here, relative(m.target, min)));
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
                        in_reach = in_range(here, relative(m.target, min), full_reach);
                    }
                    _ => {
                        m.arrived = true;
                        m.velocity = [0, 0];
                    }
                }
            }
            // A follower takes the leader's position as its goal as a
            // chaser does, and stops at its first update at which it is in
            // the follow range, or, for a start within it, at which the gap
            // is no larger than what it walked (SPEC-0019, "Following an
            // allied unit").
            let mut follower_stands = false;
            if let Order::Follow {
                leader,
                from,
                start_gap,
                standing,
            } = m.order
            {
                match position_of(state, leader).filter(|_| super::attack::standing(state, leader))
                {
                    Some(at) => {
                        let lead = relative(at, min);
                        let mut renewed = false;
                        match m.chase {
                            Some(left) if left > 0 => m.chase = Some(left - 1),
                            // A renewal: the leader's position becomes the
                            // goal only in another square of the 64 grid, and
                            // the next renewal comes 17 updates later
                            // (c-RTW-3, follower 828).
                            Some(_) => {
                                if grid64(lead) != grid64(relative(m.target, min)) {
                                    m.target = at;
                                }
                                m.chase = Some(FOLLOWER_RENEWAL);
                                renewed = true;
                            }
                            None => {
                                m.target = at;
                                m.chase = Some(countdown(here, lead));
                                renewed = true;
                            }
                        }
                        let (own, other) = (collision_of(state, h), collision_of(state, leader));
                        let reach = mul(add(add(FOLLOW_RANGE, own), other), CELL);
                        // A standing follower keeps its order and walks
                        // again when, at the renewal of its goal, the
                        // leader is out of the follow range (SPEC-0019: 77
                        // ticks after the gap passed 300 in the one case
                        // seen). PROVISIONAL: the renewal of the chase.
                        let stands = if standing {
                            !(renewed && !in_range(here, relative(m.target, min), reach))
                        } else if real_of(start_gap) > real_of(FOLLOW_RANGE) {
                            // A walking follower tests its goal point, as a
                            // chase does (c-RTW-3: 828 stood at 5491, 294 from
                            // its goal, 342 from 918).
                            in_range(here, relative(m.target, min), reach)
                        } else {
                            let gap = sub(sub(distance(here, lead), own), other);
                            real_of(gap) <= real_of(distance(from, here))
                        };
                        if stands {
                            follower_stands = true;
                            m.walking = false;
                            m.velocity = [0, 0];
                            // The first stop of a follower that started far
                            // away ends its first order: the follow proper
                            // starts at the next update with a new cluster,
                            // whose renewals come every 17 updates (c-RTW-3:
                            // 828 stopped at 4279, cluster at 4291). Later
                            // stops at the end of a path keep the count
                            // (5491).
                            if !standing && start_gap != add(FOLLOW_RANGE, 0x3F80_0000) {
                                m.chase = Some(FOLLOWER_FIRST_RENEWAL);
                            }
                            // From now on by the follow range.
                            m.order = Order::Follow {
                                leader,
                                from,
                                start_gap: add(FOLLOW_RANGE, 0x3F80_0000),
                                standing: true,
                            };
                        } else if standing {
                            m.order = Order::Follow {
                                leader,
                                from,
                                start_gap,
                                standing: false,
                            };
                            m.walking = false;
                            m.velocity = [0, 0];
                        }
                        // A standing follower that walks again searches a new
                        // path at this update (c-RTW-3: 828 at 5413).
                        if standing && renewed && !stands {
                            state.paths.remove(&h);
                        }
                    }
                    None => {
                        m.arrived = true;
                        m.velocity = [0, 0];
                    }
                }
            }
            if !m.arrived && !follower_stands {
                let goal = relative(m.target, min);
                let (dx, dy) = (sub(goal[0], here[0]), sub(goal[1], here[1]));
                // The squared distance in cells.
                let (cx, cy) = (mul(dx, CELL), mul(dy, CELL));
                let distance = real_of(add(mul(cx, cx), mul(cy, cy)));
                if let Order::Attack { target } = m.order {
                    // At the expiry of a cooldown from a swing at another
                    // target the unit is tested again with its target's
                    // position then: in range and turned within 0.2 it
                    // swings at once, else the approach starts again (stand,
                    // search at the next update). (c-RXR-1: footman 1 in 2689,
                    // footman 2 in 2629, grunt 10 in 4927; c-RXD-1: creep 8
                    // at 10.7699.)
                    if in_reach && after_swing && m.renew_stands && !m.renew_cooldown {
                        // The goal after a swing comes after the updates
                        // of its tick: nothing more at this one (SPEC-0019).
                        m.renew_stands = false;
                        m.walking = false;
                        m.velocity = [0, 0];
                    } else if in_reach {
                        // In range: stand, turn to the target and strike
                        // once the turn left is below 0.2 radians and the
                        // cooldown lets the swing hit in time (SPEC-0019,
                        // "an attacker waiting for its cooldown").
                        let cooled = state.unit(Some(h)).is_none_or(|u| {
                            let point = super::attack::weapon_for(state, u.type_id, Some(target))
                                .map_or(0, |w| super::attack::ticks(w.damage_point));
                            // A swing already timed by the cooldown on this
                            // target goes on by its timer (`attack::hits`).
                            let timed = u.attack.is_some_and(|a| a.target == target);
                            // A new attack (not a switch, which keeps the
                            // renewal of the swing before) starts a swing that
                            // waits for the cooldown on the queue
                            // (`attack::begin`; c-RXR-1, grunt 15 in 2443);
                            // otherwise the swing waits until the cooldown has
                            // run out at an update (c-RXD-1, creeps 6 and 8
                            // after their switches of 2007 and 2031).
                            // PROVISIONAL: what tells the two apart.
                            let fresh = u.cooldown_target != target
                                && (m.renew_at.is_none() || m.renew_cooldown);
                            !timed && (fresh || tick + point >= u.weapon_ready)
                        });
                        let (h, d) = heading_and_turn(&m, goal);
                        let gate_open = cooled && m.swing_gate.is_none_or(|from| tick >= from);
                        if real_of(d).abs() < ATTACK_ANGLE {
                            m.heading = h;
                            m.walking = false;
                            m.velocity = [0, 0];
                            // The swing of an attack ordered by a script
                            // waits for the update after the one at which
                            // the unit acted (SPEC-0019, "When it
                            // starts").
                            if gate_open {
                                m.arrived = true;
                                m.swing_gate = None;
                                begin_attack = Some(target);
                            }
                        } else {
                            motion(&mut m, goal, false, speed, limit, turn_rate, window);
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
                            cluster: 1,
                            chase: Some(chase_of(state, target, full_reach)),
                            building: None,
                            tree: None,
                            arrive: None,
                            place: None,
                        };
                        // The new goal after a swing comes with an update
                        // standing; a walking attacker first makes the
                        // look-ahead check of its old path. The new goal is
                        // searched at the next update (SPEC-0019, "After
                        // the swing").
                        // A standing attacker's stand does not turn its
                        // heading; a walking one turns as its look-ahead
                        // check of the old path says (SPEC-0019).
                        let cooldown = after_swing && std::mem::take(&mut m.renew_cooldown);
                        let renewing = after_swing && std::mem::take(&mut m.renew_stands);
                        let hold = renewing && m.velocity == [0, 0];
                        let steer = if renewing {
                            let mut stand = goal;
                            if !hold {
                                let old = follow::Walker {
                                    order: relative(old_goal, min),
                                    ..walker
                                };
                                match follow::steer(state, &old, tick) {
                                    follow::Steer::Walk(to) | follow::Steer::Stand(to) => {
                                        stand = to
                                    }
                                    _ => {}
                                }
                            }
                            // A new goal, first among the units handled
                            // from the next tick (SPEC-0021).
                            state.orders_given += 1;
                            m.given = state.orders_given;
                            // At the expiry a target in range is struck at once,
                            // the swing waiting for the cooldown (c-RXR-1:
                            // footman 2 in 2629).
                            let swing = cooldown
                                && position_of(state, target).is_some_and(|at| {
                                    in_range(here, relative(at, min), full_reach)
                                });
                            if swing {
                                begin_attack = Some(target);
                            } else if cooldown {
                                // The approach starts again: a new search at the
                                // next update (c-RXR-1: footman 1 at 2695).
                                state.paths.remove(&h);
                            }
                            follow::Steer::Stand(stand)
                        } else {
                            follow::steer(state, &walker, tick)
                        };
                        match steer {
                            follow::Steer::Walk(to) => {
                                motion(&mut m, to, true, speed, limit, turn_rate, window);
                            }
                            follow::Steer::Stand(_) if hold => {
                                m.velocity = [0, 0];
                                m.walking = false;
                            }
                            follow::Steer::Hold => {
                                m.velocity = [0, 0];
                                m.walking = false;
                            }
                            follow::Steer::Stand(to) => {
                                motion(&mut m, to, false, speed, limit, turn_rate, window);
                            }
                            // At the goal but not in range of the target:
                            // it waits for the next goal.
                            follow::Steer::Arrived => {
                                motion(&mut m, goal, false, speed, limit, turn_rate, window);
                            }
                        }
                    } else {
                        motion(&mut m, goal, true, speed, limit, turn_rate, window);
                    }
                } else {
                    let squared_speed = follow::squared_speed(m.velocity);
                    // A walk to a building ends touching it (SPEC-0029).
                    let building = match m.order {
                        Order::Gather { target } | Order::Return { building: target } => state
                            .unit(Some(target))
                            .filter(|b| b.footprint.is_some())
                            .map(|_| target),
                        _ => None,
                    };
                    let tree = match m.order {
                        Order::Gather { target } if building.is_none() => Some(target),
                        _ => None,
                    };
                    // A follower's search ends touching its leader, as a
                    // chase's (c-RTW-3: 828 at 5413).
                    let chase = match m.order {
                        Order::Follow { leader, .. } => {
                            let (own, other) =
                                (collision_of(state, h), collision_of(state, leader));
                            let reach = mul(add(add(FOLLOW_RANGE, own), other), CELL);
                            Some(chase_of(state, leader, reach))
                        }
                        _ => None,
                    };
                    // The reach of a gather order on a tree and the direction of the
                    // step (SPEC-0019, "Arrival at the member's place").
                    let arrive = match m.order {
                        Order::Gather { .. } if tree.is_some() => {
                            Some((GATHER_REACH, m.heading, false))
                        }
                        // A mine counts with its reach once it is visible to
                        // the worker's player, before with 0.49; the place is
                        // the segment goal (SPEC-0019; c-RXD-1).
                        Order::Gather { target } => {
                            let visible = state
                                .unit(Some(h))
                                .is_some_and(|u| super::fog::unit_visible(state, target, u.owner));
                            Some((if visible { MINE_REACH } else { ARRIVAL }, m.heading, false))
                        }
                        // The walk back to the town hall, reach 6.0 (c-RXD-1,
                        // SPEC-0019).
                        Order::Return { .. } => Some((HALL_REACH, m.heading, false)),
                        _ => None,
                    };
                    let place = (cluster > 1)
                        .then(|| state.member_now.get(&h).copied())
                        .flatten();
                    let walker = |order| follow::Walker {
                        handle: h,
                        here,
                        order,
                        class,
                        squared_speed,
                        cluster,
                        chase,
                        building,
                        tree,
                        arrive,
                        place,
                    };
                    // Round the obstacles of the pathing map (SPEC-0020), or
                    // straight without one.
                    // A held member does not follow its path; it turns
                    // towards its place (SPEC-0025).
                    let mut steer = if held {
                        follow::Steer::Stand(goal)
                    } else if state.grid.is_some() {
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
                        } else if place_on_way {
                            m.walking = false;
                            m.velocity = [0, 0];
                        } else {
                            m.arrived = true;
                            m.walking = false;
                            m.velocity = [0, 0];
                            state.paths.remove(&h);
                        }
                    }
                    match steer {
                        follow::Steer::Walk(to) => {
                            motion(&mut m, to, true, speed, limit, turn_rate, window);
                        }
                        // No motion, but the heading turns (SPEC-0020).
                        follow::Steer::Stand(to) => {
                            motion(&mut m, to, false, speed, limit, turn_rate, window);
                        }
                        follow::Steer::Hold => {
                            m.velocity = [0, 0];
                            m.walking = false;
                        }
                        // At the end of its walk to a building a worker stands
                        // and turns towards it (SPEC-0029).
                        follow::Steer::Arrived if building.is_some() => {
                            motion(&mut m, goal, false, speed, limit, turn_rate, window);
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
        // A worker whose walk to its mine, tree or drop-off ended without
        // touching it takes it as reached (PROVISIONAL: SPEC-0029 measured
        // only walks that touch).
        if m.arrived
            && let Order::Gather { target } | Order::Return { building: target } = m.order
        {
            let delivering = matches!(m.order, Order::Return { .. });
            state.harvest_reached.push((h, target, delivering));
            m.order = Order::Work { target };
            m.arrived = false;
        }
        // So too a worker sent back to its building under construction.
        if m.arrived
            && let Order::Resume { building } = m.order
            && unfinished_of_own(state, h, building)
        {
            m.order = Order::Repair { building };
            m.arrived = false;
            face_target(state, &mut m, building, min);
        }
        // The facing follows the heading (SPEC-0018).
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        // A unit in its reaction delay stands, its facing too (SPEC-0019).
        let heading = normal(m.heading);
        if !waiting {
            let (facing, step) = super::facing::follow(u.facing, heading, m.facing_step, orient);
            u.facing = facing;
            m.facing_step = step;
        }
        let facing_done = u.facing == heading;
        if m.arrived && facing_done && begin_attack.is_none() {
            u.movement = None;
        } else {
            u.movement = Some(m);
            still.push(h);
        }
        state.updated.insert(h);
        if let Some(trace) = state.agent_trace.as_mut() {
            trace.push((tick, h, m.origin, m.velocity, m.heading, m.aim));
        }
        if let Some(target) = begin_attack {
            super::attack::begin(state, h, target, tick, m.order_id);
        }
    }

    state.marked.clear();
    refresh_call_cells(state);
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

/// The cell of the 64 x 64 grid of the high-level search (SPEC-0020)
/// that holds point `p`, relative to the world corner.
fn grid64(p: [u32; 2]) -> [i32; 2] {
    p.map(|v| (real_of(v) / 64.0).floor() as i32)
}

/// The updates until a chase renews its goal after taking the goal `b`
/// from `a` (SPEC-0019): `round(0.33 d)` with `d` in cells, clamped to
/// 16..132. The square root is the correctly rounded one of IEEE 754.
/// `smart` or `move` (`order_id`) of unit `h` on unit `target`: it
/// follows it (SPEC-0019, "Following an allied unit"). A target that is
/// not a unit is walked to.
///
/// PROVISIONAL: measured with units of the same player; allies and
/// buildings are followed the same way.
fn follow_order(state: &mut State, h: u32, target: u32, order_id: i32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(at) = position_of(state, target).filter(|_| target != h) else {
        return false;
    };
    let (Some(u), Some(Object::Unit(_))) = (state.unit(Some(h)), state.objects.get(&target)) else {
        return order(state, h, Order::Point, order_id, at);
    };
    let from = relative([u.x, u.y], min);
    let gap = sub(
        sub(distance(from, relative(at, min)), collision_of(state, h)),
        collision_of(state, target),
    );
    let follow = Order::Follow {
        leader: target,
        from,
        start_gap: gap,
        standing: false,
    };
    order(state, h, follow, order_id, at)
}

/// The follow range of `move` or `smart` on an allied unit (SPEC-0019).
const FOLLOW_RANGE: u32 = 0x4396_0000;
/// The countdown a follower gets when it stops in the follow range
/// (`chase` updates until its next renewal; SPEC-0019).
const FOLLOWER_FIRST_RENEWAL: u32 = 18;
/// Updates from a follower's renewal to the next one, less one.
const FOLLOWER_RENEWAL: u32 = 16;
/// The reach of a gather order in cells: 132 / 32 (the gather range
/// 116 of `Ahar` plus 16, read for a peasant; SPEC-0019).
const GATHER_REACH: u32 = 0x4084_0000;
/// The reach of the walk back with a load in cells: 192 / 32 (`c-RXD-1`,
/// SPEC-0019).
const HALL_REACH: u32 = 0x40C0_0000;
/// The reach of a harvest order on a gold mine in cells: 66 / 32
/// (`c-RXD-1`, SPEC-0019).
const MINE_REACH: u32 = 0x4004_0000;

/// The collision size of unit `h`, 0 for none.
fn collision_of(state: &State, h: u32) -> u32 {
    state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .map_or(0, |t| t.collision)
}

/// The distance between two points.
fn distance(a: [u32; 2], b: [u32; 2]) -> u32 {
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    sqrt(add(mul(dx, dx), mul(dy, dy)))
}

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
    let Some((_, own)) = collision(u) else {
        return false;
    };
    let other = state
        .unit(Some(target))
        .and_then(collision)
        .map_or(0, |c| c.1);
    let range = super::attack::weapon_for(state, u.type_id, Some(target)).map_or(0, |w| w.range);
    let reach = add(add(mul(range, CELL), mul(own, CELL)), mul(other, CELL));
    in_range(relative([u.x, u.y], min), relative(at, min), reach)
}

/// The reaction delays that end in this tick, before its updates
/// (SPEC-0019, "Reaction delay"; SPEC-0021, "Searches of many units in the
/// same tick"): each such unit takes a new goal and comes first among the
/// units handled; delays ending in the same tick end in the order they
/// began. An attack taken at an acquisition or a call for help ends its
/// delay in the tick whose queue interval holds its end, and its goal is
/// where the target stands in that tick; its path to it is searched at
/// its next update. Called before the updates of the tick (`first`) with
/// their time `U` as `until` (the ends up to it, all of the tick without
/// updates), and after them with `None` (the
/// ends up to the clock of the next tick, SPEC-0018).
pub(super) fn reaction_ends(state: &mut State, until: Option<u32>, first: bool) {
    let (tick, clock) = (state.ticks, state.move_clock);
    let limit = until.unwrap_or_else(|| add(clock, crate::timers::TICK));
    // The ends of the timed delays in the order of their expiries (c-RXR-1:
    // trolls 18 and 19 in 5584), then by the orders.
    let mut ending: Vec<(f32, u64, u32)> = state
        .moving
        .iter()
        .filter_map(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) => {
                let m = u.movement?;
                let ends = match (m.reaction_end, m.reaction) {
                    (Some(end), _) => real_of(end) <= real_of(limit),
                    (None, Some((from, delay, earliest))) if first => {
                        let waited = mul(i2r(tick as i32 - from as i32), crate::timers::TICK);
                        tick >= earliest && real_of(waited) >= real_of(delay)
                    }
                    _ => false,
                };
                let at = m.reaction_end.map_or(f32::NEG_INFINITY, real_of);
                ends.then_some((at, m.given, h))
            }
            _ => None,
        })
        .collect();
    ending.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (_, _, h) in ending {
        let given = state.orders_given + 1;
        state.orders_given = given;
        let (timed, target) = match state.objects.get_mut(&h) {
            Some(Object::Unit(u)) => match u.movement.as_mut() {
                Some(m) => {
                    let timed = m.reaction_end.take().is_some();
                    m.reaction = None;
                    m.given = given;
                    match m.order {
                        Order::Attack { target } => (timed, target),
                        _ => continue,
                    }
                }
                None => continue,
            },
            _ => continue,
        };
        // The goal is where the target is at the start of this tick (SPEC-0019).
        if timed
            && let Some(at) = tick_start_position(state, target)
            && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
        {
            m.target = at;
            m.chase = None;
            m.goal_taken = true;
        }
    }
}

/// What a chase of `target` needs for its search: the target's position
/// at its last update (where it stands, for a unit without a move) and its
/// size class (SPEC-0019), and `reach`.
fn chase_of(state: &State, target: u32, reach: u32) -> follow::Chase {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let (at, collision) = match state.unit(Some(target)) {
        Some(t) => (
            t.movement
                .map_or_else(|| relative([t.x, t.y], min), |m| m.origin),
            state.data.units.get(&t.type_id).map_or(0, |d| d.collision),
        ),
        None => (
            position_of(state, target).map_or([0, 0], |p| relative(p, min)),
            0,
        ),
    };
    follow::Chase {
        target: at,
        target_class: crate::path::size_class(collision),
        reach,
    }
}

/// Whether unit `h` already chases `target` with a goal taken at this
/// update.
pub(super) fn chasing(state: &State, h: u32, target: u32) -> bool {
    state
        .unit(Some(h))
        .and_then(|u| u.movement)
        .is_some_and(|m| {
            m.order == Order::Attack { target } && m.renew_at.is_none() && m.chase.is_some()
        })
        && state.update_now
}

/// Unit `h` takes up its attack on `target` again after its swing
/// (SPEC-0019): the goal is where the target is now, its path is searched
/// at once, the unit moves from its next update on, and at its 12th
/// update it takes a new goal with an update standing.
pub(super) fn chase(state: &mut State, h: u32, target: u32, order_id: i32, renew_at: Option<u64>) {
    // After the swing: where the target was at the start of the tick of
    // the backswing end (c-RXR-1: rifleman 9, 5053).
    let at = match (renew_at, state.objects.get(&target)) {
        (Some(_), Some(Object::Unit(u))) => u.movement.map(|m| {
            let min = [state.world_bounds[0], state.world_bounds[1]];
            absolute(m.position(sub(state.move_clock, crate::timers::TICK)), min)
        }),
        _ => None,
    };
    let Some(at) = at.or_else(|| position_of(state, target)) else {
        return;
    };
    if !order(state, h, Order::Attack { target }, order_id, at) {
        return;
    }
    let (tick, min) = (state.ticks, [state.world_bounds[0], state.world_bounds[1]]);
    let range = state
        .unit(Some(h))
        .and_then(|u| super::attack::weapon_for(state, u.type_id, Some(target)))
        .map_or(0, |w| w.range);
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    let here = relative([u.x, u.y], min);
    if let Some(m) = u.movement.as_mut() {
        match renew_at {
            // Right after the swing: a new goal with an update standing at
            // `renew_at`.
            Some(r) => {
                m.chase = Some(u32::MAX);
                m.renew_at = Some(r);
                m.renew_stands = true;
            }
            // At the new goal itself: this goal, searched at the next
            // update.
            None => m.goal_taken = true,
        }
    }
    let data = state.data.units.get(&u.type_id);
    let own = data.map_or(0, |t| t.collision);
    let class = crate::path::size_class(own);
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
            cluster: 1,
            chase: Some(chase_of(state, target, reach)),
            building: None,
            tree: None,
            arrive: None,
            place: None,
        };
        follow::search(state, &walker, tick);
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

/// The first check of an attack-move after its order, or after a kill:
/// 0.51 s on the timer queue.
const ATTACK_MOVE_FIRST_TIME: u32 = 0x3F02_8F5C;
const HALF: u32 = 0x3F00_0000;

/// An `attack` order to a point (SPEC-0019, "Attack-move with enemies"):
/// the point, the tick of the next check for enemies, and after a kill
/// the tick at which the unit walks on (the end of its backswing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackMove {
    point: [u32; 2],
    /// The expiry of its next check on the timer queue.
    next_check: u32,
    resume_at: Option<u64>,
}

fn set_attack_move(state: &mut State, h: u32, attack_move: Option<AttackMove>) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack_move = attack_move;
    }
}

/// Unit `h` on an attack-move killed its target with the hit of `tick`:
/// it walks on after its backswing, and checks again 102 ticks after the
/// kill (SPEC-0019).
pub(super) fn killed(state: &mut State, h: u32, tick: u64, backswing: u64) {
    let time = state.timers.time();
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(a) = u.attack_move.as_mut()
    {
        a.resume_at = Some(tick + backswing);
        a.next_check = add(time, ATTACK_MOVE_FIRST_TIME);
    }
}

/// Within this distance of its guard point a unit does not walk back.
const GUARD_ACCEPT: f32 = 50.0;

/// Attack-moves going on after a kill, and ending at their point; units
/// walking back to their guard points.
pub(super) fn attack_moves(state: &mut State) {
    let tick = state.ticks;
    cooldown_expiries(state);
    let back: Vec<(u32, [u32; 2])> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            (u.guard_return == Some(tick)).then_some((h, u.home))
        })
        .collect();
    for (h, home) in back {
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        u.guard_return = None;
        let idle = u.attack.is_none() && u.movement.is_none_or(|m| !m.ordered());
        let (dx, dy) = (
            real_of(u.x) - real_of(home[0]),
            real_of(u.y) - real_of(home[1]),
        );
        if idle && dx * dx + dy * dy > GUARD_ACCEPT * GUARD_ACCEPT {
            order(state, h, Order::Point, 0, home);
        }
    }
    let units: Vec<(u32, AttackMove)> = state
        .units
        .iter()
        .filter_map(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive => u.attack_move.map(|a| (h, a)),
            _ => None,
        })
        .collect();
    for (h, a) in units {
        if a.resume_at == Some(tick) {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.attack = None;
            }
            order(state, h, Order::Point, ORDER_ATTACK, a.point);
            set_attack_move(
                state,
                h,
                Some(AttackMove {
                    resume_at: None,
                    ..a
                }),
            );
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        if a.resume_at.is_some() || u.attack.is_some() {
            continue;
        }
        match u.movement {
            Some(m) if m.ordered() => {}
            // Arrived at the point: the order ends.
            Some(m) if m.order == Order::Point => set_attack_move(state, h, None),
            // PROVISIONAL: the target is gone without a hit of this unit
            // (not measured); it walks on.
            _ => {
                order(state, h, Order::Point, ORDER_ATTACK, a.point);
            }
        }
    }
}

/// Why a unit looks for enemies now.
#[derive(Clone, Copy)]
enum Check {
    /// An idle unit at its own schedule: when the check expired on the
    /// movement clock.
    Idle { expiry: u32 },
    /// A unit attacking `current`, which it took on its own.
    Attacking { current: u32 },
    /// A unit walking an attack-move.
    AttackMove { expiry: u32 },
}

/// Units with a weapon look for enemies (SPEC-0019): idle ones on their
/// own schedule, engine timers that expire `0.5 * x` s after their
/// creation and then every 0.5 s, with `x` drawn when they were created,
/// each running in the first tick whose movement clock is at or after its
/// expiry ("Acquisition by idle units"); units walking an attack-move
/// every 100 ticks from 102 ticks after the order ("Attack-move with
/// enemies"). Of the enemies whose centres are within `acquire` and both
/// collision sizes a unit takes the nearest unit, a structure only when
/// there is no unit, and on equal distances the one with the smaller y,
/// then x. An idle unit attacks it on its own (`GetUnitCurrentOrder` 0)
/// after the reaction delay counted from the expiry of its check; an
/// attack-move stops at once and attacks after the reaction delay counted
/// from the check's tick, its order staying `attack`. Gives the units that
/// acquired a target and those that answered their calls for help, in
/// that order.
/// The earliest expiry of the acquisition checks due in this tick.
pub(super) fn first_check(state: &State) -> Option<u32> {
    let next_clock = real_of(add(state.move_clock, crate::timers::TICK));
    state
        .units
        .iter()
        .filter_map(|&h| state.unit(Some(h)).filter(|u| u.alive))
        .flat_map(|u| {
            let own = (u.acquire_phase != super::NO_PHASE).then_some(u.next_acquire);
            own.into_iter().chain(u.attack_move.map(|a| a.next_check))
        })
        .filter(|&e| real_of(e) <= next_clock)
        .min_by(|a, b| real_of(*a).total_cmp(&real_of(*b)))
}

pub(super) fn acquisitions(state: &mut State) -> Vec<u32> {
    let tick = state.ticks;
    let next_clock = add(state.move_clock, crate::timers::TICK);
    let alive: BTreeSet<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| state.unit(Some(h)).is_some_and(|u| u.alive))
        .collect();
    let mut starts = state.acquire_starts;
    // (expiry, start number, unit, check).
    let mut due: Vec<(u32, u64, u32, Check)> = Vec::new();
    for i in 0..state.units.len() {
        let h = state.units[i];
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        if !u.alive {
            continue;
        }
        // The unit's own timer runs whatever it does: the check runs in the
        // tick whose queue interval, after the clock of this tick up to that
        // of the next, holds its expiry, and the next one comes 1.0 s later
        // with a target, 0.5 s without.
        let target = match (&u.attack, u.movement) {
            // In a swing the target is the swing's.
            (Some(a), _) => Some((a.target, a.order_id)),
            (None, Some(m)) => match m.order {
                Order::Attack { target } => Some((target, m.order_id)),
                _ => None,
            },
            (None, None) => None,
        };
        // A dead target is none.
        let target = target.filter(|&(t, _)| alive.contains(&t));
        let mut expired = None;
        let seq = u.acquire_seq;
        if u.acquire_phase != super::NO_PHASE && real_of(u.next_acquire) <= real_of(next_clock) {
            expired = Some(u.next_acquire);
            let period = if target.is_some() { ONE } else { HALF };
            u.next_acquire = add(u.next_acquire, period);
            starts += 1;
            u.acquire_seq = starts;
        }
        if u.paused {
            continue;
        }
        // A creep walking home takes no target (SPEC-0030; c-RXD-1: creep
        // 7 at 4577).
        if u.creep.returning {
            continue;
        }
        // A unit on attack-move whose target died acquires at the check
        // restarted by the death (c-RXR-1: footman 2 took grunt 13 in 2581).
        if u.attack_move.is_some()
            && let Some(expiry) = expired
            && std::mem::take(&mut u.after_death)
            && target.is_none()
        {
            due.push((expiry, seq, h, Check::AttackMove { expiry }));
            continue;
        }
        // A unit on attack-move that attacks a target checks for a better
        // one as any attacker (c-RXR-1: rifleman 8 took grunt 15 in 5603).
        if u.attack_move.is_some()
            && let (Some(expiry), Some((current, _))) = (expired, target)
        {
            due.push((expiry, seq, h, Check::Attacking { current }));
            continue;
        }
        if let Some(a) = u.attack_move.as_mut() {
            if real_of(a.next_check) <= real_of(next_clock) {
                let expiry = a.next_check;
                a.next_check = add(a.next_check, HALF);
                let walking = u.attack.is_none()
                    && a.resume_at.is_none()
                    && u.movement
                        .is_some_and(|m| m.ordered() && m.order == Order::Point);
                if walking {
                    due.push((expiry, 0, h, Check::AttackMove { expiry }));
                }
            }
            continue;
        }
        // An attack the unit took on its own (order id 0) checks for a
        // better target (SPEC-0019, "Checks while attacking").
        if let (Some(expiry), Some((current, 0))) = (expired, target) {
            due.push((expiry, seq, h, Check::Attacking { current }));
            continue;
        }
        // Nor one that casts or is stunned (SPEC-0035, SPEC-0037).
        if u.attack.is_some()
            || u.movement.is_some_and(|m| m.ordered() && !m.going_back())
            || u.casting.is_some()
            || u.stunned_until.is_some_and(|t| tick < t)
        {
            continue;
        }
        // A sleeping creep does not acquire (SPEC-0030).
        if u.creep.sleeping {
            continue;
        }
        if let Some(expiry) = expired {
            due.push((expiry, seq, h, Check::Idle { expiry }));
        }
    }
    state.acquire_starts = starts;
    if due.is_empty() {
        return Vec::new();
    }
    // The checks are timers: in the order of their expiries, then of their
    // starts.
    due.sort_by(|a, b| real_of(a.0).total_cmp(&real_of(b.0)).then(a.1.cmp(&b.1)));
    let targets = Targets::new(state);
    let mut acquired = Vec::new();
    for (_, _, h, check) in due {
        // A better target than the one attacked: taken at once, without a
        // reaction delay or a call for help (SPEC-0019).
        if let Check::Attacking { current, .. } = check {
            if let Some(better) = targets.better_than(state, h, current) {
                // Also in its backswing: the unit turns to the new target at
                // its updates while the backswing ends (c-RXD-1: creep 7
                // took its target in 4457 and turned in 4459 and 4465).
                if switch_target(state, h, better) {
                    acquired.push(h);
                }
            }
            continue;
        }
        // With no enemy in range, the attacker of the last call for help
        // that reached the unit, within 2000 (c-RXR-1: troll 16 took
        // rifleman 9 at 718 in 6420). PROVISIONAL: a further test (likely
        // that it can be seen) is not known.
        let remembered = || {
            let u = state.unit(Some(h))?;
            let r = u.remembered?;
            let o = state
                .unit(Some(r))
                .filter(|o| o.alive && enemies(state, u.owner, o.owner))?;
            let (dx, dy) = (real_of(o.x) - real_of(u.x), real_of(o.y) - real_of(u.y));
            (dx * dx + dy * dy <= REMEMBERED_REACH * REMEMBERED_REACH).then_some(r)
        };
        let idle = matches!(check, Check::Idle { .. });
        let Some(target) = targets
            .nearest_enemy(state, h)
            .or_else(|| remembered().filter(|_| idle))
        else {
            continue;
        };
        let Some(Object::Unit(o)) = state.objects.get(&target) else {
            continue;
        };
        let at = [o.x, o.y];
        let (order_id, expiry) = match check {
            Check::Idle { expiry } => (0, Some(expiry)),
            Check::AttackMove { expiry } => (ORDER_ATTACK, Some(expiry)),
            Check::Attacking { .. } => continue,
        };
        // A walking unit stops for its reaction delay (c-RXR-1: footman 2
        // in 2581).
        freeze(state, h);
        if order(state, h, Order::Attack { target }, order_id, at) {
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            // The delay counts from the expiry of the check, an idle
            // unit's or an attack-move's, on the timer queue (SPEC-0019).
            // PROVISIONAL: an attack-move's check is taken to expire at the
            // end of its tick.
            let from = expiry.unwrap_or(add(state.move_clock, crate::timers::TICK));
            react_until(state, h, add(from, delay));
            acquired.push(h);
            // A creep calls its camp (SPEC-0030), in the order of
            // SPEC-0019.
            acquired.extend(super::creeps::call_for_help(state, h, target, expiry));
        }
    }
    acquired
}

/// The checks at the end of a backswing of the attackers that switched
/// targets in it: a new target within range is faced where it was at the
/// start of this tick, as after the swing (`chase`), the place of the
/// attacker's next update (SPEC-0019, "Which direction it turns to";
/// c-RXD-1: creep 6 switched at 2031, backswing end 2033, turned at 2035;
/// creep 8 switched at 2007, backswing end 2081, turned at 2083).
fn attack_checks(state: &mut State) {
    let tick = state.ticks;
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.attack_checks)
        .into_iter()
        .partition(|&(t, _, _)| t <= tick);
    state.attack_checks = later;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    for (_, h, target) in due {
        let at = match state.objects.get(&target) {
            Some(Object::Unit(u)) => u
                .movement
                .map(|m| absolute(m.position(sub(state.move_clock, crate::timers::TICK)), min)),
            _ => None,
        };
        let Some(at) = at.or_else(|| position_of(state, target)) else {
            continue;
        };
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        let range =
            super::attack::weapon_for(state, u.type_id, Some(target)).map_or(0, |w| w.range);
        let own = state.data.units.get(&u.type_id).map_or(0, |t| t.collision);
        let reach = add(
            mul(add(range, own), CELL),
            mul(collision_of(state, target), CELL),
        );
        let here = relative([u.x, u.y], min);
        if !in_range(here, relative(at, min), reach) {
            continue;
        }
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
            && m.order == (Order::Attack { target })
        {
            m.target = at;
            m.faced = true;
        }
    }
}

/// Unit `h` attacks `better` instead of its target at once: no reaction
/// delay and no call for help; a walking attacker stops at once, and a
/// new goal after a swing still waits for the cooldown (SPEC-0019).
/// Whether it took the order.
fn switch_target(state: &mut State, h: u32, better: u32) -> bool {
    let tick = state.ticks;
    let resume = state
        .unit(Some(h))
        .and_then(|u| u.attack)
        .and_then(|a| a.resume)
        .filter(|&r| r > tick);
    let renew = state.unit(Some(h)).and_then(|u| {
        u.attack
            .and_then(|a| a.renew_at)
            .or(u.movement.and_then(|m| m.renew_at))
    });
    let Some(at) = position_of(state, better) else {
        return false;
    };
    freeze(state, h);
    // An attack-move keeps its order id.
    let order_id = state
        .unit(Some(h))
        .and_then(|u| {
            u.movement
                .map(|m| m.order_id)
                .or(u.attack.map(|a| a.order_id))
        })
        .unwrap_or(0);
    if !order(state, h, Order::Attack { target: better }, order_id, at) {
        return false;
    }
    // A switch in a backswing: the check at its end is on the new target.
    if let Some(r) = resume {
        state.attack_checks.push((r, h, better));
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        m.goal_taken = true;
        if let Some(r) = renew {
            m.renew_at = Some(r);
            m.renew_stands = true;
        }
    }
    true
}

/// Whether unit `h`, attacking `current` on its own, takes `other` that
/// calls for help instead, by the rating of its checks: in range before
/// out of range, the target in range kept, else by what the enemy does and
/// then the nearer (SPEC-0019, "Checks while attacking").
fn prefers(state: &State, h: u32, current: u32, other: u32) -> bool {
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let own = state
        .data
        .units
        .get(&u.type_id)
        .map_or(0.0, |d| real_of(d.collision));
    let (x, y) = (real_of(u.x), real_of(u.y));
    let rate = |t: u32| {
        let o = state.unit(Some(t))?;
        let collision = state
            .data
            .units
            .get(&o.type_id)
            .map_or(0.0, |d| real_of(d.collision));
        let range =
            super::attack::weapon_for(state, u.type_id, Some(t)).map_or(0.0, |w| real_of(w.range));
        let (dx, dy) = (real_of(o.x) - x, real_of(o.y) - y);
        let distance = dx * dx + dy * dy;
        let reach = range + own + collision;
        Some((distance <= reach * reach, distance))
    };
    let (Some(mine), Some(theirs)) = (rate(current), rate(other)) else {
        return false;
    };
    if mine.0 != theirs.0 {
        return theirs.0;
    }
    if mine.0 {
        return false;
    }
    (threat(state, h, other), theirs.1) < (threat(state, h, current), mine.1)
}

/// The units that can be targets, by x: (x, y, collision, owner,
/// structure, handle), and the widest collision among them.
struct Targets {
    list: Vec<(f32, f32, u32, i32, bool, u32)>,
    widest: f32,
}

impl Targets {
    fn new(state: &State) -> Targets {
        let mut list: Vec<(f32, f32, u32, i32, bool, u32)> = state
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
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let widest = list.iter().map(|t| real_of(t.2)).fold(0.0f32, f32::max);
        Targets { list, widest }
    }

    /// The enemy unit `h` takes within its acquisition range, by the rule
    /// above.
    /// At a check while unit `h` attacks `current` (SPEC-0019, "Checks
    /// while attacking"): the enemy it should attack instead, if any. The
    /// enemies within half its `acquire` and both collision sizes are
    /// rated with its target: one in range of the attack before one out of
    /// it, the target in range before any other in range, and else the
    /// strictly nearer; a tie keeps the target.
    ///
    /// PROVISIONAL: only those parts of the rating were measured (with
    /// peasants as targets; open question 10 of SPEC-0019).
    fn better_than(&self, state: &State, h: u32, current: u32) -> Option<u32> {
        let u = state.unit(Some(h))?;
        let (owner, x, y) = (u.owner, real_of(u.x), real_of(u.y));
        let data = state.data.units.get(&u.type_id)?;
        let own = real_of(data.collision);
        // Half the acquisition range, or the longest range of its weapons
        // if that is more (c-RXR-1: troll headhunters, acquire 500 and
        // range 450, looked 450 and both collisions round them).
        let range = |w: &Option<crate::data::Weapon>| w.as_ref().map_or(0.0, |w| real_of(w.range));
        let half = (real_of(data.acquire) / 2.0)
            .max(range(&data.weapon))
            .max(range(&data.weapon2));
        // (in range, squared distance) of a candidate.
        let rate = |t: u32, ox: f32, oy: f32, collision: f32| {
            let range = super::attack::weapon_for(state, u.type_id, Some(t))
                .map_or(0.0, |w| real_of(w.range));
            let (dx, dy) = (ox - x, oy - y);
            let distance = dx * dx + dy * dy;
            let reach = range + own + collision;
            (distance <= reach * reach, distance)
        };
        let t = state.unit(Some(current))?;
        let collision = state
            .data
            .units
            .get(&t.type_id)
            .map_or(0.0, |d| real_of(d.collision));
        let mine = rate(current, real_of(t.x), real_of(t.y), collision);
        let mut best = (current, mine, threat(state, h, current));
        for &(ox, oy, collision, other_owner, _, other) in &self.list {
            if other == current || !enemies(state, owner, other_owner) {
                continue;
            }
            let collision = real_of(collision);
            let limit = half + own + collision;
            let (dx, dy) = (ox - x, oy - y);
            if dx.abs() > limit || dy.abs() > limit || dx * dx + dy * dy > limit * limit {
                continue;
            }
            let (in_range, distance) = rate(other, ox, oy, collision);
            let rank = threat(state, h, other);
            let (best_in, best_distance) = best.1;
            let better = if in_range != best_in {
                in_range
            } else if in_range && best.0 == current {
                false
            } else {
                (rank, distance) < (best.2, best_distance)
            };
            if better {
                best = (other, (in_range, distance), rank);
            }
        }
        (best.0 != current).then_some(best.0)
    }

    fn nearest_enemy(&self, state: &State, h: u32) -> Option<u32> {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            return None;
        };
        let (owner, x, y) = (u.owner, real_of(u.x), real_of(u.y));
        let data = state.data.units.get(&u.type_id)?;
        if data.weapon.is_none() || real_of(data.acquire) <= 0.0 {
            return None;
        }
        let reach = add(data.acquire, data.collision);
        let widest = self.widest;
        let from = self
            .list
            .partition_point(|t| t.0 < x - real_of(reach) - widest - 1.0);
        // (structure, threat, squared distance, y, x, handle).
        let mut best: Option<(bool, u8, f32, f32, f32, u32)> = None;
        for &(ox, oy, collision, other_owner, structure, other) in &self.list[from..] {
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
            if distance > limit * limit {
                continue;
            }
            let key = (structure, threat(state, h, other), distance, oy, ox, other);
            if best.is_none_or(|b| (key.0, key.1, key.2, key.3, key.4) < (b.0, b.1, b.2, b.3, b.4))
            {
                best = Some(key);
            }
        }
        best.map(|b| b.5)
    }
}

/// What enemy `other` does as unit `h` rates it (SPEC-0019, "Choice of
/// target"): 0 when it attacks `h`, 1 when it attacks another unit of
/// `h`'s player or an ally, 2 when it attacks nobody (an attack-move
/// without a target counts as nobody).
fn threat(state: &State, h: u32, other: u32) -> u8 {
    let Some(o) = state.unit(Some(other)) else {
        return 2;
    };
    let target = o
        .attack
        .map(|a| a.target)
        .or(match o.movement.map(|m| m.order) {
            Some(Order::Attack { target }) => Some(target),
            _ => None,
        });
    // Only while that attack goes on: a target that died counts as none
    // (c-RXR-1: rifleman 7 after its kill of grunt 13, rated at 6420).
    let Some(target) = target.filter(|&t| state.unit(Some(t)).is_some_and(|t| t.alive)) else {
        return 2;
    };
    if target == h {
        return 0;
    }
    let mine = state.unit(Some(h)).map(|u| u.owner);
    let theirs = state.unit(Some(target)).map(|t| t.owner);
    match (mine, theirs) {
        (Some(a), Some(b)) if a == b || state.setup.is_ally(a, b) => 1,
        _ => 2,
    }
}

/// What a missile's impact adds to CallForHelp for its call (SPEC-0019).
const IMPACT_REACH: f32 = 150.0;
/// Seconds after a call for help in which the caller makes no other.
const CALL_COOLDOWN: f32 = 3.0;
/// How far a remembered attacker of a call for help is taken.
const REMEMBERED_REACH: f32 = 2000.0;

/// The hit of `attacker` on `victim` (SPEC-0019, "Retaliation and call
/// for help"): an idle victim acquires the attacker whatever the distance,
/// and so do the idle units within `CallForHelp` and a little of it that
/// answer its call, in the order of the grid of "Order of the
/// responders" and in a chain, each answering unit calling in turn; all of
/// them wait
/// their reaction delay from this tick, the victim drawing first.
///
/// PROVISIONAL: only units with a weapon answer, workers (`peon`) never;
/// the area is a circle round the victim's centre (its exact shape is
/// open); only hits of weapons call for help, not scripted damage.
pub(super) fn retaliate(state: &mut State, victim: u32, attacker: u32, impact: Option<[u32; 2]>) {
    let tick = state.ticks;
    let (Some(v), Some(a)) = (state.unit(Some(victim)), state.unit(Some(attacker))) else {
        return;
    };
    let (owner, x, y, at) = (v.owner, real_of(v.x), real_of(v.y), [a.x, a.y]);
    if !enemies(state, owner, a.owner) {
        return;
    }
    // A sleeping creep wakes in this tick with the sleeping creeps near it,
    // and answers; one going home does not answer (SPEC-0030).
    super::creeps::wake_on_hit(state, victim, attacker);
    // A unit in its backswing takes the target only for show: no attack,
    // no draw; it acquires at its check after the backswing (c-RXR-1:
    // trolls 16 and 18 in 3689 and 3713).
    let now = real_of(state.event_time.unwrap_or_else(|| state.timers.time()));
    let swinging_back = |u: &super::Unit| real_of(u.backswing_end) > now;
    let answers = |h: u32| match state.objects.get(&h) {
        Some(Object::Unit(u)) => {
            u.alive
                && !swinging_back(u)
                && !u.paused
                && !u.creep.sleeping
                && !u.creep.returning
                && u.attack.is_none()
                && u.attack_move.is_none()
                && u.movement.is_none_or(|m| !m.ordered() || m.going_back())
                && u.casting.is_none()
                && u.stunned_until.is_none_or(|t| state.ticks >= t)
                && state
                    .data
                    .units
                    .get(&u.type_id)
                    .is_some_and(|t| t.weapon.is_some() && !t.worker)
        }
        _ => false,
    };
    // A unit attacking a target it took on its own answers by switching to
    // the attacker at once when its checks would rate the attacker better
    // (c-RXR-1: troll 16, chasing rifleman 7 far away, took footman 4 in
    // range at the call of grunt 14 in 827).
    let current = |h: u32| {
        let u = state.unit(Some(h))?;
        let target = match (u.attack, u.movement) {
            (Some(a), _) if a.order_id == 0 => a.target,
            (None, Some(m)) if m.order_id == 0 => match m.order {
                Order::Attack { target } => target,
                _ => return None,
            },
            _ => return None,
        };
        (u.alive
            && !swinging_back(u)
            && !u.paused
            && !u.creep.returning
            && u.attack_move.is_none()
            && u.casting.is_none()
            && u.stunned_until.is_none_or(|t| state.ticks >= t)
            && target != attacker)
            .then_some(target)
    };
    let switches = |h: u32| current(h).is_some_and(|c| prefers(state, h, c, attacker));
    // The victim calls whatever it is doing (in `c-RXR-1` the troll
    // headhunters answered grunt 12, which was attacking, in tick 688),
    // then each unit that answers calls in turn, in the order of the grid
    // (SPEC-0019, "Order of the responders").
    // A call starts a 3.0 s timer of the victim; while it runs, damage to
    // it makes no call (c-RXR-1: grunt 13 in 4823). PROVISIONAL: the
    // chain's callers are not held by their own timers.
    // A missile's impact calls instead, round the impact point, with
    // CallForHelp + 150 and no timer (SPEC-0019; test RZG). A unit is
    // reached when its centre is within the radius plus its collision
    // size (c-RXR-1, 6479: trolls at 611.1 in, 635.2 out).
    let calls = impact.is_some()
        || state
            .unit(Some(victim))
            .is_some_and(|v| now >= real_of(v.call_until));
    let (x, y) = impact.map_or((x, y), |p| (real_of(p[0]), real_of(p[1])));
    let radius =
        real_of(state.combat.call_for_help) + if impact.is_some() { IMPACT_REACH } else { 0.0 };
    let call_radius = real_of(state.combat.call_for_help);
    let reach_of = |u: &super::Unit, first: bool| {
        (if first { radius } else { call_radius })
            + state
                .data
                .units
                .get(&u.type_id)
                .map_or(0.0, |t| real_of(t.collision))
    };
    let mut responders = vec![victim];
    let mut reached = vec![victim];
    let mut next = 0;
    while next < responders.len() {
        if !calls {
            break;
        }
        let (cx, cy) = match state.unit(Some(responders[next])) {
            Some(c) if next > 0 => (real_of(c.x), real_of(c.y)),
            _ => (x, y),
        };
        let first = next == 0;
        next += 1;
        // PROVISIONAL: no chain of calls (c-RXR-1, 6479; test RZG).
        if !first {
            break;
        }
        // Every unit the call reaches remembers the attacker.
        reached.extend(state.units.iter().copied().filter(|&h| {
            let Some(Object::Unit(u)) = state.objects.get(&h) else {
                return false;
            };
            let (dx, dy) = (real_of(u.x) - cx, real_of(u.y) - cy);
            u.alive
                && h != attacker
                && dx * dx + dy * dy <= reach_of(u, first) * reach_of(u, first)
                && state.setup.helps(u.owner, owner)
        }));
        let near: Vec<u32> = state
            .units
            .iter()
            .copied()
            .filter(|&h| {
                let Some(Object::Unit(u)) = state.objects.get(&h) else {
                    return false;
                };
                let (dx, dy) = (real_of(u.x) - cx, real_of(u.y) - cy);
                h != attacker
                    && !responders.contains(&h)
                    && dx * dx + dy * dy <= reach_of(u, first) * reach_of(u, first)
                    && state.setup.helps(u.owner, owner)
                    && (answers(h) || switches(h))
            })
            .collect();
        responders.extend(call_order(state, near));
    }
    if !answers(victim) && !switches(victim) {
        responders.remove(0);
    }
    let switching: Vec<u32> = responders
        .iter()
        .copied()
        .filter(|&h| switches(h))
        .collect();
    if calls
        && impact.is_none()
        && let Some(Object::Unit(v)) = state.objects.get_mut(&victim)
    {
        v.call_until = (now + CALL_COOLDOWN).to_bits();
    }
    if calls {
        for &h in &reached {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.remembered = Some(attacker);
            }
        }
    }
    for h in responders {
        if switching.contains(&h) {
            switch_target(state, h, attacker);
            continue;
        }
        if order(state, h, Order::Attack { target: attacker }, 0, at) {
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            react(state, h, tick, delay, tick);
            // A creep that answers enters the fight (SPEC-0030).
            super::creeps::enter_fight(state, h, state.move_clock);
        }
    }
}

/// The cooldowns that expire in this tick for units attacking another
/// target than that of their last swing: each unit is tested with its
/// target's position now; in range of the attack and turned within 0.2
/// radians it swings at the expiry, else its approach starts again: it
/// stops and searches at its next update (SPEC-0019; c-RXR-1: footman 2
/// in 2629, footman 1 in 2689, grunt 10 in 4927; c-RXD-1: creep 8 at
/// 10.7699).
fn cooldown_expiries(state: &mut State) {
    let (start, end) = (
        real_of(state.move_clock),
        real_of(add(state.move_clock, crate::timers::TICK)),
    );
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let due: Vec<(u32, u32, i32)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state
                .unit(Some(h))
                .filter(|u| u.alive && u.attack.is_none())?;
            let m = u.movement?;
            let Order::Attack { target } = m.order else {
                return None;
            };
            let e = real_of(u.cooldown_end);
            (target != u.cooldown_target && e > start && e <= end)
                .then_some((h, target, m.order_id))
        })
        .collect();
    let tick = state.ticks;
    for (h, target, order_id) in due {
        let Some(at) = position_of(state, target).map(|p| relative(p, min)) else {
            continue;
        };
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        let Some(m) = u.movement else {
            continue;
        };
        let data = state.data.units.get(&u.type_id);
        let range =
            super::attack::weapon_for(state, u.type_id, Some(target)).map_or(0, |w| w.range);
        let other = collision_of(state, target);
        let reach = add(
            mul(range, CELL),
            mul(add(data.map_or(0, |t| t.collision), other), CELL),
        );
        let here = relative([u.x, u.y], min);
        let ready =
            in_range(here, at, reach) && real_of(heading_and_turn(&m, at).1).abs() < ATTACK_ANGLE;
        if ready {
            freeze(state, h);
            super::attack::begin(state, h, target, tick, order_id);
        } else {
            freeze(state, h);
            state.paths.remove(&h);
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(m) = u.movement.as_mut()
            {
                m.walking = false;
                m.velocity = [0, 0];
            }
        }
    }
}

/// 0.01 s, the delay of the checks of the attackers of a unit that died.
pub(super) const AFTER_DEATH: u32 = 0x3C23_D70A;

/// Unit `dead` died: the attack of each unit attacking it ends, and its
/// acquisition check timer is started again to expire 0.01 s after the
/// death, or after the end of its backswing if it is in one, in the order
/// in which they took it as their target, with no draw; those that took
/// it on their own stop (c-RXR-1: the attackers of footman 4 in 2360 and
/// of grunt 12 in 2579).
pub(super) fn target_died(state: &mut State, dead: u32) {
    let now = state.event_time.unwrap_or_else(|| state.timers.time());
    let mut attackers: Vec<(u64, u32, bool)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            let (target, order_id) = match (u.attack, u.movement) {
                (Some(a), _) => (a.target, a.order_id),
                (None, Some(m)) => match m.order {
                    Order::Attack { target } => (target, m.order_id),
                    _ => return None,
                },
                _ => return None,
            };
            (target == dead && u.acquire_phase != super::NO_PHASE)
                .then(|| (u.movement.map_or(0, |m| m.given), h, order_id == 0))
        })
        .collect();
    attackers.sort_unstable();
    for (_, h, own) in attackers {
        state.acquire_starts += 1;
        let seq = state.acquire_starts;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            let from = if f32::from_bits(u.backswing_end) > f32::from_bits(now) {
                u.backswing_end
            } else {
                now
            };
            u.next_acquire = add(from, AFTER_DEATH);
            // An attack-move goes on at once, or at the end of the
            // backswing (c-RXR-1: footman 2 searched towards its point at
            // its update of 2581).
            let left = real_of(from) - real_of(now);
            let at = state.ticks + 1 + (left / real_of(crate::timers::TICK)).ceil() as u64;
            if let Some(a) = u.attack_move.as_mut()
                && a.resume_at.is_none()
            {
                a.resume_at = Some(at);
            }
            // Below an attack it took on its own lies the walk back to its
            // guard point (c-RXR-1: grunt 14 at 4813). PROVISIONAL: the
            // 50 it accepts; creeps go home by SPEC-0030.
            if own && u.owner != super::creeps::CREEPS {
                u.guard_return = Some(at);
            }
            u.acquire_seq = seq;
            u.after_death = true;
            if own {
                u.attack = None;
            }
        }
        if own {
            halt(state, h);
        }
    }
}

/// The index of the group cluster (more than one member) of unit `h`.
pub(super) fn group_of(state: &State, h: u32) -> Option<usize> {
    state
        .clusters
        .iter()
        .position(|c| c.members.len() > 1 && c.members.contains(&h))
}

/// Where moving unit `h` holds its cells during the updates of this tick:
/// at its last update, but for a member of a group whose cluster has not
/// had all its updates yet, at its update before (SPEC-0021, "Occupied
/// cells").
pub(super) fn held_at(state: &State, h: u32, m: &Moving) -> [u32; 2] {
    match state.start_origins.get(&h) {
        Some(&before) if cluster_pending(state, h) => before,
        _ => m.origin,
    }
}

/// The velocity of moving unit `h` as the others see it during the
/// updates of this tick: its current one, but for a member of a group
/// whose cluster has not had all its updates yet, the one it had before
/// (SPEC-0021, "Occupied cells"; c-RTW-3: 918, which started at its
/// update of 5473, stood for 900's later in that tick).
pub(super) fn seen_velocity(state: &State, h: u32, m: &Moving) -> [u32; 2] {
    match state.start_velocities.get(&h) {
        Some(&before) if cluster_pending(state, h) => before,
        _ => m.velocity,
    }
}

/// Whether unit `h` is a member of a group cluster some of whose
/// members have their update of this tick still to come.
fn cluster_pending(state: &State, h: u32) -> bool {
    state
        .clusters
        .iter()
        .find(|c| c.members.len() > 1 && c.members.contains(&h))
        .is_some_and(|c| {
            c.members
                .iter()
                .any(|x| state.start_origins.contains_key(x) && !state.updated.contains(x))
        })
}
#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0018, "Order within an update tick": `U` against the clock of
    /// its tick (the sum of `k - 1` ticks) at the ticks the spec gives.
    #[test]
    fn update_time_against_the_clock() {
        let (mut u, mut clock) = (0, 0);
        let mut found = Vec::new();
        for k in 1..=2827u64 {
            if k > 1 {
                clock = add(clock, crate::timers::TICK);
            }
            if k >= FIRST_UPDATE && k % UPDATE == PHASE {
                u = if k == FIRST_UPDATE {
                    UPDATE_STEP
                } else {
                    add(u, UPDATE_STEP)
                };
                if [307, 943, 1891, 2827].contains(&k) {
                    found.push(f32::from_bits(u) - f32::from_bits(clock));
                }
            }
        }
        let expected = [0.000_000_5f32, 0.000_093_5, 0.000_533_1, 0.001_277_0];
        assert_eq!(found.len(), expected.len());
        for (f, e) in found.iter().zip(expected) {
            assert!((f - e).abs() < 2e-7, "{found:?}");
        }
    }
}
