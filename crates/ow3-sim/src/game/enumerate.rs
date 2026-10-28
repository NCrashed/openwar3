//! Forces, groups and the enumerations over players and units, with the
//! placeholder units, locations and rects they need.
//!
//! Callbacks (`ForForce`, `ForGroup`) and filters run in threads of their
//! own. A callback that sleeps ends at the sleep and the enumeration goes on
//! with the next element; a filter that sleeps counts as false (SPEC-0005).
//!
//! Each callback and filter has its own operation budget, forces go by
//! increasing player number, `GroupEnum*` and `ForceEnumPlayers*` clear the
//! group or force first, a unit is in range by the distance of its centre,
//! and `EnumDestructablesInRect` runs the action right after its filter
//! (SPEC-0006).
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: groups go in the order units
//! were added; the enumerations include dead units; the range test uses
//! the squared distance in engine arithmetic, and a rect contains what lies
//! within its bounds.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::{add, mul, sub};

use super::{
    BoolExpr, Context, Destructable, Object, State, Unit, code, handle, integer, real,
    run_new_thread, string, test_boolexpr,
};

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Runs `callback` once for each element in its own thread, with the
/// context made by `with` from the caller's.
fn for_each(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    elements: Vec<u32>,
    callback: Option<usize>,
    with: impl Fn(Context, u32) -> Context,
) {
    let Some(callback) = callback else {
        return;
    };
    let context = state.borrow().context;
    for element in elements {
        run_new_thread(vm, state, callback, with(context, element), false);
    }
}

/// The candidates that pass `filter`, at most `limit` of them, tested in
/// order with the context made by `with`.
fn select(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    candidates: Vec<u32>,
    filter: Option<u32>,
    limit: Option<i32>,
    with: impl Fn(Context, u32) -> Context,
) -> Vec<u32> {
    let context = state.borrow().context;
    let mut chosen = Vec::new();
    for candidate in candidates {
        if limit.is_some_and(|limit| chosen.len() as i64 >= i64::from(limit)) {
            break;
        }
        if test_boolexpr(vm, state, filter, with(context, candidate)) {
            chosen.push(candidate);
        }
    }
    chosen
}

/// For each candidate in turn: the filter, then, if it passed, the action
/// (SPEC-0006 for destructables).
fn filter_then_act(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    candidates: Vec<u32>,
    filter: Option<u32>,
    action: Option<usize>,
    filtering: impl Fn(Context, u32) -> Context,
    acting: impl Fn(Context, u32) -> Context,
) {
    let context = state.borrow().context;
    for candidate in candidates {
        if test_boolexpr(vm, state, filter, filtering(context, candidate))
            && let Some(action) = action
        {
            run_new_thread(vm, state, action, acting(context, candidate), false);
        }
    }
}

/// `GroupEnum*`: clears the group, then adds the units that `include`
/// accepts and that pass the filter.
fn group_enum(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    group: Option<u32>,
    filter: Option<u32>,
    limit: Option<i32>,
    include: impl Fn(&Unit) -> bool,
) {
    let candidates: Vec<u32> = {
        let mut state = state.borrow_mut();
        let Some(units) = state.group(group) else {
            return;
        };
        units.clear();
        let all = state.units.clone();
        all.into_iter()
            .filter(|&u| state.unit(Some(u)).is_some_and(&include))
            .collect()
    };
    let chosen = select(vm, state, candidates, filter, limit, |c, u| Context {
        filter_unit: Some(u),
        ..c
    });
    let mut s = state.borrow_mut();
    for u in chosen {
        group_insert(&mut s, group, u);
    }
}

/// Adds unit `u` to group `group` in the game's order (hub #15, read in
/// c-SDK-1 and on c-SCC-1's MeleeClearExcessUnits): before the first unit
/// of the same type with a larger key (the table entry of the first object
/// of its creation); with none, a hero at the front, any other unit at the
/// end.
///
/// PROVISIONAL: a unit made without the table keys by its handle.
fn group_insert(state: &mut State, group: Option<u32>, u: u32) {
    let key = |state: &State, h: u32| {
        state
            .unit(Some(h))
            .and_then(|x| x.entries.id)
            .map_or(h, |id| id[0])
    };
    let Some((type_id, k)) = state.unit(Some(u)).map(|x| (x.type_id, key(state, u))) else {
        return;
    };
    let members = state.group(group).cloned().unwrap_or_default();
    if members.contains(&u) {
        return;
    }
    let at = members.iter().position(|&o| {
        state
            .unit(Some(o))
            .is_some_and(|x| x.type_id == type_id && key(state, o) > k)
    });
    let hero = type_id.to_be_bytes()[0].is_ascii_uppercase();
    if let Some(units) = state.group(group) {
        match at {
            Some(i) => units.insert(i, u),
            None if hero => units.insert(0, u),
            None => units.push(u),
        }
    }
}

fn in_range(unit: &Unit, x: u32, y: u32, radius: u32) -> bool {
    let (dx, dy) = (sub(unit.x, x), sub(unit.y, y));
    f(add(mul(dx, dx), mul(dy, dy))) <= f(mul(radius, radius))
}

pub(super) fn location(state: &State, handle: Option<u32>) -> Option<(u32, u32)> {
    match state.objects.get(&handle?)? {
        Object::Location { x, y } => Some((*x, *y)),
        _ => None,
    }
}

pub(super) fn rect(state: &State, handle: Option<u32>) -> Option<[u32; 4]> {
    match state.objects.get(&handle?)? {
        Object::Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        } => Some([*min_x, *min_y, *max_x, *max_y]),
        _ => None,
    }
}

fn destructable(state: &State, handle: Option<u32>) -> Option<&Destructable> {
    match state.objects.get(&handle?)? {
        Object::Destructable(d) => Some(d),
        _ => None,
    }
}

fn boolexpr(state: &Rc<RefCell<State>>, expr: BoolExpr) -> Value {
    Value::Handle(Some(state.borrow_mut().create(Object::BoolExpr(expr))))
}

fn destroy(state: &Rc<RefCell<State>>, h: Option<u32>, is_kind: impl Fn(&Object) -> bool) {
    let mut state = state.borrow_mut();
    if let Some(h) = h
        && state.objects.get(&h).is_some_and(is_kind)
    {
        state.objects.remove(&h);
    }
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    // Boolexprs.
    native!(
        add,
        state,
        "native And takes boolexpr operandA, boolexpr operandB returns boolexpr",
        |vm, state, args| {
            let expr = BoolExpr::And(handle(&args[0]), handle(&args[1]));
            Ok(Some(boolexpr(state, expr)))
        }
    );
    native!(
        add,
        state,
        "native Or takes boolexpr operandA, boolexpr operandB returns boolexpr",
        |vm, state, args| {
            let expr = BoolExpr::Or(handle(&args[0]), handle(&args[1]));
            Ok(Some(boolexpr(state, expr)))
        }
    );
    native!(
        add,
        state,
        "native Not takes boolexpr operand returns boolexpr",
        |vm, state, args| { Ok(Some(boolexpr(state, BoolExpr::Not(handle(&args[0]))))) }
    );
    for declaration in [
        "native DestroyCondition takes conditionfunc c returns nothing",
        "native DestroyFilter takes filterfunc f returns nothing",
        "native DestroyBoolExpr takes boolexpr e returns nothing",
    ] {
        native!(add, state, declaration, |vm, state, args| {
            destroy(state, handle(&args[0]), |o| {
                matches!(o, Object::BoolExpr(_))
            });
            Ok(None)
        });
    }

    // Forces.
    native!(
        add,
        state,
        "native CreateForce takes nothing returns force",
        |vm, state, args| {
            let h = state.borrow_mut().create(Object::Force(BTreeSet::new()));
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native DestroyForce takes force whichForce returns nothing",
        |vm, state, args| {
            destroy(state, handle(&args[0]), |o| matches!(o, Object::Force(_)));
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native ForceAddPlayer takes force whichForce, player whichPlayer returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(id) = state.player_id(handle(&args[1]))
                && let Some(force) = state.force(handle(&args[0]))
            {
                force.insert(id);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native ForceRemovePlayer takes force whichForce, player whichPlayer returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(id) = state.player_id(handle(&args[1]))
                && let Some(force) = state.force(handle(&args[0]))
            {
                force.remove(&id);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native ForceClear takes force whichForce returns nothing",
        |vm, state, args| {
            if let Some(force) = state.borrow_mut().force(handle(&args[0])) {
                force.clear();
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native IsPlayerInForce takes player whichPlayer, force whichForce returns boolean",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = state.player_id(handle(&args[0]));
            let inside = id.is_some_and(|id| {
                state
                    .force(handle(&args[1]))
                    .is_some_and(|f| f.contains(&id))
            });
            Ok(Some(Value::Boolean(inside)))
        }
    );
    for (declaration, counted) in [
        (
            "native ForceEnumPlayers takes force whichForce, boolexpr filter returns nothing",
            false,
        ),
        (
            "native ForceEnumPlayersCounted takes force whichForce, boolexpr filter, integer countLimit returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let force = handle(&args[0]);
            let candidates: Vec<u32> = {
                let mut state = state.borrow_mut();
                let Some(players) = state.force(force) else {
                    return Ok(None);
                };
                players.clear();
                let present = state.setup.enumerable();
                present
                    .into_iter()
                    .map(|id| state.player_handle(id))
                    .collect()
            };
            let limit = counted.then(|| integer(&args[2]));
            let chosen = select(vm, state, candidates, handle(&args[1]), limit, |c, p| {
                Context {
                    filter_player: Some(p),
                    ..c
                }
            });
            let mut state = state.borrow_mut();
            let ids: Vec<i32> = chosen
                .into_iter()
                .filter_map(|p| state.player_id(Some(p)))
                .collect();
            if let Some(players) = state.force(force) {
                players.extend(ids);
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native ForForce takes force whichForce, code callback returns nothing",
        |vm, state, args| {
            let players: Vec<u32> = {
                let mut state = state.borrow_mut();
                let ids: Vec<i32> = state
                    .force(handle(&args[0]))
                    .map_or_else(Vec::new, |f| f.iter().copied().collect());
                ids.into_iter().map(|id| state.player_handle(id)).collect()
            };
            for_each(vm, state, players, code(&args[1]), |c, p| Context {
                enum_player: Some(p),
                ..c
            });
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetEnumPlayer takes nothing returns player",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.enum_player))) }
    );
    native!(
        add,
        state,
        "constant native GetFilterPlayer takes nothing returns player",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.filter_player))) }
    );

    // Groups.
    native!(
        add,
        state,
        "native CreateGroup takes nothing returns group",
        |vm, state, args| {
            let h = state.borrow_mut().create(Object::Group(Vec::new()));
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native DestroyGroup takes group whichGroup returns nothing",
        |vm, state, args| {
            // Its object of the table goes 0.0001 s later (c-SCC-1: 10710 in
            // MeleeClearExcessUnits, hub #15).
            if let Some(g) = handle(&args[0]) {
                let mut s = state.borrow_mut();
                if let Some(e) = s.table.group_objects.remove(&g) {
                    s.table.defer(e, "group");
                }
            }
            destroy(state, handle(&args[0]), |o| matches!(o, Object::Group(_)));
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GroupAddUnit takes group whichGroup, unit whichUnit returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(u) = handle(&args[1]).filter(|&u| state.unit(Some(u)).is_some()) {
                group_insert(&mut state, handle(&args[0]), u);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GroupRemoveUnit takes group whichGroup, unit whichUnit returns nothing",
        |vm, state, args| {
            let unit = handle(&args[1]);
            if let Some(units) = state.borrow_mut().group(handle(&args[0])) {
                units.retain(|&u| Some(u) != unit);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GroupClear takes group whichGroup returns nothing",
        |vm, state, args| {
            if let Some(units) = state.borrow_mut().group(handle(&args[0])) {
                units.clear();
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native FirstOfGroup takes group whichGroup returns unit",
        |vm, state, args| {
            let first = state
                .borrow_mut()
                .group(handle(&args[0]))
                .and_then(|units| units.first().copied());
            Ok(Some(Value::Handle(first)))
        }
    );
    native!(
        add,
        state,
        "constant native IsUnitInGroup takes unit whichUnit, group whichGroup returns boolean",
        |vm, state, args| {
            let unit = handle(&args[0]);
            let inside = unit.is_some()
                && state
                    .borrow_mut()
                    .group(handle(&args[1]))
                    .is_some_and(|units| units.iter().any(|&u| Some(u) == unit));
            Ok(Some(Value::Boolean(inside)))
        }
    );
    native!(
        add,
        state,
        "native ForGroup takes group whichGroup, code callback returns nothing",
        |vm, state, args| {
            // An object the first time its function is used as a callback
            // (c-SCC-1: MeleeClearExcessUnits' ForGroup, 1 then none, hub #15).
            // PROVISIONAL: shared with the actions' set of functions.
            {
                let mut s = state.borrow_mut();
                if super::code(&args[1]).is_some_and(|f| s.action_codes.insert(f)) {
                    let tick = s.table.now;
                    s.table.take(tick, "ForGroup");
                }
            }
            // PROVISIONAL: the group as it is when ForGroup starts, without
            // handles that no longer refer to units.
            let units: Vec<u32> = {
                let mut state = state.borrow_mut();
                let units = state.group(handle(&args[0])).cloned().unwrap_or_default();
                units
                    .into_iter()
                    .filter(|&u| state.unit(Some(u)).is_some())
                    .collect()
            };
            for_each(vm, state, units, code(&args[1]), |c, u| Context {
                enum_unit: Some(u),
                ..c
            });
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetEnumUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.enum_unit))) }
    );
    native!(
        add,
        state,
        "constant native GetFilterUnit takes nothing returns unit",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.filter_unit))) }
    );
    native!(
        add,
        state,
        "native GroupEnumUnitsOfPlayer takes group whichGroup, player whichPlayer, boolexpr filter returns nothing",
        |vm, state, args| {
            let owner = state.borrow().player_id(handle(&args[1]));
            group_enum(vm, state, handle(&args[0]), handle(&args[2]), None, |u| {
                Some(u.owner) == owner
            });
            Ok(None)
        }
    );
    for (declaration, counted) in [
        (
            "native GroupEnumUnitsInRange takes group whichGroup, real x, real y, real radius, boolexpr filter returns nothing",
            false,
        ),
        (
            "native GroupEnumUnitsInRangeCounted takes group whichGroup, real x, real y, real radius, boolexpr filter, integer countLimit returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let (x, y, radius) = (real(&args[1]), real(&args[2]), real(&args[3]));
            let limit = counted.then(|| integer(&args[5]));
            group_enum(vm, state, handle(&args[0]), handle(&args[4]), limit, |u| {
                in_range(u, x, y, radius)
            });
            Ok(None)
        });
    }
    for (declaration, counted) in [
        (
            "native GroupEnumUnitsInRangeOfLoc takes group whichGroup, location whichLocation, real radius, boolexpr filter returns nothing",
            false,
        ),
        (
            "native GroupEnumUnitsInRangeOfLocCounted takes group whichGroup, location whichLocation, real radius, boolexpr filter, integer countLimit returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let Some((x, y)) = location(&state.borrow(), handle(&args[1])) else {
                return Ok(None);
            };
            let radius = real(&args[2]);
            let limit = counted.then(|| integer(&args[4]));
            group_enum(vm, state, handle(&args[0]), handle(&args[3]), limit, |u| {
                in_range(u, x, y, radius)
            });
            Ok(None)
        });
    }
    for (declaration, counted) in [
        (
            "native GroupEnumUnitsInRect takes group whichGroup, rect r, boolexpr filter returns nothing",
            false,
        ),
        (
            "native GroupEnumUnitsInRectCounted takes group whichGroup, rect r, boolexpr filter, integer countLimit returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let Some([min_x, min_y, max_x, max_y]) = rect(&state.borrow(), handle(&args[1])) else {
                return Ok(None);
            };
            let limit = counted.then(|| integer(&args[3]));
            group_enum(vm, state, handle(&args[0]), handle(&args[2]), limit, |u| {
                (f(min_x)..=f(max_x)).contains(&f(u.x)) && (f(min_y)..=f(max_y)).contains(&f(u.y))
            });
            Ok(None)
        });
    }

    // Allies and enemies among the players in the game.
    for (declaration, allies) in [
        (
            "native ForceEnumAllies takes force whichForce, player whichPlayer, boolexpr filter returns nothing",
            true,
        ),
        (
            "native ForceEnumEnemies takes force whichForce, player whichPlayer, boolexpr filter returns nothing",
            false,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let force = handle(&args[0]);
            let candidates: Vec<u32> = {
                let mut state = state.borrow_mut();
                let Some(of) = state.player_id(handle(&args[1])) else {
                    return Ok(None);
                };
                let Some(players) = state.force(force) else {
                    return Ok(None);
                };
                players.clear();
                let present: Vec<i32> = state
                    .setup
                    .enumerable()
                    .into_iter()
                    .filter(|&id| state.setup.is_ally(of, id) == allies)
                    .collect();
                present
                    .into_iter()
                    .map(|id| state.player_handle(id))
                    .collect()
            };
            let chosen = select(vm, state, candidates, handle(&args[2]), None, |c, p| {
                Context {
                    filter_player: Some(p),
                    ..c
                }
            });
            let mut state = state.borrow_mut();
            let ids: Vec<i32> = chosen
                .into_iter()
                .filter_map(|p| state.player_id(Some(p)))
                .collect();
            if let Some(players) = state.force(force) {
                players.extend(ids);
            }
            Ok(None)
        });
    }

    // Destructables and the world bounds.
    native!(
        add,
        state,
        "native GetWorldBounds takes nothing returns rect",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let [min_x, min_y, max_x, max_y] = state.world_bounds;
            let h = state.create(Object::Rect {
                min_x,
                min_y,
                max_x,
                max_y,
            });
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native EnumDestructablesInRect takes rect r, boolexpr filter, code actionFunc returns nothing",
        |vm, state, args| {
            let candidates: Vec<u32> = {
                let state = state.borrow();
                let Some([min_x, min_y, max_x, max_y]) = rect(&state, handle(&args[0])) else {
                    return Ok(None);
                };
                state
                    .destructables
                    .iter()
                    .copied()
                    .filter(|&d| {
                        destructable(&state, Some(d)).is_some_and(|d| {
                            (f(min_x)..=f(max_x)).contains(&f(d.x))
                                && (f(min_y)..=f(max_y)).contains(&f(d.y))
                        })
                    })
                    .collect()
            };
            // SPEC-0006: the action runs right after its filter passed.
            filter_then_act(
                vm,
                state,
                candidates,
                handle(&args[1]),
                code(&args[2]),
                |c, d| Context {
                    filter_destructable: Some(d),
                    ..c
                },
                |c, d| Context {
                    enum_destructable: Some(d),
                    ..c
                },
            );
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetEnumDestructable takes nothing returns destructable",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.enum_destructable,
            )))
        }
    );
    native!(
        add,
        state,
        "constant native GetFilterDestructable takes nothing returns destructable",
        |vm, state, args| {
            Ok(Some(Value::Handle(
                state.borrow().context.filter_destructable,
            )))
        }
    );
    native!(
        add,
        state,
        "native GetDestructableTypeId takes destructable d returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let id = destructable(&state, handle(&args[0])).map_or(0, |d| d.type_id);
            Ok(Some(Value::Integer(id)))
        }
    );
    for (declaration, y) in [
        (
            "native GetDestructableX takes destructable d returns real",
            false,
        ),
        (
            "native GetDestructableY takes destructable d returns real",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let state = state.borrow();
            let at = destructable(&state, handle(&args[0])).map(|d| if y { d.y } else { d.x });
            Ok(Some(Value::Real(at.unwrap_or(0))))
        });
    }

    // Items, as destructables.
    native!(
        add,
        state,
        "native EnumItemsInRect takes rect r, boolexpr filter, code actionFunc returns nothing",
        |vm, state, args| {
            let candidates: Vec<u32> = {
                let state = state.borrow();
                let Some([min_x, min_y, max_x, max_y]) = rect(&state, handle(&args[0])) else {
                    return Ok(None);
                };
                state
                    .items
                    .iter()
                    .copied()
                    .filter(|h| match state.objects.get(h) {
                        Some(Object::Item(i)) => {
                            (f(min_x)..=f(max_x)).contains(&f(i.x))
                                && (f(min_y)..=f(max_y)).contains(&f(i.y))
                        }
                        _ => false,
                    })
                    .collect()
            };
            // PROVISIONAL: as for destructables.
            filter_then_act(
                vm,
                state,
                candidates,
                handle(&args[1]),
                code(&args[2]),
                |c, i| Context {
                    filter_item: Some(i),
                    ..c
                },
                |c, i| Context {
                    enum_item: Some(i),
                    ..c
                },
            );
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetEnumItem takes nothing returns item",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.enum_item))) }
    );
    native!(
        add,
        state,
        "constant native GetFilterItem takes nothing returns item",
        |vm, state, args| { Ok(Some(Value::Handle(state.borrow().context.filter_item))) }
    );

    // Units by the string id of their type, such as "footman".
    for (declaration, counted) in [
        (
            "native GroupEnumUnitsOfType takes group whichGroup, string unitname, boolexpr filter returns nothing",
            false,
        ),
        (
            "native GroupEnumUnitsOfTypeCounted takes group whichGroup, string unitname, boolexpr filter, integer countLimit returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let type_id = string(&args[1]).and_then(|n| state.borrow().data.unit_id(n));
            let limit = counted.then(|| integer(&args[3]));
            group_enum(vm, state, handle(&args[0]), handle(&args[2]), limit, |u| {
                Some(u.type_id) == type_id
            });
            Ok(None)
        });
    }

    // Locations and rects.
    native!(
        add,
        state,
        "native Location takes real x, real y returns location",
        |vm, state, args| {
            let (x, y) = (real(&args[0]), real(&args[1]));
            let h = state.borrow_mut().create(Object::Location { x, y });
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native RemoveLocation takes location whichLocation returns nothing",
        |vm, state, args| {
            // Its object of the table goes 0.0001 s later (c-SCC-1: 10830 of
            // a GetUnitLoc, hub #15).
            if let Some(l) = handle(&args[0]) {
                let mut s = state.borrow_mut();
                if let Some(e) = s.table.group_objects.remove(&l) {
                    s.table.defer(e, "location");
                }
            }
            destroy(state, handle(&args[0]), |o| {
                matches!(o, Object::Location { .. })
            });
            Ok(None)
        }
    );
    for (declaration, y) in [
        (
            "native GetLocationX takes location whichLocation returns real",
            false,
        ),
        (
            "native GetLocationY takes location whichLocation returns real",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let point = location(&state.borrow(), handle(&args[0]));
            Ok(Some(Value::Real(
                point.map_or(0, |(px, py)| if y { py } else { px }),
            )))
        });
    }
    native!(
        add,
        state,
        "native GetLocationZ takes location whichLocation returns real",
        |vm, state, args| {
            let state = state.borrow();
            let point = location(&state, handle(&args[0]));
            Ok(Some(Value::Real(
                point.map_or(0, |(x, y)| state.heights.at(x, y)),
            )))
        }
    );
    native!(
        add,
        state,
        "native Rect takes real minx, real miny, real maxx, real maxy returns rect",
        |vm, state, args| {
            let object = Object::Rect {
                min_x: real(&args[0]),
                min_y: real(&args[1]),
                max_x: real(&args[2]),
                max_y: real(&args[3]),
            };
            let h = state.borrow_mut().create(object);
            Ok(Some(Value::Handle(Some(h))))
        }
    );
    native!(
        add,
        state,
        "native RemoveRect takes rect whichRect returns nothing",
        |vm, state, args| {
            destroy(state, handle(&args[0]), |o| {
                matches!(o, Object::Rect { .. })
            });
            Ok(None)
        }
    );
    // PROVISIONAL: the centre is `mul(add(min, max), 0.5)`.
    for (declaration, axis) in [
        ("native GetRectCenterX takes rect whichRect returns real", 0),
        ("native GetRectCenterY takes rect whichRect returns real", 1),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let bounds = rect(&state.borrow(), handle(&args[0]));
            let centre =
                bounds.map_or(0, |b| mul(ow3_math::add(b[axis], b[axis + 2]), 0x3F00_0000));
            Ok(Some(Value::Real(centre)))
        });
    }
    native!(
        add,
        state,
        "native MoveLocation takes location whichLocation, real newX, real newY returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(Object::Location { x, y }) =
                handle(&args[0]).and_then(|h| state.objects.get_mut(&h))
            {
                *x = real(&args[1]);
                *y = real(&args[2]);
            }
            Ok(None)
        }
    );
    for (declaration, index) in [
        ("native GetRectMinX takes rect whichRect returns real", 0),
        ("native GetRectMinY takes rect whichRect returns real", 1),
        ("native GetRectMaxX takes rect whichRect returns real", 2),
        ("native GetRectMaxY takes rect whichRect returns real", 3),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let bounds = rect(&state.borrow(), handle(&args[0]));
            Ok(Some(Value::Real(bounds.map_or(0, |b| b[index]))))
        });
    }
}
