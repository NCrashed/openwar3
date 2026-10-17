//! The units of a running simulation, as the viewer draws them.
//!
//! The map's script runs in [`ow3_sim::game::Game`], fed with real time;
//! every frame the units it has are drawn where they stand, facing their
//! way, at the ground height `GetLocationZ` gives. A dead unit plays its
//! Death sequence once from the moment it died and stays on its last
//! frame.
//!
//! PROVISIONAL: models come from `Units\unitUI.slk` (`file`,
//! `modelScale`); custom types of the map, flying heights, decay and
//! hidden units' corpses are not handled.

use std::collections::BTreeMap;
use std::path::Path;

use ow3_formats::mpq::ArchiveChain;
use ow3_sim::game::Game;
use ow3_sim::map::Map;

use crate::camera::Mat4;
use crate::objects::Placement;

/// Milliseconds of game time per tick.
const TICK_MS: u64 = 5;

/// One unit to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitDraw {
    pub model: String,
    pub team: u8,
    pub matrix: Mat4,
    /// Milliseconds since its death, for a dead unit.
    pub dead_for: Option<i32>,
    /// The unit's handle, which picks its phase.
    pub handle: u32,
}

/// A map running in the simulation.
pub struct Sim {
    game: Game<'static>,
    models: BTreeMap<i32, (String, f32)>,
    /// The archives models are loaded from while it runs.
    pub chain: ArchiveChain,
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
            chain,
        })
    }

    /// Lets `micros` of real time pass.
    pub fn advance(&mut self, micros: u64) {
        self.game.advance(micros);
    }

    /// The units to draw now.
    pub fn draws(&self) -> Vec<UnitDraw> {
        let now = self.game.ticks();
        self.game
            .units()
            .into_iter()
            .filter(|u| !u.hidden)
            .filter_map(|u| {
                let (model, scale) = self.models.get(&u.type_id)?;
                let placement = Placement {
                    model: model.clone(),
                    replaceable: None,
                    team: u8::try_from(u.owner).ok().filter(|&t| t < 16).unwrap_or(0),
                    position: [u.x, u.y, self.game.ground_z(u.x, u.y)],
                    angle: u.facing,
                    scale: [*scale; 3],
                };
                Some(UnitDraw {
                    matrix: crate::models::placement_matrix(&placement),
                    model: placement.model,
                    team: placement.team,
                    handle: u.handle,
                    dead_for: u
                        .died_at
                        .map(|t| (now.saturating_sub(t) * TICK_MS).min(i32::MAX as u64) as i32),
                })
            })
            .collect()
    }
}
