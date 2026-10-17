//! Units, destructables and items as placeholders until they are
//! specified, and their death events.
//!
//! A death runs the triggers registered for it as an event: each enabled
//! trigger whose conditions hold runs its actions, and an action that
//! sleeps lets the code that caused the death go on (SPEC-0005).
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: registrations fire in the
//! order they were made, whatever their kind; the filter of a player unit
//! event sees the dying unit as `GetFilterUnit`; `KillUnit` has no killing
//! unit; a unit starts with the hit points of its type (`UnitBalance.slk`
//! `HP`) and `CreateUnit` with a type the game data does not know gives
//! `null` (without game data every type is accepted, with 1 hit point);
//! `UnitId` ignores ASCII case. Changing hit points never kills.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use super::{
    Context, Destructable, Item, Object, State, Unit, converted, fire_trigger, handle, integer,
    real, string, test_boolexpr,
};

const EVENT_PLAYER_UNIT_DEATH: i32 = 20;
const EVENT_UNIT_DEATH: i32 = 53;
const EVENT_WIDGET_DEATH: i32 = 89;
const UNIT_STATE_LIFE: i32 = 0;
const UNIT_STATE_MAX_LIFE: i32 = 1;
const UNIT_TYPE_DEAD: i32 = 1;
/// Hit points of a unit when there is no game data: 1.0.
const UNKNOWN_LIFE: u32 = 0x3F80_0000;

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
    let max_life = if state.data.units.is_empty() {
        UNKNOWN_LIFE
    } else {
        state.data.units.get(&type_id)?.hp
    };
    let unit = Unit {
        owner,
        type_id,
        x: at[0],
        y: at[1],
        facing: at[2],
        life: max_life,
        max_life,
        alive: true,
    };
    let h = state.create(Object::Unit(unit));
    state.units.push(h);
    Some(h)
}

/// Runs the triggers registered for the death of `widget`.
fn died(vm: &mut Vm, state: &Rc<RefCell<State>>, widget: u32) {
    let (registrations, owner) = {
        let mut state = state.borrow_mut();
        let owner = state.unit(Some(widget)).map(|u| u.owner);
        let owner_handle = owner.map(|id| state.player_handle(id));
        (state.events.clone(), owner.zip(owner_handle))
    };
    let base = Context {
        triggering_widget: Some(widget),
        ..Context::default()
    };
    for registration in registrations {
        match registration {
            Registration::Unit {
                trigger,
                unit,
                event: EVENT_UNIT_DEATH,
            } if unit == widget => {
                let context = Context {
                    event_id: Some(EVENT_UNIT_DEATH),
                    triggering_unit: Some(widget),
                    ..base
                };
                fire_trigger(vm, state, trigger, context);
            }
            Registration::PlayerUnit {
                trigger,
                player,
                event: EVENT_PLAYER_UNIT_DEATH,
                filter,
            } if owner.is_some_and(|(id, _)| id == player) => {
                let filter_context = Context {
                    filter_unit: Some(widget),
                    ..Context::default()
                };
                if test_boolexpr(vm, state, filter, filter_context) {
                    let context = Context {
                        event_id: Some(EVENT_PLAYER_UNIT_DEATH),
                        triggering_unit: Some(widget),
                        triggering_player: owner.map(|(_, h)| h),
                        ..base
                    };
                    fire_trigger(vm, state, trigger, context);
                }
            }
            Registration::Death { trigger, widget: w } if w == widget => {
                let unit = owner.is_some().then_some(widget);
                let context = Context {
                    event_id: Some(EVENT_WIDGET_DEATH),
                    triggering_unit: unit,
                    ..base
                };
                fire_trigger(vm, state, trigger, context);
            }
            _ => {}
        }
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
            Ok(Some(match state.data.unit_name(integer(&args[0])) {
                Some(name) => Value::string(name),
                None => Value::String(None),
            }))
        }
    );
    native!(
        add,
        state,
        "native KillUnit takes unit whichUnit returns nothing",
        |vm, state, args| {
            let h = handle(&args[0]);
            let killed = {
                let mut state = state.borrow_mut();
                match unit_mut(&mut state, h) {
                    Some(unit) if unit.alive => {
                        unit.alive = false;
                        unit.life = 0;
                        true
                    }
                    _ => false,
                }
            };
            if let (true, Some(h)) = (killed, h) {
                died(vm, state, h);
            }
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
            |u: &Unit| u.facing,
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
                (Some(u), Some(s)) if s as i32 == UNIT_STATE_LIFE => u.life,
                (Some(u), Some(s)) if s as i32 == UNIT_STATE_MAX_LIFE => u.max_life,
                _ => 0,
            };
            Ok(Some(Value::Real(value)))
        }
    );
    native!(
        add,
        state,
        "constant native IsUnitType takes unit whichUnit, unittype whichUnitType returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let dead = handle(&args[1]) == Some(UNIT_TYPE_DEAD as u32)
                && state.unit(handle(&args[0])).is_some_and(|u| !u.alive);
            Ok(Some(Value::Boolean(dead)))
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
                Some(Object::Unit(u)) => u.life,
                Some(Object::Destructable(d)) => d.life,
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
            let mut state = state.borrow_mut();
            let life = real(&args[1]);
            match handle(&args[0]).and_then(|h| state.objects.get_mut(&h)) {
                Some(Object::Unit(u)) if u.alive => u.life = life,
                Some(Object::Destructable(d)) if d.alive => d.life = life,
                _ => {}
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
            let mut state = state.borrow_mut();
            if let Some(d) = destructable_mut(&mut state, handle(&args[0]))
                && d.alive
            {
                d.life = real(&args[1]);
            }
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
                died(vm, state, h);
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
            let h = state.create(Object::Item(Item {
                type_id,
                x: real(&args[1]),
                y: real(&args[2]),
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
        |vm, state, args| { Ok(Some(Value::Handle(None))) }
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
