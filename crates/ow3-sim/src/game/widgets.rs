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
const EVENT_UNIT_DAMAGED: i32 = 52;
const EVENT_UNIT_DEATH: i32 = 53;
const EVENT_WIDGET_DEATH: i32 = 89;
const UNIT_STATE_LIFE: i32 = 0;
const UNIT_STATE_MAX_LIFE: i32 = 1;
const UNIT_STATE_MANA: i32 = 2;
const UNIT_STATE_MAX_MANA: i32 = 3;
const UNIT_TYPE_HERO: i32 = 0;
const UNIT_TYPE_DEAD: i32 = 1;
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

/// Creates a unit of `type_id` for `owner`, or `None` for a type the game
/// data does not know.
fn create_unit(state: &mut State, owner: i32, type_id: i32, at: [u32; 3]) -> Option<u32> {
    let data = if state.data.units.is_empty() {
        UnitType {
            hp: UNKNOWN_LIFE,
            ..UnitType::default()
        }
    } else {
        state.data.units.get(&type_id)?.clone()
    };
    // Life regenerates at `regenHP` when the type's condition holds, plus
    // a hero's strength part (SPEC-0013). PROVISIONAL: there is no day,
    // night or blight yet, so a condition other than "always" never holds.
    let life_rate = if data.regen_type == RegenType::Always {
        add(data.life_regen, data.hero_life_regen)
    } else {
        data.hero_life_regen
    };
    let mut life = Regenerating::full(data.hp, life_rate);
    let mut mana = Regenerating::full(data.max_mana, data.mana_regen);
    mana.set(data.start_mana);
    // A hero's regeneration starts 0.0001 s after it is created.
    life.delayed = is_hero(type_id);
    mana.delayed = is_hero(type_id);
    // Positions snap to a quarter (SPEC-0013).
    let snap = |v: u32| mul(floor(mul(v, FOUR)), QUARTER);
    let unit = Unit {
        owner,
        type_id,
        x: snap(at[0]),
        y: snap(at[1]),
        facing: facing::from_degrees(at[2]),
        turn: None,
        orient_interp: data.orient_interp,
        life,
        mana,
        alive: true,
        hidden: false,
        user_data: 0,
        resources: 0,
    };
    let h = state.create(Object::Unit(unit));
    state.units.push(h);
    Some(h)
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
        let owner = state.unit(Some(widget)).map(|u| u.owner);
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
}

/// Kills a living unit: it reads life 0 and stops regenerating, and the
/// triggers of its death run before this returns.
fn kill_unit(vm: &mut Vm, state: &Rc<RefCell<State>>, h: Option<u32>, killer: Option<u32>) {
    let killed = {
        let mut state = state.borrow_mut();
        match unit_mut(&mut state, h) {
            Some(unit) if unit.alive => {
                unit.alive = false;
                unit.life.base = 0;
                unit.life.rate = 0;
                unit.mana.rate = 0;
                true
            }
            _ => false,
        }
    };
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
fn damage_widget(
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
        if !unit.alive {
            return false;
        }
        let data = state
            .data
            .units
            .get(&unit.type_id)
            .cloned()
            .unwrap_or_default();
        let armor = if is_hero(unit.type_id) {
            state.combat.hero_armor(data.armor, data.agility)
        } else {
            data.armor
        };
        let damage = state
            .combat
            .damage(amount, armor, data.armor_type, attack_type, damage_type);
        (damage, state.events.clone())
    };
    let Some(target) = target else {
        return false;
    };
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
fn set_destructable_life(vm: &mut Vm, state: &Rc<RefCell<State>>, h: Option<u32>, life: u32) {
    let killed = {
        let mut state = state.borrow_mut();
        match destructable_mut(&mut state, h) {
            Some(d) if d.alive => {
                if f32::from_bits(life) <= 0.0 {
                    d.alive = false;
                    d.life = 0;
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
fn register_event(state: &Rc<RefCell<State>>, registration: Registration) -> Value {
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
            let mut state = state.borrow_mut();
            let Some(owner) = state.player_id(handle(&args[0])) else {
                return Ok(Some(Value::Handle(None)));
            };
            let at = [real(&args[2]), real(&args[3]), real(&args[4])];
            let h = create_unit(&mut state, owner, integer(&args[1]), at);
            Ok(Some(Value::Handle(h)))
        }
    );
    native!(
        add,
        state,
        "native CreateUnitByName takes player whichPlayer, string unitname, real x, real y, real face returns unit",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let owner = state.player_id(handle(&args[0]));
            let type_id = string(&args[1]).and_then(|n| state.data.unit_id(n));
            let (Some(owner), Some(type_id)) = (owner, type_id) else {
                return Ok(Some(Value::Handle(None)));
            };
            let at = [real(&args[2]), real(&args[3]), real(&args[4])];
            Ok(Some(Value::Handle(create_unit(
                &mut state, owner, type_id, at,
            ))))
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
    // PROVISIONAL: heroes do not gain experience yet, so a hero is level 1.
    native!(
        add,
        state,
        "constant native GetHeroLevel takes unit whichHero returns integer",
        |vm, state, args| {
            let level = state
                .borrow()
                .unit(handle(&args[0]))
                .map_or(0, |u| i32::from(is_hero(u.type_id)));
            Ok(Some(Value::Integer(level)))
        }
    );
    native!(
        add,
        state,
        "constant native IsUnitType takes unit whichUnit, unittype whichUnitType returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let unit = state.unit(handle(&args[0]));
            let answer = match handle(&args[1]).map(|t| t as i32) {
                Some(UNIT_TYPE_HERO) => unit.is_some_and(|u| is_hero(u.type_id)),
                Some(UNIT_TYPE_DEAD) => unit.is_some_and(|u| !u.alive),
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
                match destructable_mut(&mut state, h) {
                    Some(d) if d.alive => {
                        d.alive = false;
                        d.life = 0;
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
            let mut state = state.borrow_mut();
            let owner = state.player_id(handle(&args[0]));
            let at = super::enumerate::location(&state, handle(&args[2]));
            let (Some(owner), Some((x, y))) = (owner, at) else {
                return Ok(Some(Value::Handle(None)));
            };
            let h = create_unit(&mut state, owner, integer(&args[1]), [x, y, real(&args[3])]);
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
    let moves: [(&str, Move); 5] = [
        (
            "native SetUnitX takes unit whichUnit, real newX returns nothing",
            |u, args, state| u.x = world_relative(real(&args[1]), state.world_bounds[0]),
        ),
        (
            "native SetUnitY takes unit whichUnit, real newY returns nothing",
            |u, args, state| u.y = world_relative(real(&args[1]), state.world_bounds[1]),
        ),
        (
            "native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing",
            |u, args, state| {
                let [min_x, min_y, max_x, max_y] = state.world_bounds;
                u.x = world_relative(inside(real(&args[1]), min_x, max_x), min_x);
                u.y = world_relative(inside(real(&args[2]), min_y, max_y), min_y);
            },
        ),
        (
            "native SetUnitUserData takes unit whichUnit, integer data returns nothing",
            |u, args, _| u.user_data = integer(&args[1]),
        ),
        (
            "native ShowUnit takes unit whichUnit, boolean show returns nothing",
            |u, args, _| u.hidden = args[1] != Value::Boolean(true),
        ),
    ];
    for (declaration, apply) in moves {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(Object::Unit(mut unit)) =
                handle(&args[0]).and_then(|h| state.objects.get(&h).cloned())
            {
                apply(&mut unit, args, &state);
                if let Some(u) = unit_mut(&mut state, handle(&args[0])) {
                    *u = unit;
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
            let tick = state.ticks;
            if let Some(u) = unit_mut(&mut state, Some(h)) {
                let was_turning = u.turn.is_some();
                u.turn = Some(facing::Turn::new(real(&args[1]), tick, u.orient_interp));
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
            let mut state = state.borrow_mut();
            let at = super::enumerate::location(&state, handle(&args[1]));
            if let (Some((x, y)), Some(u)) = (at, unit_mut(&mut state, handle(&args[0]))) {
                u.x = x;
                u.y = y;
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
            let h = state.create(Object::Destructable(Destructable {
                type_id,
                x: real(&args[1]),
                y: real(&args[2]),
                life: if dead { 0 } else { max_life },
                max_life,
                alive: !dead,
            }));
            state.destructables.push(h);
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
    ] {
        native!(add, state, declaration, |vm, state, args| {
            Ok(Some(Value::Handle(state.borrow().context.triggering_unit)))
        });
    }
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
