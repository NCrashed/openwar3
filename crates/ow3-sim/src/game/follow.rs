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
use crate::random::Random;

/// 1/32, 32 and 0.49, the arrival radius in cells.
const CELL: u32 = 0x3D00_0000;
const WORLD: u32 = 0x4200_0000;
const ARRIVAL: u32 = 0x3EFA_E148;
/// Leg lengths of a segment, in blocks (10.0).
const SEGMENT: f32 = 10.0;
/// The distance in blocks at which a segment ends at its waypoint (0.49).
const SEGMENT_END: u32 = 0x3EFA_E148;
/// The iteration limit of a local search.
const LOCAL_LIMIT: u32 = 700;
/// When the iterations of the window reach this many, the searching unit
/// cannot search again for `HOLD` ticks.
const LONG_SEARCH: u32 = 64;
const HOLD: u64 = 60;
/// Iterations of local searches per window (SPEC-0021).
const BUDGET: u32 = 1100;
/// Iterations of the first high-level searches of new paths per update
/// cycle (SPEC-0021, "High-level budgets": the 2000 / 300 table).
const HIGH_BUDGET: u32 = 300;
/// Updates a unit waits for a moving one, and makes a slower one wait.
const WAIT: u32 = 5;
const MADE_TO_WAIT: u32 = 20;
/// A blocked unit this near its goal (squared, in cells: 12 cells) and
/// alone stands one update before it searches again (SPEC-0025).
const NEAR_GOAL: f32 = 144.0;

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
    /// A search again found no way closer: the order ends at the next
    /// update.
    stuck: bool,
    target: usize,
    /// What the next update must do first.
    next: Next,
    /// The first tick at which the unit may search again.
    hold_until: u64,
    /// The order point in cells the waypoints were searched to.
    order: [u32; 2],
    /// Updates the unit still stands waiting for another unit.
    wait: u32,
    /// The wait was made by another unit (`made_wait20`, SPEC-0021).
    made: bool,
    /// The patience counter of a blocked unit (SPEC-0025): 0 while its
    /// way is free.
    patience: u32,
    /// The unit touched its target at the end of its path: it counts as
    /// within its order's reach at the next arrival test (SPEC-0025,
    /// "Blocked units").
    touching: bool,
    /// The cluster's list (the 5000 search, in blocks, from the start) and
    /// the index of its current waypoint W; the unit's own list goes to W
    /// (SPEC-0020, "Two searches"; SPEC-0025).
    cluster: Vec<[u32; 2]>,
    w: usize,
    /// W in cells when it is not the order point.
    stop_at: Option<[u32; 2]>,
    /// The unit arrived at W: at its next update W moves on and it
    /// searches its own list to the new W.
    advance: bool,
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
    /// No motion and no turn (a new path refused, SPEC-0021).
    Hold,
    /// At the order point.
    Arrived,
}

/// The iterations local searches used in the current window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    window: u64,
    used: u32,
    /// The iterations of high-level searches this update cycle.
    high: u32,
}

impl Budget {
    /// The count starts again before every second update cycle, those
    /// with an odd number (SPEC-0021, "The window is two update cycles").
    pub(super) fn cycle(&mut self, number: u64) {
        self.high = 0;
        if number % 2 == 1 {
            self.window = number;
            self.used = 0;
        }
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

/// Step 1 of SPEC-0020: from waypoint `current`, the first waypoint at
/// which the legs add up to 10 blocks, with its index. The goal is `None`
/// for the order point: when no waypoint is far enough (the index is then
/// `None` too) or when that waypoint is the last of the list, also of a
/// list that ends at the closest node of a failed search (c-RYP-1:
/// peasant 3 at 1141).
fn next_segment(waypoints: &[[u32; 2]], current: usize) -> (Option<usize>, Option<[u32; 2]>) {
    let mut sum = 0u32;
    for j in current + 1..waypoints.len() {
        sum = add(sum, path::length(waypoints[j - 1], waypoints[j]));
        if real(sum) >= SEGMENT {
            let last = j == waypoints.len() - 1;
            return (
                Some(j),
                (!last).then(|| path::blocks_to_cells(waypoints[j])),
            );
        }
    }
    (None, None)
}

/// The segment goals of a path from `from` to `order` (cells) for a unit
/// of class `class`, in cells, ending at the order point: the waypoints a
/// cluster walks (SPEC-0025, "Following the cluster's path").
pub fn segment_goals(grid: &Grid, from: [u32; 2], order: [u32; 2], class: u8) -> Vec<[u32; 2]> {
    let mut goals = Vec::new();
    if let Some(found) = path::high_level(grid, from, order, class) {
        let mut current = 0;
        while let (Some(j), Some(goal)) = next_segment(&found.points, current) {
            goals.push(goal);
            current = j;
        }
    }
    goals.push(order);
    goals
}

impl Follow {
    /// Step 1: the first waypoint from the current one at which the legs
    /// add up to 10 blocks, else the order point.
    fn take_segment(&mut self, order: [u32; 2]) {
        let order = self.stop_at.unwrap_or(order);
        let (index, goal) = next_segment(&self.waypoints, self.current);
        if let Some(j) = index {
            self.current = j;
        }
        match goal {
            Some(goal) => {
                self.goal = goal;
                self.final_goal = false;
            }
            None => {
                self.goal = order;
                self.final_goal = true;
            }
        }
    }
}

/// Another unit as a unit on its way sees it.
#[derive(Clone, Copy, Debug)]
struct Other {
    moving: bool,
    squared_speed: u32,
    class: u8,
    /// Its group cluster, if it has one.
    group: Option<usize>,
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
    // Nor does such a worker see any unit (SPEC-0029), but for its searches
    // the workers waiting at a mine are in the way (c-RXI-1: worker 4's
    // search of 1279 touched two of them).
    if super::harvest::passes_through(state, h) {
        for o in super::harvest::waiting(state) {
            let Some(u) = state.unit(Some(o)) else {
                continue;
            };
            let class =
                path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
            let at = super::movement::relative([u.x, u.y], min);
            others
                .standing
                .extend(path::occupied_cells(to_cells(at), class));
        }
        return others;
    }
    for &o in &state.units {
        if o == h {
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&o) else {
            continue;
        };
        // A worker in the gathering cycle is seen by no one (SPEC-0029).
        if !u.alive || u.hidden || super::harvest::passes_through(state, o) {
            continue;
        }
        let class = path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
        // Moving also when it stopped at its own update earlier in this
        // tick, with the velocity it had then.
        let velocity = |m: &super::movement::Moving| match state.start_velocities.get(&o) {
            Some(&start) if m.velocity == [0, 0] => start,
            _ => m.velocity,
        };
        // A unit stopped this tick stands for the searches (`stands`).
        let (moving, stands, position, velocity) = match &u.movement {
            // A group member that started at its update earlier in this
            // tick, while its cluster has updates to come, still stands.
            Some(m)
                if m.velocity != [0, 0]
                    && super::movement::seen_velocity(state, o, m) == [0, 0] =>
            {
                (false, true, super::movement::held_at(state, o, m), [0, 0])
            }
            // Its speed now, also when it moved at its update earlier in
            // this tick.
            Some(m) if m.velocity != [0, 0] => (
                true,
                false,
                super::movement::held_at(state, o, m),
                m.velocity,
            ),
            Some(m) if velocity(m) != [0, 0] => {
                let here = super::movement::relative([u.x, u.y], min);
                (true, true, here, velocity(m))
            }
            _ => (
                false,
                true,
                super::movement::relative([u.x, u.y], min),
                [0, 0],
            ),
        };
        for cell in path::occupied_cells(to_cells(position), class) {
            // Marked group members are passed by as moving ones are.
            if stands && !state.marked.contains(&o) {
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
                group: super::movement::group_of(state, o),
            },
        );
    }
    others
}

/// What the units share while they decide their steps: the budget of
/// local searches, the paths of the others and the script generator.
struct Shared<'a> {
    budget: &'a mut Budget,
    paths: &'a mut BTreeMap<u32, Follow>,
    random: &'a mut Random,
    events: Vec<PathEvent>,
    /// Only the search, no step.
    search_only: bool,
    /// The group cluster of the unit deciding, if it has one.
    group: Option<usize>,
    /// The red cells of the building walked to, if any.
    building: Vec<(i32, i32)>,
}

/// What a unit decided at an update, for comparing with the game's
/// (the `.events.tsv` of the replays).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEventKind {
    /// The look-ahead check of the unit's update, at its position.
    Update,
    /// `a` waits 5 updates for `b`.
    Wait,
    /// `b` makes `a` wait 20 updates.
    MadeWait,
    /// `a` is blocked: it stands and searches again.
    Blocked,
}

/// A path decision at tick `tick`; `at` is the unit's position relative to
/// the world corner, as bit patterns of reals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathEvent {
    pub tick: u64,
    pub kind: PathEventKind,
    pub a: u32,
    pub b: Option<u32>,
    pub at: [u32; 2],
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
    /// The number of members of its group's cluster, 1 for a unit alone
    /// (SPEC-0025).
    pub cluster: usize,
    /// For an attacker, how it chases (SPEC-0019).
    pub chase: Option<Chase>,
    /// For a walk to a building, the building: the high-level search
    /// takes its footprint as free, and the last local search ends
    /// touching its red cells, which are not obstacles it met (SPEC-0029).
    pub building: Option<u32>,
    /// For a walk to a tree, the tree: the high-level search takes its
    /// footprint as free (SPEC-0020, "Two searches, and a target's
    /// footprint").
    pub tree: Option<u32>,
    /// For an order with a range (gathering), the reach in cells and the
    /// direction of the unit's step (its facing if it stands), and whether
    /// the place is the order point: arrival
    /// at its place (SPEC-0019, "Arrival at the member's place").
    pub arrive: Option<(u32, u32, bool)>,
    /// For a member of a cluster of more than one, its place as laid out
    /// now, in cells.
    pub place: Option<[u32; 2]>,
}

/// What a chase needs at an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chase {
    /// The target's position at its last update, relative to the world
    /// corner, and its size class: its footprint, which ends the search
    /// when touched (SPEC-0019).
    pub target: [u32; 2],
    pub target_class: u8,
    /// The attack range with both collision sizes, in cells.
    pub reach: u32,
}

/// What `walker` does at this update on its way to its order point;
/// `tick` is the current tick.
pub fn steer(state: &mut State, walker: &Walker, tick: u64) -> Steer {
    steer_or_search(state, walker, tick, false)
}

/// The search of `walker`'s path outside its updates, without the step
/// and its look-ahead check (SPEC-0019, "After the swing").
pub fn search(state: &mut State, walker: &Walker, tick: u64) {
    steer_or_search(state, walker, tick, true);
}

fn steer_or_search(state: &mut State, walker: &Walker, tick: u64, search_only: bool) -> Steer {
    if state.grid.is_none() {
        return Steer::Walk(walker.order);
    }
    let (at, order_cells) = (to_cells(walker.here), to_cells(walker.order));
    // The high-level search ignores units, and the footprint of the
    // building walked to (SPEC-0029).
    let footprint = walker
        .building
        .and_then(|b| state.unit(Some(b)))
        .and_then(|b| b.footprint.as_ref())
        .map(|f| (f.red().to_vec(), f.green().to_vec()));
    // After an arrival at W: W moves on along the cluster's list and the
    // unit searches its own list to it (SPEC-0025).
    if let (Some(grid), Some(f)) = (state.grid.as_mut(), state.paths.get_mut(&walker.handle))
        && f.advance
    {
        let (j, w) = next_segment(&f.cluster, f.w);
        if let Some(j) = j {
            f.w = j;
        }
        f.stop_at = w;
        let place = w.unwrap_or(order_cells);
        // The building walked to does not block it (SPEC-0029).
        if let Some((red, green)) = &footprint {
            grid.stamp(red, green, false);
        }
        let found = member_search(
            grid,
            at,
            place,
            order_cells,
            walker.class,
            walker.tree.is_some(),
        );
        if let Some((red, green)) = &footprint {
            grid.stamp(red, green, true);
        }
        let (waypoints, reached) =
            found.map_or_else(|| (Vec::new(), false), |f| (f.points, f.reached));
        f.waypoints = waypoints;
        f.reached = reached;
        f.current = 0;
        f.local.clear();
        f.next = Next::Search;
        f.touching = false;
        f.advance = false;
        f.take_segment(order_cells);
    }
    let fresh = !state.paths.contains_key(&walker.handle);
    // A new path finding the high-level budget used up is refused for
    // the tick (SPEC-0021, "High-level budgets").
    if fresh && state.budget.high > HIGH_BUDGET {
        return Steer::Hold;
    }
    let lifted = footprint.as_ref().filter(|_| fresh);
    if let (Some((red, green)), Some(grid)) = (lifted, state.grid.as_mut()) {
        grid.stamp(red, green, false);
    }
    // The first search, with limit 5000, is counted in the budget; the
    // path comes from the second (SPEC-0020, "Two searches, and a
    // target's footprint"). For a tree the blocks around it are free.
    let class = walker.class;
    // A unit alone keeps its cluster's list and walks its own list to the
    // cluster's current waypoint (c-RXD-1: creep 7 at 4561).
    let alone = walker.chase.is_none() && walker.place.is_none() && walker.cluster <= 1;
    let mut plan: (Vec<[u32; 2]>, usize, Option<[u32; 2]>) = (Vec::new(), 0, None);
    let high = fresh.then(|| {
        let grid = state.grid.as_mut().unwrap();
        let (cost, found) = if alone {
            let tree = walker.tree.is_some();
            let (cost, cluster) = if tree {
                path::high_level_to_target(grid, at, order_cells, class)
            } else {
                (
                    path::high_level_cost(grid, at, order_cells, class),
                    path::high_level_cluster(grid, at, order_cells, class),
                )
            };
            let list = cluster.map(|f| f.points).unwrap_or_default();
            let (j, w) = next_segment(&list, 0);
            let place = w.unwrap_or(order_cells);
            let found = member_search(grid, at, place, order_cells, class, tree);
            plan = (list, j.unwrap_or(0), w);
            (cost, found)
        } else if walker.tree.is_some() {
            path::high_level_to_target(grid, at, order_cells, class)
        } else {
            (
                path::high_level_cost(grid, at, order_cells, class),
                path::high_level(grid, at, order_cells, class),
            )
        };
        state.budget.high += cost;
        found
    });
    if let (Some((red, green)), Some(grid)) = (lifted, state.grid.as_mut()) {
        grid.stamp(red, green, true);
    }
    let others = others(state, walker.handle);
    let grid = state.grid.as_ref().unwrap();
    let mut follow = state.paths.remove(&walker.handle).unwrap_or_else(|| {
        let (waypoints, reached) = high.flatten().map_or_else(
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
            stuck: false,
            target: 0,
            next: Next::Search,
            hold_until: 0,
            wait: 0,
            made: false,
            patience: 0,
            touching: false,
            order: order_cells,
            cluster: plan.0,
            w: plan.1,
            stop_at: plan.2,
            advance: false,
        };
        follow.take_segment(order_cells);
        follow
    });
    let group = super::movement::group_of(state, walker.handle);
    let mut shared = Shared {
        budget: &mut state.budget,
        paths: &mut state.paths,
        random: &mut state.random,
        events: Vec::new(),
        search_only,
        // A walk to a tree ends touching it too (c-RYP-1: peasant 0 at
        // 325 walked to the corner cell of its tree).
        building: footprint.map(|f| f.0).unwrap_or_else(|| {
            walker
                .tree
                .and_then(|t| match state.objects.get(&t) {
                    Some(super::Object::Destructable(d)) => Some(d.footprint.red().to_vec()),
                    _ => None,
                })
                .unwrap_or_default()
        }),
        group,
    };
    let decided = decide(&mut follow, grid, &others, &mut shared, walker, tick);
    let events = shared.events;
    // An arrived unit searches a new path at its next update.
    if decided != Steer::Hold || follow.advance {
        state.paths.insert(walker.handle, follow);
    }
    if let Some(trace) = &mut state.path_trace {
        trace.extend(events);
    }
    decided
}

fn decide(
    follow: &mut Follow,
    grid: &Grid,
    others: &Others,
    shared: &mut Shared,
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
    // A chase with a new goal has a new path, which waits for no one.
    if walker.chase.is_some() && to_cells(walker.order) != follow.order {
        follow.wait = 0;
    }
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
    // Arrival at its place: within the order's reach and turned to it;
    // it stands, keeps its heading, and searches again at its next update
    // (SPEC-0019; c-RYP-1 peasant 4 at 847).
    // The place is the target itself for a visible unit target (a gold
    // mine), else the current waypoint (SPEC-0019).
    if let Some((reach, direction, at_order)) = walker.arrive {
        // The place: W of the cluster's list, or the order point.
        let place = match follow.stop_at {
            Some(w) if !at_order => to_world(w),
            _ if at_order => walker.order,
            _ => to_world(follow.goal),
        };
        let (dx, dy) = (sub(place[0], walker.here[0]), sub(place[1], walker.here[1]));
        let off = super::facing::angle_difference(super::movement::angle(dx, dy), direction);
        let in_reach = super::movement::in_range(walker.here, place, reach);
        if in_reach || follow.touching {
            // Not yet turned to it: it stands and turns this update
            // (c-RYP-1: peasant 5 at 1009, arrived at 1015).
            if real(off).abs() > 0.2 {
                return Steer::Stand(place);
            }
            // At its target: there; else a new search at the next update.
            if in_reach && follow.final_goal && follow.stop_at.is_none() {
                return Steer::Arrived;
            }
            if follow.stop_at.is_some() {
                follow.advance = true;
                follow.local.clear();
            }
            return Steer::Hold;
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
    // The end of a segment at a waypoint: within 0.49 blocks of it the
    // next segment is searched at once, the unit walking on (SPEC-0020,
    // step 3; c-RXD-1: creep 7 at 5215).
    if follow.next == Next::Walk && !follow.final_goal && walker.chase.is_none() {
        let half = |c: u32| mul(c, 0x3F00_0000);
        let (dx, dy) = (
            sub(half(at[0]), half(follow.goal[0])),
            sub(half(at[1]), half(follow.goal[1])),
        );
        if real(add(mul(dx, dx), mul(dy, dy))) <= real(mul(SEGMENT_END, SEGMENT_END)) {
            follow.next = Next::Segment;
        }
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
        if shared.budget.used > BUDGET {
            // Queued for a later window, without the hold.
            follow.hold_until = 0;
            return Steer::Stand(segment_goal);
        }
        let found = match walker.chase {
            // A chase searches to its goal and ends where it touches its
            // target (SPEC-0019).
            Some(chase) if follow.final_goal => path::local_touching(
                &seen,
                at,
                follow.goal,
                class,
                LOCAL_LIMIT,
                &path::occupied_cells(to_cells(chase.target), chase.target_class),
            ),
            // A walk to a building: the same, touching its footprint
            // (SPEC-0029).
            None if follow.final_goal && !shared.building.is_empty() => path::local_to_building(
                &seen,
                at,
                follow.goal,
                class,
                LOCAL_LIMIT,
                &shared.building,
            ),
            _ => path::local(&seen, at, follow.goal, class, LOCAL_LIMIT),
        };
        // A search that hits the limit costs one more.
        shared.budget.used += if found.iterations >= LOCAL_LIMIT {
            LOCAL_LIMIT + 1
        } else {
            found.iterations
        };
        if shared.budget.used >= LONG_SEARCH {
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
    // Only the search, outside an update (a chase after its swing).
    if shared.search_only {
        return Steer::Stand(segment_goal);
    }
    // Arrival at the point walked to.
    let last = follow.local.len() - 1;
    if follow.target == last && (last == 0 || close(at, follow.local[last])) {
        let reached = follow.local[last];
        if follow.partial {
            // A search again that cannot get the unit any closer to the
            // order point (its closest node is where it stands) ends the
            // order.
            //
            // PROVISIONAL: SPEC-0020 says only that the unit searches
            // again; in c-RVL-1 a footman whose order point another one
            // stood on stopped and its order ended two updates later.
            // A worker walking to a building keeps searching: it goes in
            // or delivers only touching it (SPEC-0029).
            if last == 0 && follow.final_goal && walker.building.is_none() {
                if follow.stuck {
                    // Short of W the unit goes on with the next one.
                    if follow.stop_at.is_some() {
                        follow.advance = true;
                        follow.stuck = false;
                        follow.local.clear();
                        return Steer::Hold;
                    }
                    return Steer::Arrived;
                }
                follow.stuck = true;
                return Steer::Stand(segment_goal);
            }
            // The end of a path that did not get there: search again.
            follow.next = Next::Search;
            return Steer::Stand(segment_goal);
        }
        if follow.final_goal {
            // The end of its own list at W: it arrived there.
            if follow.stop_at.is_some() {
                follow.advance = true;
                follow.local.clear();
                return Steer::Hold;
            }
            // A member at the end of its path to an old place, its place
            // now elsewhere, counts as blocked (SPEC-0025, "Blocked units";
            // c-RTW-3: 900 at 5545).
            if let Some(place) = walker.place
                && !close(at, place)
            {
                // Set and lowered in the same update.
                let fresh = follow.patience == 0;
                count_block(follow, shared, walker, order, at);
                if fresh && follow.patience > 1 {
                    follow.patience -= 1;
                }
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                follow.next = Next::Search;
                follow.local.clear();
                return Steer::Stand(segment_goal);
            }
            // A walk to a tree that ends out of reach but touching the
            // tree: no patience, it stands and turns to the tree, counted
            // as within reach (c-RYP-1: peasant 5 at 1297).
            if let Some((reach, _, _)) = walker.arrive
                && walker.tree.is_some()
                && !follow.touching
                // A path that cannot get it closer ends at the tree.
                && follow.local.len() > 1
                && !super::movement::in_range(walker.here, walker.order, reach)
                && touches(&shared.building, at, class)
            {
                follow.touching = true;
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                return Steer::Stand(walker.order);
            }
            // A walk to a building stands at the end of its path, where it
            // touched the building, before it goes in (SPEC-0029).
            if !shared.building.is_empty() {
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
            }
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
    shared.events.push(PathEvent {
        tick,
        kind: PathEventKind::Update,
        a: walker.handle,
        b: None,
        at: walker.here,
    });
    let failing: Vec<(i32, i32)> = path::ahead_cells(at, target, class)
        .into_iter()
        .filter(|&(x, y)| !grid.passable(x, y) || others.by_cell.contains_key(&(x, y)))
        .collect();
    if failing.is_empty() {
        follow.patience = 0;
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
            if !other.moving || shared.paths.get(&b).is_some_and(|f| f.wait > 0) {
                continue;
            }
            let faster = real(other.squared_speed) >= real(walker.squared_speed);
            // A unit of the same group cluster is waited for, whatever
            // its speed (SPEC-0021).
            let same_group = other.group.is_some() && other.group == shared.group;
            let event = |kind, a, b| PathEvent {
                tick,
                kind,
                a,
                b: Some(b),
                at: walker.here,
            };
            match shared.paths.get_mut(&b) {
                Some(path) if !same_group && !faster && other.class == class => {
                    path.wait = MADE_TO_WAIT;
                    path.made = true;
                    shared
                        .events
                        .push(event(PathEventKind::MadeWait, b, walker.handle));
                }
                _ => {
                    follow.wait = WAIT - 1;
                    follow.made = false;
                    shared
                        .events
                        .push(event(PathEventKind::Wait, walker.handle, b));
                    return Steer::Stand(to_world(target));
                }
            }
        }
    }
    // Blocked by units that made it wait for none: the patience counter
    // (SPEC-0021, SPEC-0025 "Blocked units") counts such blocks in a row.
    // Either way the unit stands, drops its local path and searches a new
    // one at its next update (SPEC-0020).
    //
    // PROVISIONAL: the counter is measured with units in the way; that it
    // does not count for static obstacles follows from the tests of
    // SPEC-0020 (c-PF1), which match without it. What a unit does at a
    // block with a counter of 1 is not known (SPEC-0025, open question 5);
    // here the same, with the counter staying 1.
    if !examined.is_empty() {
        count_block(follow, shared, walker, order, at);
    }
    shared.events.push(PathEvent {
        tick,
        kind: PathEventKind::Blocked,
        a: walker.handle,
        b: None,
        at: walker.here,
    });
    follow.next = Next::Search;
    follow.local.clear();
    Steer::Stand(segment_goal)
}

/// Whether a unit of `class` at `at` (cells) touches `cells`: one of its
/// cells on one of them or next to it, also diagonally.
fn touches(cells: &[(i32, i32)], at: [u32; 2], class: u8) -> bool {
    path::occupied_cells(at, class).iter().any(|&(x, y)| {
        cells
            .iter()
            .any(|&(a, b)| (a - x).abs() <= 1 && (b - y).abs() <= 1)
    })
}

/// The unit's own high-level search (limit 400) from `at` to `place`
/// (cells), for a walk to a tree with the blocks round the tree free.
fn member_search(
    grid: &mut Grid,
    at: [u32; 2],
    place: [u32; 2],
    order: [u32; 2],
    class: u8,
    tree: bool,
) -> Option<path::Found> {
    if tree {
        grid.with_free_around(order, |g| path::high_level(g, at, place, class))
    } else {
        path::high_level(grid, at, place, class)
    }
}

/// One more block of a unit's path in a row: the patience counter
/// (SPEC-0025, "Blocked units").
fn count_block(
    follow: &mut Follow,
    shared: &mut Shared,
    walker: &Walker,
    order: [u32; 2],
    at: [u32; 2],
) {
    if follow.patience == 0 {
        let (dx, dy) = (sub(order[0], at[0]), sub(order[1], at[1]));
        let near = real(add(mul(dx, dx), mul(dy, dy))) <= NEAR_GOAL;
        follow.patience = if near && walker.cluster <= 1 {
            2
        } else {
            7 + ((shared.random.step() >> 22) & 1)
        };
    } else if follow.patience > 1 {
        follow.patience -= 1;
    }
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
