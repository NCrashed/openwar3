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
//! PROVISIONAL: the arrival test compares squared distances in cells
//! (SPEC-0018, open question 1).

use std::cell::RefCell;
use std::collections::BTreeMap;
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

/// At an update tick, the time `U` of the update cycle for it (0.03 at
/// tick 7, then 0.03 more at every update tick), else `None`.
pub(super) fn update_time(state: &mut State) -> Option<u32> {
    let tick = state.ticks;
    if tick < FIRST_UPDATE || tick % UPDATE != PHASE {
        return None;
    }
    state.update_time = if tick == FIRST_UPDATE {
        UPDATE_STEP
    } else {
        add(state.update_time, UPDATE_STEP)
    };
    Some(state.update_time)
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
    /// Walk to a gold mine or tree to gather from it (SPEC-0029).
    Gather { target: u32 },
    /// Carry a load to a building that takes it.
    Return { building: u32 },
    /// Stand at a mine or tree and gather (or wait at the mine).
    Work { target: u32 },
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
    /// The tick the reaction delay counts from, the delay in seconds and
    /// the earliest tick of the update that may act: the unit acts on the
    /// order from its first update after both.
    reaction: Option<(u64, u32, u64)>,
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
    /// The number of the order, counted over the game: units are handled
    /// newest order first (SPEC-0021).
    given: u64,
}

impl Moving {
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
    m.heading = normal(h);
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
    let given = state.orders_given + 1;
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
            given,
            swing_gate: None,
            swing_waits: false,
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
            given,
            swing_gate: None,
            swing_waits: false,
        },
    };
    state.orders_given = given;
    // A new order takes the unit out of its group.
    for cluster in &mut state.clusters {
        cluster.members.retain(|&m| m != h);
    }
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
    let taken: Vec<u32> = units
        .iter()
        .copied()
        .filter(|&h| point_order_taken(state, h, name, target).0)
        .collect();
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
            state.clusters.push(Cluster {
                members,
                goal: target,
                first,
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
        if tick < cluster.first || (tick - cluster.first) % UPDATE != 0 {
            continue;
        }
        cluster
            .members
            .retain(|&h| state.unit(Some(h)).is_some_and(|u| u.alive));
        let (members, ordered) = cluster_members(state, &cluster.members, min);
        if members.len() < 2 {
            cluster.members.clear();
            continue;
        }
        if cluster.waypoints.is_empty() {
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
            let mut i = 0;
            cluster.members.retain(|_| {
                i += 1;
                !arrived[i - 1]
            });
            let mut i = 0;
            cluster.offsets.retain(|_| {
                i += 1;
                !arrived[i - 1]
            });
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
    for (&h, o) in cluster.members.iter().zip(&cluster.offsets) {
        let to = [add(w[0], o[0]), add(w[1], o[1])];
        let to = absolute(mul_both(to, CELLS_TO_WORLD), min);
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

/// A worker resumes building within this distance of a building's edge
/// (PROVISIONAL: the reach of a start, SPEC-0026, plus the building's
/// collision size).
fn resume_reach(state: &State, building: u32) -> f32 {
    let size = state
        .unit(Some(building))
        .and_then(|b| state.data.units.get(&b.type_id))
        .map_or(0.0, |t| real_of(t.collision));
    BUILD_REACH_SQUARED.sqrt() + size
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

/// Unit `h` stops where it is, its order over.
pub(super) fn stop(state: &mut State, h: u32) {
    halt(state, h);
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
    point_order_taken(state, h, name, target).1
}

/// A point order: whether the unit took it, and what the native returns.
fn point_order_taken(state: &mut State, h: u32, name: &str, target: [u32; 2]) -> (bool, bool) {
    // A building takes `setrally` and `smart` on the ground as its rally
    // point (SPEC-0027).
    if (name == "setrally" || name == "smart") && is_building(state, h) {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.rally = Some(target);
        }
        return (true, true);
    }
    let here = state.unit(Some(h)).map(|u| [u.x, u.y]);
    let (taken, done) = match (name, here) {
        ("move", _) => {
            let done = order(state, h, Order::Point, ORDER_MOVE, target);
            (done, done)
        }
        ("attack", _) => {
            let done = order(state, h, Order::Point, ORDER_ATTACK, target);
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
            next_check: state.ticks + ATTACK_MOVE_FIRST,
            resume_at: None,
        });
        set_attack_move(state, h, attack_move);
    }
    (taken, done)
}

/// `IssueTargetOrder(h, name, target)`: `attack` and `move`. A new order
/// ends an attack-move.
pub(super) fn target_order(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    let done = target_order_taken(state, h, name, target);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

/// `IssueImmediateOrder(h, name)`: `stop`, `holdposition` and `move`. A
/// new order ends an attack-move.
pub(super) fn immediate_order(state: &mut State, h: u32, name: &str) -> bool {
    let done = immediate_order_taken(state, h, name);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

fn target_order_taken(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    if name == "move" {
        // PROVISIONAL: the game takes it with the current order `move`
        // (SPEC-0019); how a unit follows its target is not specified, so
        // it walks to where the target is now.
        return match position_of(state, target) {
            Some(at) if target != h => order(state, h, Order::Point, ORDER_MOVE, at),
            _ => false,
        };
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
        // On an enemy an attack, on anything else a move to it.
        let then = if hostile(state, h, target) {
            "attack"
        } else {
            "move"
        };
        return target_order_taken(state, h, then, target);
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

fn immediate_order_taken(state: &mut State, h: u32, name: &str) -> bool {
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
const ORDER_IDS: [(&str, i32); 12] = [
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
];

/// `OrderId`: the id of `name` in any case, 0 for an unknown name.
fn order_id(name: &str) -> i32 {
    ORDER_IDS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map_or(0, |&(_, id)| id)
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
    let control = update_clusters(state, tick);
    let min = [state.world_bounds[0], state.world_bounds[1]];
    // Units updating in the same tick go by their orders, the newest first,
    // and the members of a group in the order of its cluster (SPEC-0021,
    // "Searches of many units in the same tick"; SPEC-0025).
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
        let range_cells = mul(range, CELL);
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
        let waiting = m.reaction.is_some_and(|(from, delay, earliest)| {
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
        if let Order::Gather { target } | Order::Return { building: target } = m.order
            && !waiting
            && super::harvest::reached(state, h, here, target)
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
        // work once near it.
        if let Order::Resume { building } = m.order
            && !waiting
        {
            let reach = resume_reach(state, building);
            let near = position_of(state, building).is_some_and(|at| {
                let at = relative(at, min);
                let (dx, dy) = (sub(at[0], here[0]), sub(at[1], here[1]));
                real_of(add(mul(dx, dx), mul(dy, dy))) <= reach * reach
            });
            if !unfinished_of_own(state, h, building) {
                m.arrived = true;
            } else if near {
                m.order = Order::Repair { building };
                m.arrived = false;
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
                        let gate_open = m.swing_gate.is_none_or(|from| tick >= from);
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
                                motion(&mut m, to, true, speed, limit, turn_rate, window);
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
                    let walker = |order| follow::Walker {
                        handle: h,
                        here,
                        order,
                        class,
                        squared_speed,
                        cluster,
                        chase: None,
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
        if let Some(target) = begin_attack {
            super::attack::begin(state, h, target, tick, m.order_id);
        }
    }

    state.marked.clear();
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
    let range = state
        .unit(Some(h))
        .and_then(|u| super::attack::weapon_for(state, u.type_id, Some(target)))
        .map_or(0, |w| w.range);
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
/// The first check of an attack-move after its order, or after a kill.
const ATTACK_MOVE_FIRST: u64 = 102;
const HUNDRED: u32 = 0x42C8_0000;
const HALF: u32 = 0x3F00_0000;

/// An `attack` order to a point (SPEC-0019, "Attack-move with enemies"):
/// the point, the tick of the next check for enemies, and after a kill
/// the tick at which the unit walks on (the end of its backswing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackMove {
    point: [u32; 2],
    next_check: u64,
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
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(a) = u.attack_move.as_mut()
    {
        a.resume_at = Some(tick + backswing);
        a.next_check = tick + ATTACK_MOVE_FIRST;
    }
}

/// Attack-moves going on after a kill, and ending at their point.
pub(super) fn attack_moves(state: &mut State) {
    let tick = state.ticks;
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
    /// An idle unit at its own schedule: the phase drawn at its creation
    /// and the tick its schedule counts from.
    Idle { phase: u32, start: u64 },
    /// A unit walking an attack-move.
    AttackMove,
}

/// Units with a weapon look for enemies (SPEC-0019): idle ones on their
/// own schedule, `floor(100 * x)` ticks after their creation and then
/// every 100 ticks, with `x` drawn when they were created ("Acquisition by
/// idle units"); units walking an attack-move every 100 ticks from 102
/// ticks after the order ("Attack-move with enemies"). Of the enemies
/// whose centres are within `acquire` and both collision sizes a unit
/// takes the nearest unit, a structure only when there is no unit, and on
/// equal distances the one with the smaller y, then x. An idle unit
/// attacks it on its own (`GetUnitCurrentOrder` 0) after the reaction
/// delay counted from the exact time of the check; an attack-move stops
/// at once and attacks after the reaction delay counted from the check's
/// tick, its order staying `attack`. Gives the units that acquired a
/// target, in that order.
pub(super) fn acquisitions(state: &mut State) -> Vec<u32> {
    let tick = state.ticks;
    let mut due: Vec<(u32, Check)> = Vec::new();
    for i in 0..state.units.len() {
        let h = state.units[i];
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        if !u.alive || u.paused {
            continue;
        }
        if let Some(a) = u.attack_move.as_mut() {
            if a.next_check == tick {
                a.next_check += ACQUIRE_PERIOD;
                let walking = u.attack.is_none()
                    && a.resume_at.is_none()
                    && u.movement
                        .is_some_and(|m| m.ordered() && m.order == Order::Point);
                if walking {
                    due.push((h, Check::AttackMove));
                }
            }
            continue;
        }
        if u.attack.is_some() || u.movement.is_some_and(|m| m.ordered()) {
            continue;
        }
        let Some(phase) = Some(u.acquire_phase).filter(|&x| x != super::NO_PHASE) else {
            continue;
        };
        // The checks fall in the tick `floor(100 * x)` after the creation,
        // and the delay counts from `100 * x` exactly (SPEC-0019,
        // verification 8).
        let start = u.born;
        let first = start + real_of(floor(mul(phase, HUNDRED))) as u64;
        if tick >= first && (tick - first) % ACQUIRE_PERIOD == 0 {
            due.push((h, Check::Idle { phase, start }));
        }
    }
    if due.is_empty() {
        return Vec::new();
    }
    let targets = Targets::new(state);
    let mut acquired = Vec::new();
    for (h, check) in due {
        let Some(target) = targets.nearest_enemy(state, h) else {
            continue;
        };
        let Some(Object::Unit(o)) = state.objects.get(&target) else {
            continue;
        };
        let at = [o.x, o.y];
        let (order_id, from, extra) = match check {
            Check::Idle { phase, start } => (0, start, mul(phase, HALF)),
            Check::AttackMove => (ORDER_ATTACK, tick, 0),
        };
        if order(state, h, Order::Attack { target }, order_id, at) {
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            react(state, h, from, add(delay, extra), from);
            acquired.push(h);
        }
    }
    acquired
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
        // (structure, squared distance, y, x, handle).
        let mut best: Option<(bool, f32, f32, f32, u32)> = None;
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
            let key = (structure, distance, oy, ox, other);
            if distance <= limit * limit
                && best.is_none_or(|b| (key.0, key.1, key.2, key.3) < (b.0, b.1, b.2, b.3))
            {
                best = Some(key);
            }
        }
        best.map(|b| b.4)
    }
}

/// How much farther than `CallForHelp` idle units hear a call for help
/// (SPEC-0019: about 175; 180 fits all the distances measured).
const CALL_REACH: f32 = 180.0;

/// The hit of `attacker` on `victim` (SPEC-0019, "Retaliation and call
/// for help"): an idle victim acquires the attacker whatever the distance,
/// and so do the idle units within `CallForHelp` and a little of it that
/// answer its call, in the order of their creation; all of them wait
/// their reaction delay from this tick, the victim drawing first.
///
/// PROVISIONAL: only units with a weapon answer, workers (`peon`) never;
/// the area is a circle round the victim's centre (its exact shape is
/// open); only hits of weapons call for help, not scripted damage.
pub(super) fn retaliate(state: &mut State, victim: u32, attacker: u32) {
    let tick = state.ticks;
    let (Some(v), Some(a)) = (state.unit(Some(victim)), state.unit(Some(attacker))) else {
        return;
    };
    let (owner, x, y, at) = (v.owner, real_of(v.x), real_of(v.y), [a.x, a.y]);
    if !enemies(state, owner, a.owner) {
        return;
    }
    let answers = |h: u32| match state.objects.get(&h) {
        Some(Object::Unit(u)) => {
            u.alive
                && !u.paused
                && u.attack.is_none()
                && u.attack_move.is_none()
                && u.movement.is_none_or(|m| !m.ordered())
                && state
                    .data
                    .units
                    .get(&u.type_id)
                    .is_some_and(|t| t.weapon.is_some() && !t.worker)
        }
        _ => false,
    };
    if !answers(victim) {
        return;
    }
    let reach = real_of(state.combat.call_for_help) + CALL_REACH;
    let mut responders = vec![victim];
    for &h in &state.units {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let (dx, dy) = (real_of(u.x) - x, real_of(u.y) - y);
        if h != victim
            && h != attacker
            && dx * dx + dy * dy <= reach * reach
            && state.setup.helps(u.owner, owner)
            && answers(h)
        {
            responders.push(h);
        }
    }
    for h in responders {
        if order(state, h, Order::Attack { target: attacker }, 0, at) {
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            react(state, h, tick, delay, tick);
        }
    }
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
