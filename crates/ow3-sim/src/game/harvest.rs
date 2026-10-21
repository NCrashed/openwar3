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
use ow3_math::{add, mul, sub};

use super::movement::{self, CELL, Order, relative};
use super::{Object, State};

/// The orders `harvest` and `resumeharvesting` (SPEC-0029).
pub const ORDER_HARVEST: i32 = 852_018;
pub const ORDER_RESUME_HARVESTING: i32 = 852_017;

/// The player states of gold and lumber.
const GOLD: i32 = 1;
const LUMBER: i32 = 2;

/// Ticks a worker stays inside a mine (1.0 s).
const IN_MINE: u64 = 200;
/// Ticks between two updates of a unit (SPEC-0018).
const UPDATE: u64 = 6;
/// Ticks from reaching a tree to the first hit, and from the hit that
/// fills the load to the walk back (measured in test RUF).
const FIRST_HIT: u64 = 99;
const AFTER_LAST_HIT: u64 = 114;

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
    /// The tick of its next hit on a tree, and of the walk back with a
    /// full load.
    next_hit: Option<u64>,
    back_at: Option<u64>,
    /// Out of a mine once and gathering since: its cells block only the
    /// placing of buildings (SPEC-0029, "Workers in the gathering cycle
    /// do not block each other").
    cycling: bool,
}

/// Who is inside a mine, until when, and who waits at it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mine {
    inside: Option<(u32, u64)>,
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
    reach: u32,
    area: u32,
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
        reach: a.range,
        area: a.area,
    })
}

/// Whether `h` is a gold mine.
fn is_mine(state: &State, h: u32) -> bool {
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
/// cell. Free cells are passable and not covered by a standing unit,
/// waiting workers and the worker inside a mine included.
fn exit_place(state: &State, h: u32) -> Option<[u32; 2]> {
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
    let inside = |o: u32| {
        state
            .mines
            .values()
            .any(|m| m.inside.is_some_and(|(w, _)| w == o))
    };
    let mut taken = std::collections::BTreeSet::new();
    for &o in &state.units {
        let Some(Object::Unit(w)) = state.objects.get(&o) else {
            continue;
        };
        let moving = w.movement.is_some_and(|m| m.velocity != [0, 0]);
        if o == h || !w.alive || moving || (w.hidden && !inside(o)) {
            continue;
        }
        taken.extend(crate::path::occupied_cells(
            cells([w.x, w.y]),
            class_of(w.type_id),
        ));
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

/// Sends worker `h` to its mine or tree, or with a load from elsewhere
/// first to deliver it.
fn walk_to_work(state: &mut State, h: u32) -> bool {
    let Some(g) = get(state, h) else {
        return false;
    };
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

/// What the workers that reached their mines, trees and drop-offs at the
/// updates of this tick do, then the mines and the trees of this tick.
pub(super) fn tick(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let reached = std::mem::take(&mut state.borrow_mut().harvest_reached);
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
        } else if is_tree(&s, target) {
            if let Some(mut g) = get(&s, h) {
                let tick = s.ticks;
                g.next_hit = Some(tick + FIRST_HIT);
                set(&mut s, h, Some(g));
            }
        } else {
            drop(s);
        }
    }
    forget_others(&mut state.borrow_mut());
    start_waiting(&mut state.borrow_mut());
    leave_mines(vm, state);
    hit_trees(vm, state);
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
        set(state, h, Some(g));
        for mine in state.mines.values_mut() {
            mine.waiting.retain(|&w| w != h);
        }
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
        entry.inside = Some((h, tick + UPDATE + IN_MINE));
        state.objects_change_in(tick + 2 * UPDATE);
    } else if !entry.waiting.contains(&h) {
        entry.waiting.push(h);
        // It starts to wait at its next update (SPEC-0029).
        entry.starting.push((h, tick + UPDATE));
        state.objects_change_in(tick + UPDATE);
        state.objects_change_in(tick + 2 * UPDATE);
    }
}

/// A worker that starts to wait at a busy mine holds its cells for every
/// unit: the units on them are put at the nearest free place by the
/// search of a worker coming out of a mine (SPEC-0029).
fn push_aside(state: &mut State, h: u32) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells_of = |state: &State, o: u32| -> Vec<(i32, i32)> {
        let Some(u) = state.unit(Some(o)) else {
            return Vec::new();
        };
        let class = state
            .data
            .units
            .get(&u.type_id)
            .map_or(0, |t| crate::path::size_class(t.collision));
        let r = relative([u.x, u.y], min);
        crate::path::occupied_cells([mul(r[0], CELL), mul(r[1], CELL)], class)
    };
    let mine = cells_of(state, h);
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
        .filter(|&o| cells_of(state, o).iter().any(|c| mine.contains(c)))
        .collect();
    for o in on {
        if let Some(at) = exit_place(state, o)
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
fn leave_mines(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    // Those whose time to go in has come are hidden.
    {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let entering: Vec<u32> = s
            .mines
            .values()
            .filter_map(|m| {
                m.inside
                    .filter(|&(_, t)| t == tick + IN_MINE)
                    .map(|(w, _)| w)
            })
            .collect();
        for w in entering {
            hide(&mut s, w, true);
        }
    }
    let due: Vec<(u32, u32)> = {
        let s = state.borrow();
        s.mines
            .iter()
            .filter_map(|(&mine, m)| {
                m.inside
                    .filter(|&(_, t)| t <= s.ticks)
                    .map(|(w, _)| (mine, w))
            })
            .collect()
    };
    for (mine, worker) in due {
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
            if let Some(at) = exit_place(s, worker) {
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
            if left > 0 && !entry.waiting.is_empty() {
                let next = entry.waiting.remove(0);
                entry.inside = Some((next, tick + IN_MINE));
                hide(s, next, true);
            }
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
fn hit_trees(vm: &mut Vm, state: &Rc<RefCell<State>>) {
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
        if g.back_at == Some(tick) {
            let mut s = state.borrow_mut();
            let mut g = g;
            g.back_at = None;
            set(&mut s, h, Some(g));
            walk_back(&mut s, h);
            continue;
        }
        if g.next_hit != Some(tick) {
            continue;
        }
        let Some(tree) = g.target else {
            continue;
        };
        let life = match state.borrow().objects.get(&tree) {
            Some(Object::Destructable(d)) if d.alive => d.life,
            _ => 0,
        };
        if f32::from_bits(life) <= 0.0 {
            let mut s = state.borrow_mut();
            next_tree(&mut s, h, tree, numbers);
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
            g.back_at = Some(tick + AFTER_LAST_HIT);
            set(&mut s, h, Some(g));
        } else {
            g.next_hit = Some(tick + numbers.period);
            set(&mut s, h, Some(g));
            if !is_tree(&s, tree) {
                next_tree(&mut s, h, tree, numbers);
            }
        }
    }
}

/// Worker `h`'s tree `old` is gone: it goes on at the nearest living tree
/// within `Area` of it, or takes back what it carries.
fn next_tree(state: &mut State, h: u32, old: u32, numbers: Gathering) {
    let from = position(state, old).or_else(|| position(state, h));
    let mut best: Option<(u32, f32)> = None;
    if let Some(from) = from {
        let area = f32::from_bits(numbers.area);
        for (&d, o) in &state.objects {
            let Object::Destructable(t) = o else {
                continue;
            };
            if !t.alive || !state.data.trees.contains(&t.type_id) {
                continue;
            }
            let e = squared(from, [t.x, t.y]);
            if e <= area * area && best.is_none_or(|(b, f)| e < f || (e == f && d < b)) {
                best = Some((d, e));
            }
        }
    }
    match best {
        Some((tree, _)) => {
            if let Some(mut g) = get(state, h) {
                g.target = Some(tree);
                g.next_hit = None;
                set(state, h, Some(g));
            }
            walk_to_work(state, h);
        }
        None => {
            if let Some(mut g) = get(state, h) {
                g.target = None;
                g.next_hit = None;
                set(state, h, Some(g));
            }
            if !walk_back(state, h) {
                movement::stop(state, h);
            }
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
        *state.setup.player_states.entry((owner, which)).or_default() += amount;
    }
    let target = g.target.filter(|&t| is_mine(state, t) || is_tree(state, t));
    g.target = target;
    set(state, h, Some(g));
    if target.is_none() || !walk_to_work(state, h) {
        movement::stop(state, h);
    }
}

/// Worker `h` hitting a tree: the tick of its next hit, the ticks between
/// hits and the damage point of its weapon in ticks, for the swing shown.
pub(super) fn chopping(state: &State, h: u32) -> Option<(u64, u64, u64)> {
    let next = get(state, h)?.next_hit?;
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
    for w in due {
        push_aside(state, w);
    }
}
