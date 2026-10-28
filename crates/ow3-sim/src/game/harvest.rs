//! Workers gathering gold and lumber (SPEC-0029, Human peasant).
//!
//! A worker ordered to `harvest` a gold mine or a tree walks to it
//! (order 852018). At a mine it enters when the mine is free, stays
//! hidden for 200 ticks and comes out with the gold of a trip; at a tree
//! it hits every `Dur` seconds, one lumber a hit, until it carries a load.
//! Then it carries the load to the nearest building that takes it (order
//! 852017), delivers it to its player and goes back.
//!
//! A building, mine or drop-off, is reached when the worker's footprint
//! touches a red cell of the building's pathing texture (cells equal or
//! neighbours); a tree when the worker is within the reach of `Ahar` plus
//! its collision size.
//!
//! PROVISIONAL: "nearest" is the straight distance (SPEC-0029 open
//! question 3); waiting workers enter a busy mine in the order they
//! reached it (question 2); the next tree is the nearest living one
//! within `Area`; Human workers and buildings only (question 5).

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;
use ow3_math::{add, div, mul, sub};

use super::movement::{self, CELL, Order, relative};
use super::{Object, State};

/// The orders `harvest` and `resumeharvesting` (SPEC-0029).
pub const ORDER_HARVEST: i32 = 852_018;
pub const ORDER_RESUME_HARVESTING: i32 = 852_017;

/// The player states of gold and lumber.
const GOLD: i32 = 1;
const LUMBER: i32 = 2;
const GOLD_GATHERED: i32 = 15;
const LUMBER_GATHERED: i32 = 16;
/// `PLAYER_STATE_RESOURCE_FOOD_USED`.
const FOOD_USED: i32 = 5;

/// The tenths of the gold a player keeps at a delivery with `used` food
/// in use: all up to 50, 7 up to 80, 4 above (SPEC-0029, "Upkeep": 10,
/// 7 and 4 at 41, 61 and 81 food). PROVISIONAL: the borders are the
/// usual table, not measured; lumber was not tested.
pub(super) fn upkeep_tenths(used: i32) -> i32 {
    match used {
        ..=50 => 10,
        51..=80 => 7,
        _ => 4,
    }
}

/// Ticks a worker stays inside a mine (1.0 s).
const IN_MINE: u64 = 200;
/// The time a worker stays inside a mine, in seconds (1.0).
const IN_MINE_TIME: u32 = 0x3F80_0000;
/// Ticks between two updates of a unit (SPEC-0018).
const UPDATE: u64 = 6;
/// Ticks from a work start out of reach to the check that takes another
/// tree (0.5 s), and from that check to the first hit on it.
const SWITCH_CHECK: u64 = 100;
/// The reach of the harvest of a tree, squared, in cells: (116 / 32 +
/// 0.5) squared (SPEC-0029, "Arriving at the tree").
const TREE_REACH_SQUARED: f32 = 17.015625;
/// The radius round the current tree in which another is looked for
/// (`Area1` of `Ahar`).
const OTHER_TREES: f32 = 900.0;

/// Ability codes: harvest, the gold of a mine, the returns of gold and
/// lumber and of lumber only.
fn code(c: &[u8; 4]) -> i32 {
    i32::from_be_bytes(*c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Gold,
    Lumber,
}

/// A worker's gathering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Harvest {
    /// The mine or tree it works.
    target: Option<u32>,
    /// What it carries.
    carry: Option<(Resource, i32)>,
    /// The queue times of its next hit on a tree and of the walk back
    /// with a full load (SPEC-0029, "Arriving at the tree").
    next_hit: Option<u32>,
    back_at: Option<u32>,
    /// The end of the backswing of its last hit, and of the pause after
    /// it (SPEC-0029, "When the tree is gone").
    swing_end: Option<u32>,
    pause_end: Option<u32>,
    /// The felled tree it returns to after its delivery, when it took
    /// another one at the end of its backswing.
    home: Option<u32>,
    /// The work started out of reach of its tree: the tick of the check
    /// that takes another tree (SPEC-0029, "Another tree").
    switch_at: Option<u64>,
    /// Out of a mine once and gathering since: its cells block only the
    /// placing of buildings (SPEC-0029, "Workers in the gathering cycle
    /// do not block each other").
    cycling: bool,
    /// The harvest's ready mark and retry counter of the arrival check at
    /// a tree (SPEC-0029, "Arriving at the tree").
    ready: bool,
    retries: u8,
    /// From its first arrival at its tree to the end of its first work:
    /// trees close their cells for its searches, its own included, and it
    /// still meets other units (SPEC-0029, "Entering the gathering cycle").
    at_tree: bool,
    /// The pace of the gold cycle (SPEC-0029): the expiry of its 10 s
    /// timer, its speed bonus, and the last trip with the bonus during it.
    pace_timer: u32,
    bonus: u32,
    last_trip: u32,
    last_bonus: u32,
}

/// Who is inside a mine, until when, and who waits at it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mine {
    /// The worker going in or inside, the tick it goes in, and the queue
    /// time it comes out (0 until it is in): a timer of 1.0 s from the
    /// moment it went in (SPEC-0029).
    inside: Option<(u32, u64, u32)>,
    waiting: Vec<u32>,
    /// Workers that start to wait at their next update, with its tick.
    starting: Vec<(u32, u64)>,
}

/// The numbers of the harvest ability of a worker's type: gold per trip,
/// lumber per hit, lumber per trip, ticks between hits, reach and the
/// radius to look for another tree (reals as bit patterns).
#[derive(Clone, Copy, Debug)]
struct Gathering {
    gold: i32,
    per_hit: i32,
    lumber: i32,
    period: u64,
    duration: u32,
    reach: u32,
}

fn ability_of(state: &State, type_id: i32, base: i32) -> Option<&crate::data::Ability> {
    let t = state.data.units.get(&type_id)?;
    t.abilities
        .iter()
        .filter_map(|a| state.data.abilities.get(a))
        .find(|a| a.code == base)
}

fn gathering(state: &State, h: u32) -> Option<Gathering> {
    let u = state.unit(Some(h))?;
    let a = ability_of(state, u.type_id, code(b"Ahar"))?;
    let whole = |v: u32| f32::from_bits(v) as i32;
    Some(Gathering {
        gold: whole(a.data[2]),
        per_hit: whole(a.data[0]),
        lumber: whole(a.data[1]),
        period: (f64::from(f32::from_bits(a.duration)) * 200.0).round() as u64,
        duration: a.duration,
        reach: a.range,
    })
}

/// Whether `h` is a gold mine.
pub(super) fn is_mine(state: &State, h: u32) -> bool {
    matches!(state.objects.get(&h), Some(Object::Unit(u))
        if u.alive && ability_of(state, u.type_id, code(b"Agld")).is_some())
}

/// Whether `h` is a living tree.
fn is_tree(state: &State, h: u32) -> bool {
    matches!(state.objects.get(&h), Some(Object::Destructable(d))
        if d.alive && state.data.trees.contains(&d.type_id))
}

/// Whether building `h` takes `resource` from player `owner`.
fn takes(state: &State, h: u32, owner: i32, resource: Resource) -> bool {
    let Some(Object::Unit(u)) = state.objects.get(&h) else {
        return false;
    };
    if !u.alive || u.owner != owner || u.construction.is_some() {
        return false;
    }
    // A return ability (base `Artn`, as `Argl` of the town hall and `Arlm`
    // of the lumber mill) takes gold when its `DataA1` is on and lumber
    // when its `DataB1` is.
    let field = match resource {
        Resource::Gold => 0,
        Resource::Lumber => 1,
    };
    ability_of(state, u.type_id, code(b"Artn"))
        .is_some_and(|a| f32::from_bits(a.data[field]) != 0.0)
}

/// Whether worker `h` can gather from `target` (a gold mine or a tree).
pub(super) fn can_harvest(state: &State, h: u32, target: u32) -> bool {
    gathering(state, h).is_some() && (is_mine(state, target) || is_tree(state, target))
        || pays_for(state, h, target)
        || wisp_tree(state, h, target)
}

/// Whether worker `h`, carrying something, would deliver it at `building`.
pub(super) fn can_deliver(state: &State, h: u32, building: u32) -> bool {
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    match u.harvest.and_then(|g| g.carry) {
        Some((resource, _)) => takes(state, building, u.owner, resource),
        None => false,
    }
}

/// `harvest` on `target`: the worker walks to it. Carrying something
/// else than `target` gives, or anything to a mine, it first takes that
/// back (SPEC-0029, "New orders while carrying").
pub(super) fn order_harvest(state: &mut State, h: u32, target: u32) -> bool {
    if pays_for(state, h, target) {
        return order_paid(state, h, target);
    }
    // A wisp walks to its tree (SPEC-0029, "Night Elf: wisps on trees").
    // PROVISIONAL: the order's objects are not read.
    if wisp_tree(state, h, target) {
        let mut g = get(state, h).unwrap_or_default();
        g.target = Some(target);
        set(state, h, Some(g));
        let Some(at) = position(state, target) else {
            return false;
        };
        return movement::harvest_order(state, h, Order::Gather { target }, ORDER_HARVEST, at);
    }
    if !can_harvest(state, h, target) {
        return false;
    }
    let mut g = state
        .unit(Some(h))
        .and_then(|u| u.harvest)
        .unwrap_or_default();
    g.target = Some(target);
    g.next_hit = None;
    g.back_at = None;
    set(state, h, Some(g));
    let taken = walk_to_work(state, h);
    // The order makes the objects of the walk (SPEC-0024).
    if taken && is_mine(state, target) {
        state.objects_change();
        super::table::harvest_ordered(state, h, false);
    }
    if taken && is_tree(state, target) {
        super::table::harvest_ordered(state, h, true);
    }
    taken
}

/// Worker `h` with a load takes it to `building`.
pub(super) fn order_deliver(state: &mut State, h: u32, building: u32) -> bool {
    let Some(at) = position(state, building) else {
        return false;
    };
    movement::harvest_order(
        state,
        h,
        Order::Return { building },
        ORDER_RESUME_HARVESTING,
        at,
    )
}

/// `returnresources`: worker `h` takes its load to the nearest building
/// that takes it.
pub(super) fn order_back(state: &mut State, h: u32) -> bool {
    walk_back(state, h)
}

fn set(state: &mut State, h: u32, g: Option<Harvest>) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.harvest = g;
    }
}

fn get(state: &State, h: u32) -> Option<Harvest> {
    state.unit(Some(h)).and_then(|u| u.harvest)
}

/// Where worker `h` comes out of a mine (SPEC-0029, "Where a worker comes
/// out"): `None` at its entry point when its footprint there is free,
/// else the centre of the first free cell on the rings round the entry
/// cell. Free cells are passable and not covered by the cells of the
/// units that count (`push`: a unit pushed aside by a worker that starts
/// to wait), at their last commits.
fn exit_place(state: &State, h: u32, push: bool) -> Option<[u32; 2]> {
    let grid = state.grid.as_ref()?;
    let u = state.unit(Some(h))?;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let class_of = |type_id: i32| {
        state
            .data
            .units
            .get(&type_id)
            .map_or(0, |t| crate::path::size_class(t.collision))
    };
    let cells = |p: [u32; 2]| {
        let r = relative(p, min);
        [mul(r[0], CELL), mul(r[1], CELL)]
    };
    let mut taken = std::collections::BTreeSet::new();
    for &o in &state.units {
        let Some(Object::Unit(w)) = state.objects.get(&o) else {
            continue;
        };
        // Coming out, the units count as for a local search: workers of
        // the gathering cycle only while they wait (c-RXI-1 3497, c-RXD-1
        // worker 0 at 4191). Pushing aside, walking units do not count
        // (c-RXD-1 worker 1 at 4249). SPEC-0029.
        let moving = w.movement.is_some_and(|m| m.velocity != [0, 0]);
        let waits = waiting(state).any(|x| x == o);
        let cycling = w.harvest.is_some_and(|g| g.cycling) && !waits;
        let left_out = if push { moving } else { cycling };
        if o == h || !w.alive || left_out || w.hidden {
            continue;
        }
        // Its cells at its last commit.
        let at = w.movement.map_or([w.x, w.y], |m| {
            [add(m.origin[0], min[0]), add(m.origin[1], min[1])]
        });
        taken.extend(crate::path::occupied_cells(cells(at), class_of(w.type_id)));
    }
    let class = class_of(u.type_id);
    let free = |(x, y): (i32, i32)| {
        let at = [(x as f32 + 0.5).to_bits(), (y as f32 + 0.5).to_bits()];
        crate::path::occupied_cells(at, class)
            .iter()
            .all(|&(a, b)| grid.passable(a, b) && !taken.contains(&(a, b)))
    };
    let (cx, cy) = crate::path::cell_of(cells([u.x, u.y]));
    if free((cx, cy)) {
        return None;
    }
    for n in 1..=32 {
        let (l, r, b, t) = (cx - n, cx + n, cy - n, cy + n);
        // Bottom row, right column, top row and left column, round.
        let ring = (l + 1..=r)
            .map(|x| (x, b))
            .chain((b + 1..=t).map(|y| (r, y)))
            .chain((l..r).rev().map(|x| (x, t)))
            .chain((b..t).rev().map(|y| (l, y)));
        for cell in ring {
            if free(cell) {
                let world =
                    |c: i32, m: u32| (f32::from_bits(m) + (c as f32 + 0.5) * 32.0).to_bits();
                return Some([world(cell.0, min[0]), world(cell.1, min[1])]);
            }
        }
    }
    None
}

/// Whether worker `h` is in the gathering cycle, out of a mine once:
/// no unit's way sees it (SPEC-0029).
pub(super) fn passes_through(state: &State, h: u32) -> bool {
    get(state, h).is_some_and(|g| g.cycling)
}

/// Whether worker `h` goes into its mine in this tick and has had its
/// update: it went in there, and the later updates of the tick do not see
/// it (hub #15, c-SCC-1: peon 1 walked on past peon 2 going in at 0.6).
pub(super) fn gone_in_now(state: &State, h: u32) -> bool {
    state.updated.contains(&h)
        && state.mines.values().any(|m| {
            m.inside
                .is_some_and(|(w, t, leave)| w == h && t == state.ticks && leave == 0)
        })
}

/// The workers waiting at the mines, in the order of the mines.
pub(super) fn waiting(state: &State) -> impl Iterator<Item = u32> + '_ {
    state.mines.values().flat_map(|m| m.waiting.iter().copied())
}

/// The workers working at a tree (between their hits).
pub(super) fn at_work_on_trees(state: &State) -> impl Iterator<Item = u32> + '_ {
    state.units.iter().copied().filter(move |&h| {
        get(state, h)
            .is_some_and(|g| g.next_hit.is_some() && g.target.is_some_and(|t| is_tree(state, t)))
    })
}

/// Sends worker `h` to its mine or tree, or with a load from elsewhere
/// first to deliver it.
fn walk_to_work(state: &mut State, h: u32) -> bool {
    let Some(mut g) = get(state, h) else {
        return false;
    };
    // A new walk: the arrival check finds the ready mark clear.
    g.ready = false;
    set(state, h, Some(g));
    let Some(target) = g.target else {
        return false;
    };
    let mine = is_mine(state, target);
    let wrong_load = match g.carry {
        Some((Resource::Gold, _)) => true,
        Some((Resource::Lumber, _)) => mine,
        None => false,
    };
    if wrong_load {
        return walk_back(state, h);
    }
    let Some(at) = position(state, target) else {
        return false;
    };
    movement::harvest_order(state, h, Order::Gather { target }, ORDER_HARVEST, at)
}

/// Sends worker `h` with its load to the nearest building that takes it;
/// false when there is none (it then stands).
fn walk_back(state: &mut State, h: u32) -> bool {
    let Some((resource, _)) = get(state, h).and_then(|g| g.carry) else {
        return false;
    };
    let Some(u) = state.unit(Some(h)) else {
        return false;
    };
    let (owner, from) = (u.owner, [u.x, u.y]);
    let mut best: Option<(u32, f32)> = None;
    for &b in &state.units {
        if !takes(state, b, owner, resource) {
            continue;
        }
        let Some(at) = position(state, b) else {
            continue;
        };
        let d = path_length(state, h, from, at);
        if best.is_none_or(|(_, e)| d < e) {
            best = Some((b, d));
        }
    }
    let Some((building, _)) = best else {
        movement::stop(state, h);
        return false;
    };
    let at = position(state, building).unwrap_or(from);
    movement::harvest_order(
        state,
        h,
        Order::Return { building },
        ORDER_RESUME_HARVESTING,
        at,
    )
}

/// How far worker `h` walks from `from` to `to` (world points): the legs
/// of the high-level path (SPEC-0020) and the straight rest from its end,
/// the goal being inside a building; the straight distance without a
/// grid or a path.
///
/// PROVISIONAL: SPEC-0029 gives "nearest along the path" (a wall made the
/// gold go to the farther town hall) but not how it is measured (open
/// question 3).
fn path_length(state: &State, h: u32, from: [u32; 2], to: [u32; 2]) -> f32 {
    let straight = |a: [u32; 2], b: [u32; 2]| squared(a, b).sqrt();
    let Some(grid) = state.grid.as_ref() else {
        return straight(from, to);
    };
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells = |p: [u32; 2]| {
        let r = relative(p, min);
        [mul(r[0], CELL), mul(r[1], CELL)]
    };
    let class = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .map_or(0, |t| crate::path::size_class(t.collision));
    let Some(found) = crate::path::high_level(grid, cells(from), cells(to), class) else {
        return straight(from, to);
    };
    // Blocks of 64 world units.
    let world = |p: [u32; 2]| {
        [
            f32::from_bits(min[0]) + f32::from_bits(p[0]) * 64.0,
            f32::from_bits(min[1]) + f32::from_bits(p[1]) * 64.0,
        ]
    };
    let mut length = 0.0;
    let mut last = [f32::from_bits(from[0]), f32::from_bits(from[1])];
    for &p in &found.points {
        let w = world(p);
        length += ((w[0] - last[0]).powi(2) + (w[1] - last[1]).powi(2)).sqrt();
        last = w;
    }
    let goal = [f32::from_bits(to[0]), f32::from_bits(to[1])];
    length + ((goal[0] - last[0]).powi(2) + (goal[1] - last[1]).powi(2)).sqrt()
}

fn position(state: &State, h: u32) -> Option<[u32; 2]> {
    match state.objects.get(&h)? {
        Object::Unit(u) => Some([u.x, u.y]),
        Object::Destructable(d) => Some([d.x, d.y]),
        _ => None,
    }
}

/// The squared distance of two points as a plain real.
fn squared(a: [u32; 2], b: [u32; 2]) -> f32 {
    let (dx, dy) = (sub(a[0], b[0]), sub(a[1], b[1]));
    f32::from_bits(add(mul(dx, dx), mul(dy, dy)))
}

/// Whether worker `h` at `here` (relative to the world's lower left
/// corner) has reached `target`: a building by touching a red cell of its
/// texture, a tree within reach.
pub(super) fn reached(state: &State, h: u32, here: [u32; 2], target: u32) -> bool {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    // An acolyte reaches a haunted mine within its ability's range and the
    // two collision sizes of the mine's centre (c-SDA-1: 260 to 268 for
    // 200 + 50 + 16).
    // PROVISIONAL: inferred from the distances of SPEC-0029.
    if reaches_by_distance(state, h, target) {
        let (Some(u), Some(m)) = (state.unit(Some(h)), state.unit(Some(target))) else {
            return false;
        };
        let collision = |t: i32| {
            state
                .data
                .units
                .get(&t)
                .map_or(0.0, |d| f32::from_bits(d.collision))
        };
        let range =
            ability_of(state, u.type_id, code(b"Aaha")).map_or(0.0, |a| f32::from_bits(a.range));
        let reach = range + collision(u.type_id) + collision(m.type_id);
        return squared(here, relative([m.x, m.y], min)) <= reach * reach;
    }
    match state.objects.get(&target) {
        Some(Object::Unit(b)) => {
            let Some(footprint) = b.footprint.as_deref() else {
                return false;
            };
            let class = state
                .unit(Some(h))
                .and_then(|u| state.data.units.get(&u.type_id))
                .map_or(0, |t| crate::path::size_class(t.collision));
            let at = [mul(here[0], CELL), mul(here[1], CELL)];
            footprint.touches(&crate::path::occupied_cells(at, class))
        }
        Some(Object::Destructable(d)) => {
            let Some(g) = gathering(state, h) else {
                return false;
            };
            let collision = state
                .unit(Some(h))
                .and_then(|u| state.data.units.get(&u.type_id))
                .map_or(0, |t| t.collision);
            let reach = add(g.reach, collision);
            let tree = relative([d.x, d.y], min);
            squared(here, tree) <= f32::from_bits(mul(reach, reach))
        }
        _ => false,
    }
}

/// The deliveries due at the updates of this tick, made in the updates:
/// ahead of the walks freed after them (c-RXI-1: worker 0 at 5808 before
/// worker 5's walk given up earlier).
pub(super) fn deliveries(state: &mut State) {
    let tick = state.ticks;
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.deliveries)
        .into_iter()
        .partition(|&(_, at)| at <= tick);
    state.deliveries = later;
    for (h, _) in due {
        // The walk back is made, then the old one freed (SPEC-0024).
        let gold = get(state, h).is_some_and(|g| matches!(g.carry, Some((Resource::Gold, _))));
        if gold {
            state.objects_change_in(tick + UPDATE);
            state.objects_change_in(tick + 2 * UPDATE);
        }
        deliver(state, h);
    }
}

/// What the workers that reached their mines, trees and drop-offs at the
/// updates of this tick do, then the mines and the trees of this tick.
pub(super) fn tick(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    pay_mines(&mut state.borrow_mut());
    wisp_lumber(&mut state.borrow_mut());
    let mut reached = std::mem::take(&mut state.borrow_mut().harvest_reached);
    // A worker that reached its drop-off delivers at its next update
    // (SPEC-0029: it stands at the end of its path, and gives its load at
    // the update after; c-RXI-1, peasant 5 stood at 1843, delivered at 1849
    // and walked from 1855).
    {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut s.deliveries)
            .into_iter()
            .partition(|&(_, at)| at <= tick);
        s.deliveries = later;
        for (h, target, delivering) in std::mem::take(&mut reached) {
            if delivering {
                s.deliveries.push((h, tick + UPDATE));
            } else {
                reached.push((h, target, delivering));
            }
        }
        let mut now: Vec<(u32, u32, bool)> = due.into_iter().map(|(h, _)| (h, 0, true)).collect();
        now.append(&mut reached);
        reached = now;
    }
    for (h, target, delivering) in reached {
        let mut s = state.borrow_mut();
        if delivering {
            // The walk back is made, then the old one freed (SPEC-0024).
            let gold = get(&s, h).is_some_and(|g| matches!(g.carry, Some((Resource::Gold, _))));
            if gold {
                let tick = s.ticks;
                s.objects_change_in(tick + UPDATE);
                s.objects_change_in(tick + 2 * UPDATE);
            }
            deliver(&mut s, h);
        } else if is_mine(&s, target) {
            arrive_at_mine(&mut s, h, target);
        } else if pays_for(&s, h, target) {
            arrive_paid(&mut s, h, target);
        } else if wisp_tree(&s, h, target) {
            sit_on_tree(&mut s, h, target);
        } else if is_tree(&s, target) {
            let tick = s.ticks;
            super::table::tree_arrival_ended(&mut s, h);
            // The check of the place: the range test or the touch test
            // (SPEC-0029, "Arriving at the tree").
            let passes = within_reach(&s, h, target) || touches_tree(&s, h, target);
            if let Some(mut g) = get(&s, h)
                && g.next_hit.is_none()
            {
                if passes && g.ready {
                    // The swing starts; the first hit at its swing point
                    // (c-RYP-1: peasant 0 at 1003 -> 1089, peasant 5 at
                    // 1351 -> 1437).
                    // After a tree that was gone, not before the end of the
                    // pause (c-RYP-1: peasant 0, start 10125, swing 10129).
                    let (point, _) = swing(&s, h);
                    let start = match g.pause_end.take() {
                        Some(p) if f32::from_bits(p) > f32::from_bits(s.update_time) => p,
                        _ => s.update_time,
                    };
                    g.next_hit = Some(add(start, point));
                    set(&mut s, h, Some(g));
                } else if passes || g.retries > 0 {
                    // The walk to the same tree again, with the search set
                    // of a worker at its tree (c-RYP-1: peasant 0 at 997,
                    // peasant 5 at 1309).
                    if passes {
                        g.retries = 2;
                    } else {
                        g.retries -= 1;
                    }
                    g.at_tree = true;
                    set(&mut s, h, Some(g));
                    walk_to_work(&mut s, h);
                    if passes {
                        super::table::tree_arrived(&mut s, h);
                    }
                    if let Some(mut g) = get(&s, h) {
                        g.ready = true;
                        set(&mut s, h, Some(g));
                    }
                } else {
                    // The tree is dropped; another one 0.5 s later
                    // (c-RYP-1: peasant 4 at 1243).
                    g.switch_at = Some(tick + SWITCH_CHECK);
                    set(&mut s, h, Some(g));
                    super::table::tree_dropped(&mut s, h);
                }
            }
        } else {
            drop(s);
        }
    }
    forget_others(&mut state.borrow_mut());
    start_waiting(&mut state.borrow_mut());
    enter_mines(state);
    // (The workers coming out are timers of the queue: `mines_left_before`.)
    {
        let mut s = state.borrow_mut();
        for w in std::mem::take(&mut s.harvest_waiters) {
            super::table::harvest_waits(&mut s, w);
            super::table::harvest_arrived(&mut s, w);
            super::table::walk_dropped_at_update(&mut s, w);
        }
    }
    // The rest of the work on trees; the hits are timers of the queue, in
    // order with the script's (`trees_hit_before`).
    trees_hit_before(vm, state, Some(0), false);
}

/// Workers whose order is no longer one of gathering keep their load and
/// leave the queues of the mines.
fn forget_others(state: &mut State) {
    let units = state.units.clone();
    for h in units {
        let Some(mut g) = get(state, h) else {
            continue;
        };
        let gathering = state
            .unit(Some(h))
            .and_then(|u| u.movement)
            .is_some_and(|m| m.gathering());
        if gathering || g.target.is_none() {
            continue;
        }
        g.target = None;
        g.next_hit = None;
        g.back_at = None;
        // PROVISIONAL: what ends the gathering cycle was not tested
        // (SPEC-0029); here any other order does.
        g.cycling = false;
        g.at_tree = false;
        set(state, h, Some(g));
        for mine in state.mines.values_mut() {
            mine.waiting.retain(|&w| w != h);
        }
    }
}

/// A computer's worker reaching a gold mine: an r of the harvest's timer
/// (c-SCC-1: 10807 after peon 2's going in, 11160 and 11161 for the next
/// peons at the busy mine, hub #15).
///
/// PROVISIONAL: its owner and when it goes are not read.
fn computer_harvest_timer(state: &mut State, h: u32) {
    let computer = state.unit(Some(h)).is_some_and(|u| {
        state
            .setup
            .slots
            .get(u.owner as usize)
            .is_some_and(|s| s.controller == super::players::CONTROL_COMPUTER)
    });
    if computer {
        let tick = state.table.now;
        state.table.take(tick, "computer worker: harvest timer");
    }
}

/// A worker at its mine: with a load it first takes it back; else it
/// enters if the mine is free, or waits.
fn arrive_at_mine(state: &mut State, h: u32, mine: u32) {
    if get(state, h).is_some_and(|g| g.carry.is_some()) {
        walk_back(state, h);
        return;
    }
    let tick = state.ticks;
    let entry = state.mines.entry(mine).or_default();
    if entry.inside.is_none() {
        // It goes in at the next update, freeing its walk an update
        // later (SPEC-0024).
        entry.inside = Some((h, tick + UPDATE, 0));
        state.objects_change_in(tick + 2 * UPDATE);
    } else if !entry.waiting.contains(&h) {
        entry.waiting.push(h);
        // It starts to wait at its next update (SPEC-0029).
        entry.starting.push((h, tick + UPDATE));
        state.objects_change_in(tick + UPDATE);
        state.objects_change_in(tick + 2 * UPDATE);
        computer_harvest_timer(state, h);
    }
}

/// The time a worker's round trip of the gold cycle aims at outside the
/// mine is `PACE_AIM - IN_MINE` (SPEC-0029, "Pace").
const PACE_AIM: f32 = 5.0;
/// The timer of the pace, in seconds.
const PACE_TIMER: f32 = 10.0;

/// Worker `h` goes into a gold mine: its speed is set so that its trips
/// take their aim (SPEC-0029, "Pace"; c-RXI-1, worker 5: 1.2 from its
/// second entry). From the time since its last entry, if under 10 s, the
/// trip outside `t` (less the time in the mine), weighted by the speed
/// during it, with the trip before: `q = sum / (n (K - M))`, clamped to
/// 0.5..1.2; else no bonus.
fn pace(state: &mut State, h: u32) {
    let now = state.timers.time();
    let Some(mut g) = get(state, h) else {
        return;
    };
    let f = |v: f32| v.to_bits();
    let r = f32::from_bits(sub(g.pace_timer, now)).max(0.0);
    let inside = f(IN_MINE as f32 / 200.0);
    let p = sub(f(PACE_TIMER), f(r));
    let t = sub(p, inside);
    let old = g.bonus;
    if r > 0.0 && f32::from_bits(t) > 0.0 {
        let one = f(1.0);
        let mut sum = mul(t, add(old, one));
        let mut n = one;
        if g.last_trip != 0 {
            sum = add(sum, mul(g.last_trip, add(g.last_bonus, one)));
            n = add(n, one);
        }
        let q = div(sum, mul(n, sub(f(PACE_AIM), inside)));
        let q = f32::from_bits(q).clamp(0.5, 1.2);
        g.last_trip = t;
        g.last_bonus = old;
        g.bonus = sub(f(q), one);
    } else {
        g.last_trip = 0;
        g.last_bonus = 0;
        g.bonus = 0;
    }
    g.pace_timer = add(now, f(PACE_TIMER));
    set(state, h, Some(g));
}

/// The factor of worker `h`'s speed from the pace of its gold cycle.
pub(super) fn pace_factor(state: &State, h: u32) -> Option<u32> {
    get(state, h)
        .filter(|g| g.bonus != 0)
        .map(|g| add(1.0f32.to_bits(), g.bonus))
}

/// Whether the collision circles of units `a` and `b` overlap.
fn overlaps(state: &State, a: u32, b: u32) -> bool {
    let (Some(u), Some(v)) = (state.unit(Some(a)), state.unit(Some(b))) else {
        return false;
    };
    let size = |t: i32| {
        state
            .data
            .units
            .get(&t)
            .map_or(0.0, |d| f32::from_bits(d.collision))
    };
    let reach = size(u.type_id) + size(v.type_id);
    squared([u.x, u.y], [v.x, v.y]) < reach * reach
}

/// A worker that starts to wait at a busy mine holds its cells for every
/// unit: the units on them are put at the nearest free place by the
/// search of a worker coming out of a mine (SPEC-0029).
fn push_aside(state: &mut State, h: u32) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let on: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&o| o != h)
        .filter(|&o| {
            state.unit(Some(o)).is_some_and(|u| {
                u.alive
                    && !u.hidden
                    && !state.data.units.get(&u.type_id).is_some_and(|t| t.building)
            })
        })
        // Those whose collision circles overlap its own (centre distance
        // under the two sizes; c-RXI-1: 21.9 found, c-SDE-1: 35.9 not, both
        // 32; hub #15).
        // PROVISIONAL: the test read by its result in two cases; the unit
        // state the game tests after it is taken as always set.
        .filter(|&o| overlaps(state, h, o))
        .collect();
    for o in on {
        if let Some(at) = exit_place(state, o, true)
            && let Some(Object::Unit(u)) = state.objects.get_mut(&o)
        {
            [u.x, u.y] = at;
            if let Some(m) = u.movement.as_mut() {
                m.origin = relative(at, min);
            }
        }
    }
}

fn hide(state: &mut State, h: u32, hidden: bool) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.hidden = hidden;
    }
}

/// The workers whose time in their mine is over come out with gold; the
/// next waiting one goes in in the same tick. A mine without gold left
/// dies.
fn enter_mines(state: &Rc<RefCell<State>>) {
    // Those whose time to go in has come are hidden.
    {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let entering: Vec<u32> = s
            .mines
            .values()
            .filter_map(|m| {
                m.inside
                    .filter(|&(_, t, leave)| t == tick && leave == 0)
                    .map(|(w, _, _)| w)
            })
            .collect();
        // It goes in at its update: the queue time of that update.
        let leave = add(s.update_time, IN_MINE_TIME);
        for w in entering {
            for m in s.mines.values_mut() {
                if let Some((x, _, l)) = m.inside.as_mut()
                    && *x == w
                {
                    *l = leave;
                }
            }
            // Its cells stay where it entered: the units on them are
            // pushed aside as by a worker that starts to wait (c-RXI-1
            // tick 3505).
            // Its objects of going in, then its walk ends (SPEC-0024;
            // c-RXI-1: worker 5 at 871).
            super::table::harvest_entered(&mut s, w);
            computer_harvest_timer(&mut s, w);
            super::table::harvest_arrived(&mut s, w);
            super::table::walk_dropped_at_update(&mut s, w);
            push_aside(&mut s, w);
            hide(&mut s, w, true);
            pace(&mut s, w);
        }
    }
}

/// The workers whose time in their mine ends in this tick before queue time
/// `before` (all without it) come out: timers of the queue, in order with
/// the script's (c-SDE-1: the peon out at 5.379993 after the timer of the
/// tick, hub #15).
pub(super) fn mines_left_before(vm: &mut Vm, state: &Rc<RefCell<State>>, before: Option<u32>) {
    let due: Vec<(u32, u32, u32)> = {
        let s = state.borrow();
        // The end of this tick on the queue, Q(k + 1).
        let now = add(s.move_clock, crate::timers::TICK);
        s.mines
            .iter()
            .filter_map(|(&mine, m)| {
                m.inside
                    .filter(|&(_, _, leave)| {
                        leave != 0
                            && f32::from_bits(leave) <= f32::from_bits(now)
                            && before.is_none_or(|b| f32::from_bits(leave) < f32::from_bits(b))
                    })
                    .map(|(w, _, leave)| (mine, w, leave))
            })
            .collect()
    };
    for (mine, worker, left_at) in due {
        let empty = {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            hide(s, worker, false);
            // Its walk out starts from the facing it had when it went in
            // (SPEC-0029, "Coming out").
            if let Some(Object::Unit(u)) = s.objects.get_mut(&worker)
                && let Some(m) = u.movement.as_mut()
            {
                m.heading = u.facing;
            }
            if let Some(at) = exit_place(s, worker, false) {
                let min = [s.world_bounds[0], s.world_bounds[1]];
                if let Some(Object::Unit(u)) = s.objects.get_mut(&worker) {
                    [u.x, u.y] = at;
                    if let Some(m) = u.movement.as_mut() {
                        m.origin = relative(at, min);
                    }
                }
            }
            let gold = gathering(s, worker).map_or(0, |g| g.gold);
            let left = match s.objects.get_mut(&mine) {
                Some(Object::Unit(m)) => {
                    let taken = gold.min(m.resources).max(0);
                    m.resources -= taken;
                    if let Some(mut g) = get(s, worker) {
                        g.carry = Some((Resource::Gold, taken));
                        g.cycling = true;
                        set(s, worker, Some(g));
                    }
                    s.unit(Some(mine)).map_or(0, |m| m.resources)
                }
                _ => 0,
            };
            walk_back(s, worker);
            // The walk to the drop-off is made (SPEC-0024).
            s.objects_change();
            let tick = s.ticks;
            let entry = s.mines.entry(mine).or_default();
            entry.inside = None;
            // The next waiting one goes in.
            // Only a worker that has started to wait: one that reached
            // the mine decides at its next update (c-RXI-1, peasant 4 in
            // tick 6007).
            let next = entry
                .waiting
                .iter()
                .position(|w| !entry.starting.iter().any(|(s, _)| s == w));
            let mut going_in = None;
            if left > 0
                && let Some(i) = next
            {
                let next = entry.waiting.remove(i);
                // Its timer starts as the other one's fires.
                entry.inside = Some((next, tick, add(left_at, IN_MINE_TIME)));
                hide(s, next, true);
                going_in = Some(next);
            }
            // The next going in with others still waiting: the mine's queue
            // timer (c-RXI-1: 1271, 1471, 1671).
            if going_in.is_some() && entry_waits(s, mine) {
                super::table::mine_queue_timer(s, mine, left_at);
            }
            super::table::harvest_left(s, worker, going_in);
            left <= 0
        };
        if empty {
            let waiting = state
                .borrow_mut()
                .mines
                .remove(&mine)
                .map(|m| m.waiting)
                .unwrap_or_default();
            super::widgets::kill_unit(vm, state, Some(mine), None);
            let mut s = state.borrow_mut();
            for w in waiting {
                movement::stop(&mut s, w);
            }
        }
    }
}

/// The hits on trees due in this tick, and the walks back with a full
/// load.
/// The hits on trees due in this tick before queue time `before` (all of
/// them without it): timers of the queue, in order with the script's
/// (c-SDE-1: the peon's hit at 5.079997 after the timer of the tick, hub #15).
pub(super) fn trees_hit_before(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    before: Option<u32>,
    hits_only: bool,
) {
    let units = state.borrow().units.clone();
    for h in units {
        let (tick, g, numbers) = {
            let s = state.borrow();
            let Some(g) = get(&s, h) else {
                continue;
            };
            (s.ticks, g, gathering(&s, h))
        };
        let Some(numbers) = numbers else {
            continue;
        };
        // The walk back after the last hit: a timer of the queue as the hits.
        if hits_only
            && g.back_at.is_some_and(|e| {
                due(&state.borrow(), e)
                    && before.is_none_or(|b| f32::from_bits(e) < f32::from_bits(b))
            })
        {
            let mut s = state.borrow_mut();
            let mut g = g;
            g.back_at = None;
            // Its first work ends: from now on it is in the gathering
            // cycle (SPEC-0029; c-RYP-1: peasant 0 at 3183).
            g.at_tree = false;
            g.cycling = true;
            set(&mut s, h, Some(g));
            walk_back(&mut s, h);
            super::table::harvest_turned_back(&mut s, h);
            continue;
        }
        if !hits_only {
            // At the end of its backswing a worker whose tree is gone takes
            // another tree, the search from the worker; the pause after the
            // backswing runs on (SPEC-0029, "When the tree is gone"; c-RYP-1:
            // peasant 0 at 10109, peasant 3 at 10123).
            if let Some(e) = g.swing_end.filter(|&e| due(&state.borrow(), e)) {
                let mut s = state.borrow_mut();
                let mut g = g;
                g.swing_end = None;
                if let Some(tree) = g.target.filter(|&t| !is_tree(&s, t)) {
                    g.next_hit = None;
                    g.pause_end = Some(add(e, PAUSE));
                    g.home = g.home.or(Some(tree));
                    g.at_tree = false;
                    g.cycling = true;
                    set(&mut s, h, Some(g));
                    take_another_tree(&mut s, h, false);
                    // Its order on the new tree, as at a delivery (c-RYP-1: peasant
                    // 0 at 10106, 13 parts).
                    super::table::lumber_delivered(&mut s, h);
                    continue;
                }
                set(&mut s, h, Some(g));
            }
            if g.switch_at == Some(tick) {
                let mut s = state.borrow_mut();
                take_another_tree(&mut s, h, false);
                super::table::tree_switched(&mut s, h);
                continue;
            }
        }
        let Some(hit) = g.next_hit.filter(|&e| {
            due(&state.borrow(), e) && before.is_none_or(|b| f32::from_bits(e) < f32::from_bits(b))
        }) else {
            continue;
        };
        let Some(tree) = g.target else {
            continue;
        };
        let life = match state.borrow().objects.get(&tree) {
            Some(Object::Destructable(d)) if d.alive => d.life,
            _ => 0,
        };
        if f32::from_bits(life) <= 0.0 {
            // Felled during the swing: no hit; on at the backswing end.
            let mut s = state.borrow_mut();
            let mut g = g;
            g.next_hit = None;
            g.swing_end = Some(add(hit, swing(&s, h).1));
            set(&mut s, h, Some(g));
            continue;
        }
        let one = (numbers.per_hit as f32).to_bits();
        super::widgets::set_destructable_life(vm, state, Some(tree), sub(life, one));
        let mut s = state.borrow_mut();
        let mut g = g;
        let carried = match g.carry {
            Some((Resource::Lumber, n)) => n,
            _ => 0,
        } + numbers.per_hit;
        g.carry = Some((Resource::Lumber, carried));
        if carried >= numbers.lumber {
            g.next_hit = None;
            // At the end of the swing's backswing.
            g.back_at = Some(add(hit, swing(&s, h).1));
            set(&mut s, h, Some(g));
        } else {
            g.next_hit = is_tree(&s, tree).then(|| add(hit, numbers.duration));
            g.swing_end = Some(add(hit, swing(&s, h).1));
            set(&mut s, h, Some(g));
        }
    }
}

/// Worker `h` reached the building it carries its load to: its player
/// gets the load and the worker goes back to work.
fn deliver(state: &mut State, h: u32) {
    let Some(mut g) = get(state, h) else {
        return;
    };
    let Some(owner) = state.unit(Some(h)).map(|u| u.owner) else {
        return;
    };
    if let Some((resource, amount)) = g.carry.take() {
        let which = match resource {
            Resource::Gold => GOLD,
            Resource::Lumber => LUMBER,
        };
        // Upkeep cuts the gold by the food the player uses (SPEC-0029).
        let amount = match resource {
            Resource::Gold => {
                let used = state
                    .setup
                    .player_states
                    .get(&(owner, FOOD_USED))
                    .copied()
                    .unwrap_or(0);
                amount * upkeep_tenths(used) / 10
            }
            Resource::Lumber => amount,
        };
        *state.setup.player_states.entry((owner, which)).or_default() += amount;
        // What was gathered counts the load after the upkeep (c-SCE-1: 7 and
        // 4, hub #15).
        let gathered = match resource {
            Resource::Gold => GOLD_GATHERED,
            Resource::Lumber => LUMBER_GATHERED,
        };
        *state
            .setup
            .player_states
            .entry((owner, gathered))
            .or_default() += amount;
    }
    // Back to its tree, the felled one it left included: then another
    // tree, costed from that tree's point (SPEC-0029, "When the tree is
    // gone"; c-RYP-1: peasant 0 at 11578).
    let back = g.home.take().or(g.target);
    if let Some(t) = back
        && matches!(state.objects.get(&t), Some(Object::Destructable(_)))
        && !is_tree(state, t)
    {
        g.target = Some(t);
        set(state, h, Some(g));
        take_another_tree(state, h, true);
        super::table::lumber_delivered(state, h);
        return;
    }
    let target = back.filter(|&t| is_mine(state, t) || is_tree(state, t));
    g.target = target;
    set(state, h, Some(g));
    if target.is_none() || !walk_to_work(state, h) {
        movement::stop(state, h);
    } else if target.is_some_and(|t| is_tree(state, t)) {
        super::table::lumber_delivered(state, h);
    } else if target.is_some_and(|t| is_mine(state, t)) {
        super::table::gold_delivered(state, h);
    }
}

/// Whether a worker has started to wait at mine `mine`.
fn entry_waits(state: &State, mine: u32) -> bool {
    state.mines.get(&mine).is_some_and(|e| {
        e.waiting
            .iter()
            .any(|w| !e.starting.iter().any(|(s, _)| s == w))
    })
}

/// The pause after a backswing before the next swing, in seconds (0.1).
const PAUSE: u32 = 0x3DCC_CCCD;

/// Whether queue time `e` falls in this tick: Q(k) < e <= Q(k + 1).
fn due(state: &State, e: u32) -> bool {
    f32::from_bits(e) <= f32::from_bits(add(state.move_clock, crate::timers::TICK))
}

/// The damage point and the backswing of worker `h`'s weapon on trees, in
/// seconds: its second when only that one targets trees (SPEC-0029, "Orc:
/// peons": the peon's `targs2` tree, `dmgpt2` 0.5).
fn swing(state: &State, h: u32) -> (u32, u32) {
    let t = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id));
    let on_trees =
        |w: &&crate::data::Weapon| w.targets.iter().any(|s| s.eq_ignore_ascii_case("tree"));
    let first = t.and_then(|t| t.weapon.as_ref());
    let second = t.and_then(|t| t.weapon2.as_ref()).filter(on_trees);
    let w = if first.is_some_and(|w| on_trees(&w)) {
        first
    } else {
        second.or(first)
    };
    // The rest of the swing is the cooldown less the 0.1 s pause and the
    // damage point (SPEC-0029: 0.433 + 0.567 for the peasant, 0.5 + 0.5 for
    // the peon, both of `cool2` 1.1).
    w.map_or((0, 0), |w| {
        let rest = sub(sub(w.cooldown, 0.1f32.to_bits()), w.damage_point);
        (w.damage_point, rest)
    })
}

/// Worker `h` hitting a tree: the tick of its next hit, the ticks between
/// hits and the damage point of its weapon in ticks, for the swing shown.
pub(super) fn chopping(state: &State, h: u32) -> Option<(u64, u64, u64)> {
    let e = get(state, h)?.next_hit?;
    let left = f32::from_bits(e) - f32::from_bits(add(state.move_clock, crate::timers::TICK));
    let next = state.ticks + (left / 0.005).ceil().max(0.0) as u64;
    let period = gathering(state, h)?.period;
    let point = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .and_then(|t| t.weapon.as_ref())
        .map_or(0, |w| {
            (f64::from(f32::from_bits(w.damage_point)) * 200.0).round() as u64
        });
    Some((next, period, point))
}

/// Whether worker `h` touches tree `tree`: the ring of cells round its
/// own meets one of the tree's (SPEC-0025, "Blocked units").
fn touches_tree(state: &State, h: u32, tree: u32) -> bool {
    let (Some(u), Some(Object::Destructable(d))) = (state.unit(Some(h)), state.objects.get(&tree))
    else {
        return false;
    };
    let class =
        crate::path::size_class(state.data.units.get(&u.type_id).map_or(0, |t| t.collision));
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let at = super::follow::to_cells(relative([u.x, u.y], min));
    super::follow::touches(d.footprint.red(), at, class)
}

/// Whether worker `h` is within the harvest's reach of tree `tree`.
fn within_reach(state: &State, h: u32, tree: u32) -> bool {
    let (Some(u), Some([tx, ty])) = (state.unit(Some(h)), position(state, tree)) else {
        return false;
    };
    let d = |a: u32, b: u32| (f32::from_bits(a) - f32::from_bits(b)) / 32.0;
    let (dx, dy) = (d(u.x, tx), d(u.y, ty));
    dx * dx + dy * dy <= TREE_REACH_SQUARED
}

/// Worker `h`, working out of reach of its tree, takes another: of the
/// trees within 900 of the current one, the first with the smallest cost
/// of a high-level path from the worker, each searched with its own cells
/// free and an iteration limit of 32, then of the best removals plus 16
/// (SPEC-0029, "Another tree").
///
fn take_another_tree(state: &mut State, h: u32, from_tree: bool) {
    let Some(g) = get(state, h) else {
        return;
    };
    let Some(current) = g.target else {
        return;
    };
    let (Some(u), Some([cx, cy])) = (state.unit(Some(h)), position(state, current)) else {
        return;
    };
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells = |p: [u32; 2]| {
        let r = relative(p, min);
        [mul(r[0], CELL), mul(r[1], CELL)]
    };
    // From the worker, or from the old tree's point after a delivery.
    let at = if from_tree {
        cells([cx, cy])
    } else {
        cells([u.x, u.y])
    };
    let class = state
        .data
        .units
        .get(&u.type_id)
        .map_or(0, |t| crate::path::size_class(t.collision));
    let mut trees: Vec<u32> = state
        .objects
        .iter()
        .filter(|&(&t, _)| t != current && is_tree(state, t))
        .filter(|&(_, o)| match o {
            Object::Destructable(d) => {
                let (dx, dy) = (
                    f32::from_bits(d.x) - f32::from_bits(cx),
                    f32::from_bits(d.y) - f32::from_bits(cy),
                );
                dx * dx + dy * dy <= OTHER_TREES * OTHER_TREES
            }
            _ => false,
        })
        .map(|(&t, _)| t)
        .collect();
    // Through the grid of 256 of SPEC-0019: cells by rows from the bottom,
    // each row from the left, in a cell the last created first (c-RYP-1,
    // read in the game).
    let cell = |v: u32, m: u32| ((f32::from_bits(v) - f32::from_bits(m)) / 256.0).floor() as i64;
    trees.sort_by_key(|&t| {
        position(state, t).map_or((i64::MAX, i64::MAX, 0), |[x, y]| {
            (cell(y, min[1]), cell(x, min[0]), -i64::from(t))
        })
    });
    let mut limit = 32;
    let mut best: Option<(u32, u32)> = None;
    for t in trees {
        let Some(Object::Destructable(d)) = state.objects.get(&t) else {
            continue;
        };
        let goal = cells([d.x, d.y]);
        let Some(grid) = state.grid.as_mut() else {
            return;
        };
        // The tree's blocks free, as for a walk to a tree (SPEC-0020).
        let found = grid.with_free_around(goal, |g| {
            crate::path::high_level_path_cost(g, at, goal, class, limit)
        });
        if let Some((removals, cost)) = found {
            limit = limit.min(removals + 16);
            if best.is_none_or(|(_, c)| cost < c) {
                best = Some((t, cost));
            }
        }
    }
    let Some((tree, _)) = best else {
        return;
    };
    if let Some(mut g) = get(state, h) {
        g.switch_at = None;
        g.target = Some(tree);
        g.ready = false;
        // A new tree has no walks again left (c-RYP-1: peasant 0 dropping
        // tree 9 at 62.87366).
        g.retries = 0;
        set(state, h, Some(g));
    }
    if let Some(at) = position(state, tree) {
        movement::harvest_order(state, h, Order::Gather { target: tree }, ORDER_HARVEST, at);
    }
}

/// Whether worker `h` is between its first arrival at its tree and the end
/// of its first work: its local search does not end touching its tree.
pub(super) fn at_tree(state: &State, h: u32) -> bool {
    get(state, h).is_some_and(|g| g.at_tree)
}

/// What worker `h` carries, for the interface.
pub(super) fn carried(state: &State, h: u32) -> Option<(Resource, i32)> {
    get(state, h).and_then(|g| g.carry)
}

/// The workers that start to wait at a busy mine in this tick push aside
/// the units on their cells.
fn start_waiting(state: &mut State) {
    let tick = state.ticks;
    let mut due = Vec::new();
    for mine in state.mines.values_mut() {
        mine.starting.retain(|&(w, t)| {
            if t == tick {
                due.push(w);
            }
            t > tick
        });
    }
    // At its update it goes in if the mine is free, else it waits; either
    // way the units on its cells are pushed aside and the pace of its
    // gold trip is taken (SPEC-0029).
    for w in due {
        let free = state
            .mines
            .values_mut()
            .find(|m| m.waiting.contains(&w))
            .filter(|m| m.inside.is_none());
        let goes_in = if let Some(mine) = free {
            mine.waiting.retain(|&x| x != w);
            mine.inside = Some((w, tick, 0));
            true
        } else {
            false
        };
        // One that goes in is hidden by the entries of this tick.
        if !goes_in {
            // Its walk to the mine ends here, after those going in
            // (`leave_mines`; SPEC-0024; c-RXI-1: worker 4 at 871).
            state.harvest_waiters.push(w);
            push_aside(state, w);
            pace(state, w);
        }
    }
}

/// A worker made for a computer player goes to gather at once, inside its
/// making: the gold mine nearest its player's town hall, when that is a
/// plain one it can gather from (c-SCH, c-SCJ-1, hub #15: the Orc's peons;
/// none with only a Haunted Gold Mine or no town hall).
///
/// PROVISIONAL: which town hall counts with several, the search's objects
/// and the order's drop on a far mine are not built.
pub(super) fn computer_worker_made(state: &mut State, h: u32) {
    let Some(mine) = computer_mine(state, h) else {
        return;
    };
    // Its order's kept part grows by 3 for each idle worker of its player,
    // itself not counted (c-SCX-1, hub #15: 9, 9, 12, 15, 18, 18).
    let owner = state.unit(Some(h)).map_or(-1, |u| u.owner);
    let idle = idle_workers(state, owner).saturating_sub(1);
    state.table.computer_harvest = Some(idle);
    movement::target_order(state, h, "harvest", mine);
    state.table.computer_harvest = None;
}

/// The gold mine a computer's new worker `h` is sent to: the plain one
/// nearest its player's first town hall, when it can gather from it.
pub(super) fn computer_mine(state: &State, h: u32) -> Option<u32> {
    let owner = state.unit(Some(h))?.owner;
    let computer = state
        .setup
        .slots
        .get(owner as usize)
        .is_some_and(|s| s.controller == super::players::CONTROL_COMPUTER);
    if !computer || gathering(state, h).is_none() {
        return None;
    }
    let hall = state.units.iter().copied().find(|&b| {
        state.unit(Some(b)).is_some_and(|u| {
            u.alive
                && u.owner == owner
                && state
                    .data
                    .units
                    .get(&u.type_id)
                    .is_some_and(|t| t.classes.iter().any(|c| c == "townhall"))
        })
    });
    let at = hall.and_then(|b| state.unit(Some(b))).map(|u| (u.x, u.y))?;
    let mines = [*b"ngol", *b"ugol", *b"egol"].map(i32::from_be_bytes);
    let distance = |u: &super::Unit| {
        let (dx, dy) = (
            f32::from_bits(u.x) - f32::from_bits(at.0),
            f32::from_bits(u.y) - f32::from_bits(at.1),
        );
        dx * dx + dy * dy
    };
    let nearest = state
        .units
        .iter()
        .copied()
        .filter(|&m| {
            state
                .unit(Some(m))
                .is_some_and(|u| u.alive && mines.contains(&u.type_id))
        })
        .min_by(|&a, &b| {
            let (da, db) = (
                state.unit(Some(a)).map_or(f32::MAX, distance),
                state.unit(Some(b)).map_or(f32::MAX, distance),
            );
            da.total_cmp(&db)
        })?;
    can_harvest(state, h, nearest).then_some(nearest)
}

/// The idle workers of player `owner`: its living workers with no order
/// (c-SCX-1, hub #15).
pub(super) fn idle_workers(state: &State, owner: i32) -> usize {
    state
        .units
        .iter()
        .filter(|&&w| {
            state.unit(Some(w)).is_some_and(|u| {
                u.alive
                    && u.owner == owner
                    && !u.hidden
                    && u.movement.is_none()
                    && u.attack.is_none()
                    && state
                        .data
                        .units
                        .get(&u.type_id)
                        .is_some_and(|t| super::table::worker(state, t))
            })
        })
        .count()
}

/// A mine that pays its owner for the workers at it, without trips
/// (SPEC-0029, "Undead: acolytes on a haunted mine"): a haunted mine with
/// its five places round it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PayingMine {
    /// The worker on each place, by angle 90, 162, 234, 306, 18.
    places: [Option<u32>; PLACES],
    /// The workers that count for the payments.
    working: Vec<u32>,
    /// Workers that start to count at a queue time.
    joining: Vec<(u32, u32)>,
    /// The queue time of the next payment, while any worker counts.
    expiry: Option<u32>,
    /// The time left to the next payment for one worker (seconds), while
    /// none counts.
    left: Option<u32>,
}

/// Places round a haunted mine (`Abgm` `DataC`).
const PLACES: usize = 5;
/// A worker put on its place starts to count 0.01 s later (SPEC-0029: about
/// 2 ticks).
const JOIN_DELAY: u32 = 0x3C23_D70A;

/// Whether `h` gathers from `mine` without trips: an acolyte (`Aaha`) on a
/// haunted mine (`Abgm`).
pub(super) fn pays_for(state: &State, h: u32, mine: u32) -> bool {
    let (Some(u), Some(m)) = (state.unit(Some(h)), state.unit(Some(mine))) else {
        return false;
    };
    let haunted = ability_of(state, u.type_id, code(b"Aaha")).is_some()
        && ability_of(state, m.type_id, code(b"Abgm")).is_some();
    m.alive && !m.hidden && (haunted || entangled_for(state, h, mine))
}

/// Whether `h` is a wisp (`Awha`) and `mine` an entangled mine (`Aegm`):
/// the wisps go in (SPEC-0029, "Night Elf: wisps on an entangled mine").
fn entangled_for(state: &State, h: u32, mine: u32) -> bool {
    let (Some(u), Some(m)) = (state.unit(Some(h)), state.unit(Some(mine))) else {
        return false;
    };
    ability_of(state, u.type_id, code(b"Awha")).is_some()
        && ability_of(state, m.type_id, code(b"Aegm")).is_some()
}

/// Whether acolyte `h` reaches haunted mine `mine` by the distance, not by
/// touching it (`reached`).
pub(super) fn reaches_by_distance(state: &State, h: u32, mine: u32) -> bool {
    pays_for(state, h, mine) && !entangled_for(state, h, mine)
}

/// The numbers of a paying mine's `Abgm` or `Aegm`: gold per payment,
/// radius of the places.
fn paying_numbers(state: &State, mine: u32) -> (i32, u32) {
    let a = state.unit(Some(mine)).and_then(|m| {
        ability_of(state, m.type_id, code(b"Abgm"))
            .or_else(|| ability_of(state, m.type_id, code(b"Aegm")))
    });
    // (DataD, the radius, is not loaded: 200 as read.)
    let radius = 200.0f32.to_bits();
    a.map_or((10, radius), |a| (f32::from_bits(a.data[0]) as i32, radius))
}

/// `harvest` by acolyte `h` on haunted mine `mine` (SPEC-0029): back on
/// its own place it counts again at once, without moving; else it walks
/// to the mine, refused when the five places are taken.
fn order_paid(state: &mut State, h: u32, mine: u32) -> bool {
    let wisp = entangled_for(state, h, mine);
    let p = state.paying.entry(mine).or_default();
    // A wisp inside: a new harvest is refused (c-SDC-1: 6501).
    if wisp && p.places.contains(&Some(h)) {
        return false;
    }
    if p.places.contains(&Some(h)) {
        if !p.working.contains(&h) {
            let now = state.timers.time();
            count_change(state, mine, now, Some(h), None);
        }
        return true;
    }
    // A full haunted mine refuses the order; a wisp walks to a full
    // entangled mine and loses its order there.
    if !wisp && p.places.iter().all(Option::is_some) {
        return false;
    }
    let mut g = state
        .unit(Some(h))
        .and_then(|u| u.harvest)
        .unwrap_or_default();
    g.ready = false;
    g.target = Some(mine);
    set(state, h, Some(g));
    let Some(at) = position(state, mine) else {
        return false;
    };
    movement::harvest_order(state, h, Order::Gather { target: mine }, ORDER_HARVEST, at)
}

/// Worker `h` turned to its target within reach (`movement`): its next
/// update finishes the walk. True when the mark was set, clearing it;
/// with `set`, sets it.
pub(super) fn turned_mark(state: &mut State, h: u32, set_it: bool) -> bool {
    let mut g = get(state, h).unwrap_or_default();
    let was = std::mem::replace(&mut g.ready, set_it);
    set(state, h, Some(g));
    was
}

/// Acolyte `h` reached haunted mine `mine`: it is put at once on the free
/// place nearest to where it stopped, and counts 0.01 s later.
fn arrive_paid(state: &mut State, h: u32, mine: u32) {
    if entangled_for(state, h, mine) {
        go_in_paid(state, h, mine);
        return;
    }
    let Some(here) = position(state, h) else {
        return;
    };
    let Some(centre) = position(state, mine) else {
        return;
    };
    let (_, radius) = paying_numbers(state, mine);
    let r = f32::from_bits(radius);
    let (cx, cy) = (
        f32::from_bits(centre[0]) + 16.0,
        f32::from_bits(centre[1]) + 16.0,
    );
    let points: Vec<[u32; 2]> = (0..PLACES)
        .map(|k| {
            let angle = (90.0 + 72.0 * k as f32).to_bits();
            let rad = mul(angle, (std::f32::consts::PI / 180.0).to_bits());
            [
                add(cx.to_bits(), mul(r.to_bits(), ow3_math::natives::cos(rad))),
                add(cy.to_bits(), mul(r.to_bits(), ow3_math::natives::sin(rad))),
            ]
        })
        .collect();
    let p = state.paying.entry(mine).or_default();
    let free = (0..PLACES)
        .filter(|&k| p.places[k].is_none())
        .min_by(|&a, &b| squared(here, points[a]).total_cmp(&squared(here, points[b])));
    let Some(k) = free else {
        movement::stop(state, h);
        return;
    };
    p.places[k] = Some(h);
    let at = add(state.timers.time(), JOIN_DELAY);
    p.joining.push((h, at));
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.x = points[k][0];
        u.y = points[k][1];
        u.movement = None;
    }
    state.drop_path(h);
    super::table::walk_dropped(state, h);
}

/// Wisp `h` reached entangled mine `mine` (SPEC-0029): it goes in, hidden
/// where it stopped, and counts 0.01 s later; at a mine with five inside it
/// loses its order and stands.
fn go_in_paid(state: &mut State, h: u32, mine: u32) {
    let p = state.paying.entry(mine).or_default();
    let Some(k) = (0..PLACES).find(|&k| p.places[k].is_none()) else {
        movement::stop(state, h);
        return;
    };
    p.places[k] = Some(h);
    let at = add(state.timers.time(), JOIN_DELAY);
    p.joining.push((h, at));
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.movement = None;
    }
    hide(state, h, true);
    state.drop_path(h);
    super::table::walk_dropped(state, h);
}

/// Worker `h` takes another order: it stops counting; a `stop` keeps its
/// place (SPEC-0029), any other order gives it up.
///
/// PROVISIONAL: an order that moves it away is taken to free the place.
pub(super) fn paid_order_given(state: &mut State, h: u32, keeps_place: bool) {
    if !keeps_place {
        wisp_order_given(state, h);
    }
    let Some(mine) = state
        .paying
        .iter()
        .find(|(_, p)| p.places.contains(&Some(h)))
        .map(|(&m, _)| m)
    else {
        return;
    };
    // A wisp inside ignores it (SPEC-0029; c-SDC-1: stop at 5300).
    // PROVISIONAL: read for `stop` only.
    if entangled_for(state, h, mine) {
        return;
    }
    let now = state.timers.time();
    if let Some(p) = state.paying.get_mut(&mine) {
        p.joining.retain(|&(w, _)| w != h);
    }
    if state
        .paying
        .get(&mine)
        .is_some_and(|p| p.working.contains(&h))
    {
        count_change(state, mine, now, None, Some(h));
    }
    if !keeps_place && let Some(p) = state.paying.get_mut(&mine) {
        for place in &mut p.places {
            if *place == Some(h) {
                *place = None;
            }
        }
    }
}

/// Whether worker `h` gathers at a paying mine now (its order reads
/// `harvest`).
pub(super) fn paid_working(state: &State, h: u32) -> bool {
    state
        .paying
        .values()
        .any(|p| p.working.contains(&h) || p.joining.iter().any(|&(w, _)| w == h))
        || state.wisp_trees.values().any(|&(w, _)| w == h)
}

/// The number of workers counting at `mine` changes at queue time `now`
/// (`join` comes, `leave` goes): the part of the period already done is
/// kept, the rest runs at the new rate.
fn count_change(state: &mut State, mine: u32, now: u32, join: Option<u32>, leave: Option<u32>) {
    let Some(p) = state.paying.get_mut(&mine) else {
        return;
    };
    let old = p.working.len() as f32;
    // The time one worker still needs for the next payment.
    let left = match (p.expiry, p.left) {
        (Some(e), _) => (f32::from_bits(e) - f32::from_bits(now)).max(0.0) * old,
        (None, Some(l)) => f32::from_bits(l),
        (None, None) => 5.0,
    };
    if let Some(w) = join {
        p.working.push(w);
    }
    if let Some(w) = leave {
        p.working.retain(|&x| x != w);
    }
    let n = p.working.len() as f32;
    if n > 0.0 {
        p.expiry = Some(add(now, (left / n).to_bits()));
        p.left = None;
    } else {
        p.expiry = None;
        p.left = Some(left.to_bits());
    }
}

/// The paying mines of this tick: workers that start to count, and the
/// payments due in it, in the order of their times.
fn pay_mines(state: &mut State) {
    let end = add(state.move_clock, crate::timers::TICK);
    let mines: Vec<u32> = state.paying.keys().copied().collect();
    for mine in mines {
        while let Some(p) = state.paying.get(&mine) {
            let join = p
                .joining
                .iter()
                .copied()
                .filter(|&(_, at)| f32::from_bits(at) <= f32::from_bits(end))
                .min_by(|a, b| f32::from_bits(a.1).total_cmp(&f32::from_bits(b.1)));
            let pay = p
                .expiry
                .filter(|&e| f32::from_bits(e) <= f32::from_bits(end));
            match (join, pay) {
                (Some((w, at)), pay)
                    if pay.is_none_or(|e| f32::from_bits(at) <= f32::from_bits(e)) =>
                {
                    if let Some(p) = state.paying.get_mut(&mine) {
                        p.joining.retain(|&(x, _)| x != w);
                    }
                    count_change(state, mine, at, Some(w), None);
                }
                (_, Some(e)) => {
                    paid(state, mine);
                    if let Some(p) = state.paying.get_mut(&mine) {
                        let n = p.working.len() as f32;
                        p.expiry = Some(add(e, (5.0 / n).to_bits()));
                    }
                }
                _ => break,
            }
        }
    }
}

/// A payment of `mine`: its gold to its owner, from the mine's.
///
/// PROVISIONAL: the upkeep is taken as for a delivery; an exhausted mine
/// is not read.
fn paid(state: &mut State, mine: u32) {
    let (gold, _) = paying_numbers(state, mine);
    let Some(owner) = state.unit(Some(mine)).map(|m| m.owner) else {
        return;
    };
    let used = state
        .setup
        .player_states
        .get(&(owner, FOOD_USED))
        .copied()
        .unwrap_or(0);
    let amount = gold * upkeep_tenths(used) / 10;
    *state.setup.player_states.entry((owner, GOLD)).or_default() += amount;
    *state
        .setup
        .player_states
        .entry((owner, GOLD_GATHERED))
        .or_default() += amount;
    if let Some(Object::Unit(m)) = state.objects.get_mut(&mine) {
        m.resources -= gold;
    }
}

/// Whether wisp `h` (`Awha`) can sit on tree `tree` (SPEC-0029, "Night
/// Elf: wisps on trees").
pub(super) fn wisp_tree(state: &State, h: u32, tree: u32) -> bool {
    state
        .unit(Some(h))
        .is_some_and(|u| ability_of(state, u.type_id, code(b"Awha")).is_some())
        && is_tree(state, tree)
}

/// The wisp sitting on `tree`, if any.
fn sitter(state: &State, tree: u32) -> Option<u32> {
    state.wisp_trees.get(&tree).map(|&(w, _)| w)
}

/// Wisp `h` reached tree `tree`: it is put at once on the tree's position,
/// or, the tree taken, on the nearest free tree; the owner gets `Awha`
/// `DataA` lumber every `Dur` from then on.
///
/// PROVISIONAL: the search for another tree is taken as the nearest free
/// living tree within 800.
fn sit_on_tree(state: &mut State, h: u32, tree: u32) {
    let Some(here) = position(state, tree) else {
        return;
    };
    let tree = if sitter(state, tree).is_none_or(|w| w == h) {
        Some(tree)
    } else {
        let mut best: Option<(f32, u32)> = None;
        for (&t, o) in &state.objects {
            if !matches!(o, Object::Destructable(_))
                || !is_tree(state, t)
                || sitter(state, t).is_some()
            {
                continue;
            }
            let Some(p) = position(state, t) else {
                continue;
            };
            let d = squared(here, p);
            if d <= 800.0 * 800.0 && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, t));
            }
        }
        best.map(|(_, t)| t)
    };
    let Some(tree) = tree else {
        movement::stop(state, h);
        return;
    };
    let Some(at) = position(state, tree) else {
        return;
    };
    let period = state
        .unit(Some(h))
        .and_then(|u| ability_of(state, u.type_id, code(b"Awha")))
        .map_or(8.0f32.to_bits(), |a| a.duration);
    let next = add(state.timers.time(), period);
    state.wisp_trees.insert(tree, (h, next));
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.x = at[0];
        u.y = at[1];
        u.movement = None;
    }
    state.drop_path(h);
    super::table::walk_dropped(state, h);
}

/// The lumber of the wisps on trees due in this tick.
fn wisp_lumber(state: &mut State) {
    let end = add(state.move_clock, crate::timers::TICK);
    let trees: Vec<u32> = state.wisp_trees.keys().copied().collect();
    for tree in trees {
        let Some(&(w, next)) = state.wisp_trees.get(&tree) else {
            continue;
        };
        if f32::from_bits(next) > f32::from_bits(end) {
            continue;
        }
        let Some(u) = state.unit(Some(w)) else {
            state.wisp_trees.remove(&tree);
            continue;
        };
        let owner = u.owner;
        let a = ability_of(state, u.type_id, code(b"Awha"));
        let (amount, period) = a.map_or((5, 8.0f32.to_bits()), |a| {
            (f32::from_bits(a.data[0]) as i32, a.duration)
        });
        *state
            .setup
            .player_states
            .entry((owner, LUMBER))
            .or_default() += amount;
        *state
            .setup
            .player_states
            .entry((owner, LUMBER_GATHERED))
            .or_default() += amount;
        state.wisp_trees.insert(tree, (w, add(next, period)));
    }
}

/// Wisp `h` takes another order: it leaves its tree.
///
/// PROVISIONAL: whether `stop` keeps it on the tree is not read.
fn wisp_order_given(state: &mut State, h: u32) {
    state.wisp_trees.retain(|_, &mut (w, _)| w != h);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0029, "Upkeep": 10, 7 and 4 gold of a trip of 10 with 41, 61
    /// and 81 to 181 food used.
    #[test]
    fn upkeep() {
        let kept = |used| 10 * upkeep_tenths(used) / 10;
        assert_eq!(kept(41), 10);
        assert_eq!(kept(61), 7);
        for used in [81, 101, 141, 181] {
            assert_eq!(kept(used), 4);
        }
    }
}
