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
    state.last_update_tick = state.ticks;
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
/// The arrival radius of a walk back to an acquisition's point in cells
/// (1.5625, 50 world units).
const BACK_REACH: u32 = 0x3FC8_0000;
/// The arrival radius of a point move in cells (0.49, 15.68 world units).
const ARRIVAL: u32 = 0x3EFA_E148;
/// A unit attacks once its heading is this close to its target (radians).
const ATTACK_ANGLE: f32 = 0.2;
/// AttackHalfAngle: a target further off the facing than this is faced
/// before the swing (SPEC-0019).
const ATTACK_HALF_ANGLE: f32 = 0.5;
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
    /// Walk into the range of an ability on the order point to cast it
    /// (SPEC-0043).
    CastAt,
    /// Turn to an item on the ground, walking into its reach first, and
    /// pick it up (c-SBQ-1).
    PickUp { item: u32 },
    /// Walk within DropItemRange of the order point and drop the carried
    /// `item` there (c-SBR-1).
    Drop { item: u32 },
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
    pub(super) order: Order,
    /// The id `GetUnitCurrentOrder` gives.
    order_id: i32,
    /// The point walked to (for an attack, where the target stood at the
    /// last update).
    target: [u32; 2],
    /// The direction of motion, in radians.
    pub(super) heading: u32,
    /// The tick of the next update.
    pub(super) next_update: u64,
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
    /// The call of a missile's impact stopped it: it stands from the next
    /// tick, where it is then (SPEC-0019; c-RZJ-1: gnoll 13 at 1019).
    halt_next: bool,
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
    /// A chase goal taken at a backswing end: the cluster's first update
    /// compares it with the target by 2 x 2 squares (SPEC-0019).
    follow_check: bool,
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
    /// An attack taken by a check with its target in reach: no reach
    /// limit, it only turns to the target and arrives (SPEC-0019; c-SAK-1:
    /// the Mountain King on gnoll 16 at 3616).
    unlimited: bool,
    /// A move to a point that takes up an attack on this target when it
    /// is done (SPEC-0040, the end of a Wind Walk's fade).
    then_attack: Option<u32>,
    /// A creep's move to where its target died arrived: it stands and
    /// turns to this point (relative), then the order ends (SPEC-0030).
    face_point: Option<[u32; 2]>,
    /// A helper in reach of the building it turns to, its help to start
    /// when it faces it (hub #13).
    help: Option<u32>,
    /// Its walk was made again (an Earthquake's slow): after its next update
    /// its goal is the target's place then and its path is searched at the
    /// update after (hub #10).
    remade: bool,
    /// A drop order stood within DropItemRange at its last update: it
    /// drops at this one (c-SBR-1: stopped at 9.599, dropped at 9.629385).
    drop_ready: bool,
    /// The number of the order, counted over the game: units are handled
    /// newest order first (SPEC-0021).
    pub(super) given: u64,
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

pub(super) fn absolute(r: [u32; 2], min: [u32; 2]) -> [u32; 2] {
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
        Object::Item(i) if i.carried_by.is_none() => Some([i.x, i.y]),
        Object::Item(_) => None,
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
    // Any other order ends a hold position; an attack the unit took on its
    // own does not (c-SBI-1: grunt 4 holding while it fought).
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && !(attack && order_id == 0)
    {
        u.holding = false;
    }
    let Some(Object::Unit(u)) = state.objects.get(&h) else {
        return false;
    };
    let busy = u.movement.filter(|m| m.ordered());
    state.quiet_paths.remove(&h);
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
            follow_check: false,
            renew_at: None,
            renew_cooldown: false,
            chase: None,
            renew_stands: false,
            given,
            swing_gate: None,
            swing_waits: false,
            unlimited: false,
            then_attack: None,
            face_point: None,
            help: None,
            remade: false,
            drop_ready: false,
            ..m
        },
        None => Moving {
            order,
            order_id,
            target,
            // The stored heading of a unit whose facing still turns to it
            // (c-RXR-1: troll 16 kept 2.535097 through its attacks).
            heading: u.movement.map_or(u.facing, |m| m.heading),
            // At the next update batch: the batches drift against a fixed
            // phase (c-RXD-1: creep 7, an order at 7081, its first update
            // at 7082).
            next_update: tick + 1,
            walking: false,
            origin: relative([u.x, u.y], min),
            velocity: [0, 0],
            since: clock,
            facing_step: 0,
            arrived: false,
            restart: false,
            skip: false,
            stop_after_step: false,
            halt_next: false,
            reaction: None,
            reaction_end: None,
            goal_taken: false,
            faced: false,
            follow_check: false,
            renew_at: None,
            renew_cooldown: false,
            chase: None,
            renew_stands: false,
            given,
            swing_gate: None,
            swing_waits: false,
            aim: [0, 0],
            unlimited: false,
            then_attack: None,
            face_point: None,
            help: None,
            remade: false,
            drop_ready: false,
        },
    };
    state.orders_given = given;
    // The order that took this target, kept through re-orders on it
    // (c-RZH-1: footmen 1 and 3 took creep 7 at 1103, 1 first) and lost
    // with another order (the hero took creep 10 again at 5383).
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        match order {
            Order::Attack { target: t } => {
                if u.target_taken.is_none_or(|(old, _)| old != t) {
                    u.target_taken = Some((t, given));
                }
            }
            _ => u.target_taken = None,
        }
    }
    // A new order takes the unit out of its group.
    for cluster in &mut state.clusters {
        cluster.keep(|m| m != h);
    }
    // A new path is searched at the next update (SPEC-0020); in the new
    // layout of a queued group the members keep theirs until their places
    // are laid out (SPEC-0025, "When a member searches again").
    if !state.regrouping {
        state.drop_path(h);
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
        state.drop_path(h);
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
    // The worker walks to the site snapped to the building's grid (c-SBZ-1:
    // the farm ordered at (0, -400), walked to as (0, -384), hub #13).
    let facing = super::facing::from_degrees(270f32.to_bits());
    let (sx, sy) = super::obstacles::site(state, type_id, target[0], target[1], facing);
    let target = [sx, sy];
    if !order(state, h, Order::Build { type_id }, type_id, target) {
        return false;
    }
    super::table::build_ordered(state, h);
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
        state.drop_path(h);
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
    // Its type record counts it alive (`q4`) only from its completion
    // (c-SBZ-1: G at turns 29 and 258).
    // PROVISIONAL: `q2` up by one here, down at the completion, gives the
    // same sums; a building lost under construction would tell them apart.
    super::widgets::type_record(state, owner, type_id, |q| q[4] -= 1);
    super::table::construction_started(state, h, building);
    // It enters its regions where it stands (SPEC-0049).
    super::regions::test_unit(state, building);
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
    let delay = if moved {
        moved_delay(state, h, building)
    } else {
        0
    };
    let begin = state.ticks + delay;
    let mut construction = super::Construction::new(base, rate, max, build_time, begin);
    construction.builder = h;
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

/// The ticks from the start of a building to the update at which worker
/// `h`, moved off its footprint, starts working: as a helper starts
/// (hub #13), at the update that finds its heading within 0.2 of the
/// building's centre, turning by its turn rate at each update after the
/// first (c-SBZ-1: peasant 4 moved to (720, -528), 6 degrees off the
/// barracks' centre, at once; SPEC-0026: one 41 degrees away, 12 ticks).
///
/// PROVISIONAL: SPEC-0026's 18-tick cases are not checked against it.
fn moved_delay(state: &State, h: u32, building: u32) -> u64 {
    let (Some(u), Some(b)) = (state.unit(Some(h)), state.unit(Some(building))) else {
        return WORK_DELAY_MOVED;
    };
    let heading = u.movement.map_or(u.facing, |m| normal(m.heading));
    let to = angle(sub(b.x, u.x), sub(b.y, u.y));
    let rate = state.data.units.get(&u.type_id).map_or(PI, |t| t.turn_rate);
    let mut d = real_of(super::facing::angle_difference(to, heading)).abs();
    if d < ATTACK_ANGLE {
        return 0;
    }
    let mut ticks = 0;
    loop {
        ticks += 6;
        if d < ATTACK_ANGLE {
            return ticks;
        }
        d = if d <= real_of(rate) {
            0.0
        } else {
            real_of(sub(d.to_bits(), rate))
        };
    }
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
    // The ring search of a unit made on occupied ground (SPEC-0013): rings
    // round its cell, each from the bottom side's second cell, the first
    // free place (c-SBZ-1: peasant 4 to (720, -528) at the barracks, hub
    // #13).
    let mut found = None;
    'rings: for n in 1..=8 {
        let (l, r, b, t) = (cx - n, cx + n, cy - n, cy + n);
        let ring = ((l + 1)..=r)
            .map(|i| (i, b))
            .chain(((b + 1)..=t).map(|j| (r, j)))
            .chain((l..r).rev().map(|i| (i, t)))
            .chain((b..t).rev().map(|j| (l, j)));
        for (i, j) in ring {
            let p = [i, j].map(|c| add(i2r(c), 0x3F00_0000));
            if free(p) {
                found = Some(p);
                break 'rings;
            }
        }
    }
    let Some(p) = found else {
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
    if holds_off(state, h, target) {
        return false;
    }
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
pub(super) const CALL_CELL: f32 = 256.0;

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
        super::table::game_order(state, h);
    }
    done
}

/// Unit `h`'s cast ends (SPEC-0035): it is left with no order (an
/// attack-move before it is not taken up again), and its acquisition check
/// comes 0.01 s after its queue time `now` (c-RZH-1: the Mountain King's
/// Storm Bolt ending at 991, its check taking creep 7 at 993).
pub(super) fn end_cast_at(state: &mut State, h: u32, now: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack_move = None;
        if u.acquire_phase != super::NO_PHASE {
            u.next_acquire = add(now, AFTER_DEATH);
        }
    }
    halt(state, h);
}

/// Unit `h` stops where it is, its order over.
pub(super) fn stop(state: &mut State, h: u32) {
    halt(state, h);
}

/// Whether unit `h`'s stored heading (its facing without a movement) is
/// within 0.2 of the direction to point `at` (SPEC-0035).
pub(super) fn turned_to(state: &State, h: u32, at: [u32; 2]) -> bool {
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let (here, there) = (relative([u.x, u.y], min), relative(at, min));
    let a = angle(sub(there[0], here[0]), sub(there[1], here[1]));
    let heading = u.movement.map_or(u.facing, |m| m.heading);
    real_of(super::facing::angle_difference(a, heading)).abs() < ATTACK_ANGLE
}

/// Unit `h` walks into the range of its cast on `target` (SPEC-0035).
pub(super) fn cast_order(state: &mut State, h: u32, target: u32, order_id: i32) -> bool {
    match position_of(state, target) {
        Some(at) => order(state, h, Order::Cast { target }, order_id, at),
        None => false,
    }
}

/// Unit `h` walks into range of its cast on point `at` (SPEC-0043).
pub(super) fn cast_point_order(state: &mut State, h: u32, order_id: i32, at: [u32; 2]) -> bool {
    order(state, h, Order::CastAt, order_id, at)
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

/// Takes unit `h` out of the stop of its target checks for an order
/// (SPEC-0045): whether they were stopped.
fn take_checks_off(state: &mut State, h: u32) -> bool {
    match state.objects.get_mut(&h) {
        Some(Object::Unit(u)) => std::mem::take(&mut u.checks_off),
        _ => false,
    }
}

/// After an order of unit `h` whose target checks were stopped (`off`):
/// a taken order restarts them with 0.01 s, unless it stopped them again;
/// one not taken leaves them stopped (SPEC-0045; c-RZJ-1: the Paladin's
/// attack-move at 4901).
fn resume_checks(state: &mut State, h: u32, off: bool, taken: bool) {
    let now = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && off
    {
        if !taken {
            u.checks_off = true;
        } else if !u.checks_off && u.acquire_phase != super::NO_PHASE {
            u.next_acquire = add(now, AFTER_DEATH);
        }
    }
}

/// `IssuePointOrder(h, name, target)`: what the native returns.
pub(super) fn point_order(state: &mut State, h: u32, name: &str, target: [u32; 2]) -> bool {
    // An order without Shift empties the queue (SPEC-0031).
    super::queue::clear(state, h);
    let off = take_checks_off(state, h);
    let (taken, done) = point_order_taken(state, h, name, target, true);
    resume_checks(state, h, off, taken);
    done
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
    // A point spell (SPEC-0043).
    if let Some(taken) = super::cast::point_order(state, h, name, target) {
        // A new order ends an attack-move.
        if taken {
            set_attack_move(state, h, None);
        }
        return (taken, taken);
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
        // time 0.01 s later, then every 0.5 s (SPEC-0019, "Attack-move
        // with enemies"; c-RZH-1: the checks of 303 and of 1003).
        let attack_move = (name == "attack").then(|| AttackMove {
            point: target,
            next_check: add(state.timers.time(), AFTER_DEATH),
            resume_at: None,
            resume_q: None,
            periodic: None,
        });
        set_attack_move(state, h, attack_move);
    }
    (taken, done)
}

/// `IssueTargetOrder(h, name, target)`: `attack` and `move`. A new order
/// ends an attack-move.
pub(super) fn target_order(state: &mut State, h: u32, name: &str, target: u32) -> bool {
    super::queue::clear(state, h);
    let off = take_checks_off(state, h);
    let done = target_order_taken(state, h, name, target);
    resume_checks(state, h, off, done);
    if done {
        set_attack_move(state, h, None);
    }
    done
}

/// `IssueImmediateOrder(h, name)`: `stop`, `holdposition` and `move`. A
/// new order ends an attack-move.
pub(super) fn immediate_order(state: &mut State, h: u32, name: &str) -> bool {
    super::queue::clear(state, h);
    let off = take_checks_off(state, h);
    let done = immediate_order_taken(state, h, name);
    resume_checks(state, h, off, done);
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
        let taken = match position_of(state, target) {
            Some(at) => order(
                state,
                h,
                Order::Resume { building: target },
                ORDER_REPAIR,
                at,
            ),
            None => false,
        };
        // Its objects (c-SBZ-1: peasant 1 at 7.499695, hub #13).
        if taken {
            super::table::repair_ordered(state, h);
        }
        return taken;
    }
    // A worker on a gold mine or tree gathers from it; with a load on a
    // building that takes it, it delivers there (SPEC-0029).
    if matches!(name, "smart" | "harvest") && super::harvest::can_harvest(state, h, target) {
        return super::harvest::order_harvest(state, h, target);
    }
    if name == "smart" && super::harvest::can_deliver(state, h, target) {
        return super::harvest::order_deliver(state, h, target);
    }
    // A hero on an item on the ground picks it up (c-SBQ-1).
    if name == "smart" && can_pick_up(state, h, target) {
        return pick_up_order(state, h, target);
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
            // The order starts the reaction timer, on the timer queue from
            // the order, and is carried out at its end: a chase walk is made
            // there (c-SBL-1: the hero at 8.5, its walk at 8.675406). A
            // timer already running (an acquisition's) is started again
            // (c-SBL-1: the Mountain King at 2.0, its walk at 2.062619).
            let timer = state
                .unit(Some(h))
                .and_then(|u| u.movement)
                .is_some_and(|m| m.reaction_end.is_some());
            // A script's order takes its target anew, after those that
            // acquired it since (c-SBM-1: at kobold 5's death the Far Seer,
            // which acquired it after the footman, is handled first).
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.target_taken = None;
            }
            let done = order(state, h, Order::Attack { target }, ORDER_ATTACK, at);
            if done {
                let now = state.event_time.unwrap_or_else(|| state.timers.time());
                // For a unit let go from its pause in this tick the order starts
                // 0.01 s later, and its delay with it (c-SBQ-1: the ogre at 2.0,
                // its reaction part freed at 2.254992; `table::unpause_starts`).
                let unpaused = state
                    .unit(Some(h))
                    .is_some_and(|u| u.unpaused_at == Some(state.ticks));
                let now = if unpaused { add(now, AFTER_DEATH) } else { now };
                react_until(state, h, add(now, delay));
                // Without a timer running before, it swings one update after
                // its first update past the end at the earliest (SPEC-0019,
                // "When it starts").
                if !timer
                    && !unpaused
                    && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
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
    // At an impact, which comes at the end of the tick, it stops at its
    // per-tick position of the impact's tick (SPEC-0018; c-SAK-1: ogre 18
    // at the bolt of 22.562214).
    let clock = match state.event_time {
        Some(at) if state.impact_now => at,
        _ => state.move_clock,
    };
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
        "stop" | "holdposition" => {
            // (The building a builder leaves, for its objects.)
            state.building_left =
                state
                    .unit(Some(h))
                    .and_then(|u| u.movement)
                    .and_then(|m| match m.order {
                        Order::Repair { building } => Some(building),
                        _ => None,
                    });
            let done = halt(state, h);
            if done && let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.holding = name == "holdposition";
            }
            // Its building's growth stops at the order (c-SBZ-1: the farm
            // at 422.45 from 19.4977 s, hub #13).
            let (now, clock) = (state.timers.time(), state.move_clock);
            if done
                && let Some(b) = state.building_left
                && let Some(Object::Unit(u)) = state.objects.get_mut(&b)
                && let Some(c) = u.construction.as_mut()
                && c.builder == h
                && c.gone.is_none()
            {
                c.gone = Some(now);
                c.base = c.grown(clock);
                c.tb = Some(clock);
                c.factor = 0;
                c.rate = 0;
                super::training::cancel_timer(state, super::training::EngineTimer::Construction(b));
            }
            // `stop` takes the same objects (c-RTW-3: a player's stop at
            // 5.024874).
            if done {
                super::table::hold_ordered(state, h);
            }
            done
        }
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

/// Unit `h` takes its attack anew inside a timer of the world clock's
/// queue that expires at `clock`: it stops where it is then, and searches
/// towards its target's position then at its update (SPEC-0040; c-SAK-1:
/// the Blademaster at 4621).
pub(super) fn regive_attack(state: &mut State, h: u32, clock: u32) {
    let Some((target, order_id)) =
        state
            .unit(Some(h))
            .and_then(|u| u.movement)
            .and_then(|m| match m.order {
                Order::Attack { target } => Some((target, m.order_id)),
                _ => None,
            })
    else {
        return;
    };
    let (tick, min) = (state.ticks, [state.world_bounds[0], state.world_bounds[1]]);
    let Some(at) = (match state.unit(Some(target)).and_then(|u| u.movement) {
        Some(t) => Some(absolute(t.position(clock), min)),
        None => position_of(state, target),
    }) else {
        return;
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
        && m.walking
    {
        m.commit(clock);
        m.velocity = [0, 0];
        m.walking = false;
        let p = absolute(m.origin, min);
        (u.x, u.y) = (p[0], p[1]);
        let record = (tick, h, m.origin, m.velocity, m.heading, [0, 0]);
        if let Some(trace) = state.agent_trace.as_mut() {
            trace.push(record);
        }
    }
    // Taken up as a move to that point, the attack following when it is
    // done (SPEC-0040; c-SAK-1: a plain search at 4621, no chase).
    let taken = state.unit(Some(h)).and_then(|u| u.target_taken);
    if order(state, h, Order::Point, order_id, at)
        && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
    {
        u.target_taken = taken;
        if let Some(m) = u.movement.as_mut() {
            m.then_attack = Some(target);
        }
    }
}

/// A new point order stops a walking unit at once; it starts again as
/// from standing at its next update (SPEC-0019, "Stop, hold position and
/// new orders while walking").
/// [`freeze`] for other modules.
pub(super) fn freeze_now(state: &mut State, h: u32) {
    freeze(state, h);
}

fn freeze(state: &mut State, h: u32) {
    let (clock, later) = (state.move_clock, state.player_orders);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
        && m.walking
    {
        // The call of an impact, which comes at the end of the tick: the
        // unit moves this tick and stands from the next.
        if state.impact_now {
            m.halt_next = true;
            return;
        }
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

/// Walking unit `h` stops where it is at queue time `at`.
fn stop_at(state: &mut State, h: u32, at: u32) {
    let tick = state.ticks;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
        && m.walking
    {
        m.commit(at);
        m.velocity = [0, 0];
        m.walking = false;
        let at = absolute(m.origin, min);
        (u.x, u.y) = (at[0], at[1]);
        let record = (tick, h, m.origin, m.velocity, m.heading, [0, 0]);
        if let Some(trace) = state.agent_trace.as_mut() {
            trace.push(record);
        }
    }
}

/// Unit `h` stops where it is with a commit, walking or not (a creep
/// giving up its chase, SPEC-0030; c-RXD-1: creeps 6 and 7 at 7788).
pub(super) fn stop_commit(state: &mut State, h: u32) {
    let clock = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
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
pub(super) fn stand_still(state: &mut State, h: u32, at: Option<u32>) {
    let clock = at.unwrap_or(state.move_clock);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        m.commit(clock);
        m.velocity = [0, 0];
        m.walking = false;
    }
}

/// Speed `limit` of unit `h` under a thunder clap's slow:
/// `max(limit x (1 - DataC), MinUnitSpeed)` (SPEC-0037).
fn slowed_speed(state: &State, h: u32, limit: u32) -> u32 {
    match state.unit(Some(h)).and_then(|u| u.move_slow) {
        Some(c) => {
            let cut = mul(limit, sub(0x3F80_0000, c));
            let least = state.combat.min_unit_speed;
            if real_of(cut) < real_of(least) {
                least
            } else {
                cut
            }
        }
        None => limit,
    }
}

/// Walking unit `h` was just slowed: its agent is committed at its
/// per-tick position and goes on at the slowed speed at once (SPEC-0037;
/// c-SBG-1: ogre 3 at 2.899976, 270 -> 150).
pub(super) fn slowed_now(state: &mut State, h: u32) {
    let clock = state.move_clock;
    let tick = state.ticks;
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let base = state.data.units.get(&u.type_id).map_or(0, |t| t.speed);
    if real_of(base) <= 0.0 {
        return;
    }
    let k = div(slowed_speed(state, h, base), base);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
        && m.walking
    {
        m.commit(clock);
        m.velocity = [mul(m.velocity[0], k), mul(m.velocity[1], k)];
        let record = (tick, h, m.origin, m.velocity, m.heading, m.aim);
        if let Some(trace) = state.agent_trace.as_mut() {
            trace.push(record);
        }
    }
}

/// Chasing unit `h`'s walk was made again (an Earthquake's slow): its goal
/// follows its target by the 64 grid rule at once (c-SBW-1: the ogre's
/// taken as the Blademaster at 3.799953, in reach at its update of 3.81,
/// walk freed at 3.839952). At its next update it steers on, its goal set
/// after it to the target's place then, and its path is searched at the
/// update after that (kobold 5 at 3.81 and 3.84, hub #10).
///
/// PROVISIONAL: a standing unit's remade walk (kobold 4) is not done.
pub(super) fn chase_renewed(state: &mut State, h: u32) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let here = relative([u.x, u.y], min);
    let Some(Order::Attack { target }) = u.movement.filter(|m| m.walking).map(|m| m.order) else {
        return;
    };
    let Some(at) = position_of(state, target) else {
        return;
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        if grid64(relative(at, min)) != grid64(relative(m.target, min)) {
            m.target = at;
            m.chase = Some(countdown(here, relative(at, min)));
        }
        m.remade = true;
    }
}

/// The earliest timed reaction end in this tick, if any.
pub(super) fn next_reaction_end(state: &State) -> Option<u32> {
    let limit = add(state.move_clock, crate::timers::TICK);
    state
        .moving
        .iter()
        .filter_map(|&h| state.unit(Some(h)).and_then(|u| u.movement?.reaction_end))
        .filter(|&e| real_of(e) <= real_of(limit))
        .min_by(|a, b| real_of(*a).total_cmp(&real_of(*b)))
}

/// Whether unit `h` walks a move order now.
pub(super) fn on_move(state: &State, h: u32) -> bool {
    state
        .unit(Some(h))
        .and_then(|u| u.movement)
        .is_some_and(|m| m.ordered() && m.order == Order::Point)
}

/// Unit `target` became a target again (its script invulnerability ended):
/// the idle enemies that would take it take it at once, as at a check
/// (c-SBL-1: the Mountain King on hold position, the kobold 150 away in
/// his reach, at 2.0 s, before the script's attack order). The units that
/// acquired, for their events and objects (`widgets::acquired`).
///
/// PROVISIONAL: read for one unit on hold position.
pub(super) fn acquire_vulnerable(state: &mut State, target: u32) -> Vec<u32> {
    let Some(t) = state.unit(Some(target)) else {
        return Vec::new();
    };
    let (t_owner, at) = (t.owner, [t.x, t.y]);
    let t_collision = state
        .data
        .units
        .get(&t.type_id)
        .map_or(0.0, |d| real_of(d.collision));
    let now = state.event_time.unwrap_or_else(|| state.timers.time());
    let units: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| {
            h != target
                && state.unit(Some(h)).is_some_and(|u| {
                    let acquire = state
                        .data
                        .units
                        .get(&u.type_id)
                        .map_or(0.0, |d| real_of(d.acquire));
                    // A creep takes the notice as a call: within CreepCallForHelp
                    // (c-SBV-1: kobold 4, 570.6 from the Mountain King, at the
                    // end of his Avatar's first second, hub #9).
                    let acquire = if u.owner == super::creeps::CREEPS {
                        real_of(state.combat.creep_call_for_help)
                    } else {
                        acquire
                    };
                    // Its acquisition range and both collision sizes (c-SBO-1: the
                    // Warden, 650, took the ogre 716 away; c-SBL-1: the Mountain
                    // King, 500, did not).
                    let own = state
                        .data
                        .units
                        .get(&u.type_id)
                        .map_or(0.0, |d| real_of(d.collision));
                    let acquire = acquire + own + t_collision;
                    let (dx, dy) = (real_of(u.x) - real_of(at[0]), real_of(u.y) - real_of(at[1]));
                    u.alive
                        && !u.attack_removed
                        && !u.paused
                        && !u.checks_off
                        && u.attack.is_none()
                        && u.movement.is_none_or(|m| !m.ordered())
                        // Not one in its backswing (c-SBO-1: the Warden after
                        // his kill of 10.08, at 10.5).
                        && f32::from_bits(u.backswing_end) <= f32::from_bits(now)
                        && enemies(state, u.owner, t_owner)
                        && dx * dx + dy * dy <= acquire * acquire
                })
                && !super::cast::stunned(state, h)
                && !holds_off(state, h, target)
        })
        .collect();
    // In a call's order (SPEC-0019; c-SBM-1 at 6.5: the footman before
    // the Far Seer).
    let units = call_order(state, units);
    let now = state.event_time.unwrap_or_else(|| state.timers.time());
    let mut acquired = Vec::new();
    for h in units {
        // Not one a creep's call took already.
        if acquired.contains(&h) {
            continue;
        }
        freeze(state, h);
        if order(state, h, Order::Attack { target }, 0, at) {
            if reaches_at_commit(state, h, target)
                && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(m) = u.movement.as_mut()
            {
                m.unlimited = true;
            }
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            react_until(state, h, add(now, delay));
            acquired.push(h);
            // A creep calls its camp (c-SBV-1: kobold 4 at the end of the Mountain
            // King's first Avatar second, then the others, hub #9).
            if !acquired[..acquired.len() - 1].contains(&h) {
                acquired.extend(super::creeps::call_for_help(state, h, target, Some(now)));
            }
        }
    }
    acquired
}

/// Whether unit `h` holds its position with `target` out of its reach
/// (SPEC-0019): it takes no such target, at a check or at a call (c-SBG-1).
fn holds_off(state: &State, h: u32, target: u32) -> bool {
    state.unit(Some(h)).is_some_and(|u| u.holding) && !reaches(state, h, target)
}

/// `stop` and `holdposition`: the unit stands where it is now.
fn halt(state: &mut State, h: u32) -> bool {
    state.drop_path(h);
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
            let mut s = state.borrow_mut();
            // A hero away in its Mirror Image takes the order and does
            // nothing with it (c-SAK-1: the Blademaster's attack-move of
            // 36.0 accepted, his order staying 0, no objects).
            // PROVISIONAL: read for an attack-move only.
            if s.unit(Some(h)).is_some_and(|u| u.hidden && u.alive) && name == "attack" {
                return Ok(Some(Value::Boolean(true)));
            }
            let done = point_order(&mut s, h, name, target);
            // Its objects of the engine's table (SPEC-0024).
            if done && name == "attack" {
                super::table::attack_point_ordered(&mut s, h);
            }
            if done && name == "move" {
                super::table::move_point_ordered(&mut s, h);
            }

            drop(s);
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
            // (Whether it had an order before, for the order's record.)
            let idle = state.borrow().unit(Some(h)).is_some_and(|u| {
                !u.holding && u.movement.is_none() && u.casting.is_none() && u.attack.is_none()
            });
            let done = target_order(&mut state.borrow_mut(), h, name, target);
            if done && name == "attack" {
                super::table::attack_target_ordered(&mut state.borrow_mut(), h, idle);
            }
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
            // An effect in the order's own tick runs at once, before the
            // order's 0.0001 s frees (c-SBK-1: Fan of Knives at 4.499912).
            if done && let Some(h) = handle(&args[0]) {
                super::cast::effect_at_order(vm, state, h);
            }
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
        state.last_move_clock = state.move_clock;
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
            // A stop due after an impact's call is spent all the same, its
            // commit made (c-SAK-1: ogre 18 at 4514, stopped by its order).
            if std::mem::take(&mut m.halt_next) {
                m.walking = false;
                let record = (state.ticks, *h, m.origin, m.velocity, m.heading, [0, 0]);
                if let Some(trace) = state.agent_trace.as_mut() {
                    trace.push(record);
                }
            }
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
        if std::mem::take(&mut m.halt_next) {
            m.commit(clock);
            m.velocity = [0, 0];
            m.walking = false;
            m.restart = false;
            let record = (state.ticks, *h, m.origin, m.velocity, m.heading, [0, 0]);
            if let Some(trace) = state.agent_trace.as_mut() {
                trace.push(record);
            }
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
    state.renewed_now.clear();
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
    if state.update_now {
        state.updates_started = state.ticks;
    }
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
    let mut attacks_after = Vec::new();
    let mut faced_ends = Vec::new();
    let mut help_starts: Vec<(u32, u32)> = Vec::new();
    // Units whose remade walk is searched at their next update.
    let mut remade: Vec<(u32, [u32; 2])> = Vec::new();
    for h in moving {
        let (type_id, alive) = match state.objects.get(&h) {
            Some(Object::Unit(u)) => (u.type_id, u.alive),
            _ => continue,
        };
        let data = state.data.units.get(&type_id);
        // A member of a group may be held or slowed (SPEC-0025); its own
        // limit stays.
        let limit = data.map_or(0, |t| t.speed);
        // Wind Walk (SPEC-0040).
        let limit = super::cast::wind_walk_speed(state, h).map_or(limit, |k| mul(limit, k));
        // Endurance and unholy aura (SPEC-0038).
        let limit = match state.unit(Some(h)) {
            Some(u) if !u.buffs.is_empty() => mul(limit, super::auras::speed_factor(state, u)),
            _ => limit,
        };
        // The pace of the gold cycle (SPEC-0029).
        let limit = super::harvest::pace_factor(state, h).map_or(limit, |k| mul(limit, k));
        // A thunder clap's slow (SPEC-0037).
        let limit = slowed_speed(state, h, limit);
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
        // The stored heading at the start of the update (SPEC-0024 below).
        let start_heading = m.heading;
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
        // The regions are tested at the commit (SPEC-0049).
        super::regions::committed(state, h, m.origin);
        // The walk objects it gave up are freed at its update (SPEC-0024).
        super::table::unit_updated(state, h);
        super::table::follow_leg_ended(state, h);
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
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
        // A move to a point of a Wind Walk's fade, or a creep's move to
        // where its target died, ends within 181 of the point: the unit
        // stands, and the next entry of its queue comes (SPEC-0040,
        // SPEC-0030; c-SAK-1: the Blademaster at 4801, 182.8 away at
        // 4795, then his attack-move of 4601; ogre 18 at 5173, 178.9 away,
        // 183.6 at 5167).
        let queued_move = u.creep.queued_home && u.checks_off && m.going_back();
        if (m.then_attack.is_some() || queued_move) && m.order == Order::Point {
            let p = absolute(m.origin, [state.world_bounds[0], state.world_bounds[1]]);
            let (dx, dy) = (sub(p[0], m.target[0]), sub(p[1], m.target[1]));
            if real_of(add(mul(dx, dx), mul(dy, dy))) <= MOVE_TO_TARGET * MOVE_TO_TARGET {
                m.velocity = [0, 0];
                m.walking = false;
                let then = m.then_attack.take();
                if queued_move {
                    // The creep turns to the point first (c-SAK-1: ogre
                    // 18 at 5173 and 5179, the order ending at 5185).
                    m.face_point = Some(relative(
                        m.target,
                        [state.world_bounds[0], state.world_bounds[1]],
                    ));
                } else {
                    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                        u.movement = Some(m);
                    }
                    if let Some(trace) = state.agent_trace.as_mut() {
                        trace.push((tick, h, m.origin, m.velocity, m.heading, [0, 0]));
                    }
                    state.updated.insert(h);
                    still.push(h);
                    if let Some(target) = then {
                        attacks_after.push((h, target, m.order_id));
                    }
                    continue;
                }
            }
        }
        // A helper in reach turning to its building: its help starts at the
        // update that finds its heading within 0.2 of the building's centre,
        // else it turns on by its turn rate (c-SBZ-1: peasant 2 at
        // 14.758524, hub #13).
        if let Some(building) = m.help {
            m.walking = false;
            m.velocity = [0, 0];
            let min = [state.world_bounds[0], state.world_bounds[1]];
            if let Some(at) = position_of(state, building) {
                let (heading, d) = heading_and_turn(&m, relative(at, min));
                if real_of(d).abs() < ATTACK_ANGLE {
                    m.help = None;
                    m.order = Order::Repair { building };
                    help_starts.push((h, building));
                } else {
                    m.heading = wrapped(if real_of(d).abs() <= real_of(turn_rate) {
                        add(heading, d)
                    } else if real_of(d) > 0.0 {
                        add(heading, turn_rate)
                    } else {
                        add(heading, turn_rate ^ SIGN)
                    });
                }
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.movement = Some(m);
            }
            state.updated.insert(h);
            still.push(h);
            continue;
        }
        // Turning to the point it arrived at, standing, its aim kept; the
        // order ends at the update that finds it facing the point, and its
        // checks start again 0.01 s later (SPEC-0030; c-SAK-1: ogre 18 at
        // 5185, its check of 5187).
        if let Some(p) = m.face_point {
            let (heading, d) = heading_and_turn(&m, p);
            if d == 0 {
                m.face_point = None;
                m.arrived = true;
                m.aim = [0, 0];
                faced_ends.push(h);
            } else {
                m.heading = wrapped(if real_of(d).abs() <= real_of(turn_rate) {
                    add(heading, d)
                } else if real_of(d) > 0.0 {
                    add(heading, turn_rate)
                } else {
                    add(heading, turn_rate ^ SIGN)
                });
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.movement = Some(m);
            }
            if let Some(trace) = state.agent_trace.as_mut() {
                trace.push((tick, h, m.origin, m.velocity, m.heading, m.aim));
            }
            state.updated.insert(h);
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
            } else if repair_reach(state, h, here, building) {
                // In reach its help starts if its heading is within 0.2 of the
                // building's centre (c-SBZ-1: peasant 1 at 8.489576), else it
                // stops and turns first (`help`; peasant 2 from 14.698534, hub
                // #13).
                m.arrived = false;
                m.walking = false;
                m.velocity = [0, 0];
                let at = position_of(state, building).map(|p| relative(p, min));
                let (heading, d) = at.map_or((m.heading, 0), |p| heading_and_turn(&m, p));
                if real_of(d).abs() < ATTACK_ANGLE {
                    m.order = Order::Repair { building };
                    super::table::chase_walk_ended(state, h);
                    let others = workers_of(state, building).into_iter().any(|w| w != h);
                    if others {
                        super::table::helper_started(state, h, building);
                    }
                    face_target(state, &mut m, building, min);
                } else {
                    // Its update ends with the turn (c-SBZ-1: peasant 3 in
                    // reach at 2959, started at 2971).
                    m.help = Some(building);
                    m.heading = wrapped(if real_of(d) > 0.0 {
                        add(heading, turn_rate)
                    } else {
                        add(heading, turn_rate ^ SIGN)
                    });
                    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                        u.movement = Some(m);
                    }
                    state.updated.insert(h);
                    still.push(h);
                    continue;
                }
            }
        }
        // A caster walks to its target and starts its cast at the update at
        // which it is in range (SPEC-0035).
        // PROVISIONAL: the effect came 99 ticks after the last position
        // change, taken as a start one tick before this update.
        let mut cast_turn = false;
        if let Order::Cast { target } = m.order
            && !waiting
            && !m.arrived
        {
            match position_of(state, target).filter(|_| super::attack::standing(state, target)) {
                Some(at) => {
                    let reach = super::cast::reach(state, h, target).unwrap_or(0);
                    if in_range(here, relative(at, min), reach) {
                        // In range it turns to its place, the target where
                        // it was at the order, and the cast starts when it
                        // is turned within 0.2 (SPEC-0035; c-RZH-1: the
                        // Storm Bolt ordered at 801, turned at 805, started
                        // at 811).
                        let place = relative(m.target, min);
                        let walked = m.walking;
                        if real_of(heading_and_turn(&m, place).1).abs() < ATTACK_ANGLE {
                            m.arrived = true;
                            m.walking = false;
                            m.velocity = [0, 0];
                            let from = if walked { tick.saturating_sub(1) } else { tick };
                            super::cast::start(state, h, from);
                        } else {
                            motion(&mut m, place, false, speed, limit, turn_rate, window);
                            cast_turn = true;
                            // Turning in place it has no goal (c-RZJ-1: the
                            // Paladin's Holy Light from 7808).
                            m.aim = [0, 0];
                        }
                    } else {
                        m.target = at;
                    }
                }
                None => m.arrived = true,
            }
        }
        // A pick-up: in reach of the item it turns to it, and picks it up at
        // the update that starts within 0.2 of it, as an arrival (c-SBQ-1:
        // the Archmage turning 0.5 an update, the claws picked up at
        // 3.119970 with 0.1476 left).
        if let Order::PickUp { item } = m.order
            && !waiting
            && !m.arrived
        {
            match position_of(state, item) {
                Some(at) => {
                    let own = state
                        .unit(Some(h))
                        .and_then(|u| state.data.units.get(&u.type_id))
                        .map_or(0.0, |t| real_of(t.collision));
                    let reach = ((PICKUP_RANGE + own) / 32.0).to_bits();
                    if in_range(here, relative(at, min), reach) {
                        let place = relative(m.target, min);
                        if real_of(heading_and_turn(&m, place).1).abs() < ATTACK_ANGLE {
                            m.arrived = true;
                            m.walking = false;
                            m.velocity = [0, 0];
                            state.picked_up.push((h, item));
                        } else {
                            motion(&mut m, place, false, speed, limit, turn_rate, window);
                            cast_turn = true;
                            m.aim = [0, 0];
                        }
                    } else {
                        m.target = at;
                    }
                }
                None => m.arrived = true,
            }
        }
        // A drop: within DropItemRange of its point the item is dropped there
        // (c-SBR-1: 91.2 away at 9.629385).
        // Strictly within it: the order's point exactly 100 away at the order
        // was not; the hero stops at the update that finds it within, and
        // drops at the next.
        if let Order::Drop { item } = m.order
            && !waiting
            && !m.arrived
        {
            let at = relative(m.target, min);
            let (dx, dy) = (
                mul(sub(at[0], here[0]), CELL),
                mul(sub(at[1], here[1]), CELL),
            );
            let r = DROP_RANGE / 32.0;
            if real_of(add(mul(dx, dx), mul(dy, dy))) < r * r {
                if std::mem::take(&mut m.drop_ready) {
                    m.arrived = true;
                    state.dropped.push((h, item, m.target));
                } else {
                    m.drop_ready = true;
                }
                // It goes on turning to the point (c-SBR-1: facing it, 2.3
                // degrees off, at the pick-up after).
                motion(&mut m, at, false, speed, limit, turn_rate, window);
                m.walking = false;
                m.velocity = [0, 0];
                cast_turn = true;
            }
        }
        // The same for a cast on a point: its place is the order point.
        if m.order == Order::CastAt && !waiting && !m.arrived {
            let place = relative(m.target, min);
            let reach = super::cast::point_reach(state, h);
            if in_range(here, place, reach) {
                let walked = m.walking;
                if real_of(heading_and_turn(&m, place).1).abs() < ATTACK_ANGLE {
                    m.arrived = true;
                    m.walking = false;
                    m.velocity = [0, 0];
                    let from = if walked { tick.saturating_sub(1) } else { tick };
                    super::cast::start(state, h, from);
                } else {
                    motion(&mut m, place, false, speed, limit, turn_rate, window);
                    cast_turn = true;
                }
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
        if !m.arrived && !waiting && !repairing && !turning_to_site && !cast_turn {
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
            // That renewal falls on this update: it comes after the step
            // (SPEC-0019; c-RXD-1: creep 6 at 2395).
            let mut renew_exact = false;
            // That renewal with the target in range: the update's own step
            // first, then the swing (c-RXD-1: creep 9 at 6589).
            let mut exact_swing = None;
            // The goal before a renewal after a swing (SPEC-0019).
            let old_goal = m.target;
            let mut full_reach = reach;
            if let Order::Attack { target } = m.order {
                // The target where it is at the start of this tick (SPEC-0019).
                match tick_start_position(state, target) {
                    Some(at) if super::attack::standing(state, target) => {
                        let renew_now = m.renew_at.is_some_and(|r| tick >= r);
                        renew_exact = m.renew_at == Some(tick);
                        // The goal follows the target into another 2 x 2 square
                        // at the cluster's first update (SPEC-0019, "The goal
                        // follows the target at cluster updates"; c-RXD-1:
                        // creep 6 at 6295).
                        if std::mem::take(&mut m.follow_check)
                            && let Some(now) = position_of(state, target)
                            && grid64(relative(now, min)) != grid64(relative(m.target, min))
                        {
                            m.target = now;
                        }
                        match m.chase {
                            // The new goal after a swing (SPEC-0019).
                            _ if renew_now
                                && renew_exact
                                // (a swing timed by the cooldown goes on by its
                                // timer: c-RSL-1, the footman at 2251)
                                && state.unit(Some(h)).is_some_and(|u| u.attack.is_none())
                                && in_range(
                                    here,
                                    relative(at, min),
                                    add(reach, mul(collision_of(state, target), CELL)),
                                ) =>
                            {
                                m.renew_at = None;
                                m.renew_stands = false;
                                exact_swing = Some(at);
                            }
                            // A goal taken anew by a switch just before: the
                            // update walks on, the renewal is the cooldown end's
                            // timer after the updates (`cooldown_expiries`;
                            // c-SBJ-1: ogre 4, switched at 12.918833, walking at
                            // 12.929917 before its cooldown end at 12.929940).
                            _ if renew_now && renew_exact && m.goal_taken => {
                                m.renew_at = None;
                                m.renew_stands = false;
                            }
                            _ if renew_now => {
                                m.renew_at = None;
                                after_swing = true;
                                follow::new_group(state, h);
                                // Its walk comes from the cooldown's handler at the
                                // expiry (`cooldown_expiries`; c-RXD-1: kobold
                                // 1048690 at 35.249161).
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
                        // Or by the mark of touching its target at a block.
                        in_reach = in_reach || follow::touching(state, h);
                        in_reach = in_reach || m.unlimited;
                        // A remade walk: after this update the goal is the target
                        // where it is now, searched at the next update (c-SBW-1:
                        // kobold 5 at 3.81 and 3.84, hub #10).
                        if std::mem::take(&mut m.remade) {
                            remade.push((h, at));
                            m.chase = None;
                        }
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
                                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                                    u.entries.follow_leg_end = Some(tick + UPDATE);
                                }
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
                            state.drop_path(h);
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
                        let expired = state.unit(Some(h)).is_some_and(|u| {
                            real_of(u.cooldown_end)
                                <= real_of(add(state.move_clock, crate::timers::TICK))
                        });
                        let unit = h;
                        let (h, d) = heading_and_turn(&m, goal);
                        let gate_open = cooled
                            && m.swing_gate.is_none_or(|from| tick >= from)
                            && exact_swing.is_none();
                        if real_of(d).abs() < ATTACK_ANGLE {
                            // In reach and facing, the movement ends and the
                            // chase's walk with it (c-SAK-1: footman 2, walk
                            // freed at 41.815235; the image 20 at its update
                            // of 48.718559).
                            super::table::chase_walk_ended(state, unit);
                            m.heading = h;
                            m.walking = false;
                            m.velocity = [0, 0];
                            // The swing of an attack ordered by a script
                            // waits for the update after the one at which
                            // the unit acted (SPEC-0019, "When it
                            // starts").
                            // Its movement ended, the attack tests the angle to
                            // the target where it is now against AttackHalfAngle
                            // (0.5): further off, it is faced again there (c-RXD-1:
                            // creep 8 at 7046); else the swing starts.
                            let now = position_of(state, target).map(|p| relative(p, min));
                            let off = now.is_some_and(|p| {
                                real_of(heading_and_turn(&m, p).1).abs() > ATTACK_HALF_ANGLE
                            });
                            // That new facing waits for the cooldown's expiry
                            // itself (c-SAK-1: the image of the Blademaster
                            // stood from 9783 to its expiry at 9854).
                            if gate_open && off && !expired {
                            } else if gate_open && off {
                                m.target = now.map_or(m.target, |p| absolute(p, min));
                                // The face movement is a new goal, with a
                                // walk of its own (SPEC-0024; c-SAK-1:
                                // kobold 19 at 925), first among the units
                                // handled from the next tick (SPEC-0021).
                                super::table::walk_made_at_order(state, unit);
                                state.orders_given += 1;
                                m.given = state.orders_given;
                            } else if gate_open
                                && !reaches_with(state, unit, target, AFTER_MOVEMENT)
                                && let Some(at) = position_of(state, target)
                            {
                                // The arrival's range test (range + 100) fails:
                                // a new chase walk to the target at once
                                // (c-SBX-1: raised kobold 5 at 5.339851, hub #12).
                                m.target = at;
                                // Its goal is taken at the next update, its renewals
                                // counted from there (hub #12: 5.369849).
                                m.goal_taken = true;
                                m.chase = None;
                                super::table::walk_made_at_order(state, unit);
                                state.orders_given += 1;
                                m.given = state.orders_given;
                            } else if gate_open {
                                m.arrived = true;
                                m.swing_gate = None;
                                begin_attack = Some(target);
                            }
                        } else {
                            // Not while a swing runs: the agent stands, its goal
                            // direction kept (c-RZJ-1: footman 3 swinging from
                            // 20.549961 off his target, 0.6074 kept to 21.5697).
                            let clock = real_of(state.move_clock);
                            let swinging = state.unit(Some(unit)).is_some_and(|u| {
                                u.attack.is_some_and(|a| {
                                    real_of(a.swing_q) <= clock
                                        && a.hit_at.is_some_and(|q| real_of(q) > clock)
                                }) || (u.attack.is_some() && real_of(u.backswing_end) > clock)
                            });
                            if !swinging {
                                motion(&mut m, goal, false, speed, limit, turn_rate, window);
                            }
                        }
                        // After the step, the renewal: turned within 0.2 to the
                        // target where it is, the swing starts; else it is
                        // faced there (c-RXD-1: creep 9 at 6589, creep 6 at
                        // 2107).
                        if let Some(at) = exact_swing
                            && begin_attack.is_none()
                            && m.swing_gate.is_none_or(|from| tick >= from)
                        {
                            if real_of(heading_and_turn(&m, relative(at, min)).1).abs()
                                < ATTACK_ANGLE
                            {
                                m.arrived = true;
                                m.walking = false;
                                m.velocity = [0, 0];
                                begin_attack = Some(target);
                            } else {
                                m.target = at;
                            }
                        }
                    } else if state.grid.is_some() {
                        // To the goal point by the paths of SPEC-0020 and
                        // SPEC-0021, as a point order.
                        let walker = follow::Walker {
                            flee: false,
                            handle: h,
                            here,
                            order: goal,
                            class,
                            squared_speed: follow::squared_speed(m.velocity),
                            cluster: 1,
                            chase: Some(chase_of(state, target, full_reach)),
                            building: None,
                            mine: false,
                            heading: m.heading,
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
                        if renewing {
                            state.renewed_now.insert(h);
                        }
                        let hold = renewing && m.velocity == [0, 0];
                        let mut walked = None;
                        let steer = if renewing {
                            let mut stand = goal;
                            if !hold {
                                let old = follow::Walker {
                                    order: relative(old_goal, min),
                                    ..walker
                                };
                                match follow::steer(state, &old, tick) {
                                    // A walking step of its own update, then
                                    // the renewal's stop: the heading of the
                                    // step stays.
                                    follow::Steer::Walk(to) if renew_exact => walked = Some(to),
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
                                state.drop_path(h);
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
                            follow::Steer::Stand(_) if walked.is_some() => {
                                let to = walked.unwrap_or(goal);
                                motion(&mut m, to, true, speed, limit, turn_rate, window);
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
                                // An attack's chase that arrived swings when in
                                // range: the move-done event runs the range test
                                // (c-SAK-1: footman 2 at 925, hitting at 1025).
                                if let Order::Attack { target } = m.order
                                    && begin_attack.is_none()
                                    && reaches_with(state, h, target, AFTER_MOVEMENT)
                                {
                                    begin_attack = Some(target);
                                    m.arrived = true;
                                }
                            }
                        }
                    } else {
                        motion(&mut m, goal, true, speed, limit, turn_rate, window);
                    }
                } else {
                    let squared_speed = follow::squared_speed(m.velocity);
                    // A walk to a building ends touching it (SPEC-0029).
                    // So does a walk to help on a building (c-SBZ-1: peasant 5 at the
                    // barracks, the search ending at its first touch, hub #13).
                    let building = match m.order {
                        Order::Gather { target }
                        | Order::Return { building: target }
                        | Order::Resume { building: target } => state
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
                        // A walk to cast at a unit ends touching it as a chase
                        // does (c-SAK-1: the Mountain King's Storm Bolt from 505).
                        Order::Cast { target } => super::cast::reach(state, h, target)
                            .map(|reach| chase_of(state, target, reach)),
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
                        // A walk to a point that gave up with the mark of reach
                        // arrives once turned to the point (SPEC-0025).
                        Order::Point if follow::touching(state, h) => {
                            Some((ARRIVAL, m.heading, true))
                        }
                        // A player's unit walking back to where it acquired
                        // arrives within 50 (1.5625 cells; c-SBL-1: footman 2 at
                        // 12.449922, 48.55 from the point).
                        Order::Point
                            if m.going_back()
                                && state
                                    .unit(Some(h))
                                    .is_some_and(|u| u.owner != super::creeps::CREEPS) =>
                        {
                            Some((BACK_REACH, m.heading, false))
                        }
                        _ => None,
                    };
                    let place = (cluster > 1)
                        .then(|| state.member_now.get(&h).copied())
                        .flatten();
                    let mine = building.is_some_and(|b| super::harvest::is_mine(state, b));
                    let flee = state
                        .unit(Some(h))
                        .is_some_and(|u| u.attack_removed && u.attack_move.is_some());
                    let walker = |order| follow::Walker {
                        flee,
                        handle: h,
                        here,
                        order,
                        class,
                        squared_speed,
                        cluster,
                        chase,
                        building,
                        mine,
                        heading: m.heading,
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
                            state.drop_path(h);
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
                            // A worker at its gold mine keeps its walk to its going
                            // in (`harvest::start_waiting`; c-RXI-1: walks freed at 877).
                            if matches!(m.order, Order::Gather { target } | Order::Work { target } | Order::Return { building: target } | Order::Resume { building: target } if super::harvest::is_mine(state, target) || state.unit(Some(target)).is_some_and(|b| b.footprint.is_some()))
                            {
                                state.paths.remove(&h);
                            } else {
                                state.drop_path(h);
                            }
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
        // Its help starts as in reach (above): facing the building, or after
        // turning to it (`help`).
        if m.arrived
            && let Order::Resume { building } = m.order
            && unfinished_of_own(state, h, building)
        {
            m.arrived = false;
            m.walking = false;
            m.velocity = [0, 0];
            let at = position_of(state, building).map(|p| relative(p, min));
            let (heading, d) = at.map_or((m.heading, 0), |p| heading_and_turn(&m, p));
            if real_of(d).abs() < ATTACK_ANGLE {
                m.order = Order::Repair { building };
                super::table::chase_walk_ended(state, h);
                state.helpers_started.push((h, building));
                face_target(state, &mut m, building, min);
            } else {
                m.help = Some(building);
                m.heading = wrapped(if real_of(d) > 0.0 {
                    add(heading, turn_rate)
                } else {
                    add(heading, turn_rate ^ SIGN)
                });
            }
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
        // A group member's move ends at its arrival too, turning on (c-RTW-3:
        // footman 933 at 27.809410, 0.67 rad off its heading).
        let plain_move = m.order == Order::Point
            && u.attack_move.is_none()
            && !u.entries.game_order
            && u.harvest.is_none()
            && u.casting.is_none();
        // A goal taken in reach after a swing ends at the first update that
        // starts with the stored heading on the goal direction, and its walk
        // is given up (SPEC-0024; c-SAK-1: kobold 8 at 1087, gnoll 11 at
        // 1093, kobold 9 at 1123, gnoll 12 at 1129).
        // A goal taken early at the backswing's end, the unit facing its
        // target from the swing, ends at once (c-SAK-1: 3973).
        // Not at an update before the backswing's end that made the walk,
        // in the same tick on the queue (c-RXR-1: headhunter 1048702, update
        // 35.009117, end 35.009159; the walk goes two updates on).
        let later =
            u.entries.early_walk && real_of(u.entries.early_walk_q) > real_of(state.update_time);
        if u.entries.quiet_walk && !later && (m.heading == start_heading || u.entries.early_walk) {
            u.entries.quiet_walk = false;
            u.entries.early_walk = false;
            super::table::walk_dropped(state, h);
        }
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        if m.arrived && (facing_done || plain_move) && begin_attack.is_none() {
            // A plain move that ended before its facing: the unit still
            // turns to its heading at its updates (c-RTW-3: footman (825,
            // 828), facing 5.754847 at 2227, 5.754558 at 2233).
            let turning = !facing_done;
            u.movement = turning.then_some(m);
            if turning {
                still.push(h);
            }
            // An attack-move that arrived at its place ends there; a group
            // member stays at its place in the formation (playtest item 9:
            // the group bunched on the point when its members walked on).
            if m.order == Order::Point {
                // The order ends with its walk (SPEC-0024; c-SAK-1: the
                // Blademaster at his point in 1573).
                // A move of its own, as a creep's return home, too (2616).
                let attack_move = u.attack_move.take().is_some();
                if !attack_move && u.entries.game_order {
                    // A creep's walk home, its end after the walks freed at
                    // the updates (c-SAK-1: 9786).
                    state.table.homes.push(h);
                    super::table::walk_dropped(state, h);
                } else if attack_move {
                    super::table::attack_move_arrived(state, h, None);
                    super::table::walk_dropped(state, h);
                } else if !u.entries.order.is_empty() && u.harvest.is_none() {
                    // (A worker at its mine: its order and walk go on to its
                    // going in, `harvest::start_waiting`.)
                    // Its check comes 0.01 s after the move's end (c-SBG-1:
                    // ogre 3, move ended at 8.819954, checked at 8.829953).
                    if u.acquire_phase != super::NO_PHASE {
                        u.next_acquire = add(state.update_time, AFTER_DEATH);
                    }
                    // A creep's return timer running starts again at the
                    // move's end (SPEC-0030; c-SBG-1: ogre 3, 10.084434 ->
                    // 13.82 at its move's end, 8.819954).
                    if u.owner == super::creeps::CREEPS && u.creep.return_time.is_some() {
                        u.creep.return_time =
                            Some(add(state.update_time, state.combat.guard_return_time));
                    }
                    super::table::order_ended(state, h);
                    super::table::buff_end_carried_out(state, h);
                    super::table::walk_dropped(state, h);
                }
            }
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
    // Its goal the target where it was at that update (c-SBW-1: kobold 5
    // searching at 3.84 to the Blademaster's place at 3.81).
    for (h, at) in remade {
        state.paths.remove(&h);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
        {
            m.target = at;
        }
    }
    // Helpers whose help starts: their walks end, and their objects (hub #13).
    for (h, building) in help_starts {
        super::table::chase_walk_ended(state, h);
        let others = workers_of(state, building).into_iter().any(|w| w != h);
        if others {
            super::table::helper_started(state, h, building);
        }
    }
    // A creep's move to its dead target's point ended: its checks start
    // again, and its walk home comes next.
    // PROVISIONAL: the walk home as a walk to its guard point, order id 0.
    for h in faced_ends {
        let at = add(state.update_time, AFTER_DEATH);
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        u.checks_off = false;
        u.creep.queued_home = false;
        u.next_acquire = at;
        let home = u.home;
        if order(state, h, Order::Point, 0, home) {
            super::table::queue_taken_up(state, h);
        }
    }
    // The queue after a fade's move: an attack-move it had goes on, else
    // the attack is taken again.
    // PROVISIONAL: the attack without an attack-move was not seen.
    for (h, target, order_id) in attacks_after {
        let point = state
            .unit(Some(h))
            .and_then(|u| u.attack_move)
            .map(|a| a.point);
        if let Some(point) = point {
            if order(state, h, Order::Point, ORDER_ATTACK, point) {
                super::table::queue_taken_up(state, h);
            }
        } else if let Some(at) = position_of(state, target) {
            order(state, h, Order::Attack { target }, order_id, at);
        }
    }
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
/// [`reaches`] with the target where its path agent last committed, as a
/// check sees it (SPEC-0019).
fn reaches_at_commit(state: &State, h: u32, target: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let collision = |u: &super::Unit| state.data.units.get(&u.type_id).map_or(0, |t| t.collision);
    let (Some(u), Some(t)) = (state.unit(Some(h)), state.unit(Some(target))) else {
        return false;
    };
    let at = t.movement.map_or([t.x, t.y], |m| absolute(m.origin, min));
    let range = super::attack::weapon_for(state, u.type_id, Some(target)).map_or(0, |w| w.range);
    let reach = add(
        add(mul(range, CELL), mul(collision(u), CELL)),
        mul(collision(t), CELL),
    );
    in_range(relative([u.x, u.y], min), relative(at, min), reach)
}

pub(super) fn reaches(state: &State, h: u32, target: u32) -> bool {
    reaches_with(state, h, target, 0)
}

/// The attack start that follows a movement's end tests the range plus 100
/// (SPEC-0019, "Range test"; c-SBI-1: the ogre 198.5 from grunt 4 at
/// 1.499999, range 100 tested as 200).
const AFTER_MOVEMENT: u32 = 0x42C8_0000;

/// [`reaches`] with both units where they are at movement clock `clock`:
/// a moving one at its last update moved on by its velocity (SPEC-0019,
/// "Range test"; c-SBJ-1: the fleeing Dreadlord at kobold 5's cooldown
/// end, 14.039919).
pub(super) fn reaches_at(state: &State, h: u32, target: u32, clock: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let at = |o: u32| {
        state.unit(Some(o)).map(|u| match u.movement {
            Some(m) => m.position(clock),
            None => relative([u.x, u.y], min),
        })
    };
    let collision = |o: u32| {
        state
            .unit(Some(o))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(0, |t| t.collision)
    };
    let (Some(u), Some(a), Some(b)) = (state.unit(Some(h)), at(h), at(target)) else {
        return false;
    };
    let range = super::attack::weapon_for(state, u.type_id, Some(target)).map_or(0, |w| w.range);
    let reach = add(
        add(mul(range, CELL), mul(collision(h), CELL)),
        mul(collision(target), CELL),
    );
    in_range(a, b, reach)
}

/// [`reaches`] with `extra` added to the weapon's range.
pub(super) fn reaches_with(state: &State, h: u32, target: u32, extra: u32) -> bool {
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
    let range = add(range, extra);
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
///
/// The ends are timers of the queue as the acquisition checks are: those
/// up to the end of the tick that come after its first check end after
/// the checks (`after_checks`; c-RZH-1: footman 4's check took creep 10
/// in 5261 before footman 5's delay ended, so 5 updated first in 5263).
pub(super) fn reaction_ends(state: &mut State, until: Option<u32>, first: bool) {
    let check = if until.is_none() {
        first_check(state)
    } else {
        None
    };
    ends_before(state, until, first, check);
}

/// The reaction delays of this tick that end after its first acquisition
/// check (`reaction_ends`).
pub(super) fn after_checks(state: &mut State, bound: Option<u32>) {
    ends_before(state, bound, false, None);
}

/// The reaction delays after the updates that end up to `bound` (all of
/// them without one), after the tick's first check as `reaction_ends`.
pub(super) fn reaction_ends_before(state: &mut State, bound: Option<u32>) {
    let check = first_check(state);
    ends_before(state, bound, false, check);
}

fn ends_before(state: &mut State, until: Option<u32>, first: bool, check: Option<u32>) {
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
                    (Some(end), _) => {
                        real_of(end) <= real_of(limit)
                            && check.is_none_or(|c| real_of(end) < real_of(c))
                    }
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
    for (at, _, h) in ending {
        // After the updates, the cooldowns that expire before this end come
        // first (c-RZJ-1: footman 7's at 36.149391 before footman 4's
        // reaction end at 36.152317 in 7232).
        if !first && at.is_finite() {
            cooldown_expiries(state, Some(at.to_bits()));
        }
        // So do the goals of backswings ending before it (c-SAK-1: gnoll
        // 17's at 16.019894 before footman 6's reaction end in 3205).
        if at.is_finite() {
            super::attack::after_swing_walks(state, Some(at.to_bits()));
        }
        let given = state.orders_given + 1;
        state.orders_given = given;
        super::table::reaction_ended(state, h);
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
        // The chase's walk is made at the end of the delay (SPEC-0024).
        if timed {
            super::table::walk_made_at_order(state, h);
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
pub(super) fn chase(
    state: &mut State,
    h: u32,
    target: u32,
    order_id: i32,
    renew_at: Option<u64>,
    in_reach: bool,
) {
    // Standing when the goal comes: no update of this tick saw it.
    let stood = state.unit(Some(h)).is_some_and(|u| u.movement.is_none());
    // After the swing: where the target is by its position formula with
    // the movement clock of the tick that holds the backswing end
    // (c-RXR-1: rifleman 9, 5053; c-RXD-1: creep 6 at 6978, its backswing
    // end in this tick).
    let end = state.unit(Some(h)).map_or(0, |u| u.backswing_end);
    let clock = if real_of(end) > real_of(state.move_clock) {
        state.move_clock
    } else {
        state.last_move_clock
    };
    let at = match (renew_at, state.objects.get(&target)) {
        (Some(_), Some(Object::Unit(u))) => u.movement.map(|m| {
            let min = [state.world_bounds[0], state.world_bounds[1]];
            absolute(m.position(clock), min)
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
                m.follow_check = true;
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
            flee: false,
            handle: h,
            here,
            order: goal,
            class,
            squared_speed: 0,
            cluster: 1,
            chase: Some(chase_of(state, target, reach)),
            building: None,
            mine: false,
            heading: state
                .unit(Some(h))
                .and_then(|u| u.movement)
                .map_or(0, |m| m.heading),
            tree: None,
            arrive: None,
            place: None,
        };
        // Right after a swing with the target in reach the new group is
        // not handled: no search (SPEC-0021; c-RZJ-1: footman 2 at 7210).
        if in_reach {
            follow::quiet(state, h);
            // Still a new goal with its walk (SPEC-0024; c-SAK-1: kobold
            // 8 at 1085, gnoll 11 at 1087).
            // (made at the backswing's end, `attack::after_swing_walks`).
            let early = state
                .objects
                .get_mut(&h)
                .and_then(|o| match o {
                    Object::Unit(u) => Some(std::mem::take(&mut u.entries.early_goal)),
                    _ => None,
                })
                .unwrap_or(false);
            if !early {
                super::table::walk_made_at_order(state, h);
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.entries.quiet_walk = true;
                }
            }
            // Its update in this tick came before (the goal is due at the
            // backswing's end, the tick before): that update ends it (c-SAK-1:
            // the Mountain King, goal 1572, update 1573).
            // So does an update of this tick that it missed, standing
            // there: the game took the goal at the backswing's end (c-RZJ-1:
            // the Archmage, goal 3444, update 3445, freed 3451).
            let missed = early && stood && state.update_now && state.updates_started == state.ticks;
            if (state.updated.contains(&h) || missed) && state.table.now < state.ticks {
                state.table.now = state.ticks;
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.entries.quiet_walk = false;
                }
                super::table::walk_dropped(state, h);
            }
        } else {
            // A goal taken at the backswing's end has its walk already.
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && std::mem::take(&mut u.entries.early_goal)
            {
                u.entries.walk_ready = true;
            }
            follow::search(state, &walker, tick);
        }
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
    /// After a target's death in a backswing, the queue time of that
    /// backswing's end, when the move is taken up again.
    resume_q: Option<u32>,
    /// Its periodic check, kept through that backswing.
    periodic: Option<u32>,
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
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        // The check 0.01 s after the end of its backswing (c-RZH-1, the
        // hero's kill of 2224: restart 2330, check 2332).
        let end = u.backswing_end;
        if let Some(a) = u.attack_move.as_mut() {
            a.resume_at = Some(tick + backswing);
            keep_periodic(a, add(end, AFTER_DEATH));
        }
    }
}

/// An attack-move's check restarts at `restart` (0.01 s after a backswing
/// that follows its target's death); a periodic check due before goes on
/// through the backswing (SPEC-0019, "During such a backswing").
fn keep_periodic(a: &mut AttackMove, restart: u32) {
    // (Kept when both the kill and the death restart it.)
    if real_of(a.next_check) < real_of(restart) {
        a.periodic = Some(a.next_check);
    }
    a.next_check = restart;
}

/// The value of a walk back to the guard point: within it plus its own
/// collision size a unit does not walk back, an equal distance counting as
/// there (SPEC-0019; c-RZJ-1: elemental 16, collision 32, 79.9 from its
/// point at 2625).
const GUARD_ACCEPT: f32 = 50.0;

/// Unit `h` walks back to its guard point `home` at the end of the
/// backswing in which its target died, unless it is there already
/// (SPEC-0030, "Losing the target in a backswing").
pub(super) fn guard_back(state: &mut State, h: u32, home: [u32; 2]) {
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    u.guard_return = None;
    let idle = u.attack.is_none() && u.movement.is_none_or(|m| !m.ordered());
    let (dx, dy) = (
        real_of(u.x) - real_of(home[0]),
        real_of(u.y) - real_of(home[1]),
    );
    let accept = GUARD_ACCEPT
        + state
            .data
            .units
            .get(&u.type_id)
            .map_or(0.0, |t| real_of(t.collision));
    if idle && dx * dx + dy * dy > accept * accept + 1e-3 && order(state, h, Order::Point, 0, home)
    {
        super::table::guard_walk_made(state, h);
    } else if idle {
        // It stays: what it kept from the attack goes, and its order ends
        // (c-SAK-1: the image 20 at 10950).
        super::table::backswing_ended_idle(state, h);
    }
}

/// Attack-moves whose walk ended at their point in this tick's updates
/// end there, before the walks given up in the updates are freed
/// (c-RZH-1: the Mountain King at 35.188877, his walk freed after the
/// ending's objects).
pub(super) fn attack_moves_arrived(state: &mut State) {
    let units: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| {
            state.unit(Some(h)).is_some_and(|u| {
                u.alive
                    && u.attack.is_none()
                    && u.attack_move.is_some_and(|a| a.resume_at.is_none())
                    && u.movement
                        .is_some_and(|m| !m.ordered() && m.order == Order::Point)
            })
        })
        .collect();
    for h in units {
        let Some(a) = state.unit(Some(h)).and_then(|u| u.attack_move) else {
            continue;
        };
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.next_acquire = a.next_check;
        }
        // Its walk ended short of the point, outside the arrival radius,
        // or reached it (c-SAK-1: footman 3 47.7 away at 57.388206;
        // c-RZJ-1: footman 6 14.3 away at 38.577015).
        let radius = mul(ARRIVAL, 32.0f32.to_bits());
        let short = state.unit(Some(h)).and_then(|u| {
            u.movement.map(|m| {
                let (dx, dy) = (sub(u.x, m.target[0]), sub(u.y, m.target[1]));
                real_of(add(mul(dx, dx), mul(dy, dy))) > real_of(mul(radius, radius))
            })
        });
        set_attack_move(state, h, None);
        super::table::attack_move_arrived(state, h, short);
    }
}

/// Attack-moves going on after a kill, and ending at their point; units
/// walking back to their guard points.
pub(super) fn attack_moves(state: &mut State) {
    let tick = state.ticks;
    cooldown_expiries(state, None);
    let back: Vec<(u32, [u32; 2])> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            // A creep's guard point is where its last walk home ended.
            let home = if u.owner == super::creeps::CREEPS {
                u.creep.guard.unwrap_or(u.home)
            } else {
                u.home
            };
            (u.guard_return == Some(tick)).then_some((h, home))
        })
        .collect();
    for (h, home) in back {
        guard_back(state, h, home);
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
            take_up(state, h, a);
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
            // Arrived at the point: the order ends. Its check timer is the
            // unit's own and goes on (c-SAK-1: the Blademaster, at his
            // point, checked at 45.51 and 46.01 after the order of 45.0).
            Some(m) if m.order == Order::Point => {
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.next_acquire = a.next_check;
                }
                set_attack_move(state, h, None);
                // Its objects go with it (c-SAK-1: the Blademaster at
                // 5118).
                super::table::attack_move_arrived(state, h, None);
            }
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
    /// The periodic check of an attack-move in the backswing after a kill:
    /// it only records a target (SPEC-0019, "During such a backswing").
    Record,
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

/// The ends of stuns in this tick (SPEC-0037, "The order after a stun"):
/// a unit's own order is given again as new (c-RZH-1: footman 2's
/// attack-move at 2009); an attack it acquired is not, the unit is idle and
/// checks 0.01 s later (creep 8 at 1946, taking footman 4 at 1948), a creep
/// with its return timer and guard checks started.
fn stun_ends(state: &mut State) {
    let tick = state.ticks;
    let mut ended: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| {
            state
                .unit(Some(h))
                .is_some_and(|u| u.alive && u.stunned_until == Some(tick))
        })
        .collect();
    // In the order the stuns were made (c-SBI-1: kobold 6, the first
    // stunned by War Stomp, checks first at 7.989999).
    ended.sort_by_key(|&h| {
        state
            .unit(Some(h))
            .and_then(|u| u.entries.stun_order.last().copied())
            .map_or(u32::MAX, |o| state.table.creation_of(o))
    });
    let now = state.move_clock;
    for h in ended {
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        // The order comes back at the stun's own time (SPEC-0037).
        let now = u.stun_back.unwrap_or(now);
        // A zero hit from the stunner follows 0.01 s after the order came
        // back, after the unit's check of that time (c-SAK-1: kobold 10 at
        // 1745).
        // Only for a creep (c-RZH-1: no zero hit after footman 2's bash
        // stuns).
        if u.stunner.is_some() && u.owner == super::creeps::CREEPS {
            let hit = add(now, AFTER_DEATH);
            let (mut t, mut clock) = (tick, state.move_clock);
            while real_of(hit) > real_of(add(clock, crate::timers::TICK)) {
                clock = add(clock, crate::timers::TICK);
                t += 1;
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.zero_hit = Some(t);
            }
        }
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        if let Some(a) = u.attack_move {
            point_order_taken(state, h, "attack", a.point, true);
            // Its first check 0.01 s after the order came back.
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(a) = u.attack_move.as_mut()
            {
                a.next_check = add(now, AFTER_DEATH);
            }
            continue;
        }
        // Also one whose swing the stun dropped (c-RZH-1: creep 8, swinging
        // at the bolt of 4.715).
        let acquired = u.stun_dropped_acquired
            || u.attack.is_some_and(|a| a.order_id == 0)
            || u.movement.is_some_and(|m| {
                matches!(m.order, Order::Attack { .. }) && m.order_id == 0 && m.ordered()
            });
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.stun_dropped_acquired = false;
        }
        if acquired {
            halt(state, h);
            // Its order back is none: it is idle, and its check is an idle
            // unit's (c-SAK-1: kobold 10 took footman 7 at 1745).
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.movement = None;
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && u.acquire_phase != super::NO_PHASE
            {
                u.next_acquire = add(now, AFTER_DEATH);
                // A timer started now, after those of the units back before it.
                state.acquire_starts += 1;
                u.acquire_seq = state.acquire_starts;
            }
            super::creeps::after_stun(state, h, now);
        }
    }
}

/// The checks of this tick after the updates whose expiries lie below
/// `bound` (all of them without one), the ends of stuns first in the first
/// batch.
pub(super) fn acquisitions(state: &mut State, bound: Option<u32>, first: bool) -> Vec<u32> {
    if first {
        stun_ends(state);
    }
    checks(state, bound, false)
}

/// The checks of this tick whose expiries lie below `bound` (all of them
/// without one): the checks before the updates come first (SPEC-0019;
/// c-SAK-1: the Mountain King's check at 31.168348 took kobold 15 before
/// the update at 31.169281 in 6235, standing for his reaction delay).
pub(super) fn acquisitions_before(state: &mut State, bound: Option<u32>) -> Vec<u32> {
    checks(state, bound, true)
}

fn checks(state: &mut State, bound: Option<u32>, early: bool) -> Vec<u32> {
    let tick = state.ticks;
    // Below `bound`, as the bits of a positive float grow with it.
    let next_clock = bound.map_or(add(state.move_clock, crate::timers::TICK), |b| b - 1);
    let alive: BTreeSet<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| state.unit(Some(h)).is_some_and(|u| u.alive))
        .collect();
    let mut starts = state.acquire_starts;
    // (expiry, start number, unit, check).
    let mut due: Vec<(u32, u64, u32, Check)> = Vec::new();
    // The timers restarted here, by their old expiry and start, to number
    // their new starts in the order they fired.
    let mut restarted: Vec<(u32, u64, u32)> = Vec::new();
    for i in 0..state.units.len() {
        let h = state.units[i];
        let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
            continue;
        };
        if !u.alive {
            continue;
        }
        // No check and no timer until its next order (SPEC-0045).
        if u.checks_off {
            continue;
        }
        // One whose stun ends now checks after the stun's end.
        if early && bound.is_some() && u.stunned_until == Some(tick) {
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
            restarted.push((u.next_acquire, seq, h));
        }
        // Nor one stunned (c-SAK-1: kobold 10, stunned from 740, made no
        // switch at its check of 978).
        if u.stunned_until.is_some_and(|t| tick < t) {
            continue;
        }
        // Nor one whose attack ability was removed (c-SBI-1: the heroes).
        if u.attack_removed {
            continue;
        }
        // Nor one walking invisible (SPEC-0040; c-SAK-1: the Blademaster
        // took no target in his first Wind Walk, 400 to 4400).
        if u.wind_walk.is_some_and(|w| w.invisible(tick)) {
            continue;
        }
        if u.paused {
            continue;
        }
        // A creep walking home takes no target (SPEC-0030; c-RXD-1: creep
        // 7 at 4577).
        if u.creep.returning {
            continue;
        }
        // Its periodic check in the backswing after a kill only records a
        // target (SPEC-0019; c-SAK-1: footmen 2 and 3 at 16.009998).
        if let Some(a) = u.attack_move.as_mut()
            && let Some(p) = a.periodic
            && real_of(p) <= real_of(next_clock)
        {
            if a.resume_at.is_some() {
                a.periodic = Some(add(p, HALF));
                due.push((p, 0, h, Check::Record));
            } else {
                a.periodic = None;
            }
        }
        // So does an idle unit's (c-SAK-1: the image 20 at 54.553837),
        // re-armed 0.5 s while the backswing lasts.
        if let Some(p) = u.idle_periodic
            && real_of(p) <= real_of(next_clock)
        {
            let next = add(p, HALF);
            u.idle_periodic = (real_of(next) < real_of(u.backswing_end)).then_some(next);
            due.push((p, 0, h, Check::Record));
        }
        // A unit on attack-move whose target died acquires at the check
        // restarted by the death (c-RXR-1: footman 2 took grunt 13 in 2581).
        if u.attack_move.is_some()
            && let Some(expiry) = expired
            && std::mem::take(&mut u.after_death)
            && target.is_none()
        {
            // It is the attack-move's own timer: the next check 0.5 s on, as
            // without a target (c-RXR-1: rifleman 8, 2703 then 2803).
            if let Some(a) = u.attack_move.as_mut()
                && real_of(a.next_check) <= real_of(next_clock)
            {
                a.next_check = add(a.next_check, HALF);
            }
            due.push((expiry, seq, h, Check::AttackMove { expiry }));
            continue;
        }
        if let Some(a) = u.attack_move.as_mut() {
            if real_of(a.next_check) <= real_of(next_clock) {
                let expiry = a.next_check;
                // The period follows the state at the expiry: 0.5 s without
                // a target, 1.0 s with one (c-RZH-1: footman 4, 803 then 903
                // then 1.0 s).
                a.next_check = add(a.next_check, if target.is_some() { ONE } else { HALF });
                let walking = u.attack.is_none()
                    && a.resume_at.is_none()
                    && u.movement
                        .is_some_and(|m| m.ordered() && m.order == Order::Point);
                if walking {
                    due.push((expiry, 0, h, Check::AttackMove { expiry }));
                } else if let Some((current, _)) = target {
                    // With a target the same check looks for a better one
                    // (c-RZH-1: footman 4 switched to creep 8 at 903).
                    due.push((expiry, 0, h, Check::Attacking { current }));
                }
            }
            continue;
        }
        // An attack the unit took on its own (order id 0) checks for a
        // better target (SPEC-0019, "Checks while attacking").
        // Nor a creep whose move order home has not ended (SPEC-0030;
        // c-RXD-1: creep 6 kept unit 2 at 6359).
        // Also while that order waits behind an attack taken at a call
        // (c-SAK-1: ogre 18 kept the Mountain King at 7046).
        if u.creep.homebound || u.creep.queued_home {
            continue;
        }
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
    // (c-SBV-1: footmen 7 and 6, brought back in the order 8, 7, 6, check
    // in that order at 9.009489.)
    restarted.sort_by(|a, b| real_of(a.0).total_cmp(&real_of(b.0)).then(a.1.cmp(&b.1)));
    for (_, _, h) in restarted {
        starts += 1;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.acquire_seq = starts;
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
    // Where each check's acquisitions start in `acquired`.
    let mut marks: Vec<(usize, u32)> = Vec::new();
    for (expiry, _, h, check) in due {
        marks.push((acquired.len(), expiry));
        // An idle unit's check reads its next delay when it fires: one that a
        // call of an earlier check gave a target runs on 1.0 s (c-SBJ-1: ogre
        // 4, taken by kobold 8's call at 9.919998, its next check at
        // 10.919998).
        if matches!(check, Check::Idle { .. })
            && acquired.contains(&h)
            && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && u.next_acquire == add(expiry, HALF)
        {
            u.next_acquire = add(expiry, ONE);
        }
        // The goals of backswings ending before this check come first
        // (c-SAK-1: gnoll 17 at 12.929933 before the check at 12.932871).
        super::attack::after_swing_walks(state, Some(expiry));
        // A better target than the one attacked: taken at once, without a
        // reaction delay or a call for help (SPEC-0019).
        if let Check::Attacking { current, .. } = check {
            if let Some(better) = targets.better_than(state, h, current) {
                // Also in its backswing: the unit turns to the new target at
                // its updates while the backswing ends (c-RXD-1: creep 7
                // took its target in 4457 and turned in 4459 and 4465).
                if switch_target(state, h, better) {
                    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                        u.entries.called = false;
                    }
                    acquired.push(h);
                }
            }
            continue;
        }
        // An idle unit that a call of this chain already took has no check
        // of its own (SPEC-0030; c-SAK-1: kobold 10, taken by gnoll 12's
        // relay at 4767, made no relay).
        if acquired.contains(&h) {
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
            // Only one its player sees (SPEC-0019, "Remembered enemy").
            // Nor an invulnerable one (c-SBQ-1: the Archmage, the ogre that hit
            // him made invulnerable at 9.329, no acquisition after).
            if super::cast::invulnerable(state, r) {
                return None;
            }
            if !super::fog::unit_visible(state, r, u.owner) {
                return None;
            }
            let (dx, dy) = (real_of(o.x) - real_of(u.x), real_of(o.y) - real_of(u.y));
            (dx * dx + dy * dy <= REMEMBERED_REACH * REMEMBERED_REACH).then_some(r)
        };
        let idle = matches!(check, Check::Idle { .. });
        let Some(target) = targets
            .nearest_enemy(state, h)
            // Also on an attack-move with no target (c-SAK-1: footman 4
            // took kobold 19, 1236 away, at 10005).
            // Also at a check that only records (c-RXR-1: headhunters 1048703
            // and 1048702 at 32.349987 and 32.379986).
            .or_else(|| {
                remembered()
                    .filter(|_| idle || matches!(check, Check::AttackMove { .. } | Check::Record))
            })
        else {
            continue;
        };
        // A check that only records: the event, no draw and no attack.
        if matches!(check, Check::Record) {
            state.record_only.push(h);
            acquired.push(h);
            continue;
        }
        // A unit on hold position takes none out of its reach (c-SBG-1:
        // ogre 1 and the paladin, the Mountain King 233 away).
        if holds_off(state, h, target) {
            continue;
        }
        let Some(Object::Unit(o)) = state.objects.get(&target) else {
            continue;
        };
        let at = [o.x, o.y];
        let (order_id, expiry) = match check {
            Check::Idle { expiry } => (0, Some(expiry)),
            Check::AttackMove { expiry } => (ORDER_ATTACK, Some(expiry)),
            Check::Attacking { .. } => continue,
            Check::Record => continue,
        };
        // A walking unit stops for its reaction delay (c-RXR-1: footman 2
        // in 2581).
        // An idle unit with no walk back pending stores where it stands as
        // its guard point when its check acquires (SPEC-0019; c-RZJ-1:
        // elemental 16 at 2625 and 3825; c-RXR-1: grunt 14, walking back at
        // 4813, keeps its point).
        let standing = state.unit(Some(h)).is_some_and(|u| u.movement.is_none());
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.called = false;
        }
        freeze(state, h);
        if order(state, h, Order::Attack { target }, order_id, at) {
            // A target in reach takes an order with no reach limit
            // (SPEC-0019; c-SAK-1: the Mountain King on gnoll 16 at 3616).
            if reaches_at_commit(state, h, target)
                && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(m) = u.movement.as_mut()
            {
                m.unlimited = true;
            }
            if idle
                && standing
                && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            {
                // A creep's is the point its walk back goes to (c-SAK-1:
                // kobold 10, which stayed at footman 5's death in 5442).
                if u.owner == super::creeps::CREEPS {
                    u.creep.guard = Some([u.x, u.y]);
                } else {
                    u.home = [u.x, u.y];
                }
            }
            // A player's unit already guarding starts its guard check again
            // (hub #9; c-SBV-1: footman 6 taking kobold 3 at 18.649929).
            if let Some(e) = expiry
                && state.unit(Some(h)).is_some_and(|u| {
                    u.owner != super::creeps::CREEPS && u.creep.checks_next.is_some()
                })
            {
                super::creeps::guard_start(state, h, e);
            }
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
    state.check_times = acquired
        .iter()
        .enumerate()
        .map(|(k, &h)| {
            let at = marks.iter().rev().find(|m| m.0 <= k).map_or(0, |m| m.1);
            (h, at)
        })
        .collect();
    acquired
}

/// The units of `acquired` whose checks expired before the earliest
/// reaction end still to come in this tick, and the others.
pub(super) fn checked_before_reaction_end(state: &State, acquired: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let end = state
        .moving
        .iter()
        .filter_map(|&h| state.unit(Some(h)).and_then(|u| u.movement?.reaction_end))
        .min_by(|a, b| real_of(*a).total_cmp(&real_of(*b)));
    let Some(end) = end else {
        return (acquired.to_vec(), Vec::new());
    };
    acquired.iter().partition(|&&h| {
        state
            .check_times
            .iter()
            .find(|c| c.0 == h)
            .is_none_or(|c| real_of(c.1) < real_of(end))
    })
}

/// The checks at the end of a backswing of the attackers that switched
/// targets in it: a new target within range is faced where it was at the
/// start of this tick, as after the swing (`chase`), the place of the
/// attacker's next update (SPEC-0019, "Which direction it turns to";
/// c-RXD-1: creep 6 switched at 2031, backswing end 2033, turned at 2035;
/// creep 8 switched at 2007, backswing end 2081, turned at 2083).
fn attack_checks(state: &mut State) {
    let tick = state.ticks;
    // Those whose backswings end in this tick come too: out of reach their
    // chase starts again at the end itself (c-SAK-1: footman 6, backswing
    // end 9425); in reach they wait for the check of the next tick.
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.attack_checks)
        .into_iter()
        .partition(|&(t, _, _)| t <= tick + 1);
    state.attack_checks = later;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    for (t, h, target) in due {
        let at = match state.objects.get(&target) {
            Some(Object::Unit(u)) => u
                .movement
                .map(|m| absolute(m.position(state.last_move_clock), min)),
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
        let reached = in_range(here, relative(at, min), reach);
        if t > tick && reached {
            state.attack_checks.push((t, h, target));
            continue;
        }
        if !reached {
            // Out of reach the attack starts its chase anew, although it
            // already chases that target: a stop, and a new search at the
            // next update (SPEC-0019; c-SAK-1: footman 2 at 9419, footman 6
            // at 9425).
            let order_id = u
                .movement
                .filter(|m| m.order == (Order::Attack { target }))
                .map(|m| m.order_id);
            if let (Some(id), Some(pos)) = (order_id, position_of(state, target)) {
                stop_commit(state, h);
                order(state, h, Order::Attack { target }, id, pos);
            }
            continue;
        }
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
            && m.order == (Order::Attack { target })
        {
            m.target = at;
            m.faced = true;
            // A face movement is no chase: no renewal before the one after
            // the swing (c-RXD-1: creep 8 kept its place from 2083 to 2155).
            m.chase = Some(u32::MAX);
        }
    }
}

/// The target of unit `h`'s attack order, if it has one.
pub(super) fn attack_target(state: &State, h: u32) -> Option<u32> {
    match state.unit(Some(h))?.movement?.order {
        Order::Attack { target } => Some(target),
        _ => None,
    }
}

/// Whether unit `h` has an attack order.
pub(super) fn attacking(state: &State, h: u32) -> bool {
    state
        .unit(Some(h))
        .and_then(|u| u.movement)
        .is_some_and(|m| m.ordered() && matches!(m.order, Order::Attack { .. }))
}

/// Unit `h` attacks `better` instead of its target at once: no reaction
/// delay and no call for help; a walking attacker stops at once, and a
/// new goal after a swing still waits for the cooldown (SPEC-0019).
/// Whether it took the order.
pub(super) fn switch_target(state: &mut State, h: u32, better: u32) -> bool {
    let tick = state.ticks;
    let resume = state
        .unit(Some(h))
        .and_then(|u| u.attack)
        .and_then(|a| a.resume)
        .filter(|&r| r > tick);
    // Only a renewal still to come (a switch in the backswing); one that
    // passed is stale (c-RZJ-1: the Ogre Lord, standing, switched at 2352
    // and turned at once).
    let renew = state
        .unit(Some(h))
        .and_then(|u| {
            u.attack
                .and_then(|a| a.renew_at)
                .or(u.movement.and_then(|m| m.renew_at))
        })
        .filter(|&r| r > tick);
    let Some(at) = position_of(state, better) else {
        return false;
    };
    // A unit standing when it switches starts a new movement, as at a new
    // order: a stop commit now, its first update at the next update batch
    // (SPEC-0018; c-RZJ-1: the Ogre Lord switched at 2352, turned at
    // 2353).
    let standing = state
        .unit(Some(h))
        .is_some_and(|u| u.movement.is_none_or(|m| !m.walking));
    if standing {
        stop_commit(state, h);
    }
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
        if standing {
            m.next_update = state.ticks + 1;
        }
        if let Some(r) = renew {
            m.renew_at = Some(r);
            m.renew_stands = true;
        }
    }
    true
}

/// Whether unit `h`, attacking `current` on its own, takes `other` that
/// calls for help instead, by the rating of [`rating`] and then the
/// strictly nearer (SPEC-0048; SPEC-0019, "Checks while attacking").
fn prefers(state: &State, h: u32, current: u32, other: u32) -> bool {
    prefers_by(state, h, current, other, threat)
}

/// [`prefers`] with the threat rating `threat`.
fn prefers_by(
    state: &State,
    h: u32,
    current: u32,
    other: u32,
    threat: fn(&State, u32, u32) -> u8,
) -> bool {
    let (Some(mine), Some(theirs)) = (
        rating(state, h, current, Some(current), threat),
        rating(state, h, other, Some(current), threat),
    ) else {
        return false;
    };
    // The strictly better key, else the strictly nearer; a tie keeps the
    // current target.
    theirs.0 < mine.0 || (theirs.0 == mine.0 && theirs.1 < mine.1)
}

/// The parts of the rating of SPEC-0048 that our games use, in its
/// order, as flags that are `true` where the candidate ranks lower.
type Key = [bool; 18];

/// The rating of enemy `other` by unit `h` (SPEC-0048) with `current` its
/// target, and the squared distance between their centres; `threat`
/// tells whom `other` attacks (0 `h`, 1 an ally of `h`, 2 else).
///
/// PROVISIONAL: lists D and E and the parts of the damage multiplier are
/// off in all games read and not done; the unidentified states of parts
/// 1, 4, 11 and 37 are taken as equal; part 9 repeats part 5 or is equal
/// for all, so it changes no choice and is left out; Serpent Wards and
/// the poison and ensnare parts are not done. Parts 31 and 38 rest on a
/// state not identified and are left out.
fn rating(
    state: &State,
    h: u32,
    other: u32,
    current: Option<u32>,
    threat: fn(&State, u32, u32) -> u8,
) -> Option<(Key, f32)> {
    let u = state.unit(Some(h))?;
    let o = state.unit(Some(other))?;
    let ud = state.data.units.get(&u.type_id);
    let od = state.data.units.get(&o.type_id);
    let collision = |d: Option<&crate::data::UnitType>| d.map_or(0.0, |d| real_of(d.collision));
    let (dx, dy) = (real_of(o.x) - real_of(u.x), real_of(o.y) - real_of(u.y));
    let distance = dx * dx + dy * dy;
    let range =
        super::attack::weapon_for(state, u.type_id, Some(other)).map_or(0.0, |w| real_of(w.range));
    let reach = range + collision(ud) + collision(od);
    // In range by the path agents, their last commits (SPEC-0048; c-SAK-1:
    // gnoll 16, walking, took footman 2 at 4925, footman 3 162.06 from its
    // commit).
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let agent = |w: &super::Unit| w.movement.map_or([w.x, w.y], |m| absolute(m.origin, min));
    let (o_at, u_at) = (agent(o), agent(u));
    let (ax, ay) = (
        real_of(o_at[0]) - real_of(u_at[0]),
        real_of(o_at[1]) - real_of(u_at[1]),
    );
    let in_range = ax * ax + ay * ay <= reach * reach;
    let creep = rates_as_creep(state, h);
    let big = creep && ud.is_some_and(|d| d.level >= 7);
    // An illusion is no hero here (c-SAK-1, SPEC-0040).
    let hero = crate::data::is_hero(o.type_id) && o.illusion.is_none();
    // A big creep and a hero within its range + 150 between centres (part
    // 2; c-RZJ-1: the Ogre Lord, range 100, kept no hero at 262 from the
    // Paladin at 1307).
    let near_hero = big && hero && {
        let r = range + 150.0;
        distance <= r * r
    };
    // Whom `other` attacks: its attack, else the target of its attack
    // order, while that target lives.
    let target = o
        .attack
        .map(|a| a.target)
        .or(match o.movement.map(|m| m.order) {
            Some(Order::Attack { target }) => Some(target),
            _ => None,
        })
        .and_then(|t| state.unit(Some(t)).filter(|t| t.alive));
    let attacks_enemy = target.is_some_and(|t| enemies(state, u.owner, t.owner));
    let attacks_allied_hero = target.is_some_and(|t| {
        crate::data::is_hero(t.type_id)
            && (t.owner == u.owner || state.setup.is_ally(u.owner, t.owner))
    });
    // A unit whose attack ability was removed attacks nobody (c-SBI-1: the
    // heroes without Aatk, left for grunt 4 by every creep).
    // Nor does a worker (the `Peon` classification), whatever its weapons
    // (c-SBI-1: the peon, below the grunts for the ogre; c-RXD-1: the
    // peasants alike).
    let hits_rater = !o.attack_removed
        && !od.is_some_and(|d| d.worker)
        && super::attack::weapon_for(state, o.type_id, Some(h)).is_some();
    // A stunned unit still counts as one that can attack (c-RXD-1,
    // c-RZH-1).
    let can_attack = !o.attack_removed && od.is_some_and(|d| d.weapon.is_some());
    let life = real_of(o.life.value());
    let rank = threat(state, h, other);
    let key = [
        !near_hero,                                          // 2
        creep && o.owner >= 12,                              // 3
        attacks_enemy,                                       // 5
        !can_attack,                                         // 8
        creep && !attacks_allied_hero,                       // 14
        creep && !hits_rater,                                // 15
        big && !(in_range && !near_hero),                    // 16
        big && life >= 100.0,                                // 21
        big && life >= 200.0,                                // 25
        big && life >= 300.0,                                // 26
        od.is_some_and(|d| d.armor_type == Some(FORTIFIED)), // 29
        od.is_none_or(|d| real_of(d.speed) <= 0.0),          // 30
        !hits_rater,                                         // 32
        !in_range,                                           // 33
        not_slower(state, h, other),                         // 34
        !(current == Some(other) && in_range),               // 35
        rank != 0,                                           // 36
        rank > 1,                                            // 37
    ];
    // Ties go to the nearer by the path agents too (c-SBJ-1: ogre 4 took
    // the fleeing Death Knight at 12.918833, 260.3 from it by his agent).
    let tie = ax * ax + ay * ay;
    Some((key, tie))
}

/// The index of the armor type `fort` (`combat`).
const FORTIFIED: usize = 3;

/// Whether unit `h` rates targets with the creeps' extra parts: a unit of
/// the neutral hostile player (SPEC-0019, "Target rating").
fn rates_as_creep(state: &State, h: u32) -> bool {
    state
        .unit(Some(h))
        .is_some_and(|u| u.owner == super::creeps::CREEPS)
}

/// Whether enemy `other` moves at least as fast as unit `h`: a slower one
/// ranks higher for a creep (SPEC-0048, part 34; c-RZJ-1: the creeps
/// took the Water Elemental, speed 220, over the Paladin, 270, at 1019).
fn not_slower(state: &State, h: u32, other: u32) -> bool {
    // Only creeps rate by speed (SPEC-0019, "Target rating").
    if !rates_as_creep(state, h) {
        return false;
    }
    let speed = |x: u32| {
        state
            .unit(Some(x))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(0.0, |t| real_of(t.speed))
    };
    speed(other) >= speed(h)
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
                // An invulnerable unit is no target (SPEC-0045).
                Some(Object::Unit(o))
                    if o.alive
                        && !o.hidden
                        && !super::cast::invulnerable(state, h)
                        && !super::cast::wind_walking(state, h) =>
                {
                    let data = state.data.units.get(&o.type_id);
                    let collision = data.map_or(0, |d| d.collision);
                    let structure = data.is_some_and(|d| d.building);
                    // Where its path agent was at its last commit (SPEC-0019;
                    // c-SAK-1: kobold 10 did not see footman 7 at 678).
                    let at = match o.movement {
                        Some(m) => {
                            absolute(m.origin, [state.world_bounds[0], state.world_bounds[1]])
                        }
                        _ => [o.x, o.y],
                    };
                    Some((
                        real_of(at[0]),
                        real_of(at[1]),
                        collision,
                        o.owner,
                        structure,
                        h,
                    ))
                }
                _ => None,
            })
            .collect();
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let widest = list.iter().map(|t| real_of(t.2)).fold(0.0f32, f32::max);
        Targets { list, widest }
    }

    /// At a check while unit `h` attacks `current` (SPEC-0019, "Checks
    /// while attacking"): the enemy it should attack instead, if any. The
    /// enemies within half its `acquire` and both collision sizes are
    /// rated with its target by [`rating`], then the strictly nearer; a
    /// tie keeps the target (SPEC-0048).
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
        let mut best = (current, rating(state, h, current, Some(current), threat)?);
        for &(ox, oy, collision, other_owner, _, other) in &self.list {
            if other == current || !enemies(state, owner, other_owner) {
                continue;
            }
            let limit = half + own + real_of(collision);
            let (dx, dy) = (ox - x, oy - y);
            if dx.abs() > limit || dy.abs() > limit || dx * dx + dy * dy > limit * limit {
                continue;
            }
            let Some(rated) = rating(state, h, other, Some(current), threat) else {
                continue;
            };
            // The strictly better key, else the strictly nearer; a tie
            // keeps the one found first.
            if rated.0 < best.1.0 || (rated.0 == best.1.0 && rated.1 < best.1.1) {
                best = (other, rated);
            }
        }
        (best.0 != current).then_some(best.0)
    }

    /// The enemy unit `h` takes within its acquisition range: the best
    /// [`rating`], then the nearer, then by y, x and handle (SPEC-0048).
    fn nearest_enemy(&self, state: &State, h: u32) -> Option<u32> {
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            return None;
        };
        let (owner, x, y) = (u.owner, real_of(u.x), real_of(u.y));
        let data = state.data.units.get(&u.type_id)?;
        // A worker takes no target of its own (c-SBI-1: the peon on hold
        // position, never acquiring).
        if data.weapon.is_none() || real_of(data.acquire) <= 0.0 || u.attack_removed || data.worker
        {
            return None;
        }
        let reach = add(data.acquire, data.collision);
        let widest = self.widest;
        let from = self
            .list
            .partition_point(|t| t.0 < x - real_of(reach) - widest - 1.0);
        let mut best: Option<(Key, f32, f32, f32, u32)> = None;
        for &(ox, oy, collision, other_owner, _, other) in &self.list[from..] {
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
            if dx * dx + dy * dy > limit * limit {
                continue;
            }
            let Some((key, distance)) = rating(state, h, other, None, threat) else {
                continue;
            };
            // The better key, then the nearer, then by y, x and handle.
            let rated = (key, distance, oy, ox, other);
            if best.is_none_or(|b| (rated.0, rated.1, rated.2, rated.3) < (b.0, b.1, b.2, b.3)) {
                best = Some(rated);
            }
        }
        best.map(|b| b.4)
    }
}

/// What enemy `other` does as unit `h` rates it (SPEC-0019, "Choice of
/// target"): 0 when it attacks `h`, 1 when it attacks another unit of
/// `h`'s player or an ally, 2 when it attacks nobody (an attack-move
/// without a target counts as nobody).
/// [`threat`] counting only an attack under way (a swing), not an order,
/// and not that of a stunned unit (c-RZH-1: creep 8, stunned by the Storm
/// Bolt of 943, no threat at 989).
fn swing_threat(state: &State, h: u32, other: u32) -> u8 {
    let tick = state.ticks;
    if state
        .unit(Some(other))
        .is_some_and(|o| o.attack.is_some() && o.stunned_until.is_none_or(|t| tick >= t))
    {
        threat(state, h, other)
    } else {
        2
    }
}

fn threat(state: &State, h: u32, other: u32) -> u8 {
    let Some(o) = state.unit(Some(other)) else {
        return 2;
    };
    // A stunned unit's attack is disabled: no threat (c-RZH-1: creep 8,
    // stunned by the Storm Bolt, at the hero's check of 1003).
    if o.stunned_until.is_some_and(|t| state.ticks < t) {
        return 2;
    }
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
pub(super) const IMPACT_REACH: f32 = 150.0;
/// Seconds after a call for help in which the caller makes no other.
const CALL_COOLDOWN: f32 = 3.0;
/// A creep calls at most once in 0.5 s (SPEC-0030; c-RZJ-1: creep 9).
pub(super) const CREEP_CALL_COOLDOWN: f32 = 0.5;
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
pub(super) fn retaliate(
    state: &mut State,
    victim: u32,
    attacker: u32,
    impact: Option<([u32; 2], f32)>,
) {
    let tick = state.ticks;
    let (Some(v), Some(a)) = (state.unit(Some(victim)), state.unit(Some(attacker))) else {
        return;
    };
    let (owner, x, y, at) = (v.owner, real_of(v.x), real_of(v.y), [a.x, a.y]);
    if !enemies(state, owner, a.owner) {
        return;
    }
    // An attacker that cannot be hurt is no target: no answer, no call
    // (c-SBX-1: the ogre hit by a raised kobold at 3.91995).
    // PROVISIONAL: read for a script invulnerability.
    if state
        .unit(Some(attacker))
        .is_some_and(|a| a.script_invulnerable)
    {
        return;
    }
    // A sleeping creep wakes in this tick with the sleeping creeps near it,
    // and answers; one going home does not answer (SPEC-0030).
    super::creeps::wake_on_hit(state, victim, attacker);
    // A unit that cannot attack, with no order, runs from its attacker:
    // order 851983 to the point 300 straight away from it, ended on
    // arrival; hits on the way give no new order (c-SBJ-1: the Crypt Lord
    // at 10.489942, and again at log 2625).
    // PROVISIONAL: the condition is read for one unit, a hero of a player
    // without its attack ability and with no order.
    if flees(state, victim) {
        let dx = sub(x.to_bits(), at[0]);
        let dy = sub(y.to_bits(), at[1]);
        let len = sqrt(add(mul(dx, dx), mul(dy, dy)));
        if real_of(len) > 0.0 {
            let k = div(FLEE.to_bits(), len);
            let point = [add(x.to_bits(), mul(dx, k)), add(y.to_bits(), mul(dy, k))];
            if point_order(state, victim, "attack", point) {
                super::table::flee_ordered(state, victim);
            }
        }
        return;
    }
    // A unit in its backswing takes the target only for show: no attack,
    // no draw; it acquires at its check after the backswing (c-RXR-1:
    // trolls 16 and 18 in 3689 and 3713).
    let now = real_of(state.event_time.unwrap_or_else(|| state.timers.time()));
    let swinging_back = |u: &super::Unit| real_of(u.backswing_end) > now;
    let answers = |h: u32| match state.objects.get(&h) {
        // One that took the attacker in its own response to the same pass
        // answers its call again (c-RZJ-1: footman 7 at 12.909923).
        Some(Object::Unit(u)) if state.reacquire.contains(&h) => u.alive,
        Some(Object::Unit(u)) => {
            u.alive
                // Not one walking invisible (SPEC-0040; c-SAK-1: the
                // Blademaster hit nothing).
                && u.wind_walk.is_none_or(|w| !w.invisible(tick))
                && !swinging_back(u)
                && !u.paused
                && !u.creep.sleeping
                // One on its way home answers within MaxGuardDistance of it
                // (SPEC-0030).
                && super::creeps::may_answer(state, u)
                && u.attack.is_none()
                // Idle, or walking an attack-move with no target (SPEC-0019,
                // "Spell damage"; c-RZJ-1: footmen 2, 5, 6 and 7 at 788).
                && (walking_attack_move(u)
                    || (u.attack_move.is_none()
                        && (u.movement.is_none_or(|m| !m.ordered() || m.going_back())
                            || u.creep.homebound)))
                // Nor one whose attack ability was removed (c-SBI-1: the heroes
                // at grunt 4's call of 1.799998).
                && !u.attack_removed
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
    // A unit in its backswing after a kill answers by recording the
    // attacker only: its acquired-target event fires, its attack waits for
    // the check after the backswing (SPEC-0019, "During such a backswing";
    // c-SAK-1: gnoll 17 at 4766).
    let records = |h: u32| match state.objects.get(&h) {
        Some(Object::Unit(u)) => {
            u.alive
                && swinging_back(u)
                && u.wind_walk.is_none_or(|w| !w.invisible(tick))
                && !u.paused
                && !u.creep.sleeping
                && super::creeps::may_answer(state, u)
                // Its attack over with its target's death (c-SAK-1: footmen
                // 2 and 4 at 6072, ogre 13 dead at 6052).
                && u.attack.is_none_or(|a| state.unit(Some(a.target)).is_none_or(|t| !t.alive))
                // Nor one going on to a live target (c-SAK-1: footmen 2 and
                // 6 on kobold 10 at 9397, swinging back from a hit on it).
                && u.movement.is_none_or(|m| match m.order {
                    Order::Attack { target } => {
                        state.unit(Some(target)).is_none_or(|t| !t.alive)
                    }
                    _ => true,
                })
                // Nor one whose attack ability was removed (c-SBI-1: the heroes
                // at grunt 4's call of 1.799998).
                && !u.attack_removed
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
            // Also a swing of an attack-move waiting for its cooldown: the
            // unit has that target (c-SAK-1: the Mountain King switched at
            // the call of 3689).
            (Some(a), _) if a.order_id == 0 || u.attack_move.is_some() => a.target,
            // Also one chasing on an attack-move (c-RZH-1: footmen 1 and 3
            // took creep 9 at footman 5's call in 989).
            (None, Some(m)) if m.order_id == 0 || u.attack_move.is_some() => match m.order {
                Order::Attack { target } => target,
                _ => return None,
            },
            _ => return None,
        };
        (u.alive
            && !swinging_back(u)
            && !u.paused
            && !u.creep.returning
            // Nor one whose return in progress is beyond its distance
            // (SPEC-0030; c-SAK-1: gnoll 17 kept footman 4 at gnoll 16's
            // call in 6131).
            && super::creeps::may_answer(state, u)
            && u.wind_walk.is_none_or(|w| !w.invisible(tick))
            && u.casting.is_none()
            && u.stunned_until.is_none_or(|t| state.ticks >= t)
            && target != attacker)
            .then_some(target)
    };
    // An attack-move's target counts as a threat only while its attack is
    // under way (c-RZH-1: creep 8, chasing a footman, rated below creep 9,
    // which hit footman 5).
    let switches = |h: u32| {
        current(h).is_some_and(|c| {
            if state.unit(Some(h)).is_some_and(|u| u.attack_move.is_some()) {
                prefers_by(state, h, c, attacker, swing_threat)
            } else {
                prefers(state, h, c, attacker)
            }
        })
    };
    // The victim calls whatever it is doing (in `c-RXR-1` the troll
    // headhunters answered grunt 12, which was attacking, in tick 688),
    // then each unit that answers calls in turn, in the order of the grid
    // (SPEC-0019, "Order of the responders").
    // A call starts a 3.0 s timer of the victim; while it runs, damage to
    // it makes no call (c-RXR-1: grunt 13 in 4823); a creep's runs 0.5 s.
    // A missile's impact calls instead, round the impact point, with
    // CallForHelp + 150 and no timer (SPEC-0019; test RZG). A unit is
    // reached when its centre is within the radius plus its collision
    // size (c-RXR-1, 6479: trolls at 611.1 in, 635.2 out).
    // A creep's call at an impact is held by its timer and starts it as
    // any other (SPEC-0030; c-RZJ-1: the Ogre Lord at the Archmage's
    // missile in 5917, no call of his at 5975).
    let creep_victim = state
        .unit(Some(victim))
        .is_some_and(|v| v.owner == super::creeps::CREEPS);
    let calls = !state.victim_only
        && ((impact.is_some() && !creep_victim)
            || state
                .unit(Some(victim))
                .is_some_and(|v| now >= real_of(v.call_until)));
    let (x, y) = impact.map_or((x, y), |(p, _)| (real_of(p[0]), real_of(p[1])));
    let radius = real_of(state.combat.call_for_help) + impact.map_or(0.0, |(_, area)| area);
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
                    && (answers(h) || switches(h) || records(h))
            })
            .collect();
        responders.extend(call_order(state, near));
    }
    // The group relays a hit only when its victim is engaged: it has a
    // target or takes the attacker (c-SAK-1: gnoll 16, walking home, made
    // no relay at its zero hit of 3620; kobold 10 did at 1745).
    let engaged = answers(victim)
        || switches(victim)
        || state.unit(Some(victim)).is_some_and(|v| {
            v.attack.is_some()
                || v.movement
                    .is_some_and(|m| matches!(m.order, Order::Attack { .. }))
        });
    if state.call_only || (!answers(victim) && !switches(victim) && !records(victim)) {
        responders.remove(0);
    }
    // The victim's own call comes after its group's (c-SAK-1: at 1025
    // kobold 8 called after members 9 to 14).
    let keep = usize::from(responders.first() == Some(&victim));
    let own_call = responders.split_off(keep);
    // The calls of the victim's group answer as its own does, member by
    // member (SPEC-0030; c-SAK-1: creeps 14, 16 and 18 took footman 2 at
    // his hit on kobold 8 in 1025).
    if engaged {
        for m in relay_callers(state, victim, now) {
            let Some(mu) = state.unit(Some(m)) else {
                continue;
            };
            let (cx, cy) = (real_of(mu.x), real_of(mu.y));
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
                        && dx * dx + dy * dy <= reach_of(u, false) * reach_of(u, false)
                        && state.setup.helps(u.owner, owner)
                        && (answers(h) || switches(h) || records(h))
                })
                .collect();
            responders.extend(call_order(state, near));
        }
    }
    let own_call: Vec<u32> = own_call
        .into_iter()
        .filter(|h| !responders.contains(h))
        .collect();
    responders.extend(own_call);
    let recording: Vec<u32> = responders.iter().copied().filter(|&h| records(h)).collect();
    let switching: Vec<u32> = responders
        .iter()
        .copied()
        .filter(|&h| switches(h))
        .collect();
    if calls
        && (impact.is_none() || creep_victim)
        && let Some(Object::Unit(v)) = state.objects.get_mut(&victim)
    {
        let cooldown = if v.owner == super::creeps::CREEPS {
            CREEP_CALL_COOLDOWN
        } else {
            CALL_COOLDOWN
        };
        v.call_until = (now + cooldown).to_bits();
    }
    // The other members of the victim's group try their own calls, whether
    // the victim's call was made or not (SPEC-0030, "Groups").
    let relayed = engaged && relay(state, victim, attacker, now, &mut reached);
    let calls = calls || relayed;
    if calls {
        for &h in &reached {
            // The victim itself stores nothing; the others store the
            // attacker within 2000 of them (c-SAK-1: footman 4 kept
            // kobold 19 from footman 2's calls, not kobold 10, who hit
            // footman 4 himself).
            // PROVISIONAL: the positions of that test.
            // Nor a unit under a direct order of its own, a move as an
            // attack (c-SBG-1: ogre 3, walking its script move at the
            // clap's call of 2.9, took no remembered target at 8.83).
            let direct = state.unit(Some(h)).is_some_and(|u| {
                u.attack_move.is_none()
                    && u.movement.is_some_and(|m| m.ordered() && m.order_id != 0)
            });
            if h == victim
                || direct
                || !state
                    .unit(Some(h))
                    .zip(state.unit(Some(attacker)))
                    .is_some_and(|(u, a)| {
                        let (dx, dy) = (real_of(a.x) - real_of(u.x), real_of(a.y) - real_of(u.y));
                        dx * dx + dy * dy <= REMEMBERED_REACH * REMEMBERED_REACH
                    })
            {
                continue;
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.remembered = Some(attacker);
            }
        }
    }
    // The creeps a call reaches, and the creep hit, restart a return in
    // progress (SPEC-0030).
    let when = state.event_time.unwrap_or_else(|| state.timers.time());
    if calls {
        for &h in &reached {
            super::creeps::restart_return(state, h, when);
        }
    }
    // The creeps a call reaches may cast their spells at the attacker
    // (SPEC-0047; c-RZJ-1: the Ogre Lord at 2424, creep 9 hit by footman
    // 4).
    if calls {
        for &h in &reached {
            if h != victim {
                super::creeps::consider_spell(state, h, victim, attacker, None);
            }
        }
    }
    if impact.is_none() {
        super::creeps::restart_return(state, victim, when);
    }
    for h in responders {
        // A unit on hold position answers but takes nothing out of its
        // reach (c-SBG-1: the clap's call at 2.9).
        if holds_off(state, h, attacker) {
            continue;
        }
        if recording.contains(&h) {
            state.call_acquired.push(h);
            super::table::call_answered(state, h);
            continue;
        }
        if switching.contains(&h) {
            switch_target(state, h, attacker);
            state.call_acquired.push(h);
            super::table::call_answered(state, h);
            continue;
        }
        // A unit walking an attack-move stops and takes the attacker as at
        // an acquisition.
        let walker = state.unit(Some(h)).is_some_and(walking_attack_move);
        if walker {
            freeze(state, h);
        }
        // A creep on its way home stops and comes (SPEC-0030; c-SAK-1: ogre
        // 18 at 2915).
        if state
            .unit(Some(h))
            .is_some_and(|u| u.creep.returning || u.creep.homebound)
        {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.creep.returning = false;
                u.creep.homebound = false;
            }
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.creep.queued_home = true;
            }
            freeze(state, h);
        }
        let standing = state
            .unit(Some(h))
            .is_some_and(|u| u.movement.is_none_or(|m| !m.ordered()));
        let id = if walker { ORDER_ATTACK } else { 0 };
        if order(state, h, Order::Attack { target: attacker }, id, at) {
            // A player's unit answering from idle guards its place (hub #9;
            // c-SBV-1: footman 6 at 6.799975).
            if !walker
                && standing
                && state
                    .unit(Some(h))
                    .is_some_and(|u| u.owner != super::creeps::CREEPS)
            {
                if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                    u.home = [u.x, u.y];
                }
                super::creeps::guard_start(state, h, now.to_bits());
            }
            state.call_acquired.push(h);
            super::table::call_answered(state, h);
            let delay = mul(
                state.game_random[REACTION].fraction(),
                state.combat.reaction_delay,
            );
            // It waits on the timer queue from the call (c-SAK-1: ogre 18,
            // taking the Mountain King at 34.917, stood at its update of 7028
            // and its delay ended after it, at 35.129936; footman 2, walking
            // his attack-move, at the call of 53.698528, his delay ending at
            // 53.719524; c-RZJ-1: footman 2 at the Shockwave's call of
            // 3.937711, delay 0.011478, its walk at the end, 3.949188).
            react_until(state, h, add(now.to_bits(), delay));
            // A creep that answers enters the fight (SPEC-0030).
            super::creeps::enter_fight(state, h, state.move_clock);
        }
    }
}

/// The relay of a hit on creep `victim` by `attacker` at queue time `now`
/// (SPEC-0030, "Groups"): every other living member of its group, in the
/// order of the group's places, makes its own call once its 0.5 s timer
/// has run, round itself with CreepCallForHelp; such a call starts no
/// further relay. The units the calls reach are added to `reached`;
/// whether any member called (c-RZJ-1: ogre 15 called at 3722, at the
/// fatal hit on ogre 14, and so could call again at 3827).
/// The members of victim's group that make a call at its hit at queue
/// time `now`: the living ones other than it whose call timer has run, in
/// the group's order (`relay`).
fn relay_callers(state: &State, victim: u32, now: f32) -> Vec<u32> {
    super::creeps::group_of(state, victim)
        .into_iter()
        .filter(|&c| {
            c != victim
                && state
                    .unit(Some(c))
                    .is_some_and(|u| u.alive && now >= real_of(u.call_until))
        })
        .collect()
}

/// Whether unit `h` is under Impale's stun.
fn cu_impaled(state: &State, h: u32) -> bool {
    state.unit(Some(h)).is_some_and(|u| u.entries.impale)
}

fn relay(state: &mut State, victim: u32, attacker: u32, now: f32, reached: &mut Vec<u32>) -> bool {
    let creep_reach = real_of(state.combat.creep_call_for_help);
    let mut any = false;
    for c in super::creeps::group_of(state, victim) {
        if c == victim {
            continue;
        }
        // A member thrown up by Impale makes no call, and starts no cooldown
        // (c-SBJ-1: the ogre and kobolds 7, 8 at the swarm's hits of 9.8, called
        // by kobold 8 at 9.919998). A stunned one does (c-SAK-1).
        if cu_impaled(state, c) {
            continue;
        }
        let Some(cu) = state.unit(Some(c)).filter(|u| u.alive) else {
            continue;
        };
        if now < real_of(cu.call_until) {
            continue;
        }
        let (cx, cy, c_owner) = (real_of(cu.x), real_of(cu.y), cu.owner);
        if let Some(Object::Unit(cu)) = state.objects.get_mut(&c) {
            cu.call_until = (now + CREEP_CALL_COOLDOWN).to_bits();
        }
        any = true;
        for h in state.units.clone() {
            let Some(Object::Unit(u)) = state.objects.get(&h) else {
                continue;
            };
            let collision = state
                .data
                .units
                .get(&u.type_id)
                .map_or(0.0, |t| real_of(t.collision));
            let (dx, dy) = (real_of(u.x) - cx, real_of(u.y) - cy);
            let r = creep_reach + collision;
            if u.alive
                && h != attacker
                && !reached.contains(&h)
                && dx * dx + dy * dy <= r * r
                && state.setup.helps(u.owner, c_owner)
            {
                reached.push(h);
            }
        }
    }
    any
}

/// Whether `u` walks an attack-move with no target.
fn walking_attack_move(u: &super::Unit) -> bool {
    u.attack.is_none()
        && u.attack_move.is_some()
        && u.movement
            .is_some_and(|m| m.order == Order::Point && m.ordered())
}

/// Unit `h` takes up its attack-move `a` again after a backswing.
fn take_up(state: &mut State, h: u32, a: AttackMove) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack = None;
    }
    order(state, h, Order::Point, ORDER_ATTACK, a.point);
    // A new goal with its walk; the acquisition's objects go (SPEC-0024,
    // "Unit death"; c-SAK-1: footmen 4 and 5 at 2757).
    super::table::attack_move_resumed(state, h);
    set_attack_move(
        state,
        h,
        Some(AttackMove {
            resume_at: None,
            resume_q: None,
            periodic: None,
            ..a
        }),
    );
}

/// The attack-moves taken up again at backswing ends before the update time
/// `at`: before the updates (SPEC-0018; c-RZJ-1: the Archmage at 37.40833,
/// before the update of 7484).
pub(super) fn resumes_before(state: &mut State, at: u32) {
    let due: Vec<(u32, AttackMove)> = state
        .units
        .iter()
        .filter_map(|&h| match state.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive => u.attack_move.map(|a| (h, a)),
            _ => None,
        })
        .filter(|(_, a)| a.resume_q.is_some_and(|q| real_of(q) <= real_of(at)))
        .collect();
    for (h, a) in due {
        take_up(state, h, a);
        // Before the updates of this tick it takes part in them.
        let tick = state.ticks;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
            && let Some(m) = u.movement.as_mut()
            && m.next_update > tick
        {
            m.next_update = tick;
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
/// With `until`, only those expiring up to it, the rest left for later
/// in the tick.
/// The earliest cooldown expiry of this tick that `cooldown_expiries`
/// would take a chase from.
pub(super) fn next_cooldown_expiry(state: &State) -> Option<u32> {
    let start = real_of(state.cooldowns_done.unwrap_or(state.move_clock));
    let end = real_of(add(state.move_clock, crate::timers::TICK));
    state
        .units
        .iter()
        .filter_map(|&h| {
            // (Not while it casts: a cast stops its attack, c-SBY-1: the Keeper
            // channelling Tranquility from 9.5 s.)
            let u = state.unit(Some(h)).filter(|u| {
                u.alive
                    && u.casting.is_none()
                    && (u.attack.is_none()
                        || u.attack.as_ref().is_some_and(|a| a.renew_at.is_some()))
            })?;
            let m = u.movement?;
            let Order::Attack { target } = m.order else {
                return None;
            };
            // An attack waiting for the cooldown goes too when its target left
            // its reach (c-RXD-1: kobold 1048690 at 35.249161).
            if u.attack.is_some() && reaches(state, h, target) {
                return None;
            }
            let e = real_of(u.cooldown_end);
            (e > start && e <= end && state.unit(Some(target)).is_some_and(|t| t.alive))
                .then_some(u.cooldown_end)
        })
        .min_by(|a, b| real_of(*a).total_cmp(&real_of(*b)))
}

fn cooldown_expiries(state: &mut State, until: Option<u32>) {
    let done = state.cooldowns_done.unwrap_or(state.move_clock);
    if let Some(u) = until
        && real_of(u) <= real_of(done)
    {
        return;
    }
    state.cooldowns_done = until;
    let start = real_of(done);
    let end = until.map_or(real_of(add(state.move_clock, crate::timers::TICK)), real_of);
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let due: Vec<(u32, u32, i32)> = state
        .units
        .iter()
        .filter_map(|&h| {
            // (Not while it casts: a cast stops its attack, c-SBY-1: the Keeper
            // channelling Tranquility from 9.5 s.)
            let u = state.unit(Some(h)).filter(|u| {
                u.alive
                    && u.casting.is_none()
                    && (u.attack.is_none()
                        || u.attack.as_ref().is_some_and(|a| a.renew_at.is_some()))
            })?;
            let m = u.movement?;
            let Order::Attack { target } = m.order else {
                return None;
            };
            // An attack waiting for the cooldown goes too when its target left
            // its reach (c-RXD-1: kobold 1048690 at 35.249161).
            if u.attack.is_some() && reaches(state, h, target) {
                return None;
            }
            let e = real_of(u.cooldown_end);
            // The cooldown timer's own handler: the attack start on the target
            // it holds, the same one too (c-RXD-1: kobold 1048692 at
            // 12.389934, out of reach: a chase). Nothing for a dead target
            // (c-SAK-1: the image at 56.305359).
            (e > start && e <= end && state.unit(Some(target)).is_some_and(|t| t.alive))
                .then_some((h, target, m.order_id))
        })
        .collect();
    let tick = state.ticks;
    for (h, target, order_id) in due {
        // An attack waiting for the cooldown ends with it.
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.attack = None;
        }
        // The goals of backswings ending before this expiry come first, as
        // one queue (c-SAK-1: ogre 18's before footman 2's chase at 6174).
        if let Some(end) = state.unit(Some(h)).map(|u| u.cooldown_end) {
            super::attack::after_swing_walks(state, Some(end));
        }
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
        // The routine before a swing (SPEC-0019): in range and within
        // AttackHalfAngle the swing starts; in range further off, a face
        // movement; out of range, a chase, its goal the target at this
        // tick (c-RXD-1: creep 9 at 6859).
        let in_reach = in_range(here, at, reach);
        let turned = real_of(heading_and_turn(&m, at).1).abs() <= ATTACK_HALF_ANGLE;
        // In reach but its stored heading more than AttackHalfAngle off the
        // target: a face movement that takes a walk, as a chase (SPEC-0019,
        // "The face movement takes a walk"; c-RXD-1: kobold 1048692 2.33 rad
        // off at 35.039223).
        let in_reach = in_reach && (turned || m.faced);
        // In reach but not facing it: a face movement to where the target
        // stands now, a fixed point (SPEC-0019, "Cooldown expiry").
        if in_reach && !turned && m.faced {
            let p = position_of(state, target);
            stop_commit(state, h);
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(m) = u.movement.as_mut()
                && let Some(p) = p
            {
                m.target = p;
                m.unlimited = true;
                m.chase = Some(u32::MAX);
                m.renew_at = None;
                m.renew_stands = false;
            }
            // A new goal with its walk, at the expiry (c-SAK-1: the image 20
            // at 49.25).
            if let Some(end) = state.unit(Some(h)).map(|u| u.cooldown_end) {
                state.table_at(end);
                super::table::walk_made_at_order(state, h);
                state.table.now = tick;
            }
            continue;
        }
        freeze(state, h);
        if in_reach && turned {
            super::attack::begin(state, h, target, tick, order_id);
        } else {
            let goal = tick_start_position(state, target);
            follow::regroup(state, h);
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(m) = u.movement.as_mut()
            {
                m.walking = false;
                m.velocity = [0, 0];
                if let Some(goal) = goal {
                    m.target = goal;
                }
                m.follow_check = !in_reach;
                // The chase out of reach replaces the renewal after the
                // swing: its search is at the next update (c-SAK-1: the
                // Mountain King, cooldown end 9538, search at 9543).
                if !in_reach {
                    m.renew_at = None;
                    m.renew_stands = false;
                }
                // A chase out of range is a new group, the newest (SPEC-0021;
                // c-RZJ-1: footman 7's of 7232 before the Paladin's of 7218
                // at 7250).
                if !in_reach {
                    state.orders_given += 1;
                    m.given = state.orders_given;
                }
            }
            // A chase out of reach is a new goal with its walk, at the
            // cooldown's expiry (SPEC-0024; c-SAK-1: footman 3 at 18.119986).
            // Not in a reaction delay (c-SAK-1: footman 6 at 3193, whose
            // chase came at its reaction end).
            if !in_reach
                && let Some(end) = state
                    .unit(Some(h))
                    .filter(|u| {
                        u.movement
                            .is_none_or(|m| m.reaction_end.is_none() && m.reaction.is_none())
                    })
                    .map(|u| u.cooldown_end)
            {
                state.table_at(end);
                super::table::walk_made_at_order(state, h);
                state.table.now = state.ticks;
            }
        }
    }
}

/// Whether a unit's movement walks now (a non-zero velocity).
pub(super) fn walking_now(m: &Moving) -> bool {
    m.walking && m.velocity != [0, 0]
}

/// The value of a move-to-target entry of an order queue: its arrival
/// distance before the collision size (SPEC-0030; c-SAK-1).
const MOVE_TO_TARGET: f32 = 181.0;

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
                .then(|| (u.target_taken.map_or(0, |t| t.1), h, order_id == 0))
        })
        .collect();
    attackers.sort_unstable();
    let mut to_guard = Vec::new();
    for (_, h, own) in attackers {
        // Its objects kept to the target's death go with its handling, the
        // current target's first (SPEC-0024; c-SAK-1: 4764).
        super::table::attacker_lost(state, h, dead);
        // (Whether a script's attack order on it ends here.)
        let scripted = !own
            && state
                .unit(Some(h))
                .is_some_and(|u| u.attack_move.is_none() && u.entries.script_attack);
        // A script's attack order on it ends with an object, freed 0.0001 s
        // later (c-SBL-1: the Mountain King's at kobold 3's death, 2.354989).
        if !own
            && state
                .unit(Some(h))
                .is_some_and(|u| u.attack_move.is_none() && u.entries.script_attack)
        {
            let rest = super::table::script_attack_lost(state, h, now);
            super::table::attack_ended(state, h, false);
            for o in rest {
                state.table.defer(o, "attack: order end");
            }
            // Its attack on it ends (c-SBO-1: the footman after its kill of
            // kobold 5 at 7.28, idle when kobold 6 turns vulnerable at 8.5).
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
                && u.attack.is_some_and(|a| a.target == dead)
            {
                u.attack = None;
            }
        }
        super::attack::swing_cut(state, h, now);
        // A creep that took a target on its way home goes on to the next
        // entry of its queue: a move to where the target stood, without
        // checks, its return timer started again; already there (within
        // 181 and its collision), that entry ends and its walk home comes,
        // as below (SPEC-0030; c-SAK-1: at footman 7's death in 4765 ogre
        // 18 walked there, kobold 19 was there).
        let queued = state
            .unit(Some(h))
            .filter(|u| u.creep.queued_home)
            .zip(position_of(state, dead))
            .is_some_and(|(u, p)| {
                let reach = MOVE_TO_TARGET
                    + state
                        .data
                        .units
                        .get(&u.type_id)
                        .map_or(0.0, |t| real_of(t.collision));
                let (dx, dy) = (sub(u.x, p[0]), sub(u.y, p[1]));
                real_of(add(mul(dx, dx), mul(dy, dy))) > reach * reach
            });
        // A return in progress starts again at the death of its target
        // (c-SAK-1: kobold 19 and ogre 18 at footman 7's death in 4765 and
        // footman 5's in 5444).
        let back = state.combat.guard_return_time;
        let start = state.creep_timers + 1;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            let queued = std::mem::take(&mut u.creep.queued_home);
            // A creep's only: a player's unit keeps its timer (c-SBV-1: footman
            // 6, kobold 5 dead at log 5360, hub #9).
            if queued || (u.creep.return_time.is_some() && u.owner == super::creeps::CREEPS) {
                u.creep.return_time = Some(add(now, back));
                // Started anew: equal expiries fire in the order of the
                // starts.
                u.creep.started = start;
                state.creep_timers = start;
            }
        }
        if queued && let Some(point) = position_of(state, dead) {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.attack = None;
            }
            // Stopped where it stands by the world clock (c-SAK-1: ogre 18
            // at 4765, at its position of the tick).
            let clock = state.move_clock;
            stop_at(state, h, clock);
            order(state, h, Order::Point, 0, point);
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.checks_off = true;
                u.creep.queued_home = true;
            }
            // Its attack ends with an object kept with the move, and the move
            // is a new goal with its walk (SPEC-0024; c-SAK-1: 4764).
            super::table::attack_ended(state, h, true);
            super::table::walk_made_at_order(state, h);
            super::table::attacker_done(state, h);
            continue;
        }
        let mut walk_on = None;
        state.acquire_starts += 1;
        let seq = state.acquire_starts;
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            let from = if f32::from_bits(u.backswing_end) > f32::from_bits(now) {
                u.backswing_end
            } else {
                now
            };
            // Without an attack-move, its own periodic check due in the
            // backswing goes on there and only records (c-SAK-1: the image
            // 20, ogre 18 dead at 54.398029, its check of 54.553837).
            // Under a script's attack order it ran no checks: only the one
            // at the order's end (c-SBV-1: the Mountain King, the ogre dead
            // at log 3154, his check at 17.259380, hub #9).
            let restart = add(from, AFTER_DEATH);
            if u.attack_move.is_none()
                && !scripted
                && f32::from_bits(u.backswing_end) > f32::from_bits(now)
                && real_of(u.next_acquire) < real_of(restart)
                && real_of(u.next_acquire) > real_of(now)
            {
                u.idle_periodic = Some(u.next_acquire);
            }
            u.next_acquire = restart;
            // The attack-move's check is that timer (c-RXR-1: rifleman 8,
            // grunt 12 dead at 2579, restart at its backswing end 2701).
            if let Some(a) = u.attack_move.as_mut() {
                keep_periodic(a, add(from, AFTER_DEATH));
            }
            // An attack-move goes on at once, or at the end of the
            // backswing (c-RXR-1: footman 2 searched towards its point at
            // its update of 2581).
            // Not in a backswing it walks on in the handling of the death,
            // in the order of the attackers (c-RZH-1: footmen 1, 4 and the
            // hero at 6966, their new clusters in that order).
            let left = real_of(from) - real_of(now);
            let at = state.ticks + 1 + (left / real_of(crate::timers::TICK)).ceil() as u64;
            if let Some(a) = u.attack_move.as_mut()
                && a.resume_at.is_none()
            {
                if left > 0.0 {
                    a.resume_at = Some(at);
                    a.resume_q = Some(from);
                } else {
                    walk_on = Some(a.point);
                }
            }
            // Below an attack it took on its own lies the walk back to its
            // guard point (c-RXR-1: grunt 14 at 4813). PROVISIONAL: the
            // 50 it accepts; creeps go home by SPEC-0030.
            // The walk to its guard point waits for the end of its backswing
            // (c-RXD-1: creep 9, whose hit killed unit 3 at 6665, stood in
            // its backswing); out of it, it walks at once, a player's unit as
            // a creep (c-RXR-1: grunts 1048698 and 1048699 at 11.799021).
            let swinging = f32::from_bits(u.backswing_end) > f32::from_bits(now);
            if own && swinging {
                u.guard_return = Some(at);
            }
            if own && !swinging {
                to_guard.push(h);
            }
            u.acquire_seq = seq;
            u.after_death = true;
            if own {
                u.attack = None;
            }
        }
        if let Some(point) = walk_on {
            if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
                u.attack = None;
            }
            // A walker stops where it is at the death (c-RZJ-1: footmen 4
            // and 7 at the Ogre Lord's death in 7430, standing for the
            // searches of that tick).
            // A melee kill, inside the main timer queue, reads the position
            // by the clock of the tick (c-SAK-1: the Mountain King at ogre
            // 18's death in 10883); a missile's impact at its own time.
            let clock = if state.impact_now {
                now
            } else {
                state.move_clock
            };
            stop_at(state, h, clock);
            order(state, h, Order::Point, ORDER_ATTACK, point);
            // Its new goal and walk (SPEC-0024; c-SAK-1: 3185).
            super::table::attack_move_resumed(state, h);
        }
        let mut walked = false;
        if own {
            halt(state, h);
            // An attack taken by answering a call ends with an object, freed
            // 0.0001 s later; one taken by its own check makes none
            // (SPEC-0024, "Unit death"; c-SAK-1: 4764).
            // A creep's only: a player's unit makes none (c-RXR-1: headhunter
            // 1048700 at 11.799021).
            if state
                .unit(Some(h))
                .is_some_and(|u| u.entries.by_call && u.owner == super::creeps::CREEPS)
            {
                super::table::attack_ended(state, h, false);
            }
            // A creep walks back to its guard point at once, where its
            // last walk home ended (c-RXD-1: creep 8 at 6665, turning to it
            // at 6667). Not one that took the target at a call: it stays
            // (c-SAK-1: kobold 10, taken by gnoll 12's call at 4767, at
            // footman 5's death in 5442).
            // PROVISIONAL: read for a camp's call only.
            let called = state
                .unit(Some(h))
                .is_some_and(|u| (u.entries.called || u.entries.by_call) && !u.entries.game_order);
            if to_guard.contains(&h)
                && !called
                && let Some(u) = state.unit(Some(h))
            {
                let guard = u.creep.guard.unwrap_or(u.home);
                // Already there (within 50 and its collision) it stays idle
                // (c-SAK-1: kobold 10 at footman 7's death in 4765, taken
                // by gnoll 12's relay at 4767).
                let accept = GUARD_ACCEPT
                    + state
                        .data
                        .units
                        .get(&u.type_id)
                        .map_or(0.0, |t| real_of(t.collision));
                let (dx, dy) = (
                    real_of(u.x) - real_of(guard[0]),
                    real_of(u.y) - real_of(guard[1]),
                );
                if dx * dx + dy * dy > accept * accept + 1e-3 {
                    order(state, h, Order::Point, 0, guard);
                    super::table::walk_made_at_order(state, h);
                    walked = true;
                    super::table::acquisition_lost_with(state, h);
                }
            }
            // One that stays has its order end with it, unless it is in
            // its backswing (c-SAK-1: kobold 10 at 4764 and at 5442; gnolls
            // 12 and 17, swinging back at 5442, kept theirs).
            let swinging = state
                .unit(Some(h))
                .is_some_and(|u| f32::from_bits(u.backswing_end) > f32::from_bits(now));
            if !walked && !swinging {
                super::table::order_lost_with(state, h);
            }
            // In its backswing, the objects it kept to the death go at the
            // backswing's end (c-SAK-1: the image 20 at ogre 18's death in
            // 10880, freed at 10950 with its order).
            if !walked && swinging {
                super::table::lost_at_backswing_end(state, h);
            }
        }
        super::table::attacker_done(state, h);
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
/// Unit `h`'s chase begun at a cooldown's expiry replaces the renewal after
/// its swing: it searches at its next update.
pub(super) fn no_renewal(state: &mut State, h: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        m.renew_at = None;
        m.renew_stands = false;
    }
}

/// The swing of unit `h` would start now: when it does not face its
/// target within AttackHalfAngle, its swing is dropped and a face movement
/// starts towards the target's position now, fixed, the swing coming at
/// its end (SPEC-0019, "Cooldown expiry"). Whether it did.
pub(super) fn face_at_expiry(state: &mut State, h: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let (Some(a), Some(m)) = (u.attack, u.movement) else {
        return false;
    };
    let Some(at) = position_of(state, a.target) else {
        return false;
    };
    if real_of(heading_and_turn(&m, relative(at, min)).1).abs() <= ATTACK_HALF_ANGLE {
        return false;
    }
    stop_commit(state, h);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h)
        && let Some(m) = u.movement.as_mut()
    {
        u.attack = None;
        m.target = at;
        m.faced = true;
        m.unlimited = true;
        m.chase = Some(u32::MAX);
        m.renew_at = None;
        m.renew_stands = false;
    }
    // The face movement is a new goal with its walk, at the expiry
    // (SPEC-0024; c-SAK-1: the Blademaster at 48.293095).
    let tick = state.ticks;
    state.table_at(a.swing_q);
    super::table::walk_made_at_order(state, h);
    state.table.now = tick;
    true
}

/// The units whose attack-move goes on at a backswing's end due before
/// the next tick's clock (and before `before`), with that end: the game
/// takes the new goal there, a tick or two before the walk on here
/// (SPEC-0024; c-SAK-1: footmen 4 and 5 at 2757).
pub(super) fn attack_moves_resuming(state: &State, before: Option<u32>) -> Vec<(u32, u32)> {
    let next = real_of(add(state.move_clock, crate::timers::TICK));
    state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            let q = u.attack_move?.resume_q?;
            (u.attack_move?.resume_at.is_some()
                && real_of(q) < next
                && before.is_none_or(|b| real_of(q) < real_of(b))
                && !u.entries.early_walk)
                .then_some((h, q))
        })
        .collect()
}
/// Whether unit `h` moves under an attack order.
pub(super) fn attacks(state: &State, h: u32) -> bool {
    state
        .unit(Some(h))
        .and_then(|u| u.movement)
        .is_some_and(|m| matches!(m.order, Order::Attack { .. }))
}

/// PickupItemRange of the game's constants, plus the hero's collision
/// size (c-SBQ-1: the Archmage, collision 32, picked up the potion 156.2
/// away without a step).
/// PROVISIONAL: the collision added is read from that one case.
const PICKUP_RANGE: f32 = 150.0;

/// DropItemRange of the game's constants (c-SBR-1: the salve dropped 91.2
/// from its point).
const DROP_RANGE: f32 = 100.0;

/// The order of an item drop (`UnitDropItemPoint`; c-SBR-1: 852001).
const ORDER_DROP: i32 = 852_001;

/// Hero `h` takes a drop order for the `item` it carries at `point` (c-SBR-1
/// at 9.5): the order's objects as for a pick-up, a walk.
pub(super) fn drop_order(state: &mut State, h: u32, item: u32, point: [u32; 2]) -> bool {
    if !matches!(state.objects.get(&item), Some(Object::Item(i)) if i.carried_by == Some(h)) {
        return false;
    }
    if !order(state, h, Order::Drop { item }, ORDER_DROP, point) {
        return false;
    }
    super::table::item_ordered(state, h);
    true
}

/// Whether hero `h` can pick up `item`: an item on the ground.
fn can_pick_up(state: &State, h: u32, item: u32) -> bool {
    matches!(state.objects.get(&item), Some(Object::Item(i)) if i.carried_by.is_none())
        && state
            .unit(Some(h))
            .is_some_and(|u| u.alive && crate::data::is_hero(u.type_id))
}

/// Hero `h` takes a smart order on `item` (c-SBQ-1 at 2.999973): the order
/// and its objects, a walk made as for a move.
fn pick_up_order(state: &mut State, h: u32, item: u32) -> bool {
    let Some(at) = position_of(state, item) else {
        return false;
    };
    if !order(state, h, Order::PickUp { item }, ORDER_SMART, at) {
        return false;
    }
    super::table::item_ordered(state, h);
    true
}

/// How far a unit that cannot attack runs from its attacker (c-SBJ-1).
const FLEE: f32 = 300.0;

/// Whether unit `h`, hit, runs from its attacker: it cannot attack and
/// has no order, and is not on hold position.
fn flees(state: &State, h: u32) -> bool {
    state.unit(Some(h)).is_some_and(|u| {
        u.alive
            && u.attack_removed
            // (Or a finished one: c-SBJ-1, the Crypt Lord hit again at
            // 14.118631, his flee of 10.49 arrived.)
            && u.movement.is_none_or(|m| !m.ordered())
            && u.casting.is_none()
            && !u.holding
            && u.attack_move.is_none()
            && !super::cast::stunned(state, h)
    })
}

/// The living workers building `building` now (order `repair` on it), in
/// the order of the units.
pub(super) fn workers_of(state: &State, building: u32) -> Vec<u32> {
    state
        .units
        .iter()
        .copied()
        .filter(|h| match state.objects.get(h) {
            Some(Object::Unit(u)) if u.alive => {
                u.movement.map(|m| m.order) == Some(Order::Repair { building })
            }
            _ => false,
        })
        .collect()
}

/// Whether worker `h` at `here` (cells) is near enough to `building` to
/// take up its work: within Repair's range plus both collision sizes of
/// the building's centre (c-SBZ-1: peasant 1 stopped 136.5 from the farm,
/// 50 + 72 + 16 = 138, hub #13).
fn repair_reach(state: &State, h: u32, here: [u32; 2], building: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let collision = |o: u32| {
        state
            .unit(Some(o))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(0, |t| t.collision)
    };
    let range = state
        .data
        .abilities
        .get(&i32::from_be_bytes(*b"Ahrp"))
        .map_or(0, |a| a.range);
    let Some(at) = position_of(state, building) else {
        return false;
    };
    let reach = mul(add(add(range, collision(h)), collision(building)), CELL);
    in_range(here, relative(at, min), reach)
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
