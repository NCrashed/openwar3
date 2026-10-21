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
/// Ticks a destructable shows its hit (a tree's Stand Hit is 300 ms).
const HIT_TICKS: u64 = 60;
/// Milliseconds of a worker's swing at a tree (SPEC-0029).
const CHOP_SWING_MS: i32 = 1000;

/// One unit to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitDraw {
    pub model: String,
    /// A replaceable texture id and its path, for a tree.
    pub replaceable: Option<(u32, String)>,
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

/// A camp of the creeps there are when the map starts (SPEC-0028,
/// "Creep camps"): where its first creep stood, its creeps and the sum of
/// their levels.
#[derive(Clone, Debug, PartialEq)]
struct Camp {
    at: [f32; 2],
    creeps: Vec<u32>,
    levels: i32,
    /// The player saw its place with no living creeps: no circle since.
    cleared: std::cell::Cell<bool>,
}

/// A map running in the simulation.
pub struct Sim {
    game: Game<'static>,
    models: BTreeMap<i32, crate::objects::UnitLook>,
    /// Since when each unit walks, and when and after how long the walk
    /// of each unit that stopped ended, in ticks.
    walks: BTreeMap<u32, u64>,
    halts: BTreeMap<u32, (u64, u64)>,
    /// Buildings of other players as the local player last saw them,
    /// drawn while their place is under fog (SPEC-0032, "Buildings under
    /// fog"): how they were drawn and their minimap mark.
    remembered: BTreeMap<u32, (UnitDraw, [f32; 2], u8)>,
    /// The creep camps of the start, in the order of their first creeps.
    camps: Vec<Camp>,
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
    /// The minimap (SPEC-0028).
    pub minimap: crate::minimap::Minimap,
    /// Where the map placed its destructables, in the order the game made
    /// them.
    placed: Vec<[f32; 2]>,
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
        let mut sim = Sim {
            game,
            models,
            walks: BTreeMap::new(),
            halts: BTreeMap::new(),
            remembered: BTreeMap::new(),
            effects: Vec::new(),
            point_confirm: skin_model(&mut chain, "TargetPointConfirm"),
            unit_confirm: skin_model(&mut chain, "TargetUnitConfirm"),
            colours: crate::circles::Settings::load(&mut chain),
            // PROVISIONAL: the Human console; the local player's race is not
            // looked at yet.
            hud: crate::hud::Hud::load(&mut chain, "Human"),
            ghost: None,
            day_animation: day_animation(&mut chain),
            minimap: crate::minimap::Minimap::new(loaded),
            placed: loaded.destructables.iter().map(|d| [d.x, d.y]).collect(),
            chain,
            camps: Vec::new(),
        };
        // The minimap's icons and camp colours, and the camps of the
        // creeps there are at the start (SPEC-0028, "Creep camps").
        let misc: String = sim
            .chain
            .read_all("UI\\MiscData.txt")
            .iter()
            .map(|d| String::from_utf8_lossy(d).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        sim.minimap.look = crate::minimap::Look::load(|k| sim.hud.skin(k), &misc);
        let creeps: Vec<UnitView> = sim
            .game
            .units()
            .into_iter()
            .filter(|u| u.owner == ow3_sim::game::CREEPS && u.alive)
            .collect();
        let at: Vec<[f32; 2]> = creeps.iter().map(|u| [u.x, u.y]).collect();
        for (u, camp) in creeps.iter().zip(crate::minimap::camps(&at)) {
            if camp == sim.camps.len() {
                sim.camps.push(Camp {
                    at: [u.x, u.y],
                    creeps: Vec::new(),
                    levels: 0,
                    cleared: std::cell::Cell::new(false),
                });
            }
            sim.camps[camp].creeps.push(u.handle);
            sim.camps[camp].levels += sim.level(u.type_id);
        }
        Ok(sim)
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
            self.confirm_unit(target);
            return Some(format!("#{unit}: attack #{target}"));
        }
        // A worker on its building under construction goes on with it, on a
        // gold mine gathers from it.
        if let Some(target) = on.filter(|&t| t != unit)
            && self.game.issue_target_order(unit, "smart", target)
        {
            self.confirm_unit(target);
            return Some(format!("#{unit}: smart on #{target}"));
        }
        let [x, y, z] = at?;
        // A worker on a tree gathers from it (SPEC-0029).
        if let Some((tree, tx, ty)) = self.game.tree_near(x, y, TREE_PICK)
            && self.game.issue_target_order(unit, "harvest", tree)
        {
            self.confirm_tree(tx, ty);
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

    /// The confirmation of an order on unit `target`: as large as its
    /// selection, in the colour of its circle.
    fn confirm_unit(&mut self, target: u32) {
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
    }

    /// The confirmation of an order on the tree at (`x`, `y`), in the
    /// neutral colour.
    ///
    /// PROVISIONAL: the game's look of it is not specified.
    fn confirm_tree(&mut self, x: f32, y: f32) {
        let at = [x, y, self.game.ground_z(x, y)];
        self.confirm(self.unit_confirm.clone(), at, 1.0, self.colours.neutral);
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

    /// The interface over the view for this frame: `selection` with its
    /// lead's card and the group in the information panel, the mouse at
    /// `mouse` (interface coordinates).
    pub fn interface(
        &self,
        selection: &crate::selection::Selection,
        mouse: Option<[f32; 2]>,
    ) -> crate::ui::Frame {
        let kind = |h| self.kind(h);
        let lead = selection.lead(kind);
        let s = lead.and_then(|h| self.selected(h));
        let views = self.game.units();
        let share = |v: f32, max: f32| if max > 0.0 { v / max } else { 0.0 };
        let group: Vec<crate::hud::Member> = selection
            .cells(kind)
            .into_iter()
            .filter_map(|(h, active)| views.iter().find(|u| u.handle == h).map(|u| (u, active)))
            .map(|(u, active)| crate::hud::Member {
                type_id: u.type_id,
                life: share(u.life, u.max_life),
                active,
                mana: (u.max_mana > 0.0).then(|| share(u.mana, u.max_mana)),
            })
            .collect();
        self.hud.frame(self.resources(), s.as_ref(), &group, mouse)
    }

    /// The placed destructables in a pose of their own now: where the map
    /// placed each, and its pose (Stand Hit for a while after a hit, Death
    /// once it died, held at its end).
    ///
    /// PROVISIONAL: no spec gives how a destructable shows a hit.
    pub fn destructable_poses(&self) -> Vec<([f32; 2], UnitPose)> {
        let now = self.game.ticks();
        self.game
            .changed_destructables()
            .into_iter()
            .filter_map(|c| {
                let at = *self.placed.get(c.index)?;
                let pose = match (c.died_at, c.hit_at) {
                    (Some(t), _) => UnitPose::Death(millis(now.saturating_sub(t))),
                    (None, Some(t)) if now.saturating_sub(t) < HIT_TICKS => {
                        UnitPose::Hit(millis(now - t))
                    }
                    _ => return None,
                };
                Some((at, pose))
            })
            .collect()
    }

    /// The minimap's quads, with the outline of the ground `view` the
    /// camera sees (its corners in order round it).
    pub fn minimap_quads(&self, view: Option<[[f32; 2]; 4]>) -> Vec<crate::ui::Quad> {
        use crate::minimap::Mark;
        let ngol = i32::from_be_bytes(*b"ngol");
        // Neutral passive.
        const NEUTRAL: i32 = 15;
        let units: Vec<_> = self
            .game
            .units()
            .into_iter()
            .filter(|u| u.alive && !u.hidden)
            .collect();
        let mut marks: Vec<([f32; 2], Mark)> = Vec::new();
        let in_camp = |h: u32| self.camps.iter().any(|c| c.creeps.contains(&h));
        for u in &units {
            let building = self.game.unit_stats(u.handle).is_some_and(|s| s.building);
            // Gold mines and neutral buildings show under fog too.
            let landmark = u.type_id == ngol || (u.owner == NEUTRAL && building);
            if !landmark && !self.shown(u) {
                // A creep made later by a script: a weak camp of its own.
                if u.owner == ow3_sim::game::CREEPS && !in_camp(u.handle) {
                    marks.push(([u.x, u.y], Mark::Camp { levels: None }));
                }
                continue;
            }
            let mark = if u.type_id == ngol {
                Mark::Mine
            } else if u.owner == LOCAL_PLAYER {
                Mark::Own { building }
            } else if u.owner == NEUTRAL && building {
                Mark::NeutralBuilding
            } else {
                let Ok(player) = u8::try_from(u.owner) else {
                    continue;
                };
                Mark::Player {
                    player,
                    building,
                    relation: self.game.relation(LOCAL_PLAYER, u.handle),
                }
            };
            marks.push(([u.x, u.y], mark));
        }
        // Buildings under fog keep their marks as last seen.
        for (draw, at, player) in self.remembered.values() {
            if !self.sees(at[0], at[1]) {
                marks.push((
                    *at,
                    Mark::Player {
                        player: *player,
                        building: true,
                        relation: draw.relation,
                    },
                ));
            }
        }
        // The camps of the start whose creeps are not seen, at their first
        // creeps, with the colour and size of the start. A camp whose
        // place the player saw without living creeps shows no more.
        for camp in &self.camps {
            let living: Vec<&UnitView> = units
                .iter()
                .filter(|u| camp.creeps.contains(&u.handle))
                .collect();
            if living.is_empty() && self.sees(camp.at[0], camp.at[1]) {
                camp.cleared.set(true);
            }
            if !camp.cleared.get() && !living.iter().any(|u| self.shown(u)) {
                marks.push((
                    camp.at,
                    Mark::Camp {
                        levels: Some(camp.levels),
                    },
                ));
            }
        }
        let now = self.game_seconds();
        self.minimap
            .quads(&marks, view, self.vision().as_deref(), now)
    }

    /// The level of unit type `type_id`.
    fn level(&self, type_id: i32) -> i32 {
        self.game
            .state
            .borrow()
            .data
            .units
            .get(&type_id)
            .map_or(0, |t| t.level)
    }

    /// Unit `handle` as the console shows it.
    pub fn selected(&self, handle: u32) -> Option<crate::hud::Selected> {
        let u = self.game.units().into_iter().find(|u| u.handle == handle)?;
        if !u.alive {
            return None;
        }
        let training = self.game.training(handle);
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
            order: u.order,
            have: self.game.finished_types(u.owner),
            queue: training
                .as_ref()
                .map(|(q, _)| q.clone())
                .unwrap_or_default(),
            progress: if u.constructing {
                self.game.construction_progress(handle)
            } else {
                training.map(|(_, p)| p)
            },
            constructing: u.constructing,
            level: self
                .game
                .state
                .borrow()
                .data
                .units
                .get(&u.type_id)
                .filter(|t| t.race == "creeps")
                .map(|t| t.level),
            hero: self.game.hero_progress(handle),
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
                Some((tree, tx, ty)) => {
                    let done = g.issue_target_order(unit, "harvest", tree);
                    if done {
                        self.confirm_tree(tx, ty);
                    }
                    done
                }
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
        if done {
            self.confirm_unit(target);
        }
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

    /// A right click with `units` selected: on the ground a group order to
    /// all of them (SPEC-0025, flag 0x0008 of SPEC-0028), on a unit or tree
    /// each takes it.
    pub fn command_group(
        &mut self,
        units: &[u32],
        on: Option<u32>,
        at: Option<[f32; 3]>,
        force: bool,
        queued: bool,
    ) -> Option<String> {
        let (&first, rest) = units.split_first()?;
        // With Shift: queued behind the current orders (SPEC-0031).
        if queued {
            if let Some(target) = on {
                let name = if force || self.game.hostile(first, target) {
                    "attack"
                } else {
                    "smart"
                };
                for &u in units.iter().filter(|&&u| u != target) {
                    self.game.queue_target_order(u, name, target);
                }
                self.confirm_unit(target);
                return Some(format!("{} units: {name} on #{target} queued", units.len()));
            }
            let [x, y, z] = at?;
            if rest.is_empty() {
                self.game.queue_point_order(first, "smart", x, y);
            } else {
                self.game.queue_group_point_order(units, "smart", x, y);
            }
            self.confirm(
                self.point_confirm.clone(),
                [x, y, z],
                1.0,
                self.colours.friend,
            );
            return Some(format!("{} units: to ({x:.0}, {y:.0}) queued", units.len()));
        }
        if rest.is_empty() || on.is_some() {
            let mut line = None;
            for &u in units {
                let l = self.command(u, on, at, force);
                line = line.or(l);
            }
            return line;
        }
        let [x, y, z] = at?;
        if self.game.tree_near(x, y, TREE_PICK).is_some() {
            let mut line = None;
            for &u in units {
                let l = self.command(u, None, at, force);
                line = line.or(l);
            }
            return line;
        }
        self.game.issue_group_point_order(units, "smart", x, y);
        self.confirm(
            self.point_confirm.clone(),
            [x, y, z],
            1.0,
            self.colours.friend,
        );
        Some(format!(
            "#{first} and {} more: move to ({x:.0}, {y:.0})",
            rest.len()
        ))
    }

    /// A button of the command card with `all` units selected and `active`
    /// those of the active subgroup: moves, stops and the like go to all of
    /// them (a point order as a group order), the others to the active
    /// subgroup (training, building and the rally point to its first unit).
    pub fn run_group(
        &mut self,
        all: &[u32],
        active: &[u32],
        command: crate::hud::Command,
        point: Option<[f32; 2]>,
        queued: bool,
    ) -> Option<String> {
        use crate::hud::Command;
        let name = match command {
            Command::Move => Some("move"),
            Command::Attack => Some("attack"),
            Command::Patrol => Some("patrol"),
            _ => None,
        };
        // With Shift moves, attacks, patrols, stops and holds are queued
        // (SPEC-0031).
        if queued {
            let immediate = match command {
                Command::Stop => Some("stop"),
                Command::Hold => Some("holdposition"),
                _ => None,
            };
            match (name, point, immediate) {
                (Some(name), Some([x, y]), _) => {
                    if all.len() > 1 {
                        self.game.queue_group_point_order(all, name, x, y);
                    } else {
                        for &u in all {
                            self.game.queue_point_order(u, name, x, y);
                        }
                    }
                    return Some(format!("{} units: {command:?} queued", all.len()));
                }
                (_, _, Some(name)) => {
                    for &u in all {
                        self.game.queue_immediate_order(u, name);
                    }
                    return Some(format!("{} units: {command:?} queued", all.len()));
                }
                _ => {}
            }
        }
        if let (Some(name), Some([x, y]), true) = (name, point, all.len() > 1) {
            let done = self.game.issue_group_point_order(all, name, x, y);
            return Some(format!(
                "{} units: {command:?} at ({x:.0}, {y:.0}) {}",
                all.len(),
                if done { "taken" } else { "refused" }
            ));
        }
        let units: &[u32] = match command {
            Command::Move | Command::Attack | Command::Patrol | Command::Stop | Command::Hold => {
                all
            }
            Command::Gather | Command::ReturnGoods => active,
            _ => active.get(..1).unwrap_or(&[]),
        };
        let mut line = None;
        for &u in units {
            let l = self.run(u, command, point);
            line = line.or(l);
        }
        line
    }

    /// A button that takes a unit, clicked on `target`, for `units`.
    pub fn run_on_group(
        &mut self,
        units: &[u32],
        command: crate::hud::Command,
        target: u32,
    ) -> Option<String> {
        let mut line = None;
        for &u in units.iter().filter(|&&u| u != target) {
            let l = self.run_on(u, command, target);
            line = line.or(l);
        }
        line
    }

    /// What the selection needs of unit `h` (its type and the type's
    /// `prio`), `None` for one that is gone.
    pub fn kind(&self, h: u32) -> Option<crate::selection::Kind> {
        let u = self
            .game
            .units()
            .into_iter()
            .find(|u| u.handle == h && u.alive)?;
        Some(crate::selection::Kind {
            type_id: u.type_id,
            prio: self.game.type_prio(u.type_id),
        })
    }

    /// The units of the local player among `units`, without buildings
    /// unless there are only buildings: what a rectangle selects.
    ///
    /// PROVISIONAL: SPEC-0028 measured footmen, riflemen and knights only.
    pub fn selectable(&self, units: &[u32]) -> Vec<u32> {
        let all = self.game.units();
        let own: Vec<(u32, bool)> = units
            .iter()
            .filter_map(|&h| {
                let u = all.iter().find(|u| u.handle == h)?;
                (u.alive && !u.hidden && u.owner == LOCAL_PLAYER).then(|| {
                    let building = self.game.unit_stats(h).is_some_and(|s| s.building);
                    (h, building)
                })
            })
            .collect();
        let mobile: Vec<u32> = own.iter().filter(|(_, b)| !b).map(|&(h, _)| h).collect();
        let mut chosen = if mobile.is_empty() {
            own.iter().map(|&(h, _)| h).collect()
        } else {
            mobile
        };
        // The ones made first (handles grow with creation).
        chosen.sort_unstable();
        chosen
    }

    /// The local player's units of the type and owner of unit `h` among
    /// `units` (those on the screen), nearest to it first, at most 12, `h`
    /// first: what a double click or Ctrl and a click select (SPEC-0028).
    pub fn same_type(&self, h: u32, units: &[u32]) -> Vec<u32> {
        let all = self.game.units();
        let Some(c) = all.iter().find(|u| u.handle == h) else {
            return Vec::new();
        };
        let mut near: Vec<(f32, u32)> = units
            .iter()
            .filter_map(|&o| all.iter().find(|u| u.handle == o))
            .filter(|u| u.alive && !u.hidden && u.type_id == c.type_id && u.owner == c.owner)
            .map(|u| ((u.x - c.x).powi(2) + (u.y - c.y).powi(2), u.handle))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut out = vec![h];
        out.extend(near.into_iter().map(|(_, o)| o).filter(|&o| o != h));
        out.truncate(crate::selection::MOST);
        out
    }

    /// Where the map's script last put the camera, and how many times it
    /// did.
    pub fn camera_target(&self) -> Option<([f32; 2], u32)> {
        self.game.camera_target()
    }

    /// The camera bounds: least x and y, greatest x and y.
    pub fn camera_bounds(&self) -> [f32; 4] {
        self.game.camera_bounds()
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

    /// The fog of war of the local player (SPEC-0032), cell by cell
    /// (`ow3_sim::game::fog::MASKED` and the others, rows from the
    /// bottom); `None` when all is seen: the fog is off, or the first
    /// update has not come yet. Without `FogMaskEnable` cells never seen
    /// look fogged.
    /// What the fog leaves in the world of cells never seen: some with
    /// the map's flag `0x0010`, else nothing (SPEC-0032).
    pub fn vision_masked(&self) -> f32 {
        if self.minimap.masked_partly() {
            crate::renderer::MASKED_PARTLY
        } else {
            0.0
        }
    }

    pub fn vision(&self) -> Option<Vec<u8>> {
        use ow3_sim::game::fog;
        let (enabled, mask) = self.game.fog_enabled();
        if !enabled {
            return None;
        }
        let (_, mut cells) = self.game.fog(LOCAL_PLAYER)?;
        if !mask {
            for c in &mut cells {
                *c = (*c).max(fog::FOGGED);
            }
        }
        Some(cells)
    }

    /// Whether the local player sees the point (`x`, `y`): it is visible
    /// to it, or the fog is off.
    fn sees(&self, x: f32, y: f32) -> bool {
        !self.game.fog_enabled().0
            || !self.game.fog_started(LOCAL_PLAYER)
            || self.game.fog_at(LOCAL_PLAYER, x, y) == ow3_sim::game::fog::VISIBLE
    }

    /// Whether the local player sees unit `u` as it is now: its own
    /// always, others in visible cells (SPEC-0032). Other players'
    /// buildings under fog show as they were last seen instead.
    fn shown(&self, u: &ow3_sim::game::UnitView) -> bool {
        u.owner == LOCAL_PLAYER || self.sees(u.x, u.y)
    }

    pub fn draws(&mut self) -> Vec<UnitDraw> {
        let now = self.game.ticks();
        let mut draws = Vec::new();
        let units = self.game.units();
        for u in &units {
            let pose = self.pose(u, now);
            let building = self.game.unit_stats(u.handle).is_some_and(|s| s.building);
            let remember = building && u.owner != LOCAL_PLAYER;
            if !self.shown(u) {
                // Under fog: a building as last seen, if it was.
                if let Some((draw, _, _)) = self.remembered.get(&u.handle) {
                    draws.push(draw.clone());
                }
                continue;
            }
            if remember && !u.alive {
                self.remembered.remove(&u.handle);
            }
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
            let draw = UnitDraw {
                replaceable: None,
                matrix: crate::models::placement_matrix(&placement),
                model: placement.model,
                team: placement.team,
                handle: u.handle,
                life: (u.alive && u.max_life > 0.0).then(|| (u.life / u.max_life).clamp(0.0, 1.0)),
                pose,
                selection: look.selection,
                relation: self.game.relation(LOCAL_PLAYER, u.handle),
                tint: [1.0; 3],
            };
            if remember && u.alive {
                self.remembered
                    .insert(u.handle, (draw.clone(), [u.x, u.y], placement.team));
            }
            draws.push(draw);
        }
        // Missiles in flight, pointing at their targets (SPEC-0016).
        // PROVISIONAL: drawn at a fixed height of 60 over the ground; the
        // launch height and the arc (`launchZ`, `Missilearc`) are not
        // used yet.
        for m in self.game.missiles() {
            let [x, y] = m.at;
            if !self.sees(x, y) {
                continue;
            }
            let Some(model) = self.hud.missile_model(m.type_id) else {
                continue;
            };
            let angle = (m.towards[1] - y).atan2(m.towards[0] - x);
            let placement = Placement {
                model,
                replaceable: None,
                team: 0,
                position: [x, y, self.game.ground_z(x, y) + 60.0],
                angle,
                scale: [1.0; 3],
            };
            draws.push(UnitDraw {
                replaceable: None,
                matrix: crate::models::placement_matrix(&placement),
                model: placement.model,
                team: 0,
                handle: 0,
                life: None,
                pose: UnitPose::Stand,
                selection: 0.0,
                relation: None,
                tint: [1.0; 3],
            });
        }
        // Remembered buildings that are gone: drawn until their place is
        // seen again.
        let gone: Vec<u32> = self
            .remembered
            .keys()
            .copied()
            .filter(|h| !units.iter().any(|u| u.handle == *h))
            .collect();
        for h in gone {
            let [x, y] = self.remembered[&h].1;
            if self.sees(x, y) {
                self.remembered.remove(&h);
            } else {
                draws.push(self.remembered[&h].0.clone());
            }
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
                replaceable: None,
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
                replaceable: None,
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
        // Standing with a load: Stand Gold or Stand Lumber (SPEC-0029).
        if let (UnitPose::Stand, Some((load, _))) = (pose, u.carrying) {
            return UnitPose::LoadStand(millis(now), load);
        }
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
    // A worker hitting a tree swings once a hit, 1.0 s of its 1.1 s:
    // Attack while it carries no lumber, then Attack Lumber; it stands with
    // its load in the rest (SPEC-0029, "Animations"). PROVISIONAL: each
    // hit lands at the weapon's damage point of the swing.
    if let Some((next, period, point)) = u.chopping {
        let swing = next.saturating_sub(point);
        let start = if now >= swing {
            swing
        } else {
            swing.saturating_sub(period)
        };
        let elapsed = millis(now.saturating_sub(start));
        return match u.carrying {
            Some((load, _)) if elapsed >= CHOP_SWING_MS => UnitPose::LoadStand(elapsed, load),
            Some((load, _)) => UnitPose::LoadAttack(elapsed, load),
            None => UnitPose::Attack(elapsed),
        };
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
            follower_stands: false,
            level: 0,
            xp: 0,
            attributes: [0; 3],
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
