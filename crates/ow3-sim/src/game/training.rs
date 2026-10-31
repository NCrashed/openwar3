//! Buildings training units (SPEC-0027), and the timers of the engine on
//! the movement clock that end trainings and constructions.
//!
//! An order to train pays the cost and puts the unit at the end of the
//! building's queue (at most 7). The head of the queue trains once the
//! player's food allows it; its end is a timer of the engine at the start
//! plus the build time. The trained unit appears on the first free cell
//! of rings round the building's pathing texture, starting on the side of
//! the rally point, and walks to the rally point.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;
use ow3_math::{add, i2r, mul, sub};

use super::{Context, Object, State, fire_trigger, test_boolexpr};

/// The longest queue of a building.
const QUEUE: usize = 7;
/// 0.0001: the next training of a queue starts this long after the end of
/// the one before.
const NEXT_START: u32 = 0x38D1_B717;
/// The player states of gold, lumber, food cap and food used.
const GOLD: i32 = 1;
const LUMBER: i32 = 2;
const FOOD_CAP: i32 = 4;
const FOOD_USED: i32 = 5;
/// Player unit events of a training.
const EVENT_PLAYER_UNIT_TRAIN_START: i32 = 32;
const EVENT_PLAYER_UNIT_TRAIN_CANCEL: i32 = 33;
const EVENT_PLAYER_UNIT_TRAIN_FINISH: i32 = 34;
const EVENT_PLAYER_UNIT_ISSUED_ORDER: i32 = 38;
/// The order `cancel`.
const ORDER_CANCEL: i32 = 851_976;
/// The order a trained unit gets to walk to the rally point.
pub(super) const ORDER_RALLY: i32 = 851_970;

/// What a timer of the engine does when it expires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineTimer {
    /// A building's construction is finished (SPEC-0026).
    Construction(u32),
    /// The training at the head of a building's queue ends.
    Training(u32),
    /// A dead hero's revive ends (`revive`).
    Revive(u32),
    /// A worker requested by a player-bound melee AI becomes active.
    AiWorker {
        owner: i32,
        hall: u32,
        type_id: i32,
        records: [u32; 2],
    },
}

/// The queue of a building.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue {
    /// The unit types, the head first.
    types: Vec<i32>,
    /// The expiry of the head's training once it has started.
    training: Option<u32>,
}

impl Queue {
    /// The unit types queued, the head first.
    pub fn types(&self) -> &[i32] {
        &self.types
    }

    /// The share of the head's training done at `clock`, given its build
    /// time: 0 before it starts.
    pub fn progress(&self, clock: u32, build_time: u32) -> f32 {
        let Some(expiry) = self.training else {
            return 0.0;
        };
        let total = f32::from_bits(build_time);
        if total <= 0.0 {
            return 1.0;
        }
        let left = f32::from_bits(expiry) - f32::from_bits(clock);
        (1.0 - left / total).clamp(0.0, 1.0)
    }
}

/// Starts a timer of the engine that expires at `expiry` on the movement
/// clock.
pub(super) fn start_timer(state: &mut State, expiry: u32, timer: EngineTimer) {
    state.engine_started += 1;
    let key = (expiry, state.engine_started);
    state.engine_timers.insert(key, timer);
}

pub(super) fn cancel_timer(state: &mut State, timer: EngineTimer) {
    state.engine_timers.retain(|_, t| *t != timer);
}

/// Fires the timers of the engine due in this tick, the earliest first:
/// those with an expiry at most the movement clock of the tick. While one
/// runs, the movement clock reads its expiry (SPEC-0027, "Timers of the
/// engine").
pub(super) fn fire_engine_timers(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    loop {
        // A revive ends on the world clock (c-SBQ-1, c-SBU-1: its start's
        // time plus the revive time, to the bit).
        let (clock, due) = {
            let s = state.borrow();
            let world = f32::from_bits(s.timers.time());
            let moved = f32::from_bits(s.move_clock);
            (
                s.move_clock,
                s.engine_timers
                    .iter()
                    .find(|(k, t)| match t {
                        EngineTimer::Revive(_) => world >= f32::from_bits(k.0),
                        _ => moved >= f32::from_bits(k.0),
                    })
                    .map(|(k, t)| (*k, *t)),
            )
        };
        let Some((key, timer)) = due else {
            break;
        };
        state.borrow_mut().engine_timers.remove(&key);
        if !matches!(timer, EngineTimer::Revive(_)) {
            state.borrow_mut().move_clock = key.0;
        }
        match timer {
            EngineTimer::Construction(h) => super::finish_construction(vm, state, h),
            EngineTimer::Training(h) => end_training(vm, state, h),
            EngineTimer::Revive(h) => super::revive::finish(vm, state, h),
            EngineTimer::AiWorker {
                owner,
                hall,
                type_id,
                records,
            } => finish_ai_worker(vm, state, owner, hall, type_id, records),
        }
        state.borrow_mut().move_clock = clock;
    }
}

/// Completes a worker request owned by a player-bound melee AI. Such a
/// request is not a visible building queue before completion.
fn finish_ai_worker(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    owner: i32,
    hall: u32,
    type_id: i32,
    records: [u32; 2],
) {
    {
        let mut s = state.borrow_mut();
        s.table.defer(records[1], "AI worker production");
        s.table.defer(records[0], "AI worker production");
        // The player-bound AI leaves the first two ordinary training cells
        // unused (c-SEA-1: the first produced wisp).
        let place = place(&s, hall, type_id, None, 2);
        let Some(unit) =
            super::widgets::create_unit(&mut s, owner, type_id, [place[0], place[1], 0])
        else {
            return;
        };
        super::table::unit_made(&mut s, unit);
        if type_id == i32::from_be_bytes(*b"opeo") {
            super::harvest::computer_worker_made(&mut s, unit);
        }
        let food = s.data.units.get(&type_id).map_or(0, |unit| unit.food_used);
        *s.setup.player_states.entry((owner, FOOD_USED)).or_default() -= food;
        super::widgets::type_record(&mut s, owner, type_id, |record| {
            record[2] = record[2].saturating_sub(1);
        });
        let tick = s.table.now;
        // The paid Undead worker has one completion record; the other
        // observed melee workers have two (c-SEA-1 worker completions).
        let completion_count = if type_id == i32::from_be_bytes(*b"uaco") {
            1
        } else {
            2
        };
        let completion = s
            .table
            .take_n(tick, completion_count, "AI worker completion");
        s.table.defer(completion[0], "AI worker completion");
        super::regions::test_unit(&mut s, unit);
    }
    super::regions::dispatch(vm, state);
}

fn player_state(state: &State, player: i32, which: i32) -> i32 {
    state
        .setup
        .player_states
        .get(&(player, which))
        .copied()
        .unwrap_or(0)
}

fn change_player_state(state: &mut State, player: i32, which: i32, by: i32) {
    *state
        .setup
        .player_states
        .entry((player, which))
        .or_default() += by;
}

/// Runs the triggers of player unit event `event` of `unit`'s owner, with
/// the trained unit and type as the training natives give them.
fn fire(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    event: i32,
    building: u32,
    trained_unit: Option<u32>,
    trained_type: Option<i32>,
) {
    let (registrations, owner, owner_handle) = {
        let mut s = state.borrow_mut();
        let Some(owner) = s.unit(Some(building)).map(|u| u.owner) else {
            return;
        };
        let handle = s.player_handle(owner);
        (s.events.clone(), owner, handle)
    };
    for registration in &registrations {
        if let super::widgets::Registration::PlayerUnit {
            trigger,
            player,
            event: e,
            filter,
        } = *registration
            && e == event
            && player == owner
        {
            let filter_context = Context {
                filter_unit: Some(building),
                triggering_player: Some(owner_handle),
                event_id: Some(event),
                ..Context::default()
            };
            if test_boolexpr(vm, state, filter, filter_context) {
                let context = Context {
                    event_id: Some(event),
                    triggering_player: Some(owner_handle),
                    triggering_unit: Some(building),
                    triggering_widget: Some(building),
                    trained_unit,
                    trained_type,
                    ..Context::default()
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
    }
}

/// Runs the issued order triggers of `building`'s owner for order `id`.
fn issued(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32, id: i32) {
    let (registrations, owner, owner_handle) = {
        let mut s = state.borrow_mut();
        let Some(owner) = s.unit(Some(building)).map(|u| u.owner) else {
            return;
        };
        let handle = s.player_handle(owner);
        (s.events.clone(), owner, handle)
    };
    for registration in &registrations {
        if let super::widgets::Registration::PlayerUnit {
            trigger,
            player,
            event,
            filter,
        } = *registration
            && event == EVENT_PLAYER_UNIT_ISSUED_ORDER
            && player == owner
        {
            let filter_context = Context {
                filter_unit: Some(building),
                triggering_player: Some(owner_handle),
                event_id: Some(event),
                ..Context::default()
            };
            if test_boolexpr(vm, state, filter, filter_context) {
                let context = Context {
                    event_id: Some(event),
                    triggering_player: Some(owner_handle),
                    triggering_unit: Some(building),
                    triggering_widget: Some(building),
                    issued_order: Some(id),
                    ..Context::default()
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
    }
}

/// The movement clock an order takes: that of the tick it is given in, or
/// for an order given between ticks (from a replay) that of the next.
///
/// PROVISIONAL: found with `c-RTP-1`, whose trainings end in the game's
/// ticks only so.
fn order_clock(state: &State) -> u32 {
    if state.in_tick {
        state.move_clock
    } else {
        add(state.move_clock, crate::timers::TICK)
    }
}

/// An order to `building` to train a unit of `type_id`: whether it was
/// taken (SPEC-0027, "The order"). The issued order event runs before the
/// cost is paid; a script's order reads as the building's current order
/// until the end of the tick.
///
/// PROVISIONAL: which types a building trains is not checked (any
/// finished building trains any unit type).
pub(super) fn train(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32, type_id: i32) -> bool {
    let (owner, gold, lumber, queued) = {
        let s = state.borrow();
        let Some(Object::Unit(b)) = s.objects.get(&building) else {
            return false;
        };
        let owner = b.owner;
        let ready = b.alive
            && b.construction.is_none()
            && s.data.units.get(&b.type_id).is_some_and(|t| t.building);
        let queued = b.queue.as_ref().map_or(0, |q| q.types.len());
        let Some(unit) = s.data.units.get(&type_id).filter(|t| !t.building) else {
            return false;
        };
        let (gold, lumber) = (unit.gold_cost, unit.lumber_cost);
        if !ready
            || queued >= QUEUE
            || player_state(&s, owner, GOLD) < gold
            || player_state(&s, owner, LUMBER) < lumber
        {
            return false;
        }
        (owner, gold, lumber, queued)
    };
    issued(vm, state, building, type_id);
    let clock = {
        let mut s = state.borrow_mut();
        change_player_state(&mut s, owner, GOLD, -gold);
        change_player_state(&mut s, owner, LUMBER, -lumber);
        let (in_tick, tick) = (s.in_tick, s.ticks);
        if let Some(Object::Unit(b)) = s.objects.get_mut(&building) {
            b.queue
                .get_or_insert_with(Default::default)
                .types
                .push(type_id);
            if in_tick {
                b.shown_order = Some((type_id, tick));
            }
        }
        order_clock(&s)
    };
    if queued == 0 {
        start_head(vm, state, building, clock);
    }
    true
}
/// The head of `building`'s queue starts training at `start` if the food
/// allows it (SPEC-0027, "Start of training").
fn start_head(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32, start: u32) {
    let type_id = {
        let mut s = state.borrow_mut();
        let Some(Object::Unit(b)) = s.objects.get(&building) else {
            return;
        };
        let owner = b.owner;
        let Some(&type_id) = b
            .queue
            .as_ref()
            .filter(|q| q.training.is_none())
            .and_then(|q| q.types.first())
        else {
            return;
        };
        let (food, build_time) = s
            .data
            .units
            .get(&type_id)
            .map_or((0, 0), |t| (t.food_used, t.build_time));
        let used = player_state(&s, owner, FOOD_USED);
        if food > 0 && used + food > player_state(&s, owner, FOOD_CAP) {
            return;
        }
        change_player_state(&mut s, owner, FOOD_USED, food);
        let expiry = add(start, build_time);
        if let Some(Object::Unit(b)) = s.objects.get_mut(&building)
            && let Some(q) = b.queue.as_mut()
        {
            q.training = Some(expiry);
        }
        start_timer(&mut s, expiry, EngineTimer::Training(building));
        type_id
    };
    fire(
        vm,
        state,
        EVENT_PLAYER_UNIT_TRAIN_START,
        building,
        None,
        Some(type_id),
    );
}

/// The training at the head of `building`'s queue ends (SPEC-0027, "End
/// of training"): the unit appears, walks to the rally point, and the
/// next one starts 0.0001 later.
fn end_training(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32) {
    let (unit, expiry, rally) = {
        let mut s = state.borrow_mut();
        let Some(Object::Unit(b)) = s.objects.get_mut(&building) else {
            return;
        };
        let (owner, rally) = (b.owner, b.rally);
        let Some(q) = b.queue.as_mut() else {
            return;
        };
        let Some(expiry) = q.training.take() else {
            return;
        };
        let type_id = q.types.remove(0);
        let place = place(&s, building, type_id, rally, 0);
        // The food taken at the start is the new unit's, which counts it
        // again when made.
        let food = s.data.units.get(&type_id).map_or(0, |t| t.food_used);
        change_player_state(&mut s, owner, FOOD_USED, -food);
        let Some(unit) =
            super::widgets::create_unit(&mut s, owner, type_id, [place[0], place[1], 0])
        else {
            return;
        };
        // It enters its regions where it appears (SPEC-0049).
        super::regions::test_unit(&mut s, unit);
        (unit, expiry, rally)
    };
    super::regions::dispatch(vm, state);
    let type_id = state.borrow().unit(Some(unit)).map(|u| u.type_id);
    fire(
        vm,
        state,
        EVENT_PLAYER_UNIT_TRAIN_FINISH,
        building,
        Some(unit),
        type_id,
    );
    // The unit walks to the rally point after the finish event.
    if let Some(rally) = rally {
        super::movement::rally_order(&mut state.borrow_mut(), unit, rally);
    } else {
        // A computer's worker goes to gather (c-SCJ-1, hub #15).
        // PROVISIONAL: before or after the finish event, and with a rally
        // point, are not read.
        super::harvest::computer_worker_made(&mut state.borrow_mut(), unit);
    }
    start_head(vm, state, building, add(expiry, NEXT_START));
}

/// The order `cancel` to `building`: the last unit of its queue is taken
/// out and, if it was training, its food freed; its cost is given back
/// after the issued order and cancel events, which read the order as
/// the building's current order (SPEC-0027, "Cancelling"). Whether there
/// was one.
pub(super) fn cancel(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32) -> bool {
    if state
        .borrow()
        .unit(Some(building))
        .is_some_and(|b| b.alive && b.construction.is_some())
    {
        return cancel_construction(vm, state, building);
    }
    let queued = matches!(state.borrow().objects.get(&building),
        Some(Object::Unit(b)) if b.queue.as_ref().is_some_and(|q| !q.types.is_empty()));
    if !queued {
        return false;
    }
    let shown = {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let Some(Object::Unit(b)) = s.objects.get_mut(&building) else {
            return false;
        };
        let before = b.shown_order;
        b.shown_order = Some((ORDER_CANCEL, tick + 1));
        before
    };
    issued(vm, state, building, ORDER_CANCEL);
    let (owner, type_id, was_training) = {
        let mut s = state.borrow_mut();
        let Some(Object::Unit(b)) = s.objects.get_mut(&building) else {
            return false;
        };
        let owner = b.owner;
        let Some(q) = b.queue.as_mut() else {
            return false;
        };
        let Some(type_id) = q.types.pop() else {
            return false;
        };
        let was_training = q.types.is_empty() && q.training.take().is_some();
        if was_training {
            cancel_timer(&mut s, EngineTimer::Training(building));
        }
        (owner, type_id, was_training)
    };
    fire(
        vm,
        state,
        EVENT_PLAYER_UNIT_TRAIN_CANCEL,
        building,
        None,
        Some(type_id),
    );
    {
        let mut s = state.borrow_mut();
        let (gold, lumber, food) = s
            .data
            .units
            .get(&type_id)
            .map_or((0, 0, 0), |t| (t.gold_cost, t.lumber_cost, t.food_used));
        change_player_state(&mut s, owner, GOLD, gold);
        change_player_state(&mut s, owner, LUMBER, lumber);
        if was_training {
            change_player_state(&mut s, owner, FOOD_USED, -food);
        }
        if let Some(Object::Unit(b)) = s.objects.get_mut(&building) {
            b.shown_order = shown;
        }
    }
    if was_training {
        food_freed(vm, state, owner);
    }
    true
}

/// Food of `player` was freed: the queues waiting for food start, the
/// buildings in the order they were made.
pub(super) fn food_freed(vm: &mut Vm, state: &Rc<RefCell<State>>, player: i32) {
    let (clock, waiting) = {
        let s = state.borrow();
        let waiting: Vec<u32> = s
            .units
            .iter()
            .copied()
            .filter(|h| {
                matches!(s.objects.get(h), Some(Object::Unit(u))
                    if u.owner == player && u.alive
                        && u.queue.as_ref().is_some_and(|q| q.training.is_none() && !q.types.is_empty()))
            })
            .collect();
        (s.move_clock, waiting)
    };
    for building in waiting {
        start_head(vm, state, building, clock);
    }
}

/// Where a unit of `type_id` trained by `building` appears (SPEC-0027,
/// "Where the new unit appears"): the centre of the first free cell of
/// rings round the building's pathing texture, counter-clockwise from the
/// side of the rally point.
///
/// PROVISIONAL: measured for units of size class 1; a building without a
/// pathing texture, or no free cell within 16 rings, puts the unit at the
/// building's position.
fn place(
    state: &State,
    building: u32,
    type_id: i32,
    rally: Option<[u32; 2]>,
    skip: usize,
) -> [u32; 2] {
    let Some(Object::Unit(b)) = state.objects.get(&building) else {
        return [0, 0];
    };
    let (bx, by) = (b.x, b.y);
    let (Some(footprint), Some(grid)) = (b.footprint.as_ref(), state.grid.as_ref()) else {
        return [bx, by];
    };
    let class = crate::path::size_class(state.data.units.get(&type_id).map_or(0, |t| t.collision));
    let standing = standing_cells(state);
    let free = |x: i32, y: i32| {
        let centre = [add(i2r(x), HALF), add(i2r(y), HALF)];
        crate::path::occupied_cells(centre, class)
            .iter()
            .all(|&(cx, cy)| grid.passable(cx, cy) && !standing.contains(&(cx, cy)))
    };
    let first = match rally {
        Some([rx, ry]) => {
            let (dx, dy) = (f32::from_bits(sub(rx, bx)), f32::from_bits(sub(ry, by)));
            match (dx >= 0.0, dy >= 0.0) {
                (true, true) => 0,
                (false, true) => 1,
                (false, false) => 2,
                (true, false) => 3,
            }
        }
        None => 2,
    };
    let (x0, x1, y0, y1) = footprint.rect;
    let Some((x, y)) = free_ring_cell((x0, x1, y0, y1), first, skip, free) else {
        return [bx, by];
    };
    let cell = |c: i32, m: u32| add(mul(add(i2r(c), HALF), WORLD), m);
    [
        cell(x, state.world_bounds[0]),
        cell(y, state.world_bounds[1]),
    ]
}

/// The first free cell on the expanding rings around a building footprint,
/// after `skip` otherwise suitable cells.
fn free_ring_cell(
    (x0, x1, y0, y1): (i32, i32, i32, i32),
    first: usize,
    skip: usize,
    free: impl Fn(i32, i32) -> bool,
) -> Option<(i32, i32)> {
    let mut skipped = 0;
    for n in 1..=16 {
        let (l, r, bottom, top) = (x0 - n, x1 + 1 + n, y0 - n, y1 + 1 + n);
        let sides: [Vec<(i32, i32)>; 4] = [
            (l..r).rev().map(|x| (x, top)).collect(),
            (bottom..top).rev().map(|y| (l, y)).collect(),
            (l + 1..=r).map(|x| (x, bottom)).collect(),
            (bottom + 1..=top).map(|y| (r, y)).collect(),
        ];
        for side in 0..4 {
            for &(x, y) in &sides[(first + side) % 4] {
                if free(x, y) {
                    if skipped < skip {
                        skipped += 1;
                        continue;
                    }
                    return Some((x, y));
                }
            }
        }
    }
    None
}

/// 0.5 and 32.
const HALF: u32 = 0x3F00_0000;
const WORLD: u32 = 0x4200_0000;

/// The cells of the units that stand (SPEC-0021, "Occupied cells").
fn standing_cells(state: &State) -> std::collections::BTreeSet<(i32, i32)> {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let mut cells = std::collections::BTreeSet::new();
    for h in &state.units {
        let Some(Object::Unit(u)) = state.objects.get(h) else {
            continue;
        };
        if !u.alive || u.hidden || u.movement.is_some_and(|m| m.velocity != [0, 0]) {
            continue;
        }
        let data = state.data.units.get(&u.type_id);
        if data.is_some_and(|t| t.building) {
            continue;
        }
        let class = crate::path::size_class(data.map_or(0, |t| t.collision));
        let p = super::movement::relative([u.x, u.y], min).map(|c| mul(c, CELL));
        cells.extend(crate::path::occupied_cells(p, class));
    }
    cells
}

/// 1/32.
const CELL: u32 = 0x3D00_0000;

/// A building under construction is cancelled (c-SCA-1 at 7.399702, hub
/// #14): its objects (`table::construction_cancelled`), 75 % of its gold
/// and lumber back, gold first; its type record no more under construction;
/// the building gone as a dead unit, with no food given; the cancel event.
///
/// PROVISIONAL: the worker is let go with its next order; a death event
/// and a corpse are not read.
fn cancel_construction(vm: &mut Vm, state: &Rc<RefCell<State>>, building: u32) -> bool {
    {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let Some(b) = s.unit(Some(building)) else {
            return false;
        };
        let (owner, type_id) = (b.owner, b.type_id);
        let worker = b.construction.as_ref().map_or(0, |c| c.builder);
        super::table::construction_cancelled(s, worker, building);
        let (gold, lumber) = s
            .data
            .units
            .get(&type_id)
            .map_or((0, 0), |t| (t.gold_cost, t.lumber_cost));
        *s.setup.player_states.entry((owner, GOLD)).or_default() += gold * 3 / 4;
        *s.setup.player_states.entry((owner, 2)).or_default() += lumber * 3 / 4;
        // Made and alive never; under construction no more (q1 here, as
        // `movement::start_construction` counts it).
        super::widgets::type_record(s, owner, type_id, |q| q[1] -= 1);
        cancel_timer(s, EngineTimer::Construction(building));
        super::obstacles::died(s, building);
        if let Some(Object::Unit(u)) = s.objects.get_mut(&building) {
            u.construction = None;
            u.alive = false;
            u.life.set(0);
            u.died_at = Some(s.ticks);
        }
    }
    super::widgets::construction_cancelled(vm, state, building);
    true
}
