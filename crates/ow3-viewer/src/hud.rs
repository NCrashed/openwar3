//! The game's console over the view: its textures, the menu buttons, the
//! resource bar, and for the selected unit its name, life, mana, damage
//! and armor and its command card, with the tooltip of the button under
//! the mouse.
//!
//! The buttons come from the game's data: the basic orders of
//! `Units\CommandFunc.txt`, the buildings a worker builds (`Builds`) and
//! the units a building trains (`Trains`) of the unit data, with their
//! icons (`Art`), places (`Buttonpos`), names, tips and hotkeys.
//!
//! The frames stand where SPEC-0028 places them. The world is seen only
//! between 0.13 and 0.58 of the interface's height; below, the console's
//! openings show black.

use ow3_formats::mpq::ArchiveChain;
use ow3_formats::profile::Profile;
use ow3_sim::game::UnitStats;

use crate::console::{Interface, Rect, Values};
use crate::ui::{self, Blend, Frame, Justify, Quad, Text};

/// What the resource bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resources {
    pub gold: i32,
    pub lumber: i32,
    pub food_used: i32,
    pub food_cap: i32,
}

/// The selected unit as the console shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selected {
    pub handle: u32,
    pub type_id: i32,
    /// Whether the local player commands it.
    pub own: bool,
    pub life: f32,
    pub max_life: f32,
    pub mana: f32,
    pub max_mana: f32,
    pub stats: Option<UnitStats>,
    /// A worker carrying gold or lumber (SPEC-0029).
    pub carrying: bool,
}

/// What a command button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Move,
    Stop,
    Hold,
    Attack,
    Patrol,
    /// Opens the list of buildings.
    BuildMenu,
    /// Closes a list.
    Back,
    Build(i32),
    Train(i32),
    Rally,
    /// Takes back the last unit of a building's queue.
    CancelTrain,
    /// Gather from a gold mine or tree (SPEC-0029), and take a load back.
    Gather,
    ReturnGoods,
}

impl Command {
    /// Whether it waits for a point on the ground.
    pub fn targets(self) -> bool {
        matches!(
            self,
            Command::Move
                | Command::Attack
                | Command::Patrol
                | Command::Build(_)
                | Command::Rally
                | Command::Gather
        )
    }
}

/// A button of the command card.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    /// Column 0 to 3 from the left, row 0 to 2 from the top.
    pub place: (u32, u32),
    pub icon: String,
    pub tip: String,
    pub extended: String,
    pub hotkey: Option<char>,
    pub command: Command,
}

/// The console's state: which list the command card shows and the order
/// waiting for its point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Card {
    #[default]
    Main,
    Build,
}

pub struct Hud {
    interface: Interface,
    units: Profile,
    commands: Profile,
    pub card: Card,
    /// An order waiting for a point on the ground.
    pub pending: Option<Command>,
}

/// The unit profile files of all races, the data and the strings, and
/// those of the abilities all races share.
const UNIT_FILES: [&str; 14] = [
    "Units\\HumanUnitFunc.txt",
    "Units\\OrcUnitFunc.txt",
    "Units\\NightElfUnitFunc.txt",
    "Units\\UndeadUnitFunc.txt",
    "Units\\NeutralUnitFunc.txt",
    "Units\\CampaignUnitFunc.txt",
    "Units\\HumanUnitStrings.txt",
    "Units\\OrcUnitStrings.txt",
    "Units\\NightElfUnitStrings.txt",
    "Units\\UndeadUnitStrings.txt",
    "Units\\NeutralUnitStrings.txt",
    "Units\\CampaignUnitStrings.txt",
    "Units\\CommonAbilityFunc.txt",
    "Units\\CommonAbilityStrings.txt",
];

/// The command card (SPEC-0028): the top left corner of its first
/// button, the size of a button and the step between buttons.
const CARD: [f32; 2] = [0.618_125, 0.131];
const BUTTON: f32 = 0.038;
const STEP: [f32; 2] = [0.043_625, 0.043];
/// The information panel, the resource bar, the upper button bar and the
/// view of the world (SPEC-0028).
const INFO: Rect = [0.309_375, 0.0, 0.4975, 0.114_063];
const RESOURCES: Rect = [0.461_719, 0.5782, 0.8, 0.6];
const UPPER: Rect = [0.0, 0.578, 0.34, 0.6];
const WORLD: Rect = [0.0, 0.13, 0.8, 0.58];
/// The portrait (SPEC-0028).
pub const PORTRAIT: Rect = [0.2075, 0.030_625, 0.2975, 0.116_23];
/// The life and mana texts under the portrait.
///
/// PROVISIONAL: their places are not specified yet; they stand under the
/// portrait (0.2075 to 0.2975).
const LIFE: Rect = [0.2075, 0.017, 0.2975, 0.029];
const MANA: Rect = [0.2075, 0.004, 0.2975, 0.016];

fn code(type_id: i32) -> String {
    String::from_utf8_lossy(&type_id.to_be_bytes()).into_owned()
}

fn raw(code: &str) -> Option<i32> {
    let bytes: [u8; 4] = code.as_bytes().try_into().ok()?;
    Some(i32::from_be_bytes(bytes))
}

impl Hud {
    /// The console of race `race` (`Human`, `Orc`, `NightElf`, `Undead`).
    pub fn load(chain: &mut ArchiveChain, race: &str) -> Hud {
        let mut units = Profile::default();
        for file in UNIT_FILES {
            for data in chain.read_all(file) {
                units.add(&String::from_utf8_lossy(&data));
            }
        }
        let mut commands = Profile::default();
        for file in ["Units\\CommandFunc.txt", "Units\\CommandStrings.txt"] {
            for data in chain.read_all(file) {
                commands.add(&String::from_utf8_lossy(&data));
            }
        }
        Hud {
            interface: Interface::load(chain, race),
            units,
            commands,
            card: Card::Main,
            pending: None,
        }
    }

    /// The name of a unit type.
    pub fn name(&self, type_id: i32) -> String {
        self.units
            .get(&code(type_id), "Name")
            .map_or_else(|| code(type_id), str::to_owned)
    }

    fn command_button(&self, section: &str, command: Command) -> Option<Button> {
        let place = self.commands.list(section, "Buttonpos");
        let place = (place.first()?.parse().ok()?, place.get(1)?.parse().ok()?);
        let art = self.commands.get(section, "Art")?;
        Some(Button {
            place,
            icon: self.interface.skin(art),
            tip: self
                .commands
                .get(section, "Tip")
                .unwrap_or_default()
                .to_owned(),
            extended: self
                .commands
                .get(section, "Ubertip")
                .unwrap_or_default()
                .to_owned(),
            hotkey: self
                .commands
                .get(section, "Hotkey")
                .and_then(|h| h.chars().next()),
            command,
        })
    }

    /// The button of ability `section`, in its other state (`Unart`,
    /// `UnButtonpos`, `Untip`, `Unhotkey`) when `other`.
    fn ability_button(&self, section: &str, other: bool) -> Option<Button> {
        let key = |k: &str| {
            if other {
                format!("Un{}", k.to_ascii_lowercase())
            } else {
                k.to_owned()
            }
        };
        let get = |k: &str| self.units.get(section, &key(k));
        let place = self.units.list(
            section,
            &if other {
                "UnButtonpos".to_owned()
            } else {
                "Buttonpos".to_owned()
            },
        );
        let place = (place.first()?.parse().ok()?, place.get(1)?.parse().ok()?);
        Some(Button {
            place,
            icon: self.interface.skin(get("Art")?),
            tip: get("Tip").unwrap_or_default().to_owned(),
            extended: get("Ubertip").unwrap_or_default().to_owned(),
            hotkey: get("Hotkey").and_then(|h| h.chars().next()),
            command: if other {
                Command::ReturnGoods
            } else {
                Command::Gather
            },
        })
    }

    fn unit_button(&self, type_id: i32, command: Command) -> Option<Button> {
        let section = code(type_id);
        let place = self.units.list(&section, "Buttonpos");
        let place = (
            place.first().and_then(|p| p.parse().ok()).unwrap_or(0),
            place.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
        );
        Some(Button {
            place,
            icon: self.units.get(&section, "Art")?.to_owned(),
            tip: self
                .units
                .get(&section, "Tip")
                .unwrap_or_default()
                .to_owned(),
            extended: self
                .units
                .get(&section, "Ubertip")
                .unwrap_or_default()
                .to_owned(),
            hotkey: self
                .units
                .get(&section, "Hotkey")
                .and_then(|h| h.chars().next()),
            command,
        })
    }

    /// The buttons of the command card for `selected`.
    pub fn buttons(&self, selected: Option<&Selected>) -> Vec<Button> {
        let Some(s) = selected.filter(|s| s.own) else {
            return Vec::new();
        };
        let section = code(s.type_id);
        let building = s.stats.is_some_and(|t| t.building);
        let mut out = Vec::new();
        match self.card {
            Card::Build => {
                for b in self.units.list(&section, "Builds") {
                    if let Some(t) = raw(&b)
                        && let Some(button) = self.unit_button(t, Command::Build(t))
                    {
                        out.push(button);
                    }
                }
                out.extend(self.command_button("CmdCancel", Command::Back));
            }
            Card::Main if building => {
                for t in self.units.list(&section, "Trains") {
                    if let Some(t) = raw(&t)
                        && let Some(button) = self.unit_button(t, Command::Train(t))
                    {
                        out.push(button);
                    }
                }
                if !out.is_empty() {
                    out.extend(self.command_button("CmdRally", Command::Rally));
                    out.extend(self.command_button("CmdCancelTrain", Command::CancelTrain));
                }
            }
            Card::Main => {
                out.extend(self.command_button("CmdMove", Command::Move));
                out.extend(self.command_button("CmdStop", Command::Stop));
                out.extend(self.command_button("CmdHoldPos", Command::Hold));
                out.extend(self.command_button("CmdAttack", Command::Attack));
                out.extend(self.command_button("CmdPatrol", Command::Patrol));
                if !self.units.list(&section, "Builds").is_empty() {
                    // The harvest ability: gather, or with a load its
                    // other state, return (PROVISIONAL: shown for every
                    // worker).
                    out.extend(self.ability_button("Ahar", s.carrying));
                    let race = &self.interface.race;
                    out.extend(
                        self.command_button(&format!("CmdBuild{race}"), Command::BuildMenu)
                            .or_else(|| self.command_button("CmdBuild", Command::BuildMenu)),
                    );
                }
            }
        }
        out
    }

    /// The rectangle of the button at `place`.
    fn button_rect(place: (u32, u32)) -> Rect {
        let x0 = CARD[0] + STEP[0] * place.0 as f32;
        let y1 = CARD[1] - STEP[1] * place.1 as f32;
        [x0, y1 - BUTTON, x0 + BUTTON, y1]
    }

    /// The button at `point` in interface coordinates.
    pub fn button_at(&self, selected: Option<&Selected>, point: [f32; 2]) -> Option<Button> {
        self.buttons(selected).into_iter().find(|b| {
            let r = Self::button_rect(b.place);
            point[0] >= r[0] && point[0] <= r[2] && point[1] >= r[1] && point[1] <= r[3]
        })
    }

    /// The button of hotkey `key` (compared without regard to case).
    pub fn button_of_key(&self, selected: Option<&Selected>, key: char) -> Option<Button> {
        self.buttons(selected)
            .into_iter()
            .find(|b| b.hotkey.is_some_and(|h| h.eq_ignore_ascii_case(&key)))
    }

    /// Whether `point` lies on the console rather than the view: its bottom
    /// part or its top bar.
    pub fn covers(point: [f32; 2]) -> bool {
        point[1] < WORLD[1] || point[1] > WORLD[3]
    }

    /// The console for `resources` and `selected`, with the tooltip of the
    /// button at `mouse` (interface coordinates).
    pub fn frame(
        &self,
        resources: Resources,
        selected: Option<&Selected>,
        mouse: Option<[f32; 2]>,
    ) -> Frame {
        let mut out = Frame::default();
        let none = |_: &str| None;
        let plain = Values {
            texts: &none,
            files: &none,
        };
        let screen = [0.0, 0.0, ui::WIDTH, ui::HEIGHT];
        // Black under the console's openings, below the view of the world
        // (SPEC-0028).
        out.quads.push(Quad {
            rect: [0.0, 0.0, ui::WIDTH, WORLD[1]],
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: String::new(),
            colour: [0.0, 0.0, 0.0, 1.0],
            blend: Blend::Alpha,
        });
        // The portrait lies between it and the console.
        out.under = out.quads.len();
        self.interface
            .lay_out("ConsoleUI", screen, &plain, &mut out);
        let buttons = UPPER;
        self.interface
            .lay_out("UpperButtonBarFrame", buttons, &plain, &mut out);
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
        let bar = RESOURCES;
        self.interface.lay_out(
            "ResourceBarFrame",
            bar,
            &Values {
                texts: &texts,
                files: &none,
            },
            &mut out,
        );
        if let Some(s) = selected {
            self.info(s, &mut out);
        }
        let buttons = self.buttons(selected);
        for b in &buttons {
            out.quads.push(Quad {
                rect: Self::button_rect(b.place),
                uv: [0.0, 1.0, 0.0, 1.0],
                texture: b.icon.clone(),
                colour: [1.0; 4],
                blend: Blend::Alpha,
            });
        }
        if let Some(b) = mouse.and_then(|m| self.button_at(selected, m)) {
            self.tooltip(&b, &mut out);
        }
        out
    }

    /// The name, life, mana, damage and armor of `s`.
    fn info(&self, s: &Selected, out: &mut Frame) {
        let name = self.name(s.type_id);
        let font = self.interface.skin("MasterFont");
        let text = |rect: Rect, text: String, colour: [f32; 4]| Text {
            rect,
            size: 0.0095,
            font: font.clone(),
            text,
            colour,
            justify: Justify::Centre,
        };
        if s.max_life > 0.0 {
            out.texts.push(text(
                LIFE,
                format!("{:.0} / {:.0}", s.life.ceil(), s.max_life),
                [0.0, 1.0, 0.0, 1.0],
            ));
        }
        if s.max_mana > 0.0 {
            out.texts.push(text(
                MANA,
                format!("{:.0} / {:.0}", s.mana.floor(), s.max_mana),
                [0.4, 0.6, 1.0, 1.0],
            ));
        }
        let none = |_: &str| None;
        let building = s.stats.is_some_and(|t| t.building);
        let panel = if building {
            "SimpleInfoPanelBuildingDetail"
        } else {
            "SimpleInfoPanelUnitDetail"
        };
        let names = |n: &str| match n {
            "SimpleNameValue" | "SimpleBuildingNameValue" => Some(name.clone()),
            _ => None,
        };
        self.interface.lay_out(
            panel,
            INFO,
            &Values {
                texts: &names,
                files: &none,
            },
            out,
        );
        let Some(stats) = s.stats else {
            return;
        };
        let neutral = if s.own { "" } else { "Neutral" };
        let top = INFO[3] - 0.032;
        if let Some((lo, hi)) = stats.damage {
            let kind = match stats.attack_type {
                Some(2) => "Pierce",
                Some(3) => "Siege",
                Some(4) | Some(0) => "Magic",
                Some(5) => "Chaos",
                Some(6) => "Hero",
                _ => "Normal",
            };
            let icon = self
                .interface
                .skin(&format!("InfoPanelIconDamage{kind}{neutral}"));
            let value = format!("{lo} - {hi}");
            self.icon_frame("SimpleInfoPanelIconDamage", top, icon, value, out);
        }
        if !building || stats.damage.is_none() {
            let kind = match stats
                .armor_type
                .and_then(|i| ow3_sim::combat::ARMOR_TYPES.get(i))
                .copied()
            {
                Some("small") => "Small",
                Some("medium") => "Medium",
                Some("large") => "Large",
                Some("fort") => "Fort",
                Some("hero") => "Hero",
                Some("divine") => "Divine",
                Some("none") => "None",
                _ => "Normal",
            };
            let icon = self
                .interface
                .skin(&format!("InfoPanelIconArmor{kind}{neutral}"));
            let value = format!("{:.0}", stats.armor);
            let at = if stats.damage.is_some() && !building {
                top - 0.034
            } else {
                top
            };
            self.icon_frame("SimpleInfoPanelIconArmor", at, icon, value, out);
        }
    }

    /// An icon frame of the info panel (damage, armor) with its top at
    /// `top`.
    fn icon_frame(&self, frame: &str, top: f32, icon: String, value: String, out: &mut Frame) {
        let rect = [INFO[0], top - 0.031_25, INFO[0] + 0.1, top];
        let texts = |n: &str| (n == "InfoPanelIconValue").then(|| value.clone());
        let files = |n: &str| (n == "InfoPanelIconBackdrop").then(|| icon.clone());
        self.interface.lay_out(
            frame,
            rect,
            &Values {
                texts: &texts,
                files: &files,
            },
            out,
        );
    }

    /// The tip of button `b` above the command card.
    fn tooltip(&self, b: &Button, out: &mut Frame) {
        let font = self.interface.skin("MasterFont");
        let (tip, tip_colour) = crate::console::plain(&b.tip);
        let (extended, _) = crate::console::plain(&b.extended);
        let lines = wrap(&extended, 48);
        let line = 0.012;
        let height = 0.02 + line * lines.len() as f32;
        let rect = [0.52, 0.14, ui::WIDTH - 0.005, 0.14 + height];
        out.quads.push(Quad {
            rect,
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: self.interface.skin("ToolTipBackground"),
            colour: [1.0, 1.0, 1.0, 0.9],
            blend: Blend::Alpha,
        });
        out.texts.push(Text {
            rect: [
                rect[0] + 0.005,
                rect[3] - 0.016,
                rect[2] - 0.005,
                rect[3] - 0.004,
            ],
            size: 0.011,
            font: font.clone(),
            text: tip,
            colour: tip_colour.map_or([1.0; 4], |_| [1.0; 4]),
            justify: Justify::Left,
        });
        for (i, l) in lines.into_iter().enumerate() {
            let y1 = rect[3] - 0.018 - line * i as f32;
            out.texts.push(Text {
                rect: [rect[0] + 0.005, y1 - line, rect[2] - 0.005, y1],
                size: 0.0095,
                font: font.clone(),
                text: l,
                colour: [1.0, 0.82, 0.0, 1.0],
                justify: Justify::Left,
            });
        }
    }
}

/// `text` in lines of at most `width` characters, broken at spaces and at
/// the line breaks of the text.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split("|n") {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}
