//! Player slots, start locations, alliances and the map's names: what the
//! lobby and the map's `config` function set up (SPEC-0008).
//!
//! Before `config` every slot has the game's defaults; `config` changes
//! them with its `SetPlayer...` calls; then the game fills the slots
//! ([`Setup::fill`]) and `main` runs. The neutral slots 12..15 always
//! play (SPEC-0006). `IsPlayerAlly(a, b)` is exactly the passive alliance
//! flag from `a` to `b`.
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: the local player's name is
//! "Player"; a race preference other than one of the four races (random,
//! for example) gives human; in a map without fixed player settings the
//! computer in slot 1 keeps the start location and race preference from
//! `config` and is human, and plays only when the map has two or more
//! players.

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
/// Player 15, `PLAYER_NEUTRAL_PASSIVE`.
const NEUTRAL_PASSIVE: i32 = 15;

const CONTROL_USER: i32 = 0;
const CONTROL_COMPUTER: i32 = 1;
const CONTROL_CREEP: i32 = 4;
const CONTROL_NONE: i32 = 5;
const RACE_HUMAN: i32 = 1;
const RACE_PREF_RANDOM: i32 = 32;
const SLOT_EMPTY: i32 = 0;
const SLOT_PLAYING: i32 = 1;
const SLOT_LEFT: i32 = 2;
const GAME_RESULT_VICTORY: i32 = 0;
const GAME_RESULT_DEFEAT: i32 = 1;
const EVENT_PLAYER_DEFEAT: i32 = 13;
const EVENT_PLAYER_VICTORY: i32 = 14;
const ALLIANCE_PASSIVE: i32 = 0;
const ALLIANCE_HELP_REQUEST: i32 = 1;
const ALLIANCE_HELP_RESPONSE: i32 = 2;
const ALLIANCE_SHARED_VISION: i32 = 5;
const PLAYER_STATE_GIVES_BOUNTY: i32 = 7;
const MAP_LOCK_ALLIANCE_CHANGES: i32 = 1024;
const MAP_LOCK_RANDOM_SEED: i32 = 32768;
/// `war3map.w3i` flag: fixed player settings.
const FIXED_PLAYER_SETTINGS: u32 = 0x20;
/// The name of the local player's profile.
const PROFILE_NAME: &str = "Player";
const COMPUTER_NAME: &str = "Computer (Normal)";

#[derive(Clone, Debug)]
pub struct Slot {
    /// In the game: `PLAYER_SLOT_STATE_PLAYING`, otherwise empty.
    pub present: bool,
    /// Removed from the game (`RemovePlayer`): `PLAYER_SLOT_STATE_LEFT`.
    pub left: bool,
    pub controller: i32,
    /// The race played, 0 before the slots are filled.
    pub race: i32,
    pub race_preference: i32,
    pub selectable: bool,
    pub team: i32,
    pub start_location: i32,
    pub color: i32,
    /// Bit pattern of a real.
    pub handicap: u32,
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
    /// Map flags that are set, as the bit masks of `ConvertMapFlag`.
    pub map_flags: i32,
    /// The map has fixed player settings (`war3map.w3i` flag 0x20).
    pub fixed_players: bool,
    /// Slot names of `war3map.w3i` with trigger strings resolved.
    map_names: BTreeMap<i32, String>,
    /// `(player, tech)`: the most a player may have, and the level
    /// researched.
    pub tech_max: BTreeMap<(i32, i32), i32>,
    pub tech_levels: BTreeMap<(i32, i32), i32>,
    /// `(player, state)`: values of `SetPlayerState`.
    pub player_states: BTreeMap<(i32, i32), i32>,
    /// Values of `SetFloatGameState` (bit patterns) and
    /// `SetIntegerGameState`.
    pub float_game_states: BTreeMap<i32, u32>,
    pub integer_game_states: BTreeMap<i32, i32>,
    /// The clock (SPEC-0017): the hours set last, the game time since the
    /// start (0.005 added each tick) and the game time they were set at.
    pub clock_base: u32,
    pub clock_time: u32,
    pub clock_start: u32,
    /// Whether a script set the time of day (`SetFloatGameState`).
    pub clock_set: bool,
    /// The clock stands (`SuspendTimeOfDay`).
    pub clock_suspended: bool,
}

/// A slot as a lobby set it (for a replay).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbySlot {
    /// Played by a person or by the computer; neither for an empty slot.
    pub user: bool,
    pub computer: bool,
    /// As `GetPlayerRacePreference` gives it: 1 human, 2 orc, 4 night elf,
    /// 8 undead, 32 random.
    pub race_preference: i32,
    pub team: i32,
    pub color: i32,
    pub handicap: f32,
    pub name: String,
}

fn default_name(id: i32) -> String {
    format!("Player {}", id + 1)
}

/// The race a preference gives: one of the four races, or human.
fn race_of(preference: i32) -> i32 {
    match preference {
        1 => 1,
        2 => 2,
        8 => 3,
        4 => 4,
        _ => RACE_HUMAN,
    }
}

impl Setup {
    /// The slots before `config` runs (SPEC-0008).
    pub fn before_config() -> Setup {
        let slots = (0..SLOTS)
            .map(|id| {
                let neutral = id >= PLAYABLE;
                Slot {
                    present: neutral,
                    left: false,
                    controller: if neutral { CONTROL_CREEP } else { CONTROL_NONE },
                    race: 0,
                    race_preference: RACE_PREF_RANDOM,
                    selectable: true,
                    team: if neutral { -1 } else { 0 },
                    start_location: -1,
                    color: id,
                    handicap: 1.0f32.to_bits(),
                    name: default_name(id),
                }
            })
            .collect();
        let mut alliances = BTreeSet::new();
        for p in 0..SLOTS {
            for kind in [
                ALLIANCE_PASSIVE,
                ALLIANCE_HELP_REQUEST,
                ALLIANCE_SHARED_VISION,
            ] {
                alliances.insert((p, p, kind));
            }
            alliances.insert((p, NEUTRAL_PASSIVE, ALLIANCE_PASSIVE));
            alliances.insert((NEUTRAL_PASSIVE, p, ALLIANCE_PASSIVE));
        }
        let player_states = (PLAYABLE..SLOTS)
            .map(|p| ((p, PLAYER_STATE_GIVES_BOUNTY), 1))
            .collect();
        Setup {
            local_player: 0,
            slots,
            start_locations: BTreeMap::new(),
            alliances,
            player_count: 0,
            team_count: 0,
            map_name: String::new(),
            map_description: String::new(),
            strings: TriggerStrings::default(),
            map_flags: 0,
            fixed_players: false,
            map_names: BTreeMap::new(),
            tech_max: BTreeMap::new(),
            tech_levels: BTreeMap::new(),
            player_states,
            // A map starts at 0:00; a melee map's 8:00 is its melee start's
            // (SPEC-0017).
            float_game_states: BTreeMap::from([(2, 0.0f32.to_bits())]),
            integer_game_states: BTreeMap::new(),
            clock_base: 0.0f32.to_bits(),
            clock_time: 0,
            clock_start: 0,
            clock_set: false,
            clock_suspended: false,
        }
    }

    /// Single player without a map, with the slots filled: player 0 plays
    /// as a user, for scripts tested without a map.
    pub fn single_player() -> Setup {
        let mut setup = Setup::before_config();
        setup.slots[0].controller = CONTROL_USER;
        setup.player_count = 1;
        setup.fixed_players = true;
        setup.fill();
        setup
    }

    /// `map` before its `config` runs.
    pub fn from_map(map: &Map) -> Setup {
        let mut setup = Setup::before_config();
        setup.strings = map.strings.clone();
        setup.fixed_players = map.info.flags & FIXED_PLAYER_SETTINGS != 0;
        setup.map_names = map
            .info
            .players
            .iter()
            .map(|p| (p.number, map.strings.display(&p.name).to_owned()))
            .collect();
        setup
    }

    /// The game fills the slots after `config` and before `main`, as
    /// single player (SPEC-0008).
    pub fn fill(&mut self) {
        if self.fixed_players {
            // The first user slot is the local player; computer slots
            // play; other user slots stay empty.
            self.local_player = (0..PLAYABLE)
                .find(|&id| self.slots[id as usize].controller == CONTROL_USER)
                .unwrap_or(0);
            for id in 0..PLAYABLE {
                let local = id == self.local_player;
                let slot = &mut self.slots[id as usize];
                slot.present = local || slot.controller == CONTROL_COMPUTER;
                slot.race = race_of(slot.race_preference);
                slot.name = if local {
                    PROFILE_NAME.to_owned()
                } else if slot.present {
                    self.map_names
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| default_name(id))
                } else {
                    default_name(id)
                };
            }
        } else {
            // Melee: the local player in slot 0 and one computer in slot 1,
            // both with a random race.
            self.local_player = 0;
            let computer = self.player_count >= 2;
            for id in 0..PLAYABLE {
                let slot = &mut self.slots[id as usize];
                slot.present = id == 0 || (id == 1 && computer);
                slot.race_preference = RACE_PREF_RANDOM;
                slot.race = race_of(RACE_PREF_RANDOM);
                slot.name = default_name(id);
                if slot.present {
                    slot.controller = if id == 0 {
                        CONTROL_USER
                    } else {
                        CONTROL_COMPUTER
                    };
                    slot.team = id;
                }
            }
            self.slots[0].name = PROFILE_NAME.to_owned();
            if computer {
                self.slots[1].name = COMPUTER_NAME.to_owned();
            }
        }
        self.finish_fill();
    }

    /// Fills the slots as a lobby set them, for a replay: `lobby[i]` is
    /// player `i`, and `local` the player who saved it. PROVISIONAL: a
    /// random race becomes the race `race_of` gives it, as when the game
    /// fills the slots itself.
    pub fn fill_lobby(&mut self, lobby: &[LobbySlot], local: i32) {
        self.local_player = local;
        for id in 0..PLAYABLE {
            let slot = &mut self.slots[id as usize];
            match lobby.get(id as usize).filter(|l| l.user || l.computer) {
                Some(l) => {
                    slot.present = true;
                    slot.controller = if l.computer {
                        CONTROL_COMPUTER
                    } else {
                        CONTROL_USER
                    };
                    slot.race_preference = l.race_preference;
                    slot.race = race_of(l.race_preference);
                    slot.team = l.team;
                    slot.color = l.color;
                    slot.handicap = l.handicap.to_bits();
                    slot.name = l.name.clone();
                }
                None => {
                    slot.present = false;
                    slot.name = default_name(id);
                }
            }
        }
        self.finish_fill();
    }

    /// What filling the slots ends with, either way.
    fn finish_fill(&mut self) {
        for id in 0..PLAYABLE {
            let slot = &mut self.slots[id as usize];
            if !slot.present {
                slot.team = -1;
            }
        }
        self.map_flags |= MAP_LOCK_RANDOM_SEED;
        if self.fixed_players {
            self.map_flags |= MAP_LOCK_ALLIANCE_CHANGES;
        }
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

    /// The players that see with the units of `giver` (SPEC-0032,
    /// "Allied vision"): itself and those it gives shared vision, in
    /// increasing order.
    pub fn vision_receivers(&self, giver: i32) -> Vec<i32> {
        let mut out: Vec<i32> = self
            .alliances
            .range((giver, i32::MIN, i32::MIN)..=(giver, i32::MAX, i32::MAX))
            .filter(|&&(_, _, kind)| kind == ALLIANCE_SHARED_VISION)
            .map(|&(_, b, _)| b)
            .collect();
        if !out.contains(&giver) {
            out.push(giver);
            out.sort_unstable();
        }
        out
    }

    /// `IsPlayerAlly(a, b)`: the passive alliance flag from `a` to `b`.
    pub fn is_ally(&self, a: i32, b: i32) -> bool {
        self.alliances.contains(&(a, b, ALLIANCE_PASSIVE))
    }

    /// Whether units of `helper` answer a call for help of units of
    /// `caller` (SPEC-0019, "Retaliation and call for help"): the same
    /// player, or `helper` passive and with help response toward `caller`,
    /// which asks for help from `helper`. PROVISIONAL: the game was tested
    /// with all of these in both directions, and with help response alone
    /// (no answer); which flags are needed is open.
    pub fn helps(&self, helper: i32, caller: i32) -> bool {
        helper == caller
            || (self.is_ally(helper, caller)
                && self
                    .alliances
                    .contains(&(helper, caller, ALLIANCE_HELP_RESPONSE))
                && self
                    .alliances
                    .contains(&(caller, helper, ALLIANCE_HELP_REQUEST)))
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

/// Runs the triggers of player event `event` of player `id`.
fn player_event(vm: &mut Vm, state: &Rc<RefCell<State>>, event: i32, id: i32) {
    let (registrations, handle) = {
        let mut s = state.borrow_mut();
        (s.events.clone(), s.player_handle(id))
    };
    super::begin_event(state);
    for registration in &registrations {
        if let super::widgets::Registration::Player {
            trigger,
            player,
            event: e,
        } = *registration
            && e == event
            && player == id
        {
            let context = super::Context {
                event_id: Some(event),
                triggering_player: Some(handle),
                ..super::Context::default()
            };
            super::fire_trigger(vm, state, trigger, context);
        }
    }
    super::end_event(state);
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
            if let (Some(slot), Some(value)) = (state.setup.slot(id), value) {
                *field(slot) = value;
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
            let present = state.setup.slot(id).map(|s| (s.present, s.left));
            Ok(Some(match present {
                Some((_, true)) => converted(SLOT_LEFT),
                Some((true, _)) => converted(SLOT_PLAYING),
                Some((false, _)) => converted(SLOT_EMPTY),
                None => Value::Handle(None),
            }))
        }
    );
    native!(
        add,
        state,
        "native TriggerRegisterPlayerEvent takes trigger whichTrigger, player whichPlayer, playerevent whichPlayerEvent returns event",
        |vm, state, args| {
            let player = state.borrow().player_id(handle(&args[1]));
            let (Some(trigger), Some(player), Some(event)) =
                (handle(&args[0]), player, handle(&args[2]))
            else {
                return Ok(Some(Value::Handle(None)));
            };
            let registration = super::widgets::Registration::Player {
                trigger,
                player,
                event: event as i32,
            };
            Ok(Some(super::widgets::register_event(state, registration)))
        }
    );
    native!(
        add,
        state,
        "native RemovePlayer takes player whichPlayer, playergameresult gameResult returns nothing",
        |vm, state, args| {
            let Some(id) = state.borrow().player_id(handle(&args[0])) else {
                return Ok(None);
            };
            // Its defeat or victory event first (c-SCF-1: player 1's defeat
            // before its great hall's death event, hub #15).
            let event = match handle(&args[1]).map(|r| r as i32) {
                Some(GAME_RESULT_VICTORY) => Some(EVENT_PLAYER_VICTORY),
                Some(GAME_RESULT_DEFEAT) => Some(EVENT_PLAYER_DEFEAT),
                _ => None,
            };
            if let Some(event) = event {
                player_event(vm, state, event, id);
            }
            // The victor stays in the game with its units (c-SCD-1: player 0
            // after its victory, slot 1, its units its own).
            // PROVISIONAL: a tie and a neutral result are not read.
            if event != Some(EVENT_PLAYER_DEFEAT) {
                return Ok(None);
            }
            // Its slot is left (c-SCF-1, hub #15). (Blizzard.j gives its
            // units to the neutral victim before, with SetUnitOwner.)
            // PROVISIONAL: its objects and when the slot changes in the tick
            // are not read.
            let mut s = state.borrow_mut();
            if let Some(slot) = s.setup.slot(Some(id)) {
                slot.left = true;
            }
            Ok(None)
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

    // Tech limits and research, player states and game states, kept as
    // scripts set them. PROVISIONAL: nothing changes them on its own (no
    // research, income or day and night yet); a tech without a limit reads
    // -1 (no limit) and every other value reads 0 until set.
    native!(
        add,
        state,
        "constant native SetPlayerTechMaxAllowed takes player whichPlayer, integer techid, integer maximum returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(p) = player(&state, &args[0]) {
                let key = (p, integer(&args[1]));
                state.setup.tech_max.insert(key, integer(&args[2]));
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerTechMaxAllowed takes player whichPlayer, integer techid returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let value = player(&state, &args[0])
                .and_then(|p| state.setup.tech_max.get(&(p, integer(&args[1]))))
                .copied()
                .unwrap_or(-1);
            Ok(Some(Value::Integer(value)))
        }
    );
    for (declaration, adds) in [
        (
            "constant native SetPlayerTechResearched takes player whichPlayer, integer techid, integer setToLevel returns nothing",
            false,
        ),
        (
            "constant native AddPlayerTechResearched takes player whichPlayer, integer techid, integer levels returns nothing",
            true,
        ),
    ] {
        native!(add, state, declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(p) = player(&state, &args[0]) {
                let level = state
                    .setup
                    .tech_levels
                    .entry((p, integer(&args[1])))
                    .or_insert(0);
                *level = if adds {
                    level.wrapping_add(integer(&args[2]))
                } else {
                    integer(&args[2])
                };
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "constant native GetPlayerTechResearched takes player whichPlayer, integer techid, boolean specificonly returns boolean",
        |vm, state, args| {
            let state = state.borrow();
            let level = player(&state, &args[0])
                .and_then(|p| state.setup.tech_levels.get(&(p, integer(&args[1]))))
                .copied()
                .unwrap_or(0);
            Ok(Some(Value::Boolean(level > 0)))
        }
    );
    native!(
        add,
        state,
        "native SetPlayerState takes player whichPlayer, playerstate whichPlayerState, integer value returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let (Some(p), Some(s)) = (player(&state, &args[0]), handle(&args[1])) {
                state
                    .setup
                    .player_states
                    .insert((p, s as i32), integer(&args[2]));
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerState takes player whichPlayer, playerstate whichPlayerState returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let value = match (player(&state, &args[0]), handle(&args[1])) {
                // The upkeep rates follow the food used, in the same tick
                // (c-SCE-1: 0, 30 at 53, 60 at 83; lumber's stays 0; hub
                // #15).
                (Some(p), Some(13)) => {
                    let used = state.setup.player_states.get(&(p, 5)).copied().unwrap_or(0);
                    Some(100 - 10 * super::harvest::upkeep_tenths(used))
                }
                (Some(_), Some(14)) => Some(0),
                (Some(p), Some(s)) => state.setup.player_states.get(&(p, s as i32)).copied(),
                _ => None,
            };
            Ok(Some(Value::Integer(value.unwrap_or(0))))
        }
    );
    // The clock stands while suspended and goes on from its hour after
    // (SPEC-0017; c-SBS-1: noon all game long).
    native!(
        add,
        state,
        "native SuspendTimeOfDay takes boolean b returns nothing",
        |vm, state, args| {
            let b = matches!(args[0], Value::Boolean(true));
            let mut state = state.borrow_mut();
            let setup = &mut state.setup;
            if b != setup.clock_suspended {
                setup.clock_base = setup
                    .float_game_states
                    .get(&2)
                    .copied()
                    .unwrap_or(setup.clock_base);
                setup.clock_start = setup.clock_time;
            }
            setup.clock_suspended = b;
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native SetFloatGameState takes fgamestate whichFloatGameState, real value returns nothing",
        |vm, state, args| {
            if let Some(s) = handle(&args[0]) {
                let value = real(&args[1]);
                let mut state = state.borrow_mut();
                let setup = &mut state.setup;
                // The time of day before the call: as set, or 0.05 an hour
                // per second from the start (hub #14: 0.05 at 1.0 s).
                let old = match setup.float_game_states.get(&(s as i32)) {
                    Some(&v) if setup.clock_set => v,
                    _ => ow3_math::mul(setup.clock_time, 0x3D4C_CCCD),
                };
                setup.float_game_states.insert(s as i32, value);
                // The clock goes on from the time set (SPEC-0017).
                if s == 2 {
                    setup.clock_set = true;
                    setup.clock_base = value;
                    setup.clock_start = setup.clock_time;
                    let tick = state.table.now;
                    super::table::time_of_day_set(&mut state.table, tick, old, value);
                }
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetFloatGameState takes fgamestate whichFloatGameState returns real",
        |vm, state, args| {
            let state = state.borrow();
            let value = handle(&args[0])
                .and_then(|s| state.setup.float_game_states.get(&(s as i32)))
                .copied();
            Ok(Some(Value::Real(value.unwrap_or(0))))
        }
    );
    native!(
        add,
        state,
        "native SetIntegerGameState takes igamestate whichIntegerGameState, integer value returns nothing",
        |vm, state, args| {
            if let Some(s) = handle(&args[0]) {
                let value = integer(&args[1]);
                state
                    .borrow_mut()
                    .setup
                    .integer_game_states
                    .insert(s as i32, value);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetIntegerGameState takes igamestate whichIntegerGameState returns integer",
        |vm, state, args| {
            let state = state.borrow();
            let value = handle(&args[0])
                .and_then(|s| state.setup.integer_game_states.get(&(s as i32)))
                .copied();
            Ok(Some(Value::Integer(value.unwrap_or(0))))
        }
    );
    // 1.27b runs The Frozen Throne: VERSION_FROZEN_THRONE.
    native!(
        add,
        state,
        "native VersionGet takes nothing returns version",
        |vm, state, args| { Ok(Some(converted(1))) }
    );

    // Map flags (bit masks) and handicaps.
    native!(
        add,
        state,
        "native SetMapFlag takes mapflag whichMapFlag, boolean value returns nothing",
        |vm, state, args| {
            let flag = handle(&args[0]).map_or(0, |h| h as i32);
            let mut state = state.borrow_mut();
            if args[1] == Value::Boolean(true) {
                state.setup.map_flags |= flag;
            } else {
                state.setup.map_flags &= !flag;
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native IsMapFlagSet takes mapflag whichMapFlag returns boolean",
        |vm, state, args| {
            let flag = handle(&args[0]).map_or(0, |h| h as i32);
            let set = flag != 0 && state.borrow().setup.map_flags & flag == flag;
            Ok(Some(Value::Boolean(set)))
        }
    );
    native!(
        add,
        state,
        "constant native SetPlayerHandicap takes player whichPlayer, real handicap returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            if let Some(slot) = state.setup.slot(id) {
                slot.handicap = real(&args[1]);
            }
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "constant native GetPlayerHandicap takes player whichPlayer returns real",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            let id = player(&state, &args[0]);
            let handicap = state.setup.slot(id).map_or(0, |s| s.handicap);
            Ok(Some(Value::Real(handicap)))
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

/// Wraps a native that returns a string so that calling it in `config`
/// is noted (SPEC-0008).
pub(super) fn noting_config(f: NativeFn, state: Rc<RefCell<State>>) -> NativeFn {
    Rc::new(move |vm: &mut Vm, args: &[Value]| {
        {
            let mut state = state.borrow_mut();
            if state.in_config {
                state.config_called_string_native = true;
            }
        }
        f(vm, args)
    })
}
