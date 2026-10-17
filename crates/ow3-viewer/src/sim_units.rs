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
use ow3_sim::game::{Game, UnitView};
use ow3_sim::map::Map;

use crate::camera::Mat4;
use crate::models::UnitPose;
use crate::objects::Placement;

/// Milliseconds of game time per tick.
const TICK_MS: u64 = 5;

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

    /// The light models the script chose (SPEC-0017).
    pub fn day_night_models(&self) -> Option<(String, String)> {
        self.game.day_night_models()
    }

    /// The time of day in hours.
    pub fn time_of_day(&self) -> f32 {
        self.game.time_of_day()
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
                    life: (u.alive && u.max_life > 0.0)
                        .then(|| (u.life / u.max_life).clamp(0.0, 1.0)),
                    pose: pose(&u, now),
                })
            })
            .collect()
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
