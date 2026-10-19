//! The units of a running simulation, as the viewer draws them.
//!
//! The map's script runs in [`ow3_sim::game::Game`], fed with real time;
//! every frame the units it has are drawn where they stand, facing their
//! way, at the ground height `GetLocationZ` gives. A dead unit plays its
//! Death sequence once from the moment it died and stays on its last
//! frame.
//!
//! PROVISIONAL: models come from `Units\unitUI.slk` (`file`,
//! `modelScale`), flying units at their `moveHeight`; custom types of the map, decay and
//! hidden units' corpses are not handled.

use std::collections::BTreeMap;
use std::path::Path;

use ow3_formats::mpq::ArchiveChain;
use ow3_sim::game::{Game, Relation, UnitView};
use ow3_sim::map::Map;

use crate::camera::Mat4;
use crate::models::UnitPose;
use crate::objects::Placement;

/// Milliseconds of game time per tick.
const TICK_MS: u64 = 5;
/// The order of a worker building (`repair`, SPEC-0026).
const ORDER_REPAIR: i32 = 852_024;
/// A click on the ground this near a tree is on the tree (PROVISIONAL:
/// trees are not picked by their models).
const TREE_PICK: f32 = 64.0;

/// One unit to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitDraw {
    pub model: String,
    pub team: u8,
    pub matrix: Mat4,
    /// What it is doing.
    pub pose: UnitPose,
    /// Its share of its maximum life, for a living unit that has any.
    pub life: Option<f32>,
    /// The unit's handle, which picks its phase.
    pub handle: u32,
    /// Its selection scale, and how the local player sees it.
    pub selection: f32,
    pub relation: Option<Relation>,
    /// A colour its model is multiplied with: white for units.
    pub tint: [f32; 3],
}

/// The player whose view the viewer shows.
const LOCAL_PLAYER: i32 = 0;

/// A map running in the simulation.
pub struct Sim {
    game: Game<'static>,
    models: BTreeMap<i32, crate::objects::UnitLook>,
    /// Since when each unit walks, and when and after how long the walk
    /// of each unit that stopped ended, in ticks.
    walks: BTreeMap<u32, u64>,
    halts: BTreeMap<u32, (u64, u64)>,
    /// Confirmations of orders being shown.
    effects: Vec<Effect>,
    /// The models of `TargetPointConfirm` and `TargetUnitConfirm`, with the
    /// length of their Stand sequence in milliseconds.
    point_confirm: Option<(String, u64)>,
    unit_confirm: Option<(String, u64)>,
    /// The colours of `[SelectionCircle]`, for the confirmations too.
    colours: crate::circles::Settings,
    /// The archives models are loaded from while it runs.
    pub chain: ArchiveChain,
    /// The console over the view.
    pub hud: crate::hud::Hud,
    /// The ground point under the mouse, where a building waiting for its
    /// place is shown.
    pub ghost: Option<[f32; 2]>,
    /// Hours of a day and the seconds of animation they span
    /// (`DayHours`, `ScaledAnimTime` of `Units\MiscData.txt`), for the
    /// time of day indicator (SPEC-0028).
    day_animation: (f32, f32),
}

/// A model played once where an order went.
#[derive(Clone, Debug, PartialEq)]
struct Effect {
    model: String,
    at: [f32; 3],
    scale: f32,
    tint: [f32; 3],
    /// The tick it started at and its length in ticks.
    start: u64,
    length: u64,
}

/// `DayHours` and `ScaledAnimTime` of `[Misc]` in `Units\MiscData.txt`,
/// 24 and 60 when missing.
fn day_animation(chain: &mut ArchiveChain) -> (f32, f32) {
    let mut profile = ow3_formats::profile::Profile::default();
    for text in chain.read_all("Units\\MiscData.txt") {
        profile.add(&String::from_utf8_lossy(&text));
    }
    let number = |key: &str, default: f32| {
        profile
            .get("Misc", key)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default)
    };
    (number("DayHours", 24.0), number("ScaledAnimTime", 60.0))
}

/// A model named by `key` of `UI\war3skins.txt` (`[Default]`, as `.mdx`)
/// and the length of its Stand sequence in milliseconds.
fn skin_model(chain: &mut ArchiveChain, key: &str) -> Option<(String, u64)> {
    let text = chain.read("UI\\war3skins.txt")?;
    let text = String::from_utf8_lossy(&text);
    let mut inside = false;
    let mut path = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.eq_ignore_ascii_case("[Default]");
        } else if let Some(value) = line
            .split_once('=')
            .filter(|(k, _)| inside && k.trim() == key)
            .map(|(_, v)| v.trim())
        {
            path = Some(match value.strip_suffix(".mdl") {
                Some(stem) => format!("{stem}.mdx"),
                None => value.to_owned(),
            });
        }
    }
    let path = path?;
    let model = ow3_formats::mdx::Model::parse(&chain.read(&path)?).ok()?;
    let stand = model
        .sequences
        .iter()
        .find(|s| s.name.to_ascii_lowercase().starts_with("stand"))
        .or(model.sequences.first())?;
    Some((
        path,
        u64::from(stand.interval[1].saturating_sub(stand.interval[0])),
    ))
}

impl Sim {
    /// Loads the map and runs its `config` and `main`.
    pub fn start(game_data: &Path, map: &Path) -> Result<Sim, String> {
        let loaded = Map::load(game_data, map).map_err(|e| e.to_string())?;
        // The game borrows the map and its checked script for as long as
        // the viewer runs.
        let loaded: &'static Map = Box::leak(Box::new(loaded));
        let program = loaded.check().map_err(|e| e.to_string())?;
        let program = Box::leak(Box::new(program));
        let mut game = Game::load_map(program, loaded).map_err(|e| e.to_string())?;
        game.start().map_err(|e| e.to_string())?;
        let mut chain = ArchiveChain::open(game_data, map).map_err(|e| e.to_string())?;
        let models = crate::objects::unit_models(&mut chain);
        Ok(Sim {
            game,
            models,
            walks: BTreeMap::new(),
            halts: BTreeMap::new(),
            effects: Vec::new(),
            point_confirm: skin_model(&mut chain, "TargetPointConfirm"),
            unit_confirm: skin_model(&mut chain, "TargetUnitConfirm"),
            colours: crate::circles::Settings::load(&mut chain),
            // PROVISIONAL: the Human console; the local player's race is not
            // looked at yet.
            hud: crate::hud::Hud::load(&mut chain, "Human"),
            ghost: None,
            day_animation: day_animation(&mut chain),
            chain,
        })
    }

    /// A line about unit `handle`: type, owner, life, mana and attack.
    pub fn describe(&self, handle: u32) -> Option<String> {
        let u = self.game.units().into_iter().find(|u| u.handle == handle)?;
        let code = String::from_utf8_lossy(&u.type_id.to_be_bytes()).into_owned();
        let mut line = format!(
            "{code} #{handle} of player {}: life {:.1}/{:.0}",
            u.owner, u.life, u.max_life
        );
        if u.max_mana > 0.0 {
            line += &format!(", mana {:.1}/{:.0}", u.mana, u.max_mana);
        }
        if !u.alive {
            line += ", dead";
        } else if let Some(attack) = u.attack {
            line += &format!(", attacking #{}", attack.target);
        }
        Some(line)
    }

    /// A right click with `unit` selected, on unit `on` or else on the
    /// ground at `at`: an enemy is attacked, otherwise the unit moves to
    /// the point (`smart`); with `force` any unit clicked is attacked. A
    /// line about the order given, if any.
    pub fn command(
        &mut self,
        unit: u32,
        on: Option<u32>,
        at: Option<[f32; 3]>,
        force: bool,
    ) -> Option<String> {
        if let Some(target) = on.filter(|&t| t != unit && (force || self.game.hostile(unit, t))) {
            if !self.game.issue_target_order(unit, "attack", target) {
                return None;
            }
            // The confirmation on the target, as large as its selection, in
            // the colour of its circle.
            if let Some(u) = self.game.units().into_iter().find(|u| u.handle == target) {
                let scale = self.models.get(&u.type_id).map_or(1.0, |l| l.selection);
                let at = [u.x, u.y, self.game.ground_z(u.x, u.y)];
                let colour = match self.game.relation(LOCAL_PLAYER, target) {
                    Some(Relation::Friend) => self.colours.friend,
                    Some(Relation::Enemy) => self.colours.enemy,
                    _ => self.colours.neutral,
                };
                self.confirm(self.unit_confirm.clone(), at, scale, colour);
            }
            return Some(format!("#{unit}: attack #{target}"));
        }
        // A worker on its building under construction goes on with it.
        if let Some(target) = on.filter(|&t| t != unit)
            && self.game.issue_target_order(unit, "smart", target)
        {
            return Some(format!("#{unit}: smart on #{target}"));
        }
        let [x, y, z] = at?;
        // A worker on a tree gathers from it (SPEC-0029).
        if let Some((tree, ..)) = self.game.tree_near(x, y, TREE_PICK)
            && self.game.issue_target_order(unit, "harvest", tree)
        {
            return Some(format!("#{unit}: harvest #{tree}"));
        }
        self.game.issue_point_order(unit, "smart", x, y);
        self.confirm(
            self.point_confirm.clone(),
            [x, y, z],
            1.0,
            self.colours.friend,
        );
        Some(format!("#{unit}: move to ({x:.0}, {y:.0})"))
    }

    /// Shows the confirmation `model` at `at` in the colour `colour`.
    fn confirm(
        &mut self,
        model: Option<(String, u64)>,
        at: [f32; 3],
        scale: f32,
        colour: [f32; 4],
    ) {
        if let Some((model, millis)) = model {
            self.effects.push(Effect {
                model,
                at,
                scale,
                tint: [colour[0], colour[1], colour[2]],
                start: self.game.ticks(),
                length: millis / TICK_MS,
            });
        }
    }

    /// `stop` for `unit`.
    pub fn stop(&mut self, unit: u32) {
        self.game.issue_immediate_order(unit, "stop");
    }

    /// The light models the script chose (SPEC-0017).
    pub fn day_night_models(&self) -> Option<(String, String)> {
        self.game.day_night_models()
    }

    /// The terrain fog the script set (SPEC-0017), if any.
    pub fn terrain_fog(&self) -> Option<ow3_sim::game::TerrainFog> {
        self.game.terrain_fog()
    }

    /// The game time in seconds: 0.005 s a tick.
    pub fn game_seconds(&self) -> f32 {
        self.game.ticks() as f32 * 0.005
    }

    /// The interface over the view for this frame, with `selected` and the
    /// mouse at `mouse` (interface coordinates).
    pub fn interface(&self, selected: Option<u32>, mouse: Option<[f32; 2]>) -> crate::ui::Frame {
        let s = selected.and_then(|h| self.selected(h));
        self.hud.frame(self.resources(), s.as_ref(), mouse)
    }

    /// Unit `handle` as the console shows it.
    pub fn selected(&self, handle: u32) -> Option<crate::hud::Selected> {
        let u = self.game.units().into_iter().find(|u| u.handle == handle)?;
        if !u.alive {
            return None;
        }
        Some(crate::hud::Selected {
            handle,
            type_id: u.type_id,
            own: u.owner == LOCAL_PLAYER,
            life: u.life,
            max_life: u.max_life,
            mana: u.mana,
            max_mana: u.max_mana,
            stats: self.game.unit_stats(handle),
            carrying: u.carrying.is_some(),
        })
    }

    /// Carries out `command` of the command card for `unit`, at `point` on
    /// the ground for one that takes a point. A line about it.
    pub fn run(
        &mut self,
        unit: u32,
        command: crate::hud::Command,
        point: Option<[f32; 2]>,
    ) -> Option<String> {
        use crate::hud::{Card, Command};
        let g = &mut self.game;
        let done = match (command, point) {
            (Command::BuildMenu, _) => {
                self.hud.card = Card::Build;
                return None;
            }
            (Command::Back, _) => {
                self.hud.card = Card::Main;
                return None;
            }
            (Command::Stop, _) => g.issue_immediate_order(unit, "stop"),
            (Command::Hold, _) => g.issue_immediate_order(unit, "holdposition"),
            (Command::CancelTrain, _) => g.issue_immediate_order(unit, "cancel"),
            (Command::ReturnGoods, _) => g.issue_immediate_order(unit, "returnresources"),
            // On the ground: the tree there (a mine is clicked as a unit).
            (Command::Gather, Some([x, y])) => match g.tree_near(x, y, TREE_PICK) {
                Some((tree, ..)) => g.issue_target_order(unit, "harvest", tree),
                None => false,
            },
            (Command::Train(t), _) => g.issue_train_order(unit, t),
            (Command::Move, Some([x, y])) => g.issue_point_order(unit, "move", x, y),
            (Command::Attack, Some([x, y])) => g.issue_point_order(unit, "attack", x, y),
            (Command::Patrol, Some([x, y])) => g.issue_point_order(unit, "patrol", x, y),
            (Command::Rally, Some([x, y])) => g.issue_point_order(unit, "setrally", x, y),
            (Command::Build(t), Some([x, y])) => {
                let (x, y) = g.building_site(t, x, y);
                self.hud.card = Card::Main;
                g.issue_build_order(unit, t, x, y)
            }
            _ => return None,
        };
        Some(format!(
            "#{unit}: {command:?}{} {}",
            point.map_or_else(String::new, |[x, y]| format!(" at ({x:.0}, {y:.0})")),
            if done { "taken" } else { "refused" }
        ))
    }

    /// The portrait model of unit `h` and its team (SPEC-0028): the
    /// `_portrait` file next to its model if there is one, else the model
    /// itself, and the orc peon's portrait for a unit without a model.
    pub fn portrait(&self, h: u32) -> Option<(String, u8)> {
        let u = self.game.units().into_iter().find(|u| u.handle == h)?;
        let team = u8::try_from(u.owner).ok().filter(|&t| t < 16).unwrap_or(0);
        let model = match self.models.get(&u.type_id) {
            Some(look) => {
                let base = look
                    .model
                    .strip_suffix(".mdx")
                    .or_else(|| look.model.strip_suffix(".MDX"))
                    .unwrap_or(&look.model);
                let portrait = format!("{base}_portrait.mdx");
                if self.chain.contains(&portrait) {
                    portrait
                } else {
                    look.model.clone()
                }
            }
            None => "units\\orc\\peon\\peon_portrait.mdx".to_owned(),
        };
        Some((model, team))
    }

    /// Carries out `command` of the command card for `unit` on unit
    /// `target` clicked; `None` when the command takes no unit, and the
    /// ground point is used.
    pub fn run_on(
        &mut self,
        unit: u32,
        command: crate::hud::Command,
        target: u32,
    ) -> Option<String> {
        use crate::hud::Command;
        let name = match command {
            Command::Attack => "attack",
            Command::Move => "move",
            Command::Gather => "harvest",
            _ => return None,
        };
        let done = self.game.issue_target_order(unit, name, target);
        Some(format!(
            "#{unit}: {command:?} on #{target} {}",
            if done { "taken" } else { "refused" }
        ))
    }

    /// The living tree nearest to (`x`, `y`) within `radius`.
    pub fn tree_near(&self, x: f32, y: f32, radius: f32) -> Option<(u32, f32, f32)> {
        self.game.tree_near(x, y, radius)
    }

    /// Orders worker `unit` to gather from mine or tree `target`
    /// (SPEC-0029); whether it took the order.
    pub fn harvest(&mut self, unit: u32, target: u32) -> bool {
        self.game.issue_target_order(unit, "harvest", target)
    }

    /// The local player's gold, lumber and food for the resource bar.
    pub fn resources(&self) -> crate::hud::Resources {
        let state = |s| self.game.player_state(LOCAL_PLAYER, s);
        crate::hud::Resources {
            gold: state(1),
            lumber: state(2),
            food_used: state(5),
            food_cap: state(4),
        }
    }

    /// The time of day in hours.
    pub fn time_of_day(&self) -> f32 {
        self.game.time_of_day()
    }

    /// The time of day indicator's model and its time in its Stand
    /// sequence in milliseconds: `h / DayHours * ScaledAnimTime` seconds
    /// (SPEC-0028).
    pub fn time_indicator(&self) -> Option<(String, i32)> {
        let model = self.hud.time_indicator_model()?;
        let (hours, seconds) = self.day_animation;
        let t = self.time_of_day() / hours * seconds * 1000.0;
        Some((model, t as i32))
    }

    /// Lets `micros` of real time pass.
    pub fn advance(&mut self, micros: u64) {
        self.game.advance(micros);
    }

    /// The units to draw now.
    /// The units of the game as it stands.
    pub fn units(&self) -> Vec<ow3_sim::game::UnitView> {
        self.game.units()
    }

    pub fn draws(&mut self) -> Vec<UnitDraw> {
        let now = self.game.ticks();
        let mut draws = Vec::new();
        for u in self.game.units() {
            let pose = self.pose(&u, now);
            if u.hidden {
                continue;
            }
            let Some(look) = self.models.get(&u.type_id) else {
                continue;
            };
            // The model is drawn at the centre of the unit's footprint
            // (SPEC-0023, "Where models are drawn").
            let shift = self
                .game
                .state
                .borrow()
                .data
                .units
                .get(&u.type_id)
                .map_or(0.0, |t| {
                    crate::objects::unit_model_shift(t.collision, t.building)
                });
            let (x, y) = (u.x - shift, u.y - shift);
            let placement = Placement {
                model: look.model.clone(),
                replaceable: None,
                team: u8::try_from(u.owner).ok().filter(|&t| t < 16).unwrap_or(0),
                position: [x, y, self.game.ground_z(x, y) + look.fly_height],
                angle: u.facing,
                scale: [look.scale; 3],
            };
            draws.push(UnitDraw {
                matrix: crate::models::placement_matrix(&placement),
                model: placement.model,
                team: placement.team,
                handle: u.handle,
                life: (u.alive && u.max_life > 0.0).then(|| (u.life / u.max_life).clamp(0.0, 1.0)),
                pose,
                selection: look.selection,
                relation: self.game.relation(LOCAL_PLAYER, u.handle),
                tint: [1.0; 3],
            });
        }
        // A building waiting for its place, on its grid under the mouse:
        // green where it may stand, red where not (SPEC-0026).
        // PROVISIONAL: the game's look of it is not specified.
        if let (Some(crate::hud::Command::Build(t)), Some([x, y])) = (self.hud.pending, self.ghost)
            && let Some(look) = self.models.get(&t)
        {
            let (x, y) = self.game.building_site(t, x, y);
            let allowed = self.game.place_allowed(LOCAL_PLAYER, t, x, y);
            let shift = self
                .game
                .state
                .borrow()
                .data
                .units
                .get(&t)
                .map_or(0.0, |d| {
                    crate::objects::unit_model_shift(d.collision, d.building)
                });
            let (x, y) = (x - shift, y - shift);
            let placement = Placement {
                model: look.model.clone(),
                replaceable: None,
                team: LOCAL_PLAYER as u8,
                position: [x, y, self.game.ground_z(x, y)],
                angle: 3.0 * std::f32::consts::FRAC_PI_2,
                scale: [look.scale; 3],
            };
            draws.push(UnitDraw {
                matrix: crate::models::placement_matrix(&placement),
                model: placement.model,
                team: placement.team,
                handle: 0,
                life: None,
                pose: UnitPose::Stand,
                selection: 0.0,
                relation: None,
                tint: if allowed {
                    [0.3, 1.0, 0.3]
                } else {
                    [1.0, 0.25, 0.25]
                },
            });
        }
        // The confirmations still playing, with handle 0, which no unit has.
        self.effects.retain(|e| now < e.start + e.length);
        for e in &self.effects {
            let placement = Placement {
                model: e.model.clone(),
                replaceable: None,
                team: 0,
                position: e.at,
                angle: 0.0,
                scale: [e.scale; 3],
            };
            draws.push(UnitDraw {
                matrix: crate::models::placement_matrix(&placement),
                model: placement.model,
                team: 0,
                handle: 0,
                life: None,
                pose: UnitPose::Once(millis(now - e.start)),
                selection: 0.0,
                relation: None,
                tint: e.tint,
            });
        }
        draws
    }

    /// What unit `u` is doing at tick `now`: dead, swinging, walking or
    /// standing. The start and end of walks are kept from frame to frame.
    fn pose(&mut self, u: &UnitView, now: u64) -> UnitPose {
        if u.walking {
            self.halts.remove(&u.handle);
            let since = *self.walks.entry(u.handle).or_insert(now);
            return match u.carrying {
                // With a load: Walk Gold or Walk Lumber (SPEC-0029).
                Some((load, _)) => UnitPose::Carry(millis(now - since), load),
                None => UnitPose::Walk(millis(now - since)),
            };
        }
        if let Some(since) = self.walks.remove(&u.handle) {
            self.halts.insert(u.handle, (now, now - since));
        }
        let pose = pose(u, now);
        match (pose, self.halts.get(&u.handle)) {
            (UnitPose::Stand, Some(&(stopped, walked))) => UnitPose::Halt {
                walked: millis(walked),
                since: millis(now - stopped),
            },
            _ => pose,
        }
    }
}

/// Ticks as milliseconds.
fn millis(ticks: u64) -> i32 {
    (ticks * TICK_MS).min(i32::MAX as u64) as i32
}

/// What unit `u` is doing at tick `now`: dead, swinging or standing. A
/// swing starts its weapon's damage point before each hit, so that the
/// hit lands at that point of the Attack sequence.
///
/// PROVISIONAL: the swing plays the Attack sequence at its own speed from
/// that moment; the game's link between the animation and `dmgpt1` is not
/// specified.
fn pose(u: &UnitView, now: u64) -> UnitPose {
    if let Some(died) = u.died_at {
        return UnitPose::Death(millis(now.saturating_sub(died)));
    }
    if u.order == ORDER_REPAIR && !u.walking {
        return UnitPose::Work(millis(now));
    }
    // A worker hitting a tree swings so that each hit lands at its damage
    // point (PROVISIONAL: SPEC-0029 does not name the sequence; Attack).
    if let Some((next, period, point)) = u.chopping {
        let swing = next.saturating_sub(point);
        let start = if now >= swing {
            swing
        } else {
            swing.saturating_sub(period)
        };
        return UnitPose::Attack(millis(now.saturating_sub(start)));
    }
    let Some(attack) = u.attack else {
        return UnitPose::Stand;
    };
    let swing = attack.next_hit.saturating_sub(attack.damage_point);
    let start = if now >= swing {
        swing
    } else {
        swing.saturating_sub(attack.cooldown)
    };
    if now >= start {
        UnitPose::Attack(millis(now - start))
    } else {
        UnitPose::Stand
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_sim::game::AttackView;

    fn unit(attack: Option<AttackView>, died_at: Option<u64>) -> UnitView {
        UnitView {
            handle: 1,
            type_id: 0,
            owner: 0,
            x: 0.0,
            y: 0.0,
            facing: 0.0,
            alive: died_at.is_none(),
            hidden: false,
            died_at,
            attack,
            life: 1.0,
            max_life: 1.0,
            mana: 0.0,
            max_mana: 0.0,
            walking: false,
            target: None,
            order: 0,
            constructing: false,
            carrying: None,
            resources: 0,
            chopping: None,
        }
    }

    /// A weapon with a damage point of 0.66 s and a cooldown of 1.5 s,
    /// ordered at tick 100: its first hit at tick 232.
    #[test]
    fn swings_end_at_their_hits() {
        let attack = AttackView {
            target: 2,
            next_hit: 232,
            cooldown: 300,
            damage_point: 132,
        };
        let u = unit(Some(attack), None);
        // The swing starts with the order and reaches the hit after 0.66 s.
        assert_eq!(pose(&u, 100), UnitPose::Attack(0));
        assert_eq!(pose(&u, 232), UnitPose::Attack(660));
        // After the hit the next one is 300 ticks on; until its swing
        // starts the last swing goes on (and the renderer lets the unit
        // stand once the Attack sequence is over).
        let after = unit(
            Some(AttackView {
                next_hit: 532,
                ..attack
            }),
            None,
        );
        assert_eq!(pose(&after, 300), UnitPose::Attack(1000));
        assert_eq!(pose(&after, 400), UnitPose::Attack(0));
        assert_eq!(pose(&unit(None, None), 5), UnitPose::Stand);
        assert_eq!(pose(&unit(None, Some(10)), 30), UnitPose::Death(100));
    }
}
