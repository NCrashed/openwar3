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
    /// Its chase group was made anew (`fn new_group`): while its search is
    /// held it does not walk the old path.
    new_group: bool,
    /// The unit arrived at W: at its next update W moves on and it
    /// searches its own list to the new W.
    advance: bool,
    /// A chase whose path ended: it arrives at its next update.
    end_blocked: bool,
    /// A worker at the end of its path to a mine, turning to it: its stop
    /// was logged.
    turning_in: bool,
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
    /// By the field of the searching unit's path (`move_kind`, its owner):
    /// each has its own table (c-SBJ-1: kobold 5's search of 13.679911
    /// not held by the heroes' long searches; analyst 11-15 02:05).
    used: [u32; 16],
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
            self.used = [0; 16];
        }
    }
}

pub(super) fn to_cells(p: [u32; 2]) -> [u32; 2] {
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
    /// Its movement type, a key of `move_kind`.
    kind: u8,
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
    // A unit in Wind Walk sees no one in its way either (SPEC-0040;
    // c-SAK-1: the Blademaster walked through kobold 9 at 847).
    if state
        .unit(Some(h))
        .is_some_and(|u| u.wind_walk.is_some_and(|w| w.on(state.ticks)))
    {
        return others;
    }
    // Nor does such a worker see any unit (SPEC-0029), but for its searches
    // the workers waiting at a mine are in the way (c-RXI-1: worker 4's
    // search of 1279 touched two of them).
    if super::harvest::passes_through(state, h) {
        // So are the workers working at a tree (c-RYP-1: peasant 2 at its
        // tree in peasant 5's search of 3537).
        for o in super::harvest::waiting(state).chain(super::harvest::at_work_on_trees(state)) {
            let Some(u) = state.unit(Some(o)) else {
                continue;
            };
            let class =
                path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
            let at = super::movement::relative([u.x, u.y], min);
            let cells = path::occupied_cells(to_cells(at), class);
            // A worker at work on a tree also stops its look-ahead
            // (c-RYP-1: peasant 4 at 5449, blocked by peasant 1).
            if super::harvest::at_work_on_trees(state).any(|w| w == o) {
                for &cell in &cells {
                    others.by_cell.entry(cell).or_default().push(o);
                }
                others.units.insert(
                    o,
                    Other {
                        moving: false,
                        squared_speed: 0,
                        kind: move_kind(state, o),
                        group: None,
                    },
                );
            }
            others.standing.extend(cells);
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
        // A unit in Wind Walk is in no one's way, from its cast on
        // (SPEC-0040; c-SAK-1: footman 4 walked past the standing
        // Blademaster at 433, before his fade ended).
        if u.wind_walk.is_some_and(|w| w.on(state.ticks)) {
            continue;
        }
        // A worker in the gathering cycle is seen by no one (SPEC-0029).
        if !u.alive || u.hidden || super::harvest::passes_through(state, o) {
            continue;
        }
        let class = path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
        // The look-ahead reads the velocity the other unit has at the
        // check. A unit alone that stopped at its update earlier in this
        // tick stands (c-RZH-1: footman 4 blocked by footman 6, blocked at
        // 5983 and at its target at 6049), but the stand of a new goal
        // after a swing comes after the updates (c-RXD-1: creep 7 waits
        // for creep 8 at 2803), and so do the stops of the members of a
        // group cluster, after their cluster's update (c-RTW-3: 954 waits
        // for 936 at 4855).
        let grouped =
            state.renewed_now.contains(&o) || super::movement::group_of(state, o).is_some();
        let velocity = |m: &super::movement::Moving| match state.start_velocities.get(&o) {
            Some(&start) if m.velocity == [0, 0] && grouped => start,
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
                kind: move_kind(state, o),
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
    /// The movement type of the unit deciding (`move_kind`).
    kind: u8,
    /// The red cells of the building walked to, if any.
    building: Vec<(i32, i32)>,
    /// The cells of the tree walked to, for the touch test at the end of
    /// a path; they do not end a local search (SPEC-0020: the tree's own
    /// layer that closes its cells comes first in its cells).
    tree: Vec<(i32, i32)>,
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

/// Unit `h`'s chase group was made anew (the new goal after a swing,
/// SPEC-0019): its local path goes with the old group, and while its next
/// search is held it stands and turns to its goal (c-RXD-1: creep 9 from
/// 8018 to 8066).
pub(super) fn new_group(state: &mut State, h: u32) {
    state.quiet_paths.remove(&h);
    if let Some(f) = state.paths.get_mut(&h) {
        f.new_group = true;
    }
}

/// A unit at an update, as `steer` needs it.
pub struct Walker {
    pub handle: u32,
    /// A unit that cannot attack running from its attacker (SPEC-0019).
    pub flee: bool,
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
    /// That building is a gold mine.
    pub mine: bool,
    /// Its walk heading (SPEC-0018), as a bit pattern of radians.
    pub heading: u32,
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
    // An amphibious unit searches its move kind's grid (hub #14: the
    // Giant Sea Turtle).
    let amphibious = state
        .unit(Some(walker.handle))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| t.move_type == "amph")
        && state.amphibious.is_some();
    if amphibious {
        std::mem::swap(&mut state.grid, &mut state.amphibious);
    }
    let steer = steer_in_grid(state, walker, tick, search_only);
    if amphibious {
        std::mem::swap(&mut state.grid, &mut state.amphibious);
    }
    steer
}

fn steer_in_grid(state: &mut State, walker: &Walker, tick: u64, search_only: bool) -> Steer {
    if state.grid.is_none() {
        return Steer::Walk(walker.order);
    }
    // A group made by the chase right after a swing, its target in reach,
    // is not handled: no search, no look-ahead (SPEC-0021; c-RZJ-1:
    // footman 2 from 7210, the elemental from 7244).
    if state.quiet_paths.contains(&walker.handle) {
        return Steer::Stand(walker.order);
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
    // A new path is a new walk (SPEC-0024, "Walks").
    if !state.paths.contains_key(&walker.handle) {
        super::table::path_searched(state, walker.handle);
    }
    let others = others(state, walker.handle);
    let grid = state.grid.as_ref().unwrap();
    let held = state.path_holds.remove(&walker.handle);
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
            hold_until: held.unwrap_or(0),
            wait: 0,
            made: false,
            patience: 0,
            touching: false,
            order: order_cells,
            cluster: plan.0,
            w: plan.1,
            stop_at: plan.2,
            advance: false,
            new_group: held.is_some(),
            end_blocked: false,
            turning_in: false,
        };
        follow.take_segment(order_cells);
        follow
    });
    let at_tree = super::harvest::at_tree(state, walker.handle);
    // The tree's cells that can be touched: not those also covered by a
    // tree created later, whose entries come first there and close the
    // cell (c-RYP-1: peasant 3's search of 4929).
    let tree_cells = walker
        .tree
        .and_then(|t| match state.objects.get(&t) {
            Some(super::Object::Destructable(d)) => {
                let later: HashSet<(i32, i32)> = state
                    .objects
                    .iter()
                    .filter(|&(&o, _)| o > t)
                    .filter_map(|(_, o)| match o {
                        super::Object::Destructable(e) if e.alive => Some(e.footprint.red()),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .collect();
                Some(
                    d.footprint
                        .red()
                        .iter()
                        .copied()
                        .filter(|c| !later.contains(c))
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        })
        .unwrap_or_default();
    let group = super::movement::group_of(state, walker.handle);
    let kind = move_kind(state, walker.handle);
    let mut shared = Shared {
        budget: &mut state.budget,
        paths: &mut state.paths,
        random: &mut state.random,
        events: Vec::new(),
        search_only,
        // A worker ends its local search touching its tree, except between
        // its first arrival there and the end of its first work (SPEC-0020,
        // "Trees in the local search"; c-RYP-1: peasant 0 at 313 and 3993,
        // peasant 5 at 1315).
        building: footprint.map(|f| f.0).unwrap_or_else(|| {
            if at_tree {
                Vec::new()
            } else {
                tree_cells.clone()
            }
        }),
        tree: tree_cells,
        group,
        kind,
    };
    let decided = decide(&mut follow, grid, &others, &mut shared, walker, tick);
    let events = shared.events;
    // An arrived unit searches a new path at its next update.
    // A worker at its gold mine gives its walk up as it goes in or starts
    // to wait (`harvest::start_waiting`; c-RXI-1: walks freed at 877).
    let at_mine = state.unit(Some(walker.handle)).and_then(|u| u.movement).is_some_and(|m| {
        matches!(m.order, super::movement::Order::Work { target } | super::movement::Order::Gather { target } | super::movement::Order::Return { building: target } if super::harvest::is_mine(state, target) || state.unit(Some(target)).is_some_and(|b| b.footprint.is_some()))
    });
    if decided != Steer::Hold || follow.advance {
        state.paths.insert(walker.handle, follow);
    } else if !at_mine {
        super::table::walk_dropped(state, walker.handle);
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
        return Steer::Stand(member_place(follow, to_world(follow.goal)));
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
            // At its target, or touching its tree: the walk ends there
            // (SPEC-0029, "Arriving at the tree"); else a new search at the
            // next update.
            if follow.final_goal && follow.stop_at.is_none() {
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
    // While its search is held the unit walks on along its old local path,
    // if it has one (c-RZJ-1: creep 15, goal renewed at 769, searched at
    // 787, turning and walking along the path of 727 in between).
    let held = tick < follow.hold_until && !follow.local.is_empty() && !follow.new_group;
    if follow.next == Next::Search && !held {
        if tick < follow.hold_until {
            return Steer::Stand(segment_goal);
        }
        if shared.budget.used[usize::from(shared.kind & 0xF)] > BUDGET {
            // Queued for a later window, without the hold.
            follow.hold_until = 0;
            return Steer::Stand(segment_goal);
        }
        follow.new_group = false;
        follow.end_blocked = false;
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
        shared.budget.used[usize::from(shared.kind & 0xF)] += if found.iterations >= LOCAL_LIMIT {
            LOCAL_LIMIT + 1
        } else {
            found.iterations
        };
        if shared.budget.used[usize::from(shared.kind & 0xF)] >= LONG_SEARCH {
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
            // A path of only its own position is a block of the order's
            // walk, with the patience counter; at P = 1 the unit gives up
            // with the mark of reach and arrives once turned to the point
            // (SPEC-0025, "Blocked units"; c-RZH-1: footman 4 and the hero
            // at 7028, their orders ending at 7034 and 7040).
            if last == 0
                && follow.final_goal
                && walker.building.is_none()
                && walker.tree.is_none()
                && walker.chase.is_none()
                && follow.stop_at.is_none()
            {
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                if follow.patience == 1 {
                    follow.touching = true;
                    return Steer::Stand(walker.order);
                }
                let fresh = follow.patience == 0;
                count_block(follow, shared, walker, order, at);
                if fresh && follow.patience > 1 {
                    follow.patience -= 1;
                }
                follow.local.clear();
                follow.next = Next::Search;
                return Steer::Stand(segment_goal);
            }
            if last == 0 && follow.final_goal && walker.building.is_none() && walker.tree.is_none()
            {
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
            // The end of a path that did not get there: blocked, with the
            // patience counter, and a new search at the next update
            // (SPEC-0025, "Blocked units"; c-RXD-1: creep 9 at 5719).
            if follow.final_goal {
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                // At P = 1 a walk to a tree gives up: the mark of reach,
                // standing and turning to the tree (SPEC-0025, case 2;
                // c-RYP-1: peasant 4 at 1231).
                if follow.patience == 1 && walker.tree.is_some() {
                    follow.touching = true;
                    return Steer::Stand(walker.order);
                }
                // Set and lowered in the same update (P = 2 -> 1 for a
                // unit alone near its goal: peasant 4 at 1225).
                let fresh = follow.patience == 0;
                count_block(follow, shared, walker, order, at);
                if fresh && follow.patience > 1 {
                    follow.patience -= 1;
                }
                follow.local.clear();
                follow.next = Next::Search;
                return Steer::Stand(if walker.tree.is_some() {
                    walker.order
                } else {
                    segment_goal
                });
            }
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
                && !super::movement::in_range(walker.here, walker.order, reach)
                && touches(&shared.tree, at, class)
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
            // Out of reach and not touching: blocked, with the patience
            // counter; at P = 1 it gives up and gets the mark of reach
            // (SPEC-0025, "Blocked units", case 2; c-RYP-1: peasant 4 at
            // 1225 and 1231).
            if let Some((reach, _, _)) = walker.arrive
                && walker.tree.is_some()
                && !follow.touching
                && !super::movement::in_range(walker.here, walker.order, reach)
            {
                if follow.patience == 1 {
                    follow.touching = true;
                } else {
                    let fresh = follow.patience == 0;
                    count_block(follow, shared, walker, order, at);
                    if fresh && follow.patience > 1 {
                        follow.patience -= 1;
                    }
                    follow.local.clear();
                    follow.next = Next::Search;
                }
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                return Steer::Stand(walker.order);
            }
            // A flee at the end of its path still outside its limit of the goal
            // goes on: it stands and tries a new local path at each update
            // until one is found (c-SBJ-1: the Dreadlord at its last waypoint at
            // 13.829909, a path found at 13.949908; analyst 11-14 22:30).
            if walker.flee && !super::movement::in_range(walker.here, walker.order, 0) {
                follow.local.clear();
                follow.next = Next::Search;
                return Steer::Stand(walker.order);
            }
            // A chase at the end of its path: the member step reports it
            // blocked and it stands; the group arrives at its next update
            // (c-SAK-1: footman 2 blocked at 919, swinging at 925).
            // A walk with no path (its local path only its own position) has no
            // stop: it arrives at its first update when turned (c-SBI-1: the
            // ogre's chase of 2.099999, arrived at 2.129998).
            let no_path = follow.local.len() <= 1 && follow.local.first().is_none_or(|&p| p == at);
            if walker.chase.is_some() && !follow.end_blocked && !no_path {
                follow.end_blocked = true;
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
                return Steer::Stand(segment_goal);
            }
            // Then it arrives once its walk heading is within 0.2 of its goal
            // point, turning at its update until then (SPEC-0019; c-SBJ-1: the
            // ogre 0.7584 off at its stop of 1.799998, 0.2584 at 1.829998,
            // arrived at 1.859998; c-SAK-1: footman 2, 0.6882 at 919, arrived at
            // 925).
            if walker.chase.is_some() {
                let to = walker.order;
                let (dx, dy) = (sub(to[0], walker.here[0]), sub(to[1], walker.here[1]));
                let off =
                    super::facing::angle_difference(super::movement::angle(dx, dy), walker.heading);
                if real(off).abs() > 0.2 {
                    return Steer::Stand(to);
                }
            }
            // A walk to a building stands at the end of its path, where it
            // touched the building, before it goes in (SPEC-0029).
            if !shared.building.is_empty() && !follow.turning_in {
                shared.events.push(PathEvent {
                    tick,
                    kind: PathEventKind::Blocked,
                    a: walker.handle,
                    b: None,
                    at: walker.here,
                });
            }
            // At the end of its path to a mine a worker turns its walk heading
            // to the mine's point, 0.6 rad an update, and arrives at the next
            // update with 0.2 or less left: one more update for a turn over
            // 0.8 (c-RXD-1: peasant 1048687 stopped 0.971 off at 21.747351,
            // arrived at 21.807341; c-RXI-1: worker 0, 0.6596 off at 5.339851,
            // arrived at the next update). PROVISIONAL: the tolerance 0.2 lies
            // in the range read, 0.0596 .. 0.371.
            if walker.mine
                && follow.stop_at.is_none()
                && let Some((_, direction, _)) = walker.arrive
            {
                let (dx, dy) = (
                    sub(walker.order[0], walker.here[0]),
                    sub(walker.order[1], walker.here[1]),
                );
                let off =
                    super::facing::angle_difference(super::movement::angle(dx, dy), direction);
                if real(off).abs() > 0.8 {
                    follow.turning_in = true;
                    return Steer::Stand(walker.order);
                }
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
                Some(path) if !same_group && !faster && other.kind == shared.kind => {
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
    // An attacker blocked where it touches its target gets the mark of
    // reach, without the patience counter (SPEC-0025, "Blocked units";
    // c-RZH-1: creep 9 at 907 next to the standing footman 5, swinging at
    // 913).
    let touching_target = walker.chase.is_some_and(|chase| {
        touches(
            &path::occupied_cells(to_cells(chase.target), chase.target_class),
            at,
            class,
        )
    });
    if touching_target {
        follow.touching = true;
    } else if !examined.is_empty() {
        // A member of a group standing at its block counts afresh, drawing
        // again (c-RTW-3: footman 900 at 5467 and 5473, standing short
        // of its place).
        if walker.cluster > 1 && walker.squared_speed == 0 {
            follow.patience = 0;
        }
        let fresh = follow.patience == 0;
        count_block(follow, shared, walker, order, at);
        if fresh && follow.patience == 2 {
            follow.patience -= 1;
        }
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

/// The place a member turns to while it goes on waiting: its cluster's
/// waypoint W, if it has one; at the update that starts the wait it turns
/// to its local path point instead, as it was about to step there. Read in
/// the game in two cases (c-RXR-1: unit 8 at 307 and 313; c-RXD-1: creep 7
/// at 4603 and 4609); the code path behind it was not read.
fn member_place(follow: &Follow, otherwise: [u32; 2]) -> [u32; 2] {
    follow.stop_at.map_or(otherwise, to_world)
}

/// Whether unit `h`'s walk has the mark of reach from touching its target
/// (SPEC-0025, "Blocked units").
pub(super) fn touching(state: &State, h: u32) -> bool {
    state.paths.get(&h).is_some_and(|f| f.touching)
}

/// The 4-bit field of unit `h`'s path compared at blocks: units with
/// different values are never made to wait for each other (SPEC-0021,
/// "Blocked by a unit"). It is the owning player (c-SBJ-1: 0 for the
/// four heroes of player 0, foot and hover alike, 0xC for the creeps of
/// player 12; analyst 11-15 03:00).
/// PROVISIONAL: read in one map with one player and the creeps.
fn move_kind(state: &State, h: u32) -> u8 {
    state.unit(Some(h)).map_or(0xFF, |u| (u.owner & 0xF) as u8)
}

/// The contact test of an attack start (SPEC-0019): a cell round unit `h`'s
/// footprint holds `target`'s footprint (c-SBJ-1: the ogre swinging at the
/// Crypt Lord out of reach at 3.209999).
///
/// PROVISIONAL: positions at the units' last updates; the cells a unit
/// that moved still holds from its last footprint are not counted.
pub(super) fn contact(state: &State, h: u32, target: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let (Some(u), Some(t)) = (state.unit(Some(h)), state.unit(Some(target))) else {
        return false;
    };
    let class_of = |u: &super::Unit| {
        path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision))
    };
    let cells = path::occupied_cells(
        to_cells(super::movement::relative([t.x, t.y], min)),
        class_of(t),
    );
    touches(
        &cells,
        to_cells(super::movement::relative([u.x, u.y], min)),
        class_of(u),
    )
}

/// Whether a unit of `class` at `at` (cells) touches `cells`: one of its
/// cells on one of them or next to it, also diagonally.
pub(super) fn touches(cells: &[(i32, i32)], at: [u32; 2], class: u8) -> bool {
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
        // Near the goal of its path: the end of its high-level list
        // (SPEC-0025; c-RYP-1: peasant 3 at 1435, list ending at its
        // closest node).
        let goal = follow
            .waypoints
            .last()
            .map_or(order, |&p| path::blocks_to_cells(p));
        let (dx, dy) = (sub(goal[0], at[0]), sub(goal[1], at[1]));
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

/// Unit `h` starts a chase as a new group (SPEC-0019, "A held search"):
/// its path goes, and a search of its old path that is still held stays
/// held for the new one, the unit standing and turning to its goal until
/// then (c-RZJ-1: the Paladin at his cooldown end 7218, last search 7190,
/// searched at 7250).
pub(super) fn regroup(state: &mut State, h: u32) {
    state.quiet_paths.remove(&h);
    let tick = state.ticks;
    if let Some(f) = state.paths.remove(&h)
        && f.hold_until > tick
    {
        state.path_holds.insert(h, f.hold_until);
    }
    super::table::walk_dropped(state, h);
}

/// Unit `h`'s new group comes from the chase right after a swing with its
/// target in reach: its path is not handled until its next group
/// (SPEC-0021).
pub(super) fn quiet(state: &mut State, h: u32) {
    state.quiet_paths.insert(h);
}

/// Unit `h`'s walk keeps its path to its new goal `order` (world): a slow
/// renewed at a later action changes the goal, not the lists (c-SBW-1:
/// kobold 5 from 4.83, hub #10).
pub(super) fn keep_path(state: &mut State, h: u32, order: [u32; 2]) {
    if let Some(f) = state.paths.get_mut(&h) {
        f.order = to_cells(order);
    }
}
/// Whether unit `h`'s last search found no point nearer to its goal than
/// where it stands: a partial path of its own place alone (c-SCA-1: the
/// turtle's chase of the archer above the cliff at 10.629217, hub #14).
pub(super) fn found_nothing_nearer(state: &State, h: u32) -> bool {
    state
        .paths
        .get(&h)
        .is_some_and(|f| f.partial && f.local.len() <= 1)
}
