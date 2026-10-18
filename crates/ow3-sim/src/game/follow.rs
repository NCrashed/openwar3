//! How a ground unit follows its path round static obstacles (SPEC-0020,
//! "Following the path").
//!
//! At its first update after an order the unit searches the high-level
//! list of waypoints to the order point. It then walks to segment goals
//! about 10 blocks apart along that list, each by a local path of cells
//! that it shortcuts where it can see far enough, and stands one update
//! at the end of each local path, when the cells one step ahead are
//! blocked, and while a search is held back after a long one.
//!
//! PROVISIONAL: a unit whose block is not free walks straight to the order
//! point; a local path that cannot leave its start counts as walked; the
//! search budgets shared between units and repeated blocks (open questions
//! 3 and 4) are not modelled.

use ow3_math::{add, mul, sub};

use super::State;
use crate::path::{self, Grid};

/// 1/32, 32 and 0.49, the arrival radius in cells.
const CELL: u32 = 0x3D00_0000;
const WORLD: u32 = 0x4200_0000;
const ARRIVAL: u32 = 0x3EFA_E148;
/// Leg lengths of a segment, in blocks (10.0).
const SEGMENT: f32 = 10.0;
/// The iteration limit of a local search.
const LOCAL_LIMIT: u32 = 700;
/// A search of this many iterations or more holds back the next one of
/// the same unit for `HOLD` ticks.
const LONG_SEARCH: u32 = 64;
const HOLD: u64 = 60;

fn real(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// The path a unit follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Follow {
    /// The high-level waypoints in blocks, from the start, and whether
    /// the last one is the order point.
    waypoints: Vec<[u32; 2]>,
    reached: bool,
    /// The waypoint last taken as a segment goal (0: the start).
    current: usize,
    /// The segment goal in cells, and whether it is the order point.
    goal: [u32; 2],
    final_goal: bool,
    /// The local path in cells, whether it ends short of its goal, and the
    /// point walked to.
    local: Vec<[u32; 2]>,
    partial: bool,
    target: usize,
    /// What the next update must do first.
    next: Next,
    /// The first tick at which the unit may search again.
    hold_until: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    /// Follow the local path.
    Walk,
    /// Search a local path to the segment goal.
    Search,
    /// Take the next segment goal, then search.
    Segment,
}

/// What a unit does at an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steer {
    /// Turn and move towards this point (relative to the world corner).
    Walk([u32; 2]),
    /// No motion, the heading turning towards this point.
    Stand([u32; 2]),
    /// At the order point.
    Arrived,
}

fn to_cells(p: [u32; 2]) -> [u32; 2] {
    p.map(|c| mul(c, CELL))
}

fn to_world(p: [u32; 2]) -> [u32; 2] {
    p.map(|c| mul(c, WORLD))
}

fn close(a: [u32; 2], b: [u32; 2]) -> bool {
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    real(add(mul(dx, dx), mul(dy, dy))) <= real(mul(ARRIVAL, ARRIVAL))
}

impl Follow {
    /// Step 1: the first waypoint from the current one at which the legs
    /// add up to 10 blocks, else the order point.
    fn take_segment(&mut self, order: [u32; 2]) {
        let mut sum = 0u32;
        for j in self.current + 1..self.waypoints.len() {
            sum = add(sum, path::length(self.waypoints[j - 1], self.waypoints[j]));
            if real(sum) >= SEGMENT {
                self.current = j;
                // The last waypoint of a list that got there is the order
                // point itself.
                if self.reached && j == self.waypoints.len() - 1 {
                    break;
                }
                self.goal = path::blocks_to_cells(self.waypoints[j]);
                self.final_goal = false;
                return;
            }
        }
        self.goal = order;
        self.final_goal = true;
    }
}

/// What unit `h` of size class `class` at `here` (relative to the world
/// corner) does at this update on its way to `order`; `tick` is the
/// current tick.
pub fn steer(
    state: &mut State,
    h: u32,
    here: [u32; 2],
    order: [u32; 2],
    class: u8,
    tick: u64,
) -> Steer {
    let Some(grid) = state.grid.as_ref() else {
        return Steer::Walk(order);
    };
    let (at, order_cells) = (to_cells(here), to_cells(order));
    let follow = state.paths.entry(h).or_insert_with(|| {
        let (waypoints, reached) = path::high_level(grid, at, order_cells, class).map_or_else(
            || (Vec::new(), false),
            |found| (found.points, found.reached),
        );
        let mut follow = Follow {
            waypoints,
            reached,
            current: 0,
            goal: order_cells,
            final_goal: true,
            local: Vec::new(),
            target: 0,
            partial: false,
            next: Next::Search,
            hold_until: 0,
        };
        follow.take_segment(order_cells);
        follow
    });
    if follow.waypoints.is_empty() {
        return Steer::Walk(order);
    }
    if follow.next == Next::Segment {
        follow.take_segment(order_cells);
        follow.next = Next::Search;
    }
    let segment_goal = to_world(follow.goal);
    if follow.next == Next::Search {
        if tick < follow.hold_until {
            return Steer::Stand(segment_goal);
        }
        let found = path::local(grid, at, follow.goal, class, LOCAL_LIMIT);
        if found.iterations >= LONG_SEARCH {
            follow.hold_until = tick + HOLD;
        }
        follow.local = found.points;
        follow.partial = !found.reached;
        // A search that met nothing in the way walks straight to the end.
        follow.target = if found.clear {
            follow.local.len() - 1
        } else {
            1.min(follow.local.len() - 1)
        };
        follow.next = Next::Walk;
    }
    // Arrival at the point walked to.
    let last = follow.local.len() - 1;
    if follow.target == last && (last == 0 || close(at, follow.local[last])) {
        let reached = follow.local[last];
        if follow.partial {
            // The end of a path that did not get there: search again.
            follow.next = Next::Search;
            return Steer::Stand(segment_goal);
        }
        if follow.final_goal {
            return Steer::Arrived;
        }
        follow.next = Next::Segment;
        return Steer::Stand(to_world(reached));
    }
    if close(at, follow.local[follow.target]) {
        follow.target = farthest_seen(grid, at, &follow.local, follow.target, class);
    }
    let target = follow.local[follow.target];
    if !grid.ahead_free(at, target, class) {
        follow.next = Next::Search;
        follow.local.clear();
        return Steer::Stand(segment_goal);
    }
    Steer::Walk(to_world(target))
}

/// The point after `target` and then the farthest one that can be seen
/// from `at`, trying them in order until one cannot be seen.
fn farthest_seen(
    grid: &Grid,
    at: [u32; 2],
    points: &[[u32; 2]],
    target: usize,
    class: u8,
) -> usize {
    let mut best = target + 1;
    for (i, &p) in points.iter().enumerate().skip(target + 2) {
        if !grid.sees(at, p, class) {
            break;
        }
        best = i;
    }
    best
}
