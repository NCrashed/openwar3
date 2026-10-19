//! The game's console over the view: its textures, the menu buttons and
//! the resource bar with the local player's gold, lumber and food.
//!
//! PROVISIONAL: the places of the top-level frames on the screen are not
//! in the definition files and not yet specified. The button bar stands
//! at the top left and the resource bar at the top right, over the parts
//! of the console's top as wide as they are (0.34 and 0.338).

use ow3_formats::mpq::ArchiveChain;

use crate::console::Interface;
use crate::ui::{self, Frame};

/// What the resource bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resources {
    pub gold: i32,
    pub lumber: i32,
    pub food_used: i32,
    pub food_cap: i32,
}

pub struct Hud {
    interface: Interface,
}

impl Hud {
    /// The console of race `race` (`Human`, `Orc`, `NightElf`, `Undead`).
    pub fn load(chain: &mut ArchiveChain, race: &str) -> Hud {
        Hud {
            interface: Interface::load(chain, race),
        }
    }

    /// The console with `resources`.
    pub fn frame(&self, resources: Resources) -> Frame {
        let mut out = Frame::default();
        let screen = [0.0, 0.0, ui::WIDTH, ui::HEIGHT];
        let none = |_: &str| None;
        self.interface.lay_out("ConsoleUI", screen, &none, &mut out);
        let buttons = [0.0, ui::HEIGHT - 0.022, 0.34, ui::HEIGHT];
        self.interface
            .lay_out("UpperButtonBarFrame", buttons, &none, &mut out);
        let upkeep = self.interface.string("UPKEEP_NONE");
        let texts = |name: &str| match name {
            "ResourceBarGoldText" => Some(resources.gold.to_string()),
            "ResourceBarLumberText" => Some(resources.lumber.to_string()),
            "ResourceBarSupplyText" => {
                Some(format!("{}/{}", resources.food_used, resources.food_cap))
            }
            "ResourceBarUpkeepText" => Some(upkeep.clone()),
            _ => None,
        };
        let bar = [
            ui::WIDTH - 0.338_281_25,
            ui::HEIGHT - 0.0218,
            ui::WIDTH,
            ui::HEIGHT,
        ];
        self.interface
            .lay_out("ResourceBarFrame", bar, &texts, &mut out);
        out
    }
}
