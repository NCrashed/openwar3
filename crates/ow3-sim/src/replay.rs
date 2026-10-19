//! Playing a replay (`ow3_formats::replay`) in the simulation: the map
//! starts with the replay's slots and seed, and its time slots are played
//! one after the other, the actions of each player applied at the start of
//! the slot and the game then advanced by the slot's time.
//!
//! PROVISIONAL, until the specs say how the game does it:
//! - the objects of the actions are matched with units by the first of
//!   their two ids taken as the unit's handle;
//! - an order goes to every unit the player has selected;
//! - `smart` on a unit is an attack, and a move when the attack is not
//!   taken;
//! - the seed reseeds the generators as `SetRandomSeed` does;
//! - the actions of a slot come before its ticks, and a slot of `ms`
//!   milliseconds runs the ticks whose 5 ms end within it.

use std::collections::BTreeMap;

use ow3_formats::replay::{Action, Event, Replay};
use ow3_jass::check::Program;

use crate::game::players::LobbySlot;
use crate::game::{Game, order_name};
use crate::map::Map;

/// Milliseconds of game time a tick.
const TICK_MS: u64 = 5;
/// The object id of no object.
const NONE: [u32; 2] = [u32::MAX; 2];

/// A replay being played.
pub struct Playback<'a> {
    pub game: Game<'a>,
    events: Vec<Event>,
    next: usize,
    /// Game time of the slots played, in milliseconds.
    ms: u64,
    /// The game's player of each replay player id.
    players: BTreeMap<u8, i32>,
    /// What each replay player has selected, and its control groups.
    selections: BTreeMap<u8, Vec<u32>>,
    groups: BTreeMap<(u8, u8), Vec<u32>>,
    /// What could not be played, in the order met.
    pub notes: Vec<String>,
}

impl<'a> Playback<'a> {
    /// Starts `map` (compiled into `program`) as `replay` set it up.
    pub fn new(
        program: &'a Program,
        map: &'a Map,
        replay: &Replay,
    ) -> Result<Playback<'a>, String> {
        let mut game = Game::load_map(program, map).map_err(|e| e.to_string())?;
        let name = |id: u8| {
            replay
                .players
                .iter()
                .find(|p| p.id == id)
                .map_or_else(String::new, |p| p.name.clone())
        };
        let mut players = BTreeMap::new();
        let lobby: Vec<LobbySlot> = replay
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let used = s.status == 2;
                if used && !s.computer {
                    players.insert(s.player, i as i32);
                }
                LobbySlot {
                    user: used && !s.computer,
                    computer: used && s.computer,
                    race_preference: i32::from(s.race & 0x2F),
                    team: i32::from(s.team),
                    color: i32::from(s.color),
                    handicap: f32::from(s.handicap),
                    name: if s.computer {
                        String::new()
                    } else {
                        name(s.player)
                    },
                }
            })
            .collect();
        let local = replay
            .players
            .iter()
            .find(|p| p.host)
            .and_then(|p| players.get(&p.id))
            .copied()
            .unwrap_or(0);
        game.start_lobby(&lobby, local, replay.seed)
            .map_err(|e| e.to_string())?;
        Ok(Playback {
            game,
            events: replay.events.clone(),
            next: 0,
            ms: 0,
            players,
            selections: BTreeMap::new(),
            groups: BTreeMap::new(),
            notes: Vec::new(),
        })
    }

    /// Game time played, in milliseconds.
    pub fn ms(&self) -> u64 {
        self.ms
    }

    /// Plays the events up to and including the next time slot; false at
    /// the end of the replay.
    pub fn step_slot(&mut self) -> bool {
        while let Some(event) = self.events.get(self.next).cloned() {
            self.next += 1;
            if let Event::TimeSlot { ms, actions } = event {
                for (player, list) in actions {
                    for action in list {
                        self.apply(player, &action);
                    }
                }
                let before = self.ms / TICK_MS;
                self.ms += u64::from(ms);
                for _ in before..self.ms / TICK_MS {
                    self.game.step();
                }
                return true;
            }
        }
        false
    }

    /// Plays the whole replay.
    pub fn run(&mut self) {
        while self.step_slot() {}
    }

    /// The unit an action means by `ids`, if there is one.
    fn unit(&self, ids: [u32; 2]) -> Option<u32> {
        self.game
            .units()
            .iter()
            .find(|u| u.handle == ids[0])
            .map(|u| u.handle)
    }

    fn units(&mut self, ids: &[[u32; 2]]) -> Vec<u32> {
        let found: Vec<u32> = ids.iter().filter_map(|&i| self.unit(i)).collect();
        if found.len() < ids.len() {
            self.note(format!("unknown objects among {ids:x?}"));
        }
        found
    }

    fn note(&mut self, text: String) {
        self.notes.push(format!("{} ms: {text}", self.ms));
    }

    fn apply(&mut self, player: u8, action: &Action) {
        if !self.players.contains_key(&player) {
            self.note(format!("an action of player {player}, who has no slot"));
            return;
        }
        let selected = self.selections.get(&player).cloned().unwrap_or_default();
        match action {
            Action::Select { mode, units } => {
                let units = self.units(units);
                let selection = self.selections.entry(player).or_default();
                match mode {
                    1 => selection.extend(
                        units
                            .iter()
                            .filter(|u| !selection.contains(u))
                            .copied()
                            .collect::<Vec<_>>(),
                    ),
                    2 => selection.retain(|u| !units.contains(u)),
                    _ => {}
                }
            }
            Action::AssignGroup { group, units } => {
                let units = self.units(units);
                self.groups.insert((player, *group), units);
            }
            Action::SelectGroup(group) => {
                let units = self
                    .groups
                    .get(&(player, *group))
                    .cloned()
                    .unwrap_or_default();
                self.selections.insert(player, units);
            }
            Action::Order { order, .. } => {
                let Some(name) = self.order(*order) else {
                    return;
                };
                for unit in selected {
                    self.game.issue_immediate_order(unit, name);
                }
            }
            Action::PointOrder { order, x, y, .. } => {
                let Some(name) = self.order(*order) else {
                    return;
                };
                for unit in selected {
                    self.game.issue_point_order(unit, name, *x, *y);
                }
            }
            Action::TargetOrder {
                order,
                x,
                y,
                target,
                ..
            } => {
                let Some(name) = self.order(*order) else {
                    return;
                };
                let target = (*target != NONE).then(|| self.unit(*target)).flatten();
                for unit in selected {
                    match target {
                        Some(t) if name == "smart" => {
                            if !self.game.issue_target_order(unit, "attack", t) {
                                self.game.issue_target_order(unit, "move", t);
                            }
                        }
                        Some(t) => {
                            self.game.issue_target_order(unit, name, t);
                        }
                        None => {
                            self.game.issue_point_order(unit, name, *x, *y);
                        }
                    }
                }
            }
            // Speed, pauses, saving, the interface and the like change
            // nothing in the game's state that is simulated.
            Action::Pause
            | Action::Resume
            | Action::SetSpeed(_)
            | Action::IncreaseSpeed
            | Action::DecreaseSpeed
            | Action::SaveGame(_)
            | Action::SaveFinished(_)
            | Action::PreSubselection
            | Action::SelectSubgroup { .. }
            | Action::EscapePressed
            | Action::MinimapSignal { .. } => {}
            other => self.note(format!("not played: {other:?}")),
        }
    }

    /// The name of order `id`, or a note that it is not known.
    fn order(&mut self, id: u32) -> Option<&'static str> {
        let name = order_name(id as i32);
        if name.is_none() {
            self.note(format!("unknown order {id:#x}"));
        }
        name
    }
}
