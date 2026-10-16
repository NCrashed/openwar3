//! Player slots, start locations, alliances and the map's names: what the
//! lobby and the map's `config` function set up.
//!
//! The neutral slots 12..15 are always playing (SPEC-0006).
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: without a lobby, the game is
//! single player. The local player is the first user slot of
//! `war3map.w3i` (player 0 without a map); the other slots playing are
//! the computer slots, of the map or set by `config`. Slots start from
//! `war3map.w3i`: controller from the slot type, race (human for a
//! selectable race), name with trigger strings resolved, start location by
//! player number, team by force. Slots the map does not list are users for
//! 0..11 and neutral for 12..15, named "Player <n>". A player is an ally of
//! itself and of the players it gives `ALLIANCE_PASSIVE`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use ow3_formats::map::strings::TriggerStrings;
use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use super::{Object, State, converted, handle, integer, real, string};
use crate::map::Map;

pub const SLOTS: i32 = 16;
/// Players 0..11; 12..15 are the neutral players.
pub const PLAYABLE: i32 = 12;

const CONTROL_USER: i32 = 0;
const CONTROL_COMPUTER: i32 = 1;
const CONTROL_RESCUABLE: i32 = 2;
const CONTROL_NEUTRAL: i32 = 3;
const RACE_HUMAN: i32 = 1;
const SLOT_EMPTY: i32 = 0;
const SLOT_PLAYING: i32 = 1;
const ALLIANCE_PASSIVE: i32 = 0;

#[derive(Clone, Debug)]
pub struct Slot {
    /// In the game: `PLAYER_SLOT_STATE_PLAYING`, otherwise empty.
    pub present: bool,
    pub controller: i32,
    pub race: i32,
    pub race_preference: i32,
    pub selectable: bool,
    pub team: i32,
    pub start_location: i32,
    pub color: i32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Setup {
    pub local_player: i32,
    pub slots: Vec<Slot>,
    /// Start locations by index, as bit patterns of reals.
    pub start_locations: BTreeMap<i32, (u32, u32)>,
    /// `(source, other, alliance type)` that are set.
    pub alliances: BTreeSet<(i32, i32, i32)>,
    pub player_count: i32,
    pub team_count: i32,
    pub map_name: String,
    pub map_description: String,
    pub strings: TriggerStrings,
}

impl Setup {
    /// Single player without a map: player 0 is the only player.
    pub fn single_player() -> Setup {
        let slots = (0..SLOTS)
            .map(|id| Slot {
                present: id == 0 || id >= PLAYABLE,
                controller: if id < 12 {
                    CONTROL_USER
                } else {
                    CONTROL_NEUTRAL
                },
                race: RACE_HUMAN,
                race_preference: 0,
                selectable: false,
                team: id,
                start_location: id,
                color: id,
                name: format!("Player {}", id + 1),
            })
            .collect();
        Setup {
            local_player: 0,
            slots,
            start_locations: BTreeMap::new(),
            alliances: BTreeSet::new(),
            player_count: 0,
            team_count: 0,
            map_name: String::new(),
            map_description: String::new(),
            strings: TriggerStrings::default(),
        }
    }

    /// Single player on `map`, before its `config` runs.
    pub fn from_map(map: &Map) -> Setup {
        let mut setup = Setup::single_player();
        setup.strings = map.strings.clone();
        setup.slots[0].present = false;
        let info = &map.info;
        setup.local_player = info
            .players
            .iter()
            .find(|p| p.kind == 1)
            .map_or(0, |p| p.number);
        for player in &info.players {
            let Some(slot) = usize::try_from(player.number)
                .ok()
                .and_then(|n| setup.slots.get_mut(n))
            else {
                continue;
            };
            slot.controller = match player.kind {
                2 => CONTROL_COMPUTER,
                3 => CONTROL_NEUTRAL,
                4 => CONTROL_RESCUABLE,
                _ => CONTROL_USER,
            };
            slot.race = if (1..=4).contains(&player.race) {
                player.race
            } else {
                RACE_HUMAN
            };
            slot.present = player.number == setup.local_player || player.kind == 2;
            slot.name = map.strings.display(&player.name).to_owned();
            setup.start_locations.insert(
                player.number,
                (player.start_x.to_bits(), player.start_y.to_bits()),
            );
        }
        for (team, force) in info.forces.iter().enumerate() {
            for slot in 0..SLOTS {
                if force.players & (1 << slot) != 0 {
                    setup.slots[slot as usize].team = team as i32;
                }
            }
        }
        setup.player_count = info.players.len() as i32;
        setup.team_count = info.forces.len() as i32;
        if let Some(slot) = setup.slots.get_mut(setup.local_player as usize) {
            slot.present = true;
        }
        setup
    }

    /// Players whose slot is playing.
    pub fn present(&self) -> Vec<i32> {
        (0..SLOTS)
            .filter(|&id| self.slots[id as usize].present)
            .collect()
    }

    /// What `ForceEnumPlayers` puts in a force: the players 0..11 whose
    /// slot is playing, users and computers; the neutral players 12..15
    /// are left out although their slots are playing (SPEC-0006).
    pub fn enumerable(&self) -> Vec<i32> {
        (0..PLAYABLE)
            .filter(|&id| self.slots[id as usize].present)
            .collect()
    }

    fn slot(&mut self, id: Option<i32>) -> Option<&mut Slot> {
        self.slots.get_mut(usize::try_from(id?).ok()?)
    }

    pub fn is_ally(&self, a: i32, b: i32) -> bool {
        a == b || self.alliances.contains(&(a, b, ALLIANCE_PASSIVE))
    }
}

/// The name of the native a declaration declares.
pub(super) fn native_name(declaration: &str) -> &str {
    declaration
        .split_whitespace()
        .find(|w| *w != "constant" && *w != "native")
        .unwrap_or("")
}

/// Wraps a native so that it sees trigger string references in its string
/// arguments resolved (SPEC-0003).
pub(super) fn resolving(f: NativeFn, state: Rc<RefCell<State>>) -> NativeFn {
    Rc::new(move |vm: &mut Vm, args: &[Value]| {
        let resolved: Vec<Value> = {
            let state = state.borrow();
            args.iter()
                .map(|arg| match arg {
                    Value::String(Some(text)) => Value::string(state.setup.strings.display(text)),
                    other => other.clone(),
                })
                .collect()
        };
        f(vm, &resolved)
    })
}

fn player(state: &State, value: &Value) -> Option<i32> {
    state.player_id(handle(value))
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    // Map and slot setup, called from `config`.
    native!(
        add,
        state,
        "native SetMapName takes string name returns nothing",
        |vm, state, args| {
            state.borrow_mut().setup.map_name = string(&args[0]).unwrap_or("").to_owned();
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetMapDescription takes string description returns nothing",
        |vm, state, args| {
            state.borrow_mut().setup.map_description = string(&args[0]).unwrap_or("").to_owned();
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetPlayers takes integer playercount returns nothing",
        |vm, state, args| {
            state.borrow_mut().setup.player_count = integer(&args[0]);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetPlayers takes nothing returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(state.borrow().setup.player_count))) }
    );
    native!(
        add,
        state,
        "native SetTeams takes integer teamcount returns nothing",
        |vm, state, args| {
            state.borrow_mut().setup.team_count = integer(&args[0]);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetTeams takes nothing returns integer",
        |vm, state, args| { Ok(Some(Value::Integer(state.borrow().setup.team_count))) }
    );
    native!(
        add,
        state,
        "native DefineStartLocation takes integer whichStartLoc, real x, real y returns nothing",
        |vm, state, args| {
            let at = (real(&args[1]), real(&args[2]));
            state
                .borrow_mut()
                .setup
                .start_locations
                .insert(integer(&args[0]), at);
            Ok(None)
        }
    );
    for (declaration, y) in [
        (
            "constant native GetStartLocationX takes integer whichStartLocation returns real",
            false,
        ),
        (
            "constant native GetStartLocationY takes integer whichStartLocation returns real",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let at = state
                .borrow()
                .setup
                .start_locations
                .get(&integer(&args[0]))
                .copied();
            Ok(Some(Value::Real(
                at.map_or(0, |(x, yy)| if y { yy } else { x }),
            )))
        });
    }
    native!(
        add,
        state,
        "constant native GetStartLocationLoc takes integer whichStartLocation returns location",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let at = state.setup.start_locations.get(&integer(&args[0])).copied();
            Ok(Some(Value::Handle(
                at.map(|(x, y)| state.create(Object::Location { x, y })),
            )))
        }
    );
    for declaration in [
        "native SetPlayerStartLocation takes player whichPlayer, integer startLocIndex returns nothing",
        "native ForcePlayerStartLocation takes player whichPlayer, integer startLocIndex returns nothing",
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            if let Some(slot) = state.setup.slot(id) {
                slot.start_location = integer(&args[1]);
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native GetPlayerStartLocation takes player whichPlayer returns integer",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let index = state.setup.slot(id).map_or(-1, |s| s.start_location);
            Ok(Some(Value::Integer(index)))
        }
    );

    // Slot properties: an integer field, set with a `Convert*` value or an
    // integer and read back the same way.
    type Field = fn(&mut Slot) -> &mut i32;
    let fields: [(&str, &str, Field, bool); 4] = [
        (
            "native SetPlayerController takes player whichPlayer, mapcontrol controlType returns nothing",
            "native GetPlayerController takes player whichPlayer returns mapcontrol",
            |s| &mut s.controller,
            true,
        ),
        (
            "native SetPlayerColor takes player whichPlayer, playercolor color returns nothing",
            "native GetPlayerColor takes player whichPlayer returns playercolor",
            |s| &mut s.color,
            true,
        ),
        (
            "native SetPlayerTeam takes player whichPlayer, integer whichTeam returns nothing",
            "native GetPlayerTeam takes player whichPlayer returns integer",
            |s| &mut s.team,
            false,
        ),
        (
            "native SetPlayerRacePreference takes player whichPlayer, racepreference whichRacePreference returns nothing",
            "native IsPlayerRacePrefSet takes player whichPlayer, racepreference pref returns boolean",
            |s| &mut s.race_preference,
            true,
        ),
    ];
    for (setter, getter, field, is_handle) in fields {
        native!(add, state, setter, |vm, state, args| {
            let value = if is_handle {
                handle(&args[1]).map(|h| h as i32)
            } else {
                Some(integer(&args[1]))
            };
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let local = state.setup.local_player;
            if let (Some(slot), Some(value)) = (state.setup.slot(id), value) {
                *field(slot) = value;
                // PROVISIONAL: in single player a slot that `config` gives
                // to a computer plays, and another user slot stays empty.
                if setter.contains("SetPlayerController") && id < Some(PLAYABLE) {
                    slot.present = id == Some(local) || value == CONTROL_COMPUTER;
                }
            }
            Ok(None)
        });
        if getter.contains("IsPlayerRacePrefSet") {
            native!(add, state, getter, |vm, state, args| {
                let mut state = state.borrow_mut();
                let id = player(&state, &args[0]);
                let pref = handle(&args[1]).map_or(0, |h| h as i32);
                let set = state
                    .setup
                    .slot(id)
                    .is_some_and(|s| s.race_preference & pref != 0);
                Ok(Some(Value::Boolean(set)))
            });
            continue;
        }
        native!(add, state, getter, |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let value = state.setup.slot(id).map(|s| *field(s));
            Ok(Some(match (value, is_handle) {
                (Some(v), true) => converted(v),
                (None, true) => Value::Handle(None),
                (v, false) => Value::Integer(v.unwrap_or(0)),
            }))
        });
    }
    native!(
        add,
        state,
        "native SetPlayerRaceSelectable takes player whichPlayer, boolean value returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            if let Some(slot) = state.setup.slot(id) {
                slot.selectable = args[1] == Value::Boolean(true);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetPlayerSelectable takes player whichPlayer returns boolean",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let selectable = state.setup.slot(id).is_some_and(|s| s.selectable);
            Ok(Some(Value::Boolean(selectable)))
        }
    );
    native!(
        add,
        state,
        "native GetPlayerSlotState takes player whichPlayer returns playerslotstate",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let present = state.setup.slot(id).map(|s| s.present);
            Ok(Some(match present {
                Some(true) => converted(SLOT_PLAYING),
                Some(false) => converted(SLOT_EMPTY),
                None => Value::Handle(None),
            }))
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerRace takes player whichPlayer returns race",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let race = state.setup.slot(id).map(|s| s.race);
            Ok(Some(race.map_or(Value::Handle(None), converted)))
        }
    );
    native!(
        add,
        state,
        "native SetPlayerName takes player whichPlayer, string name returns nothing",
        |vm, state, args| {
            let name = string(&args[1]).unwrap_or("").to_owned();
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            if let Some(slot) = state.setup.slot(id) {
                slot.name = name;
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native GetPlayerName takes player whichPlayer returns string",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let name = state.setup.slot(id).map(|s| s.name.clone());
            Ok(Some(match name {
                Some(name) => Value::string(&name),
                None => Value::String(None),
            }))
        }
    );
    native!(
        add,
        state,
        "constant native GetLocalPlayer takes nothing returns player",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = state.setup.local_player;
            Ok(Some(Value::Handle(Some(state.player_handle(id)))))
        }
    );

    // Alliances.
    native!(
        add,
        state,
        "native SetPlayerAlliance takes player sourcePlayer, player otherPlayer, alliancetype whichAllianceSetting, boolean value returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let (Some(a), Some(b), Some(kind)) = (
                player(&state, &args[0]),
                player(&state, &args[1]),
                handle(&args[2]),
            ) else {
                return Ok(None);
            };
            let key = (a, b, kind as i32);
            if args[3] == Value::Boolean(true) {
                state.setup.alliances.insert(key);
            } else {
                state.setup.alliances.remove(&key);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerAlliance takes player sourcePlayer, player otherPlayer, alliancetype whichAllianceSetting returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let set = match (
                player(&state, &args[0]),
                player(&state, &args[1]),
                handle(&args[2]),
            ) {
                (Some(a), Some(b), Some(kind)) => {
                    state.setup.alliances.contains(&(a, b, kind as i32))
                }
                _ => false,
            };
            Ok(Some(Value::Boolean(set)))
        }
    );
    for (declaration, ally) in [
        (
            "constant native IsPlayerAlly takes player whichPlayer, player otherPlayer returns boolean",
            true,
        ),
        (
            "constant native IsPlayerEnemy takes player whichPlayer, player otherPlayer returns boolean",
            false,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let state = state.borrow();
            let result = match (player(&state, &args[0]), player(&state, &args[1])) {
                (Some(a), Some(b)) => state.setup.is_ally(a, b) == ally,
                _ => false,
            };
            Ok(Some(Value::Boolean(result)))
        });
    }
}
