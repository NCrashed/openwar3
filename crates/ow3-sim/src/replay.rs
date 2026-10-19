//! Playing a replay (`ow3_formats::replay`) in the simulation: the map
//! starts with the replay's slots, and its time slots are played
//! one after the other, the actions of each player applied at the start of
//! the slot and the game then advanced by the slot's time.
//!
//! PROVISIONAL, until the specs say how the game does it:
//! - the objects of the actions are known only from a list of their
//!   creation numbers and types read from the game (the second id of an
//!   object is its creation number, SPEC-0024, which the simulation
//!   cannot compute yet);
//! - an order goes to every unit the player has selected;
//! - `smart` on a unit is an attack, and a move when the attack is not
//!   taken;
//! - the game runs in steps of 25 ms, and the actions of a slot are
//!   applied after the whole steps of the slots before (SPEC-0024 gives
//!   the checksum's time `B` and that the actions come after it; when
//!   the game applies them is inferred from that).

use std::collections::BTreeMap;

use ow3_formats::replay::{Action, Event, Replay, fold};
use ow3_jass::check::Program;

use crate::checksum::time_part;
use crate::game::players::LobbySlot;
use crate::game::{Game, order_name};
use crate::map::Map;

/// Milliseconds of game time a tick.
const TICK_MS: u64 = 5;
/// The object id of no object.
const NONE: [u32; 2] = [u32::MAX; 2];
/// The flag of an order given to a group of units (SPEC-0024, SPEC-0025).
const GROUP_ORDER: u16 = 0x0008;

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
    /// The creation numbers and types of the units, in creation order.
    ids: Vec<(u32, i32)>,
    /// The tick the game runs to in the current time slot.
    end: u64,
    /// What could not be played, in the order met.
    pub notes: Vec<String>,
    /// The state of the game when each turn's checksum is taken, the
    /// first for turn 1.
    pub marks: Vec<Marks>,
}

/// What of the game's state a turn checksum (SPEC-0024) sees, as far as
/// the simulation has it: the generators, the handles made, and folds of
/// the units, of those walking, of the buildings under construction and of
/// the players' selections. Positions
/// and life are not part of the checksum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub e: u32,
    pub r: u32,
    pub handles: u32,
    pub units: u32,
    pub walking: u32,
    pub constructing: u32,
    pub selections: u32,
}

impl Marks {
    /// The names of the parts that differ between `self` and `other`.
    pub fn changes(&self, other: &Marks) -> Vec<&'static str> {
        [
            ("E", self.e != other.e),
            ("R", self.r != other.r),
            ("handles", self.handles != other.handles),
            ("units", self.units != other.units),
            ("walking", self.walking != other.walking),
            ("constructing", self.constructing != other.constructing),
            ("selections", self.selections != other.selections),
        ]
        .into_iter()
        .filter(|&(_, changed)| changed)
        .map(|(name, _)| name)
        .collect()
    }
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
        game.start_lobby(&lobby, local).map_err(|e| e.to_string())?;
        Ok(Playback {
            game,
            events: replay.events.clone(),
            next: 0,
            ms: 0,
            players,
            selections: BTreeMap::new(),
            groups: BTreeMap::new(),
            ids: Vec::new(),
            end: 0,
            notes: Vec::new(),
            marks: Vec::new(),
        })
    }

    /// Keeps the marks of the checksum of the turn being played.
    fn mark(&mut self) {
        let (e, r) = self.game.generator_sums();
        let mut marks = Marks {
            e,
            r,
            handles: self.game.handles_made(),
            ..Marks::default()
        };
        for u in self.game.units() {
            marks.units = fold(marks.units, u.handle);
            if u.constructing {
                marks.constructing = fold(marks.constructing, u.handle);
            }
            // A group member starts and stops walking without objects of
            // its own (the cluster keeps the path).
            if u.walking && !self.game.grouped(u.handle) {
                marks.walking = fold(marks.walking, u.handle);
            }
        }
        for (player, units) in &self.selections {
            marks.selections = fold(marks.selections, u32::from(*player));
            for &u in units {
                marks.selections = fold(marks.selections, u);
            }
        }
        self.marks.push(marks);
    }

    /// Game time played, in milliseconds.
    pub fn ms(&self) -> u64 {
        self.ms
    }

    /// Plays the events up to and including the next time slot; false at
    /// the end of the replay.
    ///
    /// The game runs in steps of 25 ms (SPEC-0024, `B`): when it has run
    /// the whole steps of the slots before, the turn's checksum is taken,
    /// the slot's actions are applied, and the game runs on to the last
    /// whole step within the slot's end.
    pub fn step_slot(&mut self) -> bool {
        if !self.begin_slot() {
            return false;
        }
        while self.game.ticks() < self.end {
            self.game.step();
        }
        true
    }

    /// Plays one tick of the game, starting the next time slot when the
    /// game has run those before; false at the end of the replay.
    pub fn step_tick(&mut self) -> bool {
        if !self.begin_due_slots() {
            return false;
        }
        self.game.step();
        true
    }

    /// Starts the time slots whose turn the game has reached, applying
    /// their actions before the next tick; false at the end of the replay.
    pub fn begin_due_slots(&mut self) -> bool {
        while self.game.ticks() >= self.end {
            if !self.begin_slot() {
                return false;
            }
        }
        true
    }

    /// Starts the next time slot: the turn's checksum, then its actions.
    fn begin_slot(&mut self) -> bool {
        while let Some(event) = self.events.get(self.next).cloned() {
            self.next += 1;
            if let Event::TimeSlot { ms, actions, .. } = event {
                // The turn's checksum comes before its actions (SPEC-0024,
                // "When actions take effect"); after a slot of 0 ms it
                // repeats the one before.
                match self.marks.last() {
                    Some(&last) if ms == 0 => self.marks.push(last),
                    _ => self.mark(),
                }
                for (player, list) in actions {
                    for action in list {
                        self.apply(player, &action);
                    }
                }
                self.ms += u64::from(ms);
                self.end = u64::from(time_part(self.ms)) / TICK_MS;
                return true;
            }
        }
        false
    }

    /// Plays the whole replay.
    pub fn run(&mut self) {
        while self.step_slot() {}
    }

    /// Tells the playback the units of the replay's object ids: their
    /// creation numbers and types, as read from the game (`<replay>.units.txt`,
    /// see [`read_object_ids`]).
    pub fn set_object_ids(&mut self, ids: Vec<(u32, i32)>) {
        self.ids = ids;
        self.ids.sort_unstable();
    }

    /// Reads the list of object ids next to the replay at `path`
    /// (`name.units.txt` for `name.w3g`), if there is one.
    pub fn load_object_ids(&mut self, path: &std::path::Path) {
        if let Ok(text) = std::fs::read_to_string(path.with_extension("units.txt")) {
            self.set_object_ids(read_object_ids(&text));
        }
    }

    /// The unit an action means by `ids`, if there is one: the object with
    /// that creation number (the second id, SPEC-0024) is the unit of the
    /// same type that has as many units of its type created before it.
    ///
    /// PROVISIONAL: the creation numbers cannot be computed (SPEC-0024,
    /// open question 6), so only the units given by `set_object_ids` are
    /// known.
    fn unit(&self, ids: [u32; 2]) -> Option<u32> {
        let &(_, type_id) = self.ids.iter().find(|&&(n, _)| n == ids[1])?;
        let before = self
            .ids
            .iter()
            .filter(|&&(n, t)| t == type_id && n < ids[1])
            .count();
        self.game
            .units()
            .iter()
            .filter(|u| u.type_id == type_id)
            .nth(before)
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
            Action::PointOrder {
                flags, order, x, y, ..
            } => {
                // A build order carries the building's type as its order
                // (SPEC-0026). PROVISIONAL: with several workers selected
                // the first that takes it builds.
                if order_name(*order as i32).is_none() {
                    if !selected
                        .iter()
                        .any(|&u| self.game.issue_build_order(u, *order as i32, *x, *y))
                    {
                        self.note(format!("build order {order:#x} not taken"));
                    }
                    return;
                }
                let Some(name) = self.order(*order) else {
                    return;
                };
                self.point_order(&selected, *flags, name, *x, *y);
            }
            Action::TargetOrder {
                flags,
                order,
                x,
                y,
                target,
                ..
            } => {
                let Some(name) = self.order(*order) else {
                    return;
                };
                let Some(target) = (*target != NONE).then(|| self.unit(*target)).flatten() else {
                    self.point_order(&selected, *flags, name, *x, *y);
                    return;
                };
                for unit in selected {
                    if name == "smart" {
                        if !self.game.issue_target_order(unit, "attack", target) {
                            self.game.issue_target_order(unit, "move", target);
                        }
                    } else {
                        self.game.issue_target_order(unit, name, target);
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

    /// A point order to the units `selected`: with the group flag of the
    /// action they walk in formation (SPEC-0025).
    fn point_order(&mut self, selected: &[u32], flags: u16, name: &str, x: f32, y: f32) {
        if flags & GROUP_ORDER != 0 {
            self.game.issue_group_point_order(selected, name, x, y);
        } else {
            for &unit in selected {
                self.game.issue_point_order(unit, name, x, y);
            }
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

/// The units of a replay's object ids from a list read from the game: a
/// line per unit with its creation number followed by its type code,
/// such as `827 830 hfoo player 0` (entry, creation number, type, owner)
/// or `7dc0014 827 830 hfoo` (with the address first); lines starting
/// with `#` are comments. The creation numbers and types.
pub fn read_object_ids(text: &str) -> Vec<(u32, i32)> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let w: Vec<&str> = line.split_whitespace().collect();
            let at = w
                .iter()
                .position(|word| word.len() == 4 && word.as_bytes()[0].is_ascii_alphabetic())?;
            let code: [u8; 4] = w[at].as_bytes().try_into().ok()?;
            Some((
                w.get(at.checked_sub(1)?)?.parse().ok()?,
                i32::from_be_bytes(code),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids() {
        let hfoo = i32::from_be_bytes(*b"hfoo");
        assert_eq!(read_object_ids("7dc0014 827 830 hfoo\n"), [(830, hfoo)]);
        assert_eq!(
            read_object_ids("# entry creation type owner\n845 848 hfoo neutral hostile\n"),
            [(848, hfoo)]
        );
    }
}
