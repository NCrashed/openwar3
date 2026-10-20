//! Queued orders (SPEC-0031): orders given with Shift.
//!
//! Each unit has a queue behind its current order. A queued order given
//! to a unit without a current order is carried out at once; else it goes
//! to the end of the queue. An order without Shift replaces the current
//! order and empties the queue. When the current order ends, the first
//! queued order starts as a new order given at that update. An order with
//! no end of its own (`patrol`) keeps the rest waiting.
//!
//! A queued group order lays out its places (SPEC-0025) as if the members
//! stood at their places of the order before, and each member walks to
//! its place on its own when its order before ends.
//!
//! PROVISIONAL: a queued `stop` makes the unit stand 12 ticks more
//! before the next order (the one case measured: 24 ticks at a 90 degree
//! turn, which alone gives 12); queued orders on units, building and
//! training orders with Shift are not covered (open question 1).

use std::collections::VecDeque;

use ow3_math::mul;

use super::movement;
use super::{Object, State};

/// A queued order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Queued {
    /// An order to a point (`move`, `attack`, `patrol`, ...): its name.
    Point {
        name: &'static str,
        target: [u32; 2],
    },
    /// An order on a widget.
    Target { name: &'static str, target: u32 },
    /// `stop` and `holdposition`.
    Immediate { name: &'static str },
}

/// Ticks a queued `stop` keeps the unit standing.
const STOP_TICKS: u64 = 12;

/// The queue of a unit and the tick until which it waits after a queued
/// `stop`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    orders: VecDeque<Queued>,
    wait_until: Option<u64>,
    shown: i32,
    /// The id of the order the queue goes on from: the current one when
    /// the first was queued, then each started from it.
    last: i32,
}

/// Whether unit `h` has a current order: it walks, attacks, attack-moves,
/// gathers, builds, or waits after a queued `stop`.
fn busy(state: &State, h: u32) -> bool {
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    u.movement.is_some_and(|m| m.ordered())
        || u.attack.is_some()
        || u.attack_move.is_some()
        || u.orders.wait_until.is_some_and(|t| t > state.ticks)
}

/// An order without Shift: the queue of unit `h` is emptied.
pub(super) fn clear(state: &mut State, h: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.orders = Queue::default();
    }
}

/// Queues `order` for unit `h`: at once for a unit without a current
/// order, else at the end of its queue. Whether the unit took it.
pub(super) fn push(state: &mut State, h: u32, order: Queued) -> bool {
    let idle = !busy(state, h)
        && state
            .unit(Some(h))
            .is_some_and(|u| u.orders.orders.is_empty());
    if idle {
        return start(state, h, order);
    }
    match state.objects.get_mut(&h) {
        Some(Object::Unit(u)) if u.alive => {
            if u.orders.orders.is_empty() {
                u.orders.last = u.movement.map_or(0, |m| m.last_order_id());
            }
            u.orders.orders.push_back(order);
            true
        }
        _ => false,
    }
}

/// Starts `order` for unit `h` as a new order, keeping the rest of its
/// queue.
fn start(state: &mut State, h: u32, order: Queued) -> bool {
    let rest = match state.objects.get_mut(&h) {
        Some(Object::Unit(u)) => std::mem::take(&mut u.orders),
        _ => return false,
    };
    let done = match order {
        Queued::Point { name, target } => movement::point_order(state, h, name, target),
        Queued::Target { name, target } => movement::target_order(state, h, name, target),
        Queued::Immediate { name: "stop" } => true,
        Queued::Immediate { name } => movement::immediate_order(state, h, name),
    };
    let tick = state.ticks;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.orders = rest;
        if order == (Queued::Immediate { name: "stop" }) {
            // The current order reads the one before through the wait
            // (SPEC-0031: 851986 throughout between two moves).
            u.orders.wait_until = Some(tick + STOP_TICKS);
            u.orders.shown = u.orders.last;
        } else if let Some(m) = u.movement {
            u.orders.last = m.last_order_id();
        }
    }
    done
}

/// The order id unit `h` shows while it waits after a queued `stop`.
pub(super) fn shown_order(state: &State, h: u32) -> Option<i32> {
    let u = state.unit(Some(h))?;
    u.orders
        .wait_until
        .filter(|&t| t > state.ticks)
        .map(|_| u.orders.shown)
}

/// The units whose current order ended start their next queued order
/// (after the updates of the tick, as an order given at that update).
pub(super) fn pass(state: &mut State) {
    let units = state.units.clone();
    for h in units {
        let next = match state.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive && !u.orders.orders.is_empty() => {
                (!busy(state, h)).then_some(())
            }
            _ => None,
        };
        if next.is_none() {
            continue;
        }
        let order = match state.objects.get_mut(&h) {
            Some(Object::Unit(u)) => {
                u.orders.wait_until = None;
                u.orders.orders.pop_front()
            }
            _ => None,
        };
        if let Some(order) = order {
            start(state, h, order);
        }
    }
}

/// A queued group order `name` to `target` for `units` (SPEC-0031,
/// "Groups"): the places are laid out from the members' places of the
/// order before (the last point each has queued, else the point it walks
/// to, else where it stands), and each member queues a walk to its own.
pub(super) fn push_group(state: &mut State, units: &[u32], name: &'static str, target: [u32; 2]) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cell = |p: [u32; 2]| {
        let r = movement::relative(p, min);
        [mul(r[0], movement::CELL), mul(r[1], movement::CELL)]
    };
    let mut members = Vec::new();
    let mut handles = Vec::new();
    for &h in units {
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        let before = u
            .orders
            .orders
            .iter()
            .rev()
            .find_map(|q| match q {
                Queued::Point { target, .. } => Some(*target),
                _ => None,
            })
            .or_else(|| u.movement.filter(|m| m.ordered()).map(|m| m.target_point()))
            .unwrap_or([u.x, u.y]);
        let data = state.data.units.get(&u.type_id);
        members.push(super::formation::Member {
            position: cell(before),
            radius: mul(data.map_or(0, |t| t.collision), movement::CELL),
            rank: data.map_or(0, |t| t.formation),
        });
        handles.push(h);
    }
    if members.len() < 2 {
        for h in handles {
            push(state, h, Queued::Point { name, target });
        }
        return;
    }
    let places = movement::group_places(state, &members, cell(target));
    for (h, place) in handles.into_iter().zip(places) {
        let target = movement::cells_to_world(place, min);
        push(state, h, Queued::Point { name, target });
    }
}
