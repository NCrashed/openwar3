//! Regions and their enter and leave events (SPEC-0049).
//!
//! A region is a set of cells of 32 x 32 world units. A unit is tested
//! against the regions when it is placed by `CreateUnit` (or made by the
//! game), moved by `SetUnitX`, `SetUnitY` and `SetUnitPosition`, and at
//! the commits of its path agent while it walks: it enters a region when
//! the cell of its last test is outside and the new one inside, and leaves
//! it on the opposite move. Death, removal and `ShowUnit` are no tests.
//!
//! The filter of a registration runs at the crossing. An event it lets
//! through takes a slot on the timer queue like a 0 s timer started then
//! ([`Slot::RegionEvent`]); its conditions and actions run when a slot
//! fires. The events whose slots expire at the same time run in the order
//! of SPEC-0049: by region, the newest first, then by the order of the
//! registrations on the region, then by the order of the crossings.
//!
//! PROVISIONAL (SPEC-0049 leaves them open; listed in specs/QUESTIONS.md):
//! - The events of a call are those whose slots expire at the same time;
//!   enter events of a call come before its leave events, the order read
//!   in c-SAQ for one call.
//! - Adding or clearing cells is no test: a unit inside cells added later
//!   enters only at its next test that moves it from a cell outside.
//! - `RegionClearRect` and `RegionClearCell` take the cells off as
//!   `RegionAddRect` and `RegionAddCell` add them; `RemoveRegion` ends its
//!   events, also those queued.
//! - Commits of a walker other than at its update (stops, new orders) are
//!   tested at the next dispatch, at the committed position, and the
//!   filters of walkers run after the updates of the tick.
//! - The filter sees the region as `GetTriggeringRegion` and the event as
//!   `GetTriggerEventId`.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use super::widgets::Registration;
use super::{Context, Object, Slot, State, fire_trigger, handle, real, test_boolexpr};

/// `EVENT_GAME_ENTER_REGION` and `EVENT_GAME_LEAVE_REGION`.
pub(super) const EVENT_ENTER: i32 = 5;
pub(super) const EVENT_LEAVE: i32 = 6;

/// A region: rows of cells, each row its runs of cells (first, last),
/// sorted, apart and not touching.
#[derive(Clone, Debug, Default)]
pub struct Region {
    /// The number of its creation: newer regions come first.
    seq: u64,
    rows: BTreeMap<i32, Vec<(i32, i32)>>,
}

impl Region {
    pub fn contains(&self, cell: (i32, i32)) -> bool {
        self.rows
            .get(&cell.1)
            .is_some_and(|runs| runs.iter().any(|&(a, b)| a <= cell.0 && cell.0 <= b))
    }

    /// Adds cells `x0..=x1` of the rows `y0..=y1`.
    fn add(&mut self, (x0, x1): (i32, i32), (y0, y1): (i32, i32)) {
        for y in y0..=y1 {
            let runs = self.rows.entry(y).or_default();
            runs.push((x0, x1));
            runs.sort_unstable();
            let mut merged: Vec<(i32, i32)> = Vec::with_capacity(runs.len());
            for &(a, b) in runs.iter() {
                match merged.last_mut() {
                    Some(last) if i64::from(a) <= i64::from(last.1) + 1 => last.1 = last.1.max(b),
                    _ => merged.push((a, b)),
                }
            }
            *runs = merged;
        }
    }

    /// Takes off cells `x0..=x1` of the rows `y0..=y1`.
    fn clear(&mut self, (x0, x1): (i32, i32), (y0, y1): (i32, i32)) {
        let rows: Vec<i32> = self.rows.range(y0..=y1).map(|(&y, _)| y).collect();
        for y in rows {
            let Some(runs) = self.rows.get_mut(&y) else {
                continue;
            };
            let mut kept = Vec::with_capacity(runs.len() + 1);
            for &(a, b) in runs.iter() {
                if b < x0 || a > x1 {
                    kept.push((a, b));
                    continue;
                }
                if a < x0 {
                    kept.push((a, x0 - 1));
                }
                if b > x1 {
                    kept.push((x1 + 1, b));
                }
            }
            if kept.is_empty() {
                self.rows.remove(&y);
            } else {
                *runs = kept;
            }
        }
    }
}

/// The cell of one coordinate: `floor(v / 32)`, exact for every float.
pub fn cell(v: u32) -> i32 {
    (f32::from_bits(v) * (1.0 / 32.0)).floor() as i32
}

pub fn cell_of(at: [u32; 2]) -> (i32, i32) {
    (cell(at[0]), cell(at[1]))
}

/// The cells of a rect, both ends included (SPEC-0049): x and y ranges.
fn rect_cells(r: [u32; 4]) -> ((i32, i32), (i32, i32)) {
    ((cell(r[0]), cell(r[2])), (cell(r[1]), cell(r[3])))
}

/// What the region test of a unit saw last: the cell it was tested at,
/// and the committed position of its path agent then.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tested {
    cell: Option<(i32, i32)>,
    origin: Option<[u32; 2]>,
}

/// The order of the queued events of one call: enter before leave, the
/// newest region first, the registrations in order, the crossings in
/// order.
type Key = (bool, Reverse<u64>, usize, u64);

/// A crossing whose filter has not run yet.
#[derive(Clone, Copy, Debug)]
pub(super) struct Crossing {
    unit: u32,
    /// The queue time of the crossing when it is not the queue time now
    /// (`State::event_time`).
    at: Option<u32>,
    region: u32,
    trigger: u32,
    filter: Option<u32>,
    enter: bool,
    key: Key,
}

/// An event waiting for its slot on the timer queue.
#[derive(Clone, Copy, Debug)]
pub(super) struct Queued {
    slot: crate::timers::TimerId,
    unit: u32,
    region: u32,
    trigger: u32,
    enter: bool,
    key: Key,
}

fn region(state: &State, h: Option<u32>) -> Option<&Region> {
    match state.objects.get(&h?)? {
        Object::Region(r) => Some(r),
        _ => None,
    }
}

fn region_mut(state: &mut State, h: Option<u32>) -> Option<&mut Region> {
    match state.objects.get_mut(&h?)? {
        Object::Region(r) => Some(r),
        _ => None,
    }
}

/// Tests unit `h` at its position now (`CreateUnit`, `SetUnitX`, ...).
pub(super) fn test_unit(state: &mut State, h: u32) {
    if let Some(u) = state.unit(Some(h)) {
        let at = [u.x, u.y];
        test_at(state, h, at);
    }
}

/// Tests unit `h` at `at`: the regions whose cells it entered or left
/// since its last test give their registrations' crossings, the newest
/// region first.
pub(super) fn test_at(state: &mut State, h: u32, at: [u32; 2]) {
    let new = cell_of(at);
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    let old = u.region_test.cell.replace(new);
    if old == Some(new) || state.regions.is_empty() {
        return;
    }
    for &r in state.regions.iter().rev() {
        let Some(region) = region(state, Some(r)) else {
            continue;
        };
        let was = old.is_some_and(|c| region.contains(c));
        let now = region.contains(new);
        if was == now {
            continue;
        }
        let seq = region.seq;
        state.region_entries += 1;
        let entry = state.region_entries;
        for (i, registration) in state.events.iter().enumerate() {
            if let Registration::Region {
                trigger,
                region: on,
                enter,
                filter,
            } = *registration
                && on == r
                && enter == now
            {
                state.crossings.push_back(Crossing {
                    unit: h,
                    at: state.event_time,
                    region: r,
                    trigger,
                    filter,
                    enter,
                    key: (!enter, Reverse(seq), i, entry),
                });
            }
        }
    }
}

/// The commit of walker `h` at its update, at `origin` (relative to the
/// world's corner).
pub(super) fn committed(state: &mut State, h: u32, origin: [u32; 2]) {
    if state.regions.is_empty() {
        return;
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.region_test.origin = Some(origin);
    }
    let min = [state.world_bounds[0], state.world_bounds[1]];
    test_at(state, h, super::movement::absolute(origin, min));
}

/// The walkers whose path agents committed a new position since their
/// last test are tested at it.
fn walkers(state: &mut State) {
    if state.regions.is_empty() {
        return;
    }
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let mut moved = Vec::new();
    for &h in &state.units {
        if let Some(Object::Unit(u)) = state.objects.get(&h)
            && let Some(m) = u.movement
            && u.region_test.origin != Some(m.origin)
        {
            moved.push((h, m.origin));
        }
    }
    for (h, origin) in moved {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.region_test.origin = Some(origin);
        }
        test_at(state, h, super::movement::absolute(origin, min));
    }
}

/// Before the first region there is nothing to test against: when it is
/// made, every unit's last test is taken to be where it is now.
fn first_region(state: &mut State) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    for &h in &state.units {
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            let origin = u.movement.map(|m| m.origin);
            let at = origin.map_or([u.x, u.y], |o| super::movement::absolute(o, min));
            u.region_test = Tested {
                cell: Some(cell_of(at)),
                origin,
            };
        }
    }
}

/// Runs the filters of the crossings found so far, each event they let
/// through taking its slot on the timer queue.
pub(super) fn dispatch(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    walkers(&mut state.borrow_mut());
    loop {
        let Some(c) = state.borrow_mut().crossings.pop_front() else {
            break;
        };
        let filter_context = Context {
            filter_unit: Some(c.unit),
            triggering_region: Some(c.region),
            event_id: Some(if c.enter { EVENT_ENTER } else { EVENT_LEAVE }),
            ..Context::default()
        };
        if test_boolexpr(vm, state, c.filter, filter_context) {
            let mut s = state.borrow_mut();
            // A 0 s timer started at the crossing: 0.0001 s after its
            // queue time.
            let time = c.at.unwrap_or_else(|| s.timers.time());
            let slot = s.timers.start_from(time, 0, Slot::RegionEvent);
            s.region_events.push(Queued {
                slot,
                unit: c.unit,
                region: c.region,
                trigger: c.trigger,
                enter: c.enter,
                key: c.key,
            });
        }
    }
}

/// A slot of the region events fired at `expiry`: the first in order of
/// the events whose slots expire then runs.
pub(super) fn fire(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    slot: crate::timers::TimerId,
    expiry: u32,
) {
    let event = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let timers = &s.timers;
        let first = s
            .region_events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.slot == slot || timers.expiry(e.slot) == Some(expiry))
            .min_by_key(|(_, e)| e.key)
            .map(|(i, _)| i);
        let Some(i) = first else {
            return;
        };
        // The event that runs gives its own slot, still queued, to the one
        // whose slot this was.
        let taken = s.region_events[i].slot;
        if let Some(e) = s.region_events.iter_mut().find(|e| e.slot == slot) {
            e.slot = taken;
        }
        s.region_events.remove(i)
    };
    if region(&state.borrow(), Some(event.region)).is_none() {
        return;
    }
    super::table::copies_freed(&mut state.borrow_mut());
    let context = Context {
        event_id: Some(if event.enter {
            EVENT_ENTER
        } else {
            EVENT_LEAVE
        }),
        triggering_unit: Some(event.unit),
        triggering_region: Some(event.region),
        ..Context::default()
    };
    fire_trigger(vm, state, event.trigger, context);
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native CreateRegion takes nothing returns region",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if state.regions.is_empty() {
                first_region(&mut state);
            }
            state.region_seq += 1;
            let region = Region {
                seq: state.region_seq,
                ..Region::default()
            };
            let h = state.create(Object::Region(region));
            state.regions.push(h);
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native RemoveRegion takes region whichRegion returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(h) = handle(&args[0])
                && region(&state, Some(h)).is_some()
            {
                state.objects.remove(&h);
                state.regions.retain(|&r| r != h);
            }
            Ok(None)
        }
    );
    for (declaration, add_cells) in [
        (
            "native RegionAddRect takes region whichRegion, rect r returns nothing",
            true,
        ),
        (
            "native RegionClearRect takes region whichRegion, rect r returns nothing",
            false,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(r) = super::enumerate::rect(&state, handle(&args[1])) else {
                return Ok(None);
            };
            let (xs, ys) = rect_cells(r);
            if let Some(region) = region_mut(&mut state, handle(&args[0])) {
                if add_cells {
                    region.add(xs, ys);
                } else {
                    region.clear(xs, ys);
                }
            }
            Ok(None)
        });
    }
    type Point = fn(&State, &[Value]) -> Option<[u32; 2]>;
    let by_coordinates: Point = |_, args| Some([real(&args[1]), real(&args[2])]);
    let by_location: Point =
        |state, args| super::enumerate::location(state, handle(&args[1])).map(|(x, y)| [x, y]);
    for (declaration, add_cell, point) in [
        (
            "native RegionAddCell takes region whichRegion, real x, real y returns nothing",
            true,
            by_coordinates,
        ),
        (
            "native RegionAddCellAtLoc takes region whichRegion, location whichLocation returns nothing",
            true,
            by_location,
        ),
        (
            "native RegionClearCell takes region whichRegion, real x, real y returns nothing",
            false,
            by_coordinates,
        ),
        (
            "native RegionClearCellAtLoc takes region whichRegion, location whichLocation returns nothing",
            false,
            by_location,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(at) = point(&state, args) else {
                return Ok(None);
            };
            let (x, y) = cell_of(at);
            if let Some(region) = region_mut(&mut state, handle(&args[0])) {
                if add_cell {
                    region.add((x, x), (y, y));
                } else {
                    region.clear((x, x), (y, y));
                }
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native IsUnitInRegion takes region whichRegion, unit whichUnit returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let inside = match (
                region(&state, handle(&args[0])),
                state.unit(handle(&args[1])),
            ) {
                (Some(r), Some(u)) => r.contains(cell_of([u.x, u.y])),
                _ => false,
            };
            Ok(Some(Value::Boolean(inside)))
        }
    );
    for (declaration, point) in [
        (
            "native IsPointInRegion takes region whichRegion, real x, real y returns boolean",
            by_coordinates,
        ),
        (
            "native IsLocationInRegion takes region whichRegion, location whichLocation returns boolean",
            by_location,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let state = state.borrow();
            let inside = match (region(&state, handle(&args[0])), point(&state, args)) {
                (Some(r), Some(at)) => r.contains(cell_of(at)),
                _ => false,
            };
            Ok(Some(Value::Boolean(inside)))
        });
    }
    native!(
        add,
        state,
        "constant native GetTriggeringRegion takes nothing returns region",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.triggering_region,
            )))
        }
    );
    for (declaration, event) in [
        (
            "constant native GetEnteringUnit takes nothing returns unit",
            EVENT_ENTER,
        ),
        (
            "constant native GetLeavingUnit takes nothing returns unit",
            EVENT_LEAVE,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let context = state.borrow().context;
            let unit = context
                .triggering_unit
                .filter(|_| context.event_id == Some(event));
            Ok(Some(Value::Handle(unit)))
        });
    }
    for (declaration, enter) in [
        (
            "native TriggerRegisterEnterRegion takes trigger whichTrigger, region whichRegion, boolexpr filter returns event",
            true,
        ),
        (
            "native TriggerRegisterLeaveRegion takes trigger whichTrigger, region whichRegion, boolexpr filter returns event",
            false,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let (Some(trigger), Some(region)) = (handle(&args[0]), handle(&args[1])) else {
                return Ok(Some(Value::Handle(None)));
            };
            let registration = Registration::Region {
                trigger,
                region,
                enter,
                filter: handle(&args[2]),
            };
            Ok(Some(super::widgets::register_event(state, registration)))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(v: f32) -> u32 {
        v.to_bits()
    }

    fn inside(r: &Region, x: f32, y: f32) -> bool {
        r.contains(cell_of([bits(x), bits(y)]))
    }

    /// SPEC-0049, "Regions are sets of cells": `RegionAddCell(r, 10, 10)`.
    #[test]
    fn a_cell() {
        let mut r = Region::default();
        let (x, y) = cell_of([bits(10.0), bits(10.0)]);
        r.add((x, x), (y, y));
        for (x, y) in [(0.0, 0.0), (9.0, 10.0), (25.9, 10.0), (31.9, 31.9)] {
            assert!(inside(&r, x, y), "({x}, {y})");
        }
        for (x, y) in [(32.0, 32.0), (42.1, 10.0), (-5.9, 10.0), (-0.01, -0.01)] {
            assert!(!inside(&r, x, y), "({x}, {y})");
        }
    }

    /// SPEC-0049, verification 1: `RegionAddRect(r, Rect(0, 0, 256, 256))`
    /// takes in the cell of the maximum edge.
    #[test]
    fn a_rect() {
        let mut r = Region::default();
        let (xs, ys) = rect_cells([0.0, 0.0, 256.0, 256.0].map(bits));
        assert_eq!((xs, ys), ((0, 8), (0, 8)));
        r.add(xs, ys);
        for x in [0.0, 255.9, 256.0, 256.1, 287.9] {
            assert!(inside(&r, x, 100.0), "x {x}");
        }
        for x in [-0.1, -31.9, -32.1, 288.0] {
            assert!(!inside(&r, x, 100.0), "x {x}");
        }
        assert!(inside(&r, 0.0, 0.0));
        assert!(!inside(&r, -0.01, -0.01));
    }

    #[test]
    fn cells_merge_and_clear() {
        let mut r = Region::default();
        r.add((0, 3), (0, 0));
        r.add((5, 9), (0, 0));
        r.add((4, 4), (0, 0));
        assert_eq!(r.rows[&0], [(0, 9)]);
        r.clear((2, 3), (0, 1));
        assert_eq!(r.rows[&0], [(0, 1), (4, 9)]);
        r.clear((-5, 20), (0, 0));
        assert!(r.rows.is_empty());
    }
}
