//! How a ground unit follows its path round obstacles (SPEC-0020,
//! "Following the path", and SPEC-0021).
//!
//! At its first update after an order the unit searches the high-level
//! list of waypoints to the order point. It then walks to segment goals
//! about 10 blocks apart along that list, each by a local path of cells
//! that it shortcuts where it can see far enough, and stands one update
//! at the end of each local path, when the cells one step ahead are
//! blocked, and while a search is held back after a long one.
//!
//! Other units occupy the footprints at the cells of their positions at
//! their last updates (SPEC-0021). Searches and shortcuts see the units
//! that stand, the look-ahead check all of them. A unit blocked by a
//! moving one waits 5 updates, or makes it wait 20 when it is slower and
//! of the same size class. Local searches share a budget of iterations.
//!
//! PROVISIONAL: a unit whose block is not free walks straight to the order
//! point; a local path that cannot leave its start counts as walked;
//! repeated blocks (SPEC-0020 open question 3) are not modelled; the
//! budget window is 2 ticks (SPEC-0021 open question 1) and a queued path
//! searches at its unit's next update; a moving unit that follows no path
//! is never made to wait.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ow3_math::{add, mul, sub};

use super::{Object, State};
use crate::path::{self, Cells, Grid, WithUnits};

/// 1/32, 32 and 0.49, the arrival radius in cells.
const CELL: u32 = 0x3D00_0000;
const WORLD: u32 = 0x4200_0000;
const ARRIVAL: u32 = 0x3EFA_E148;
/// Leg lengths of a segment, in blocks (10.0).
const SEGMENT: f32 = 10.0;
/// The iteration limit of a local search.
const LOCAL_LIMIT: u32 = 700;
/// When the iterations of the window reach this many, the searching unit
/// cannot search again for `HOLD` ticks.
const LONG_SEARCH: u32 = 64;
const HOLD: u64 = 60;
/// Iterations of local searches per window (SPEC-0021).
const BUDGET: u32 = 1100;
const WINDOW: u64 = 2;
/// Updates a unit waits for a moving one, and makes a slower one wait.
const WAIT: u32 = 5;
const MADE_TO_WAIT: u32 = 20;

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
    /// The order point in cells the waypoints were searched to.
    order: [u32; 2],
    /// Updates the unit still stands waiting for another unit.
    wait: u32,
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

/// The iterations local searches used in the current window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    window: u64,
    used: u32,
}

impl Budget {
    fn used(&mut self, tick: u64) -> u32 {
        if self.window != tick / WINDOW {
            *self = Budget {
                window: tick / WINDOW,
                used: 0,
            };
        }
        self.used
    }
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

/// The squared speed of velocity `v` as SPEC-0021 compares it.
pub fn squared_speed(v: [u32; 2]) -> u32 {
    add(mul(v[1], v[1]), mul(v[0], v[0]))
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

/// Another unit as a unit on its way sees it.
#[derive(Clone, Copy, Debug)]
struct Other {
    moving: bool,
    squared_speed: u32,
    class: u8,
}

/// The cells other units occupy, seen from one unit.
struct Others {
    /// The cells of the units that stand.
    standing: HashSet<(i32, i32)>,
    /// The units on each occupied cell, in the order of creation.
    by_cell: HashMap<(i32, i32), Vec<u32>>,
    units: HashMap<u32, Other>,
}

/// The other ground units than `h` with the cells they occupy: a moving
/// unit at its position of its last update.
fn others(state: &State, h: u32) -> Others {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let mut others = Others {
        standing: HashSet::new(),
        by_cell: HashMap::new(),
        units: HashMap::new(),
    };
    for &o in &state.units {
        if o == h {
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&o) else {
            continue;
        };
        if !u.alive || u.hidden {
            continue;
        }
        let class = path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
        let (moving, position, velocity) = match &u.movement {
            Some(m) if m.velocity != [0, 0] => (true, m.origin, m.velocity),
            _ => (false, super::movement::relative([u.x, u.y], min), [0, 0]),
        };
        for cell in path::occupied_cells(to_cells(position), class) {
            if !moving {
                others.standing.insert(cell);
            }
            others.by_cell.entry(cell).or_default().push(o);
        }
        others.units.insert(
            o,
            Other {
                moving,
                squared_speed: squared_speed(velocity),
                class,
            },
        );
    }
    others
}

/// A unit at an update, as `steer` needs it.
pub struct Walker {
    pub handle: u32,
    /// Its committed position and the order point, relative to the world
    /// corner.
    pub here: [u32; 2],
    pub order: [u32; 2],
    pub class: u8,
    /// Its squared speed at the start of the update.
    pub squared_speed: u32,
    /// For an attacker, how it chases (SPEC-0019).
    pub chase: Option<Chase>,
}

/// What a chase needs at an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chase {
    /// The target's position now, relative to the world corner.
    pub target: [u32; 2],
    /// The attack range alone and with both collision sizes, in cells.
    pub range: u32,
    pub reach: u32,
}

/// What `walker` does at this update on its way to its order point;
/// `tick` is the current tick.
pub fn steer(state: &mut State, walker: &Walker, tick: u64) -> Steer {
    if state.grid.is_none() {
        return Steer::Walk(walker.order);
    }
    let others = others(state, walker.handle);
    let grid = state.grid.as_ref().unwrap();
    let (at, order_cells) = (to_cells(walker.here), to_cells(walker.order));
    let mut follow = state.paths.remove(&walker.handle).unwrap_or_else(|| {
        // The high-level search ignores units.
        let (waypoints, reached) = path::high_level(grid, at, order_cells, walker.class)
            .map_or_else(
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
            partial: false,
            target: 0,
            next: Next::Search,
            hold_until: 0,
            wait: 0,
            order: order_cells,
        };
        follow.take_segment(order_cells);
        follow
    });
    let decided = decide(
        &mut follow,
        grid,
        &others,
        &mut state.budget,
        &mut state.paths,
        walker,
        tick,
    );
    state.paths.insert(walker.handle, follow);
    decided
}

fn decide(
    follow: &mut Follow,
    grid: &Grid,
    others: &Others,
    budget: &mut Budget,
    paths: &mut BTreeMap<u32, Follow>,
    walker: &Walker,
    tick: u64,
) -> Steer {
    if follow.waypoints.is_empty() {
        return Steer::Walk(walker.order);
    }
    let (at, class) = (to_cells(walker.here), walker.class);
    let seen = WithUnits {
        grid,
        occupied: &others.standing,
    };
    if follow.wait > 0 {
        follow.wait -= 1;
        return Steer::Stand(to_world(follow.goal));
    }
    let order = to_cells(walker.order);
    // A chase with a new goal: a new chase path; the attacker's own path
    // stays when its waypoint does (SPEC-0019).
    if walker.chase.is_some() && order != follow.order {
        let (waypoints, reached) = path::high_level(grid, at, order, class).map_or_else(
            || (Vec::new(), false),
            |found| (found.points, found.reached),
        );
        let (old_goal, old_final) = (follow.goal, follow.final_goal);
        follow.waypoints = waypoints;
        follow.reached = reached;
        follow.current = 0;
        follow.order = order;
        follow.take_segment(order);
        if follow.final_goal || old_final || follow.goal != old_goal {
            follow.next = Next::Search;
        }
        if follow.waypoints.is_empty() {
            return Steer::Walk(walker.order);
        }
    }
    // A waypoint of a chase path is passed in range of the attack: the
    // attacker stands this update with a new path to the next one.
    let mut passed = false;
    if let Some(chase) = walker.chase
        && follow.next == Next::Walk
        && !follow.final_goal
        && super::movement::in_range(walker.here, to_world(follow.goal), chase.reach)
    {
        follow.take_segment(order);
        follow.next = Next::Search;
        passed = true;
    }
    if follow.next == Next::Segment {
        follow.take_segment(order);
        follow.next = Next::Search;
    }
    let segment_goal = to_world(follow.goal);
    if follow.next == Next::Search {
        if tick < follow.hold_until {
            return Steer::Stand(segment_goal);
        }
        if budget.used(tick) > BUDGET {
            // Queued for a later window, without the hold.
            follow.hold_until = 0;
            return Steer::Stand(segment_goal);
        }
        let found = match walker.chase {
            // A chase searches to where its target is now, up to its range.
            Some(chase) if follow.final_goal => path::local_near(
                &seen,
                at,
                to_cells(chase.target),
                class,
                LOCAL_LIMIT,
                chase.range,
            ),
            _ => path::local(&seen, at, follow.goal, class, LOCAL_LIMIT),
        };
        budget.used += found.iterations;
        if budget.used >= LONG_SEARCH {
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
        if passed {
            return Steer::Stand(segment_goal);
        }
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
        follow.target = farthest_seen(&seen, at, &follow.local, follow.target, class);
    }
    let target = follow.local[follow.target];
    // The look-ahead check sees every unit.
    let failing: Vec<(i32, i32)> = path::ahead_cells(at, target, class)
        .into_iter()
        .filter(|&(x, y)| !grid.passable(x, y) || others.by_cell.contains_key(&(x, y)))
        .collect();
    if failing.is_empty() {
        return Steer::Walk(to_world(target));
    }
    // Moving units in the way: wait for them, or make the slower ones of
    // the same class wait.
    let mut examined = BTreeSet::new();
    for cell in &failing {
        for &b in others.by_cell.get(cell).map_or(&[][..], Vec::as_slice) {
            if !examined.insert(b) {
                continue;
            }
            let other = others.units[&b];
            if !other.moving || paths.get(&b).is_some_and(|f| f.wait > 0) {
                continue;
            }
            let faster = real(other.squared_speed) >= real(walker.squared_speed);
            match paths.get_mut(&b) {
                Some(path) if !faster && other.class == class => path.wait = MADE_TO_WAIT,
                _ => {
                    follow.wait = WAIT - 1;
                    return Steer::Stand(to_world(target));
                }
            }
        }
    }
    // Blocked as by a wall: search again at the next update.
    follow.next = Next::Search;
    follow.local.clear();
    Steer::Stand(segment_goal)
}

/// The point after `target` and then the farthest one that can be seen
/// from `at`, trying them in order until one cannot be seen.
fn farthest_seen(
    cells: &impl Cells,
    at: [u32; 2],
    points: &[[u32; 2]],
    target: usize,
    class: u8,
) -> usize {
    let mut best = target + 1;
    for (i, &p) in points.iter().enumerate().skip(target + 2) {
        if !cells.sees(at, p, class) {
            break;
        }
        best = i;
    }
    best
}
