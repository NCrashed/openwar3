//! Units, destructables and items as placeholders until they are
//! specified, and their death events (SPEC-0009).
//!
//! A death runs the triggers registered for it as an event: each enabled
//! trigger whose conditions hold runs its actions, and an action that
//! sleeps lets the code that caused the death go on (SPEC-0005). A unit
//! starts with the hit points of its type, and a type the game data does
//! not know gives `null` (without game data every type is accepted, with 1
//! hit point). `SetDestructableLife(d, 0)` kills.
//!
//! Life and mana of units, their regeneration, facing and position follow
//! SPEC-0013: life set to 0.405 or less kills, and the maxima cannot be
//! set.
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: `KillUnit` has no killing
//! unit; setting the hit points of a destructable to less than 0 kills
//! like 0; life regeneration limited to night, day or blight never runs.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use ow3_math::{add, floor, mul, sub};

use crate::data::{RegenType, UnitType, is_hero};

use super::{
    Context, Destructable, Item, Object, Regenerating, State, Unit, converted, facing,
    fire_trigger, handle, integer, real, string, test_boolexpr,
};

const EVENT_PLAYER_UNIT_DEATH: i32 = 20;
/// `PLAYER_STATE_RESOURCE_FOOD_CAP` and `PLAYER_STATE_RESOURCE_FOOD_USED`.
const FOOD_CAP: i32 = 4;
const FOOD_USED: i32 = 5;
/// The game generator the alarm of a new building draws (SPEC-0026).
const ALARM: usize = 21;
/// 0.5 as a bit pattern of a real.
const HALF_SECOND: u32 = 0x3F00_0000;
/// 1.2 as a bit pattern of a real: the timer of the objects a building
/// frees after its creation (SPEC-0024).
const BUILDING_OBJECTS: u32 = 0x3F99_999A;
const EVENT_UNIT_DAMAGED: i32 = 52;
const EVENT_UNIT_ACQUIRED_TARGET: i32 = 60;
const EVENT_UNIT_DEATH: i32 = 53;
const EVENT_UNIT_HERO_LEVEL: i32 = 78;
const EVENT_PLAYER_HERO_LEVEL: i32 = 41;
const EVENT_WIDGET_DEATH: i32 = 89;
const UNIT_STATE_LIFE: i32 = 0;
const UNIT_STATE_MAX_LIFE: i32 = 1;
const UNIT_STATE_MANA: i32 = 2;
const UNIT_STATE_MAX_MANA: i32 = 3;
const UNIT_TYPE_HERO: i32 = 0;
const UNIT_TYPE_DEAD: i32 = 1;
const UNIT_TYPE_STRUCTURE: i32 = 2;
const UNIT_TYPE_GIANT: i32 = 9;
const UNIT_TYPE_SUMMONED: i32 = 10;
const UNIT_TYPE_UNDEAD: i32 = 14;
const UNIT_TYPE_MECHANICAL: i32 = 15;
const UNIT_TYPE_PEON: i32 = 16;
const UNIT_TYPE_SAPPER: i32 = 17;
const UNIT_TYPE_TOWNHALL: i32 = 18;
const UNIT_TYPE_ANCIENT: i32 = 19;
const UNIT_TYPE_TAUREN: i32 = 20;
const UNIT_TYPE_SLEEPING: i32 = 23;
/// Hit points of a unit when there is no game data: 1.0.
const UNKNOWN_LIFE: u32 = 0x3F80_0000;
/// Life at or below which a unit dies: 0.405 (SPEC-0013).
const DEATH_LIFE: f32 = 0.405;
const FOUR: u32 = 0x4080_0000;
const QUARTER: u32 = 0x3E80_0000;
/// Sign bit of a real.
const SIGN: u32 = 0x8000_0000;
/// How far `SetUnitPosition` keeps a unit from the world's edges: 32.0.
const EDGE: u32 = 0x4200_0000;

/// A trigger registered for an event, kept in [`State`].
#[derive(Clone, Debug)]
pub enum Registration {
    /// `TriggerRegisterUnitEvent`.
    Unit { trigger: u32, unit: u32, event: i32 },
    /// `TriggerRegisterPlayerUnitEvent`.
    PlayerUnit {
        trigger: u32,
        player: i32,
        event: i32,
        filter: Option<u32>,
    },
    /// `TriggerRegisterTimerExpireEvent`.
    TimerExpire { trigger: u32, timer: u32 },
    /// `TriggerRegisterDeathEvent` of a unit, destructable or item.
    Death { trigger: u32, widget: u32 },
    /// `TriggerRegisterEnterRegion` (`enter`) and
    /// `TriggerRegisterLeaveRegion` (SPEC-0049).
    Region {
        trigger: u32,
        region: u32,
        enter: bool,
        filter: Option<u32>,
    },
}

fn unit_mut(state: &mut State, h: Option<u32>) -> Option<&mut Unit> {
    match state.objects.get_mut(&h?)? {
        Object::Unit(unit) => Some(unit),
        _ => None,
    }
}

fn destructable_mut(state: &mut State, h: Option<u32>) -> Option<&mut Destructable> {
    match state.objects.get_mut(&h?)? {
        Object::Destructable(d) => Some(d),
        _ => None,
    }
}

fn item(state: &State, h: Option<u32>) -> Option<&Item> {
    match state.objects.get(&h?)? {
        Object::Item(item) => Some(item),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(super) enum Attribute {
    Strength,
    Agility,
    Intelligence,
}

/// Sets an attribute of hero `h` (SPEC-0013, "Hero attributes"). Gives the
/// life to set through [`set_unit_life`] when the hero dies of it.
pub(super) fn set_attribute(
    state: &mut State,
    h: Option<u32>,
    attribute: Attribute,
    value: i32,
) -> Option<u32> {
    state.unit(h).filter(|u| u.alive && is_hero(u.type_id))?;
    let u = unit_mut(state, h)?;
    let old = match attribute {
        Attribute::Strength => std::mem::replace(&mut u.strength, value),
        Attribute::Agility => std::mem::replace(&mut u.agility, value),
        Attribute::Intelligence => std::mem::replace(&mut u.intelligence, value),
    };
    if old == value {
        return None;
    }
    // The regeneration of the attribute: its new part added to the running
    // rate, its old part taken off 0.0001 s later (SPEC-0033).
    let (added, removed) = (
        crate::data::attribute_regen(value),
        crate::data::attribute_regen(old),
    );
    let shift = |state: &mut Regenerating, per_point: i32, least: f32| {
        let change = ow3_math::i2r(per_point * (value - old));
        let moved = add(state.value(), change);
        state.max = add(state.max, change);
        let moved = if f32::from_bits(moved) < least {
            least.to_bits()
        } else {
            moved
        };
        state.set(moved);
        state.change_rate(added, removed);
    };
    match attribute {
        Attribute::Strength => {
            if f32::from_bits(add(u.life.max, ow3_math::i2r(25 * (value - old)))) <= 0.0 {
                // The life becomes the new max life, and the hero dies.
                u.life.max = add(u.life.max, ow3_math::i2r(25 * (value - old)));
                return Some(u.life.max);
            }
            shift(&mut u.life, 25, 1.0);
        }
        Attribute::Intelligence => {
            shift(&mut u.mana, 15, 0.0);
        }
        Attribute::Agility => {}
    }
    // The running cooldown follows the new attack speed (SPEC-0034).
    if matches!(attribute, Attribute::Agility)
        && let Some(h) = h
    {
        let now = state.event_time.unwrap_or_else(|| state.timers.time());
        super::attack::agility_changed(state, h, old, now);
    }
    None
}

/// Day turned into night or back: units whose `regenHP` applies only at
/// night go on at their new rate.
pub(super) fn night_changed(state: &mut State) {
    super::creeps::night_changed(state);
    let night = state.night;
    for i in 0..state.units.len() {
        let h = state.units[i];
        let Some(Object::Unit(u)) = state.objects.get(&h) else {
            continue;
        };
        let Some(data) = state.data.units.get(&u.type_id) else {
            continue;
        };
        if !u.alive || data.regen_type != RegenType::Night {
            continue;
        }
        let rate = life_rate(data, hero_part(u.type_id, u.strength), night);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.life.retarget(rate);
        }
    }
}

/// The part of a hero's life regeneration from `strength`; 0 for others.
fn hero_part(type_id: i32, strength: i32) -> u32 {
    if is_hero(type_id) {
        crate::data::attribute_regen(strength)
    } else {
        0
    }
}

/// The life regeneration of a unit of `data` with a hero's strength part
/// `hero_part`, by day or at `night`.
pub(super) fn life_rate(data: &UnitType, hero_part: u32, night: bool) -> u32 {
    match data.regen_type {
        RegenType::Always => add(data.life_regen, hero_part),
        RegenType::Night if night => add(data.life_regen, hero_part),
        _ => hero_part,
    }
}

/// `CreateUnit` and its kin: a unit that is not a building and whose
/// footprint at the snapped point overlaps another unit's is moved to the
/// centre of the first cell of a ring search round its own cell where it
/// overlaps none (SPEC-0013, "Occupied ground").
///
/// PROVISIONAL: unpassable terrain and buildings are not tested; they do
/// not move the unit here.
pub(super) fn create_unit_by_script(
    state: &mut State,
    owner: i32,
    type_id: i32,
    at: [u32; 3],
) -> Option<u32> {
    let snap = |v: u32| mul(floor(mul(v, FOUR)), QUARTER);
    let (x, y) = off_other_units(state, type_id, [snap(at[0]), snap(at[1])]);
    let h = create_unit(state, owner, type_id, [x, y, at[2]])?;
    // It enters the regions at the place it stands (SPEC-0049).
    super::regions::test_unit(state, h);
    Some(h)
}

/// Where a new unit of `type_id` at the snapped point `at` stands
/// (`create_unit_by_script`).
fn off_other_units(state: &State, type_id: i32, at: [u32; 2]) -> (u32, u32) {
    use crate::path::{occupied_cells, size_class};
    let Some(t) = state.data.units.get(&type_id).filter(|t| !t.building) else {
        return (at[0], at[1]);
    };
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells = |p: [u32; 2]| super::follow::to_cells(super::movement::relative(p, min));
    let mut taken = std::collections::BTreeSet::new();
    for &o in &state.units {
        let Some(Object::Unit(u)) = state.objects.get(&o) else {
            continue;
        };
        let Some(d) = state.data.units.get(&u.type_id) else {
            continue;
        };
        if u.alive && !u.hidden && !d.building {
            taken.extend(occupied_cells(cells([u.x, u.y]), size_class(d.collision)));
        }
    }
    let class = size_class(t.collision);
    let free = |c: [u32; 2]| occupied_cells(c, class).iter().all(|k| !taken.contains(k));
    let here = cells(at);
    if free(here) {
        return (at[0], at[1]);
    }
    let (cx, cy) = (
        f32::from_bits(floor(here[0])) as i32,
        f32::from_bits(floor(here[1])) as i32,
    );
    // The centre of cell `c` on one axis, in cells and in world units.
    let centre = |c: i32| add(ow3_math::i2r(c), 0.5f32.to_bits());
    for n in 1..=64 {
        let (l, r, b, t) = (cx - n, cx + n, cy - n, cy + n);
        let ring = ((l + 1)..=r)
            .map(|i| (i, b))
            .chain(((b + 1)..=t).map(|j| (r, j)))
            .chain((l..r).rev().map(|i| (i, t)))
            .chain((b..t).rev().map(|j| (l, j)));
        for (i, j) in ring {
            let c = [centre(i), centre(j)];
            if free(c) {
                let world = |v: u32, m: u32| add(mul(v, 0x4200_0000), m);
                return (world(c[0], min[0]), world(c[1], min[1]));
            }
        }
    }
    (at[0], at[1])
}

/// Creates a unit of `type_id` for `owner`, or `None` for a type the game
/// data does not know.
pub(super) fn create_unit(
    state: &mut State,
    owner: i32,
    type_id: i32,
    at: [u32; 3],
) -> Option<u32> {
    let data = if state.data.units.is_empty() {
        UnitType {
            hp: UNKNOWN_LIFE,
            ..UnitType::default()
        }
    } else {
        state.data.units.get(&type_id)?.clone()
    };
    // Life regenerates at `regenHP` when the type's condition holds, plus
    // a hero's strength part (SPEC-0013). PROVISIONAL: the conditions `day`
    // and `blight` never hold yet.
    let mut life = Regenerating::full(data.hp, life_rate(&data, data.hero_life_regen, state.night));
    let mut mana = Regenerating::full(data.max_mana, data.mana_regen);
    mana.set(data.start_mana);
    // A hero's regeneration starts 0.0001 s after it is created.
    life.delayed = is_hero(type_id);
    mana.delayed = is_hero(type_id);
    // Positions snap to a quarter (SPEC-0013), a building's to the grid
    // of its pathing texture, which it puts in the grid (SPEC-0022).
    let snap = |v: u32| mul(floor(mul(v, FOUR)), QUARTER);
    let facing = facing::from_degrees(at[2]);
    let ((x, y), footprint) =
        super::obstacles::place_building(state, type_id, snap(at[0]), snap(at[1]), facing);
    // The phase of its acquisition checks, from the generator of the
    // damage dice (SPEC-0019).
    let acquire_phase = if data.weapon.is_some() {
        state.game_random[crate::random::ATTACK_DICE].fraction()
    } else {
        super::NO_PHASE
    };
    // A building's alarm ability draws game generator 21 once (SPEC-0026).
    //
    // PROVISIONAL: measured for a farm started by a worker; every building
    // made is taken to have the ability (c-RTP-1, whose script makes a
    // barracks and farms, matches only so).
    if data.building {
        state.game_random[ALARM].fraction();
    }
    let unit = Unit {
        owner,
        type_id,
        x,
        y,
        facing,
        turn: None,
        attack: None,
        movement: None,
        attack_move: None,
        acquire_phase,
        // The first check expires 0.5 x s after the creation, counted on
        // the timer queue (SPEC-0019).
        next_acquire: add(state.timers.time(), mul(acquire_phase, HALF_SECOND)),
        acquire_seq: 0,
        after_death: false,
        checks_off: false,
        guard_return: None,
        target_taken: None,
        call_cells: Vec::new(),
        call_until: 0,
        remembered: None,
        paused: false,
        strength: data.strength,
        agility: f32::from_bits(data.agility) as i32,
        intelligence: data.intelligence,
        xp: 0,
        level: i64::from(is_hero(type_id)),
        skills: Vec::new(),
        spent: 0,
        ready: Vec::new(),
        casting: None,
        proc_counters: Vec::new(),
        procced: None,
        stunned_until: None,
        stun_back: None,
        slowed_until: None,
        buffs: Vec::new(),
        aura_learned: None,
        invulnerable_until: None,
        wind_walk: None,
        illusion: None,
        stunner: None,
        zero_hit: None,
        weapon_ready: 0,
        cooldown_end: 0,
        cooldown_seq: 0,
        cooldown_target: 0,
        backswing_end: 0,
        orient_interp: data.orient_interp,
        life,
        mana,
        alive: true,
        died_at: None,
        hidden: false,
        user_data: 0,
        // A gold mine holds `DataA` of its ability `Agld` (SPEC-0029).
        resources: data
            .abilities
            .iter()
            .filter_map(|a| state.data.abilities.get(a))
            .find(|a| a.code == i32::from_be_bytes(*b"Agld"))
            .map_or(0, |a| f32::from_bits(a.data[0]) as i32),
        harvest: None,
        home: [x, y],
        creep: super::creeps::born(state, owner, type_id),
        orders: super::queue::Queue::default(),
        footprint: footprint.map(Box::new),
        construction: None,
        queue: None,
        rally: None,
        shown_order: None,
        region_test: Default::default(),
        entries: Default::default(),
    };
    let h = state.create(Object::Unit(Box::new(unit)));
    state.units.push(h);
    super::creeps::join_group(state, h);
    // A unit uses its food, a building gives its food (SPEC-0027).
    //
    // PROVISIONAL: measured only with trained units and finished
    // buildings; units made by a script are taken to count the same.
    let (key, food) = if data.building {
        (FOOD_CAP, data.food_made)
    } else {
        (FOOD_USED, data.food_used)
    };
    if food != 0 {
        *state.setup.player_states.entry((owner, key)).or_default() += food;
    }
    // A building frees objects of its own by a timer of 1.2 s from its
    // creation (SPEC-0024, measured for a town hall made by a script).
    // PROVISIONAL: only town halls (barracks and farms of c-RTP-1 change
    // nothing then), and not one a worker starts (c-RSN-1).
    if data.building && data.classes.iter().any(|c| c == "townhall") {
        let expiry = add(state.timers.time(), BUILDING_OBJECTS);
        state.objects_change_at(expiry, h);
    }
    // It sees at once (SPEC-0032).
    super::fog::born(state, h);
    Some(h)
}

const EVENT_PLAYER_UNIT_SUMMON: i32 = 47;
const EVENT_UNIT_SUMMON: i32 = 84;

/// Runs the triggers of `EVENT_PLAYER_UNIT_SUMMON` for the units summoned
/// in this tick, for the summoned unit's player, then `EVENT_UNIT_SUMMON`
/// of the summoner (SPEC-0044). The filter sees the summoner as
/// `GetFilterUnit`; in the actions `GetTriggerUnit` is the summoned unit,
/// or the summoner in the unit event (probe c-SAM).
///
/// PROVISIONAL: the order of the two events in the tick.
pub(super) fn summons(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let queued = std::mem::take(&mut state.borrow_mut().summons);
    for (summoner, summoned) in queued {
        let (registrations, owner) = {
            let mut state = state.borrow_mut();
            let Some(owner) = state.unit(Some(summoned)).map(|u| u.owner) else {
                continue;
            };
            let owner_handle = state.player_handle(owner);
            (state.events.clone(), (owner, owner_handle))
        };
        super::begin_event(state);
        for registration in &registrations {
            if let Registration::PlayerUnit {
                trigger,
                player,
                event: e,
                filter,
            } = *registration
                && e == EVENT_PLAYER_UNIT_SUMMON
                && player == owner.0
            {
                let filter_context = Context {
                    filter_unit: Some(summoner),
                    triggering_player: Some(owner.1),
                    event_id: Some(EVENT_PLAYER_UNIT_SUMMON),
                    ..Context::default()
                };
                if test_boolexpr(vm, state, filter, filter_context) {
                    let context = Context {
                        event_id: Some(EVENT_PLAYER_UNIT_SUMMON),
                        triggering_player: Some(owner.1),
                        triggering_unit: Some(summoned),
                        triggering_widget: Some(summoned),
                        summoned_unit: Some(summoned),
                        summoning_unit: Some(summoner),
                        ..Context::default()
                    };
                    fire_trigger(vm, state, trigger, context);
                }
            }
        }
        super::end_event(state);
        super::begin_event(state);
        for registration in &registrations {
            if let Registration::Unit {
                trigger,
                unit,
                event: EVENT_UNIT_SUMMON,
            } = *registration
                && unit == summoner
            {
                let context = Context {
                    event_id: Some(EVENT_UNIT_SUMMON),
                    triggering_unit: Some(summoner),
                    triggering_widget: Some(summoner),
                    summoned_unit: Some(summoned),
                    summoning_unit: Some(summoner),
                    ..Context::default()
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
        super::end_event(state);
    }
}

const EVENT_PLAYER_UNIT_CONSTRUCT_START: i32 = 26;
const EVENT_PLAYER_UNIT_CONSTRUCT_FINISH: i32 = 28;

/// Runs the triggers of the player unit events of buildings started and
/// finished in this tick (SPEC-0026), queued as `(finished, building)`.
/// The filter sees the building as `GetFilterUnit`.
///
/// PROVISIONAL: the unit events of a building (`EVENT_UNIT_CONSTRUCT_*`)
/// are not run.
pub(super) fn constructions(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let queued = std::mem::take(&mut state.borrow_mut().constructions);
    for (finished, building) in queued {
        let event = if finished {
            EVENT_PLAYER_UNIT_CONSTRUCT_FINISH
        } else {
            EVENT_PLAYER_UNIT_CONSTRUCT_START
        };
        let (registrations, owner) = {
            let mut state = state.borrow_mut();
            let Some(owner) = state.unit(Some(building)).map(|u| u.owner) else {
                continue;
            };
            let owner_handle = state.player_handle(owner);
            (state.events.clone(), (owner, owner_handle))
        };
        for registration in &registrations {
            if let Registration::PlayerUnit {
                trigger,
                player,
                event: e,
                filter,
            } = *registration
                && e == event
                && player == owner.0
            {
                let filter_context = Context {
                    filter_unit: Some(building),
                    triggering_player: Some(owner.1),
                    event_id: Some(event),
                    ..Context::default()
                };
                if test_boolexpr(vm, state, filter, filter_context) {
                    let context = Context {
                        event_id: Some(event),
                        triggering_player: Some(owner.1),
                        triggering_unit: Some(building),
                        triggering_widget: Some(building),
                        ..Context::default()
                    };
                    fire_trigger(vm, state, trigger, context);
                }
            }
        }
    }
}

/// Runs the triggers registered for the death of `widget` (SPEC-0009):
/// first the widget death registrations, then the player unit events of
/// its owner, then its unit events, each kind in the order registered.
///
/// The filter of a player unit event sees the killing unit as
/// `GetFilterUnit`, the owner as `GetTriggerPlayer` and the event and
/// trigger of the thread in which the unit died; without a killing unit
/// the filter is not called and the trigger fires.
fn died(vm: &mut Vm, state: &Rc<RefCell<State>>, widget: u32, killer: Option<u32>) {
    let (registrations, owner, dying_thread) = {
        let mut state = state.borrow_mut();
        // A dead destructable blocks by its texture of death, a dead
        // building no more (SPEC-0022).
        super::obstacles::died(&mut state, widget);
        // The heroes near a dying unit take its experience (SPEC-0033).
        if state.unit(Some(widget)).is_some() {
            super::heroes::death(&mut state, widget);
        }
        let owner = state.unit(Some(widget)).map(|u| u.owner);
        // Its food is freed (SPEC-0027): the food used of a unit, the food
        // of a finished building.
        let food = state.unit(Some(widget)).and_then(|u| {
            let t = state.data.units.get(&u.type_id)?;
            match (t.building, u.construction.is_none()) {
                (true, true) => Some((FOOD_CAP, t.food_made)),
                (true, false) => None,
                (false, _) => Some((FOOD_USED, t.food_used)),
            }
        });
        if let (Some(owner), Some((key, food))) = (owner, food)
            && food != 0
        {
            *state.setup.player_states.entry((owner, key)).or_default() -= food;
        }
        let owner_handle = owner.map(|id| state.player_handle(id));
        let dying_thread = state.context;
        (state.events.clone(), owner.zip(owner_handle), dying_thread)
    };
    let unit = owner.is_some().then_some(widget);
    let base = Context {
        triggering_widget: Some(widget),
        triggering_unit: unit,
        killing_unit: killer,
        ..Context::default()
    };
    for registration in &registrations {
        if let Registration::Death { trigger, widget: w } = *registration
            && w == widget
        {
            let context = Context {
                event_id: Some(EVENT_WIDGET_DEATH),
                ..base
            };
            fire_trigger(vm, state, trigger, context);
        }
    }
    for registration in &registrations {
        if let Registration::PlayerUnit {
            trigger,
            player,
            event: EVENT_PLAYER_UNIT_DEATH,
            filter,
        } = *registration
            && let Some((id, player_handle)) = owner
            && id == player
        {
            let passes = match killer {
                None => true,
                Some(killer) => {
                    let filter_context = Context {
                        filter_unit: Some(killer),
                        triggering_player: Some(player_handle),
                        event_id: dying_thread.event_id,
                        triggering_trigger: dying_thread.triggering_trigger,
                        ..Context::default()
                    };
                    test_boolexpr(vm, state, filter, filter_context)
                }
            };
            if passes {
                let context = Context {
                    event_id: Some(EVENT_PLAYER_UNIT_DEATH),
                    triggering_player: Some(player_handle),
                    ..base
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
    }
    for registration in &registrations {
        if let Registration::Unit {
            trigger,
            unit: u,
            event: EVENT_UNIT_DEATH,
        } = *registration
            && u == widget
        {
            let context = Context {
                event_id: Some(EVENT_UNIT_DEATH),
                ..base
            };
            fire_trigger(vm, state, trigger, context);
        }
    }
    // The levels its experience gave (SPEC-0034).
    hero_levels(vm, state);
    // Queues waiting for food go on (SPEC-0027).
    if let Some((owner, _)) = owner {
        super::training::food_freed(vm, state, owner);
    }
}

/// Runs the level events of the heroes that gained levels: for each level
/// the `EVENT_UNIT_HERO_LEVEL` triggers of the hero, then the
/// `EVENT_PLAYER_HERO_LEVEL` triggers of its owner whose filter passes
/// with the hero as `GetFilterUnit` (SPEC-0034).
///
/// PROVISIONAL: the order of the two kinds, and of these events against
/// the death events of the unit whose experience gave the level.
pub(super) fn hero_levels(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    loop {
        let (h, registrations, owner) = {
            let mut s = state.borrow_mut();
            if s.leveled.is_empty() {
                return;
            }
            let h = s.leveled.remove(0);
            let owner = s.unit(Some(h)).map(|u| u.owner);
            let owner = owner.map(|id| (id, s.player_handle(id)));
            (h, s.events.clone(), owner)
        };
        let context = Context {
            triggering_unit: Some(h),
            triggering_widget: Some(h),
            ..Context::default()
        };
        super::begin_event(state);
        for registration in &registrations {
            if let Registration::Unit {
                trigger,
                unit,
                event: EVENT_UNIT_HERO_LEVEL,
            } = *registration
                && unit == h
            {
                let context = Context {
                    event_id: Some(EVENT_UNIT_HERO_LEVEL),
                    ..context
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
        super::end_event(state);
        super::begin_event(state);
        for registration in &registrations {
            if let Registration::PlayerUnit {
                trigger,
                player,
                event: EVENT_PLAYER_HERO_LEVEL,
                filter,
            } = *registration
                && let Some((id, player_handle)) = owner
                && id == player
            {
                let filter_context = Context {
                    filter_unit: Some(h),
                    triggering_player: Some(player_handle),
                    event_id: Some(EVENT_PLAYER_HERO_LEVEL),
                    ..Context::default()
                };
                if test_boolexpr(vm, state, filter, filter_context) {
                    let context = Context {
                        event_id: Some(EVENT_PLAYER_HERO_LEVEL),
                        triggering_player: Some(player_handle),
                        ..context
                    };
                    fire_trigger(vm, state, trigger, context);
                }
            }
        }
        super::end_event(state);
    }
}

/// Kills a living unit: it reads life 0 and stops regenerating, and the
/// triggers of its death run before this returns.
pub(super) fn kill_unit(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: Option<u32>,
    killer: Option<u32>,
) {
    let killed = {
        let mut state = state.borrow_mut();
        let tick = state.ticks;
        match unit_mut(&mut state, h) {
            Some(unit) if unit.alive => {
                unit.alive = false;
                unit.died_at = Some(tick);
                unit.life.base = 0;
                unit.life.rate = 0;
                unit.mana.rate = 0;
                true
            }
            _ => false,
        }
    };
    if let (true, Some(h)) = (killed, h) {
        super::table::unit_died(&mut state.borrow_mut(), h);
        super::movement::target_died(&mut state.borrow_mut(), h);
    }
    if let (true, Some(h)) = (killed, h) {
        died(vm, state, h, killer);
    }
}

/// `SetUnitState(u, UNIT_STATE_LIFE, life)` and `SetWidgetLife` of a unit
/// (SPEC-0013): limited to the maximum, and 0.405 or less kills, by
/// `killer` if there is one.
fn set_unit_life(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: Option<u32>,
    life: u32,
    killer: Option<u32>,
) {
    if f32::from_bits(life) <= DEATH_LIFE {
        kill_unit(vm, state, h, killer);
        return;
    }
    let mut state = state.borrow_mut();
    if let Some(u) = unit_mut(&mut state, h)
        && u.alive
    {
        u.life.set(life);
    }
}

/// `UnitDamageTarget` (SPEC-0015) of a unit, destructable or item.
///
/// Destructables and items take the full amount times the multiplier of
/// their armor type, without armor value. A destructable at 0 life dies
/// as with `SetDestructableLife`; an item at 0 life runs its death
/// triggers, without a killing unit, and stays.
pub(super) fn damage_widget(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    source: Option<u32>,
    target: Option<u32>,
    amount: u32,
    attack_type: i32,
    damage_type: i32,
) -> bool {
    // The armor type, life and kind of a target that is not a unit.
    let other = {
        let s = state.borrow();
        if s.unit(source).is_none() {
            return false;
        }
        match target.and_then(|h| s.objects.get(&h)) {
            Some(Object::Unit(_)) => None,
            Some(Object::Destructable(d)) if d.alive => {
                let armor = s.data.destructable_armor.get(&d.type_id).copied().flatten();
                Some((armor, d.life, true))
            }
            Some(Object::Item(i)) => {
                let armor = s.data.item_armor.get(&i.type_id).copied().flatten();
                Some((armor, i.life, false))
            }
            _ => return false,
        }
    };
    let Some((armor_type, life, destructable)) = other else {
        return damage_unit(vm, state, source, target, amount, attack_type, damage_type);
    };
    let damage = state
        .borrow()
        .combat
        .damage(amount, 0, armor_type, attack_type, damage_type);
    let left = sub(life, damage);
    if destructable {
        set_destructable_life(vm, state, target, left);
    } else {
        // An item at 0 life runs its death triggers and stays (SPEC-0015).
        let died_now = {
            let mut s = state.borrow_mut();
            match target.and_then(|h| s.objects.get_mut(&h)) {
                Some(Object::Item(item)) => {
                    item.life = if f32::from_bits(left) < 0.0 { 0 } else { left };
                    f32::from_bits(life) > 0.0 && f32::from_bits(item.life) <= 0.0
                }
                _ => false,
            }
        };
        if let (true, Some(item)) = (died_now, target) {
            died(vm, state, item, None);
        }
    }
    true
}

/// Runs the `EVENT_UNIT_ACQUIRED_TARGET` triggers of the units that found
/// a target at their check (SPEC-0019), unit by unit in the order of the
/// acquisitions, each unit's in the order registered.
pub(super) fn acquired(vm: &mut Vm, state: &Rc<RefCell<State>>, units: &[u32]) {
    acquired_with(vm, state, units, true);
}

/// `acquired` for the units that took a target at a call: their objects
/// were taken at the call (`table::call_answered`).
pub(super) fn acquired_at_call(vm: &mut Vm, state: &Rc<RefCell<State>>, units: &[u32]) {
    state.borrow_mut().quiet_events = true;
    acquired_with(vm, state, units, false);
    state.borrow_mut().quiet_events = false;
}

fn acquired_with(vm: &mut Vm, state: &Rc<RefCell<State>>, units: &[u32], objects: bool) {
    if units.is_empty() {
        return;
    }
    let registrations = state.borrow().events.clone();
    for &h in units {
        super::begin_event(state);
        for registration in &registrations {
            if let Registration::Unit {
                trigger,
                unit,
                event: EVENT_UNIT_ACQUIRED_TARGET,
            } = *registration
                && unit == h
            {
                let context = Context {
                    event_id: Some(EVENT_UNIT_ACQUIRED_TARGET),
                    triggering_unit: Some(h),
                    triggering_widget: Some(h),
                    ..Context::default()
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
        super::end_event(state);
        // The objects of the attack it takes (SPEC-0024, "Acquisition").
        if objects {
            super::table::acquired(&mut state.borrow_mut(), h);
        }
    }
}

/// `UnitDamageTarget` of a unit (SPEC-0015): the damage after armor and
/// the attack type, the `EVENT_UNIT_DAMAGED` triggers of the target while
/// it still has its old life, then the damage taken from the life it has
/// after them, which may kill it with `source` as the killer. False
/// without a source or a living target.
///
/// PROVISIONAL: ethereal and magic immune units are treated like the
/// others.
fn damage_unit(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    source: Option<u32>,
    target: Option<u32>,
    amount: u32,
    attack_type: i32,
    damage_type: i32,
) -> bool {
    let (damage, registrations) = {
        let state = state.borrow();
        let (Some(_), Some(unit)) = (state.unit(source), state.unit(target)) else {
            return false;
        };
        if !unit.alive || target.is_some_and(|t| super::cast::invulnerable(&state, t)) {
            return false;
        }
        let data = state
            .data
            .units
            .get(&unit.type_id)
            .cloned()
            .unwrap_or_default();
        let armor = if is_hero(unit.type_id) {
            state
                .combat
                .hero_armor(data.armor, ow3_math::i2r(unit.agility))
        } else {
            data.armor
        };
        // Devotion aura (SPEC-0038).
        let armor = ow3_math::add(armor, super::auras::armor(&state, unit));
        let damage = state
            .combat
            .damage(amount, armor, data.armor_type, attack_type, damage_type);
        // An illusion deals its factor of damage and takes its own
        // (SPEC-0040; c-SAK-1: the image's hits 0, hits on it doubled).
        let damage = match state.unit(source).and_then(|u| u.illusion) {
            Some(i) => ow3_math::mul(damage, i.dealt),
            None => damage,
        };
        let damage = match unit.illusion {
            Some(i) => ow3_math::mul(damage, i.taken),
            None => damage,
        };
        (damage, state.events.clone())
    };
    let Some(target) = target else {
        return false;
    };
    super::begin_event(state);
    for registration in &registrations {
        if let Registration::Unit {
            trigger,
            unit,
            event: EVENT_UNIT_DAMAGED,
        } = *registration
            && unit == target
        {
            let context = Context {
                event_id: Some(EVENT_UNIT_DAMAGED),
                triggering_unit: Some(target),
                triggering_widget: Some(target),
                event_damage: Some(damage),
                damage_source: source,
                ..Context::default()
            };
            fire_trigger(vm, state, trigger, context);
        }
    }
    super::end_event(state);
    let life = state
        .borrow()
        .unit(Some(target))
        .filter(|u| u.alive)
        .map(|u| u.life.value());
    if let Some(life) = life {
        set_unit_life(vm, state, Some(target), sub(life, damage), source);
    }
    true
}

/// A coordinate as the engine stores it, relative to the world's lower
/// left corner `min` (SPEC-0013).
fn world_relative(v: u32, min: u32) -> u32 {
    sub(add(v, min ^ SIGN), min ^ SIGN)
}

/// `v` kept inside `min + 32 ..= max - 32`, as `SetUnitPosition` does.
fn inside(v: u32, min: u32, max: u32) -> u32 {
    let (low, high) = (add(min, EDGE), sub(max, EDGE));
    if f32::from_bits(low) > f32::from_bits(high) {
        v
    } else if f32::from_bits(v) < f32::from_bits(low) {
        low
    } else if f32::from_bits(v) > f32::from_bits(high) {
        high
    } else {
        v
    }
}

/// Sets the hit points of a living destructable; 0 or less kills it and
/// runs its death events (SPEC-0009). PROVISIONAL: also for values below 0
/// and for `SetWidgetLife`.
pub(super) fn set_destructable_life(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: Option<u32>,
    life: u32,
) {
    let killed = {
        let mut state = state.borrow_mut();
        let tick = state.ticks;
        match destructable_mut(&mut state, h) {
            Some(d) if d.alive => {
                if f32::from_bits(life) < f32::from_bits(d.life) {
                    d.hit_at = Some(tick);
                }
                if f32::from_bits(life) <= 0.0 {
                    d.alive = false;
                    d.life = 0;
                    d.died_at = Some(tick);
                    true
                } else {
                    d.life = life;
                    false
                }
            }
            _ => false,
        }
    };
    if let (true, Some(h)) = (killed, h) {
        died(vm, state, h, None);
    }
}
pub(super) fn register_event(state: &Rc<RefCell<State>>, registration: Registration) -> Value {
    let mut state = state.borrow_mut();
    state.events.push(registration);
    Value::Handle(Some(state.create(Object::Event)))
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    // Units.
    native!(
        add,
        state,
        "native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit",
        |vm, state, args| {
            let mut s = state.borrow_mut();
            let Some(owner) = s.player_id(handle(&args[0])) else {
                return Ok(Some(Value::Handle(None)));
            };
            let at = [real(&args[2]), real(&args[3]), real(&args[4])];
            let h = create_unit_by_script(&mut s, owner, integer(&args[1]), at);
            if let Some(h) = h {
                super::table::unit_made(&mut s, h);
            }
            drop(s);
            // The filters of its regions run before the call returns
            // (SPEC-0049).
            super::regions::dispatch(vm, state);
            Ok(Some(Value::Handle(h)))
        }
    );
    native!(
        add,
        state,
        "native CreateUnitByName takes player whichPlayer, string unitname, real x, real y, real face returns unit",
        |vm, state, args| {
            let mut s = state.borrow_mut();
            let owner = s.player_id(handle(&args[0]));
            let type_id = string(&args[1]).and_then(|n| s.data.unit_id(n));
            let (Some(owner), Some(type_id)) = (owner, type_id) else {
                return Ok(Some(Value::Handle(None)));
            };
            let at = [real(&args[2]), real(&args[3]), real(&args[4])];
            let h = create_unit_by_script(&mut s, owner, type_id, at);
            drop(s);
            super::regions::dispatch(vm, state);
            Ok(Some(Value::Handle(h)))
        }
    );
    native!(
        add,
        state,
        "constant native UnitId takes string unitIdString returns integer",
        |vm, state, args| {
            let id = string(&args[0]).and_then(|n| state.borrow().data.unit_id(n));
            Ok(Some(Value::Integer(id.unwrap_or(0))))
        }
    );
    native!(
        add,
        state,
        "constant native UnitId2String takes integer unitId returns string",
        |vm, state, args| {
            let state = state.borrow();
            // SPEC-0009: "" for an unknown type and for a custom hero.
            let name = state.data.unit_name(integer(&args[0])).unwrap_or("");
            Ok(Some(Value::string(name)))
        }
    );
    native!(
        add,
        state,
        "native KillUnit takes unit whichUnit returns nothing",
        |vm, state, args| {
            kill_unit(vm, state, handle(&args[0]), None);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native RemoveUnit takes unit whichUnit returns nothing",
        |vm, state, args| {
            let unit = handle(&args[0]);
            let mut state = state.borrow_mut();
            if let Some(h) = unit
                && matches!(state.objects.get(&h), Some(Object::Unit(_)))
            {
                super::obstacles::died(&mut state, h);
                state.objects.remove(&h);
                state.units.retain(|&u| u != h);
            }
            Ok(None)
        }
    );
    for (declaration, field) in [
        (
            "constant native GetUnitX takes unit whichUnit returns real",
            (|u: &Unit| u.x) as fn(&Unit) -> u32,
        ),
        (
            "constant native GetUnitY takes unit whichUnit returns real",
            |u: &Unit| u.y,
        ),
        (
            "constant native GetUnitFacing takes unit whichUnit returns real",
            |u: &Unit| facing::to_degrees(u.facing),
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let value = state.borrow().unit(handle(&args[0])).map_or(0, field);
            Ok(Some(Value::Real(value)))
        });
    }
    native!(
        add,
        state,
        "constant native GetUnitTypeId takes unit whichUnit returns integer",
        |vm, state, args| {
            let id = state
                .borrow()
                .unit(handle(&args[0]))
                .map_or(0, |u| u.type_id);
            Ok(Some(Value::Integer(id)))
        }
    );
    native!(
        add,
        state,
        "constant native GetOwningPlayer takes unit whichUnit returns player",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let owner = state.unit(handle(&args[0])).map(|u| u.owner);
            Ok(Some(Value::Handle(owner.map(|id| state.player_handle(id)))))
        }
    );
    native!(
        add,
        state,
        "constant native GetUnitState takes unit whichUnit, unitstate whichUnitState returns real",
        |vm, state, args| {
            let state = state.borrow();
            let value = match (state.unit(handle(&args[0])), handle(&args[1])) {
                (Some(u), Some(s)) => match s as i32 {
                    UNIT_STATE_LIFE if u.alive => u.life.value(),
                    UNIT_STATE_MAX_LIFE => u.life.max,
                    UNIT_STATE_MANA => u.mana.value(),
                    UNIT_STATE_MAX_MANA => u.mana.max,
                    _ => 0,
                },
                _ => 0,
            };
            Ok(Some(Value::Real(value)))
        }
    );
    // Heroes (SPEC-0009): by the first character of the type's code.
    native!(
        add,
        state,
        "constant native IsHeroUnitId takes integer unitId returns boolean",
        |vm, state, args| { Ok(Some(Value::Boolean(is_hero(integer(&args[0]))))) }
    );
    // Hero attributes (SPEC-0013, "Hero attributes"): a change takes
    // effect at once, `permanent` or not. Strength moves max life by 25 a
    // point and the life by as much, to no less than 1, and restarts the
    // life regeneration 0.0001 s later at the new rate; a max life of 0
    // or less kills the hero inside the call. Intelligence moves max mana
    // by 15 a point and the mana by as much, to no less than 0, and
    // restarts mana regeneration the same way (PROVISIONAL: inferred).
    // Agility is kept for `GetHeroAgi` (PROVISIONAL: armor and damage
    // still come from the type). There are no items, so no bonuses.
    for (declaration, attribute) in [
        (
            "native SetHeroStr takes unit whichHero, integer newStr, boolean permanent returns nothing",
            Attribute::Strength,
        ),
        (
            "native SetHeroAgi takes unit whichHero, integer newAgi, boolean permanent returns nothing",
            Attribute::Agility,
        ),
        (
            "native SetHeroInt takes unit whichHero, integer newInt, boolean permanent returns nothing",
            Attribute::Intelligence,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let h = handle(&args[0]);
            let dies = set_attribute(&mut state.borrow_mut(), h, attribute, integer(&args[1]));
            if let Some(life) = dies {
                set_unit_life(vm, state, h, life, None);
            }
            Ok(None)
        });
    }
    for (declaration, attribute) in [
        (
            "native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer",
            Attribute::Strength,
        ),
        (
            "native GetHeroAgi takes unit whichHero, boolean includeBonuses returns integer",
            Attribute::Agility,
        ),
        (
            "native GetHeroInt takes unit whichHero, boolean includeBonuses returns integer",
            Attribute::Intelligence,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let value = state
                .borrow()
                .unit(handle(&args[0]))
                .map_or(0, |u| match attribute {
                    Attribute::Strength => u.strength,
                    Attribute::Agility => u.agility,
                    Attribute::Intelligence => u.intelligence,
                });
            Ok(Some(Value::Integer(value)))
        });
    }
    // A hero starts with 1 skill point when its type has hero data, with
    // 0 when it is made from another type (SPEC-0009), and gains one a
    // level (SPEC-0034), less those spent (SPEC-0035).
    native!(
        add,
        state,
        "native GetHeroSkillPoints takes unit whichHero returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let points = state
                .unit(handle(&args[0]))
                .map_or(0, |u| super::heroes::skill_points(&state, u));
            Ok(Some(Value::Integer(points)))
        }
    );
    native!(
        add,
        state,
        "constant native IsUnitType takes unit whichUnit, unittype whichUnitType returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let unit = state.unit(handle(&args[0]));
            let data = unit.and_then(|u| state.data.units.get(&u.type_id));
            let class = |name: &str| data.is_some_and(|t| t.classes.iter().any(|c| c == name));
            // PROVISIONAL: the types beyond hero and dead follow the
            // type's data (`isbldg` and the classes of `type`), and
            // sleeping a creep's sleep (SPEC-0030); the others are false.
            let answer = match handle(&args[1]).map(|t| t as i32) {
                Some(UNIT_TYPE_HERO) => unit.is_some_and(|u| is_hero(u.type_id)),
                Some(UNIT_TYPE_DEAD) => unit.is_some_and(|u| !u.alive),
                Some(UNIT_TYPE_STRUCTURE) => data.is_some_and(|t| t.building),
                Some(UNIT_TYPE_GIANT) => class("giant"),
                Some(UNIT_TYPE_SUMMONED) => class("summoned"),
                Some(UNIT_TYPE_UNDEAD) => class("undead"),
                Some(UNIT_TYPE_MECHANICAL) => class("mechanical"),
                Some(UNIT_TYPE_PEON) => class("peon"),
                Some(UNIT_TYPE_SAPPER) => class("sapper"),
                Some(UNIT_TYPE_TOWNHALL) => class("townhall"),
                Some(UNIT_TYPE_ANCIENT) => class("ancient"),
                Some(UNIT_TYPE_TAUREN) => class("tauren"),
                Some(UNIT_TYPE_SLEEPING) => unit.is_some_and(|u| u.creep.sleeping),
                _ => false,
            };
            Ok(Some(Value::Boolean(answer)))
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerTypedUnitCount takes player whichPlayer, string unitName, boolean includeIncomplete, boolean includeUpgrades returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let owner = state.player_id(handle(&args[0]));
            let type_id = string(&args[1]).and_then(|n| state.data.unit_id(n));
            let count = state
                .units
                .iter()
                .filter_map(|&u| state.unit(Some(u)))
                .filter(|u| u.alive && Some(u.owner) == owner && Some(u.type_id) == type_id)
                .count();
            Ok(Some(Value::Integer(count as i32)))
        }
    );

    // A `common.ai` native that maps may declare and call (SPEC-0001).
    native!(
        add,
        state,
        "native UnitAlive takes unit id returns boolean",
        |vm, state, args| {
            let alive = state
                .borrow()
                .unit(handle(&args[0]))
                .is_some_and(|u| u.alive);
            Ok(Some(Value::Boolean(alive)))
        }
    );

    // Hit points of units, destructables and items.
    native!(
        add,
        state,
        "native GetWidgetLife takes widget whichWidget returns real",
        |vm, state, args| {
            let state = state.borrow();
            let life = match handle(&args[0]).and_then(|h| state.objects.get(&h)) {
                Some(Object::Unit(u)) if u.alive => u.life.value(),
                Some(Object::Destructable(d)) => d.life,
                Some(Object::Item(i)) => i.life,
                _ => 0,
            };
            Ok(Some(Value::Real(life)))
        }
    );
    native!(
        add,
        state,
        "native SetWidgetLife takes widget whichWidget, real newLife returns nothing",
        |vm, state, args| {
            let h = handle(&args[0]);
            let life = real(&args[1]);
            let is_unit = state.borrow().unit(h).is_some();
            if is_unit {
                set_unit_life(vm, state, h, life, None);
            } else {
                set_destructable_life(vm, state, h, life);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetDestructableLife takes destructable d returns real",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let life = destructable_mut(&mut state, handle(&args[0])).map_or(0, |d| d.life);
            Ok(Some(Value::Real(life)))
        }
    );
    native!(
        add,
        state,
        "native GetDestructableMaxLife takes destructable d returns real",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let life = destructable_mut(&mut state, handle(&args[0])).map_or(0, |d| d.max_life);
            Ok(Some(Value::Real(life)))
        }
    );
    native!(
        add,
        state,
        "native SetDestructableLife takes destructable d, real life returns nothing",
        |vm, state, args| {
            set_destructable_life(vm, state, handle(&args[0]), real(&args[1]));
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native KillDestructable takes destructable d returns nothing",
        |vm, state, args| {
            let h = handle(&args[0]);
            let killed = {
                let mut state = state.borrow_mut();
                let tick = state.ticks;
                match destructable_mut(&mut state, h) {
                    Some(d) if d.alive => {
                        d.alive = false;
                        d.life = 0;
                        d.died_at = Some(tick);
                        true
                    }
                    _ => false,
                }
            };
            if let (true, Some(h)) = (killed, h) {
                died(vm, state, h, None);
            }
            Ok(None)
        }
    );

    // More about units: creation at a location, position, owner, state
    // and data kept for scripts. PROVISIONAL: positions are not limited
    // to the map, `SetUnitState` changes hit points but never kills, and
    // `changeColor` of `SetUnitOwner` is ignored.
    native!(
        add,
        state,
        "native CreateUnitAtLoc takes player id, integer unitid, location whichLocation, real face returns unit",
        |vm, state, args| {
            let mut s = state.borrow_mut();
            let owner = s.player_id(handle(&args[0]));
            let at = super::enumerate::location(&s, handle(&args[2]));
            let (Some(owner), Some((x, y))) = (owner, at) else {
                return Ok(Some(Value::Handle(None)));
            };
            let h = create_unit_by_script(&mut s, owner, integer(&args[1]), [x, y, real(&args[3])]);
            drop(s);
            super::regions::dispatch(vm, state);
            Ok(Some(Value::Handle(h)))
        }
    );
    native!(
        add,
        state,
        "constant native GetUnitLoc takes unit whichUnit returns location",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let at = state.unit(handle(&args[0])).map(|u| (u.x, u.y));
            Ok(Some(Value::Handle(
                at.map(|(x, y)| state.create(Object::Location { x, y })),
            )))
        }
    );
    type Move = fn(&mut Unit, &[Value], &State);
    // The moves test the regions at once (SPEC-0049).
    let moves: [(&str, Move, bool); 6] = [
        (
            "native SetUnitX takes unit whichUnit, real newX returns nothing",
            |u, args, state| u.x = world_relative(real(&args[1]), state.world_bounds[0]),
            true,
        ),
        (
            "native SetUnitY takes unit whichUnit, real newY returns nothing",
            |u, args, state| u.y = world_relative(real(&args[1]), state.world_bounds[1]),
            true,
        ),
        (
            "native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing",
            |u, args, state| {
                let [min_x, min_y, max_x, max_y] = state.world_bounds;
                u.x = world_relative(inside(real(&args[1]), min_x, max_x), min_x);
                u.y = world_relative(inside(real(&args[2]), min_y, max_y), min_y);
            },
            true,
        ),
        (
            "native SetUnitUserData takes unit whichUnit, integer data returns nothing",
            |u, args, _| u.user_data = integer(&args[1]),
            false,
        ),
        (
            "native ShowUnit takes unit whichUnit, boolean show returns nothing",
            |u, args, _| u.hidden = args[1] != Value::Boolean(true),
            false,
        ),
        (
            "native PauseUnit takes unit whichUnit, boolean flag returns nothing",
            |u, args, _| u.paused = args[1] == Value::Boolean(true),
            false,
        ),
    ];
    for (declaration, apply, moves) in moves {
        native!(add, state, declaration, |vm, state, args| {
            let mut s = state.borrow_mut();
            let Some(h) = handle(&args[0]) else {
                return Ok(None);
            };
            if let Some(Object::Unit(mut unit)) = s.objects.get(&h).cloned() {
                apply(&mut unit, args, &s);
                if let Some(u) = unit_mut(&mut s, Some(h)) {
                    *u = *unit;
                }
                if moves {
                    super::regions::test_unit(&mut s, h);
                    drop(s);
                    super::regions::dispatch(vm, state);
                }
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native SetUnitFacing takes unit whichUnit, real facingAngle returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(h) = handle(&args[0]) else {
                return Ok(None);
            };
            // The facing turns at the updates of the shared cycle
            // (SPEC-0013, SPEC-0018). A unit walking an order is not
            // affected at all; one that has arrived and still turns takes
            // the angle as its heading (PROVISIONAL: not measured).
            let first = super::movement::next_update_after(state.ticks);
            let heading = facing::from_degrees(real(&args[1]));
            if let Some(u) = unit_mut(&mut state, Some(h)) {
                if let Some(m) = u.movement.as_mut() {
                    if !m.ordered() {
                        m.heading = heading;
                    }
                    return Ok(None);
                }
                let was_turning = u.turn.is_some();
                u.turn = Some(facing::Turn::new(heading, first, u.orient_interp));
                if !was_turning {
                    state.turning.push(h);
                }
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetUnitPositionLoc takes unit whichUnit, location whichLocation returns nothing",
        |vm, state, args| {
            let mut s = state.borrow_mut();
            let at = super::enumerate::location(&s, handle(&args[1]));
            let h = handle(&args[0]);
            if let (Some((x, y)), Some(u)) = (at, unit_mut(&mut s, h)) {
                u.x = x;
                u.y = y;
                super::regions::test_unit(&mut s, h.expect("a unit"));
                drop(s);
                super::regions::dispatch(vm, state);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetUnitUserData takes unit whichUnit returns integer",
        |vm, state, args| {
            let data = state
                .borrow()
                .unit(handle(&args[0]))
                .map_or(0, |u| u.user_data);
            Ok(Some(Value::Integer(data)))
        }
    );
    // How `whichPlayer` regards the unit's owner (SPEC-0008, "Units"):
    // `IsPlayerAlly` and `IsPlayerEnemy` with the players swapped.
    for (declaration, ally) in [
        (
            "constant native IsUnitAlly takes unit whichUnit, player whichPlayer returns boolean",
            true,
        ),
        (
            "constant native IsUnitEnemy takes unit whichUnit, player whichPlayer returns boolean",
            false,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let state = state.borrow();
            let owner = state.unit(handle(&args[0])).map(|u| u.owner);
            let player = state.player_id(handle(&args[1]));
            let result = owner
                .zip(player)
                .is_some_and(|(owner, player)| state.setup.is_ally(player, owner) == ally);
            Ok(Some(Value::Boolean(result)))
        });
    }
    // The distance minus the unit's collision size is at most the range
    // (SPEC-0006).
    native!(
        add,
        state,
        "native IsUnitInRangeXY takes unit whichUnit, real x, real y, real distance returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let near = state.unit(handle(&args[0])).is_some_and(|u| {
                let collision = state.data.units.get(&u.type_id).map_or(0, |t| t.collision);
                let (dx, dy) = (
                    ow3_math::sub(u.x, real(&args[1])),
                    ow3_math::sub(u.y, real(&args[2])),
                );
                // IEEE sqrt: the spec's boundary (969 at 1000) holds exactly.
                let distance = f32::from_bits(ow3_math::add(mul(dx, dx), mul(dy, dy)))
                    .sqrt()
                    .to_bits();
                f32::from_bits(ow3_math::sub(distance, collision)) <= f32::from_bits(real(&args[3]))
            });
            Ok(Some(Value::Boolean(near)))
        }
    );
    native!(
        add,
        state,
        "native IsUnitPaused takes unit whichUnit returns boolean",
        |vm, state, args| {
            let paused = state
                .borrow()
                .unit(handle(&args[0]))
                .is_some_and(|u| u.paused);
            Ok(Some(Value::Boolean(paused)))
        }
    );
    native!(
        add,
        state,
        "constant native IsUnitHidden takes unit whichUnit returns boolean",
        |vm, state, args| {
            let hidden = state
                .borrow()
                .unit(handle(&args[0]))
                .is_some_and(|u| u.hidden);
            Ok(Some(Value::Boolean(hidden)))
        }
    );
    native!(
        add,
        state,
        "native UnitIsSleeping takes unit whichUnit returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let asleep = handle(&args[0]).is_some_and(|h| super::creeps::sleeping(&state, h));
            Ok(Some(Value::Boolean(asleep)))
        }
    );
    native!(
        add,
        state,
        "native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let owner = state.player_id(handle(&args[1]));
            if let (Some(owner), Some(u)) = (owner, unit_mut(&mut state, handle(&args[0]))) {
                u.owner = owner;
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetUnitState takes unit whichUnit, unitstate whichUnitState, real newVal returns nothing",
        |vm, state, args| {
            let h = handle(&args[0]);
            let value = real(&args[2]);
            match handle(&args[1]).map(|s| s as i32) {
                Some(UNIT_STATE_LIFE) => set_unit_life(vm, state, h, value, None),
                Some(UNIT_STATE_MANA) => {
                    let mut state = state.borrow_mut();
                    if let Some(u) = unit_mut(&mut state, h)
                        && u.alive
                    {
                        u.mana.set(value);
                    }
                }
                // SPEC-0013: the maxima cannot be set.
                _ => {}
            }
            Ok(None)
        }
    );
    for (declaration, adds) in [
        (
            "native SetResourceAmount takes unit whichUnit, integer amount returns nothing",
            false,
        ),
        (
            "native AddResourceAmount takes unit whichUnit, integer amount returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(u) = unit_mut(&mut state, handle(&args[0])) {
                let amount = integer(&args[1]);
                u.resources = if adds {
                    u.resources.wrapping_add(amount)
                } else {
                    amount
                };
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native GetResourceAmount takes unit whichUnit returns integer",
        |vm, state, args| {
            let amount = state
                .borrow()
                .unit(handle(&args[0]))
                .map_or(0, |u| u.resources);
            Ok(Some(Value::Integer(amount)))
        }
    );
    for (declaration, buildings) in [
        (
            "constant native GetPlayerUnitCount takes player whichPlayer, boolean includeIncomplete returns integer",
            false,
        ),
        (
            "constant native GetPlayerStructureCount takes player whichPlayer, boolean includeIncomplete returns integer",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let state = state.borrow();
            let owner = state.player_id(handle(&args[0]));
            let count = state
                .units
                .iter()
                .filter_map(|&u| state.unit(Some(u)))
                .filter(|u| u.alive && Some(u.owner) == owner)
                .filter(|u| {
                    !buildings || state.data.units.get(&u.type_id).is_some_and(|t| t.building)
                })
                .count();
            Ok(Some(Value::Integer(count as i32)))
        });
    }

    // Destructables made by scripts.
    // x and y are the second and third arguments in all three.
    for (declaration, dead) in [
        (
            "native CreateDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable",
            false,
        ),
        (
            "native CreateDestructableZ takes integer objectid, real x, real y, real z, real face, real scale, integer variation returns destructable",
            false,
        ),
        (
            "native CreateDeadDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let type_id = integer(&args[0]);
            let max_life = match state.data.destructables.get(&type_id) {
                Some(&hp) => hp,
                None if state.data.destructables.is_empty() => UNKNOWN_LIFE,
                None => return Ok(Some(Value::Handle(None))),
            };
            // The facing follows z in `CreateDestructableZ`.
            let face = if declaration.contains("real z,") {
                4
            } else {
                3
            };
            let facing = facing::from_degrees(real(&args[face]));
            let (x, y) = super::obstacles::place_destructable(
                &state,
                type_id,
                real(&args[1]),
                real(&args[2]),
                facing,
                !dead,
            );
            let died_at = dead.then_some(state.ticks);
            let h = state.create(Object::Destructable(Destructable {
                type_id,
                x,
                y,
                life: if dead { 0 } else { max_life },
                max_life,
                alive: !dead,
                facing,
                footprint: super::obstacles::Footprint::default(),
                hit_at: None,
                died_at,
            }));
            state.destructables.push(h);
            super::obstacles::refresh_destructable(&mut state, h);
            Ok(Some(Value::Handle(Some(h))))
        });
    }
    native!(
        add,
        state,
        "native RemoveDestructable takes destructable d returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(h) = handle(&args[0])
                && matches!(state.objects.get(&h), Some(Object::Destructable(_)))
            {
                super::obstacles::gone(&mut state, h);
                state.objects.remove(&h);
                state.destructables.retain(|&d| d != h);
            }
            Ok(None)
        }
    );

    // Items.
    native!(
        add,
        state,
        "native CreateItem takes integer itemid, real x, real y returns item",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let type_id = integer(&args[0]);
            if !state.data.items.is_empty() && !state.data.items.contains_key(&type_id) {
                return Ok(Some(Value::Handle(None)));
            }
            let life = state
                .data
                .items
                .get(&type_id)
                .copied()
                .unwrap_or(UNKNOWN_LIFE);
            let h = state.create(Object::Item(Item {
                type_id,
                x: real(&args[1]),
                y: real(&args[2]),
                life,
            }));
            state.items.push(h);
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native RemoveItem takes item whichItem returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(h) = handle(&args[0])
                && matches!(state.objects.get(&h), Some(Object::Item(_)))
            {
                state.objects.remove(&h);
                state.items.retain(|&i| i != h);
            }
            Ok(None)
        }
    );
    // An item stays visible even at 0 life (SPEC-0015). PROVISIONAL:
    // items cannot be hidden or picked up yet.
    native!(
        add,
        state,
        "native IsItemVisible takes item whichItem returns boolean",
        |vm, state, args| {
            let visible = item(&state.borrow(), handle(&args[0])).is_some();
            Ok(Some(Value::Boolean(visible)))
        }
    );
    native!(
        add,
        state,
        "native GetItemTypeId takes item i returns integer",
        |vm, state, args| {
            let id = item(&state.borrow(), handle(&args[0])).map_or(0, |i| i.type_id);
            Ok(Some(Value::Integer(id)))
        }
    );
    for (declaration, y) in [
        ("native GetItemX takes item i returns real", false),
        ("native GetItemY takes item i returns real", true),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let at = item(&state.borrow(), handle(&args[0])).map(|i| if y { i.y } else { i.x });
            Ok(Some(Value::Real(at.unwrap_or(0))))
        });
    }

    // Death events and their responses.
    native!(
        add,
        state,
        "native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event",
        |vm, state, args| {
            let (Some(trigger), Some(unit), Some(event)) =
                (handle(&args[0]), handle(&args[1]), handle(&args[2]))
            else {
                return Ok(Some(Value::Handle(None)));
            };
            let event = event as i32;
            Ok(Some(register_event(
                state,
                Registration::Unit {
                    trigger,
                    unit,
                    event,
                },
            )))
        }
    );
    native!(
        add,
        state,
        "native TriggerRegisterPlayerUnitEvent takes trigger whichTrigger, player whichPlayer, playerunitevent whichPlayerUnitEvent, boolexpr filter returns event",
        |vm, state, args| {
            let player = state.borrow().player_id(handle(&args[1]));
            let (Some(trigger), Some(player), Some(event)) =
                (handle(&args[0]), player, handle(&args[2]))
            else {
                return Ok(Some(Value::Handle(None)));
            };
            let registration = Registration::PlayerUnit {
                trigger,
                player,
                event: event as i32,
                filter: handle(&args[3]),
            };
            Ok(Some(register_event(state, registration)))
        }
    );
    native!(
        add,
        state,
        "native TriggerRegisterTimerExpireEvent takes trigger whichTrigger, timer t returns event",
        |vm, state, args| {
            let (Some(trigger), Some(timer)) = (handle(&args[0]), handle(&args[1])) else {
                return Ok(Some(Value::Handle(None)));
            };
            Ok(Some(register_event(
                state,
                Registration::TimerExpire { trigger, timer },
            )))
        }
    );
    native!(
        add,
        state,
        "native TriggerRegisterDeathEvent takes trigger whichTrigger, widget whichWidget returns event",
        |vm, state, args| {
            let (Some(trigger), Some(widget)) = (handle(&args[0]), handle(&args[1])) else {
                return Ok(Some(Value::Handle(None)));
            };
            Ok(Some(register_event(
                state,
                Registration::Death { trigger, widget },
            )))
        }
    );
    for declaration in [
        "constant native GetTriggerUnit takes nothing returns unit",
        "constant native GetDyingUnit takes nothing returns unit",
        "constant native GetLevelingUnit takes nothing returns unit",
        "constant native GetConstructingStructure takes nothing returns unit",
        "constant native GetConstructedStructure takes nothing returns unit",
    ] {
        native!(add, state, declaration, |vm, state, args| {
            Ok(Some(Value::Handle(state.borrow().context.triggering_unit)))
        });
    }
    // The units of a summon event (SPEC-0044).
    native!(
        add,
        state,
        "constant native GetSummonedUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.summoned_unit))) }
    );
    native!(
        add,
        state,
        "constant native GetSummoningUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.summoning_unit))) }
    );
    // The unit and type of a training event (SPEC-0027).
    native!(
        add,
        state,
        "constant native GetTrainedUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.trained_unit))) }
    );
    native!(
        add,
        state,
        "constant native GetTrainedUnitType takes nothing returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                state.borrow().context.trained_type.unwrap_or(0),
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetIssuedOrderId takes nothing returns integer",
        |vm, state, args| {
            Ok(Some(Value::Integer(
                state.borrow().context.issued_order.unwrap_or(0),
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetKillingUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.killing_unit))) }
    );
    native!(
        add,
        state,
        "native UnitDamageTarget takes unit whichUnit, widget target, real amount, boolean attack, boolean ranged, attacktype attackType, damagetype damageType, weapontype weaponType returns boolean",
        |vm, state, args| {
            let attack = handle(&args[5]).map_or(-1, |h| h as i32);
            let damage = handle(&args[6]).map_or(-1, |h| h as i32);
            let done = damage_widget(
                vm,
                state,
                handle(&args[0]),
                handle(&args[1]),
                real(&args[2]),
                attack,
                damage,
            );
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "constant native GetEventDamage takes nothing returns real",
        |vm, state, args| {
            Ok(Some(Value::Real(
                state.borrow().context.event_damage.unwrap_or(0),
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetEventDamageSource takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.damage_source))) }
    );
    native!(
        add,
        state,
        "constant native GetTriggerPlayer takes nothing returns player",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.triggering_player,
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetTriggerWidget takes nothing returns widget",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.triggering_widget,
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetTriggerDestructable takes nothing returns destructable",
        |vm, state, args| {
            let state = state.borrow();
            let widget = state
                .context
                .triggering_widget
                .filter(|h| matches!(state.objects.get(h), Some(Object::Destructable(_))));
            Ok(Some(Value::Handle(widget)))
        }
    );
    native!(
        add,
        state,
        "constant native GetTriggerEventId takes nothing returns eventid",
        |vm, state, args| {
            Ok(Some(
                state
                    .borrow()
                    .context
                    .event_id
                    .map_or(Value::Handle(None), converted),
            ))
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_relative_coordinates() {
        let min = (-12288.0f32).to_bits();
        let max = 12288.0f32.to_bits();
        let read = |v: f32| f32::from_bits(world_relative(v.to_bits(), min));
        assert_eq!(read(-1000.3).to_bits(), 0xC47A_1340);
        assert_eq!(read(-1200.7).to_bits(), 0xC496_1668);
        assert_eq!(read(100.0), 100.0);
        let clamped = world_relative(inside(50000.0f32.to_bits(), min, max), min);
        assert_eq!(f32::from_bits(clamped), 12256.0);
        assert_eq!(
            f32::from_bits(inside((-50000.0f32).to_bits(), min, max)),
            -12256.0
        );
    }
}
