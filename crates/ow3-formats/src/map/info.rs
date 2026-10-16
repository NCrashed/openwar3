//! `war3map.w3i`: map name, players, forces, loading screens and other
//! settings from the editor's scenario properties.
//!
//! Supports format version 18 (Reign of Chaos maps) and 25 (The Frozen
//! Throne maps up to 1.27). Layout reference: the community W3X/W3M format
//! notes (originally by Zepir, revised on Hive Workshop).

use crate::binary::{Error, RawCode, Reader, Result};

pub const VERSION_REIGN_OF_CHAOS: u32 = 18;
pub const VERSION_FROZEN_THRONE: u32 = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct MapInfo {
    pub format_version: u32,
    /// How many times the map was saved.
    pub saves: u32,
    pub editor_version: u32,
    pub name: String,
    pub author: String,
    pub description: String,
    pub recommended_players: String,
    /// Camera bounds as four corner points: left-bottom, right-top,
    /// left-top, right-bottom, each as x, y.
    pub camera_bounds: [f32; 8],
    /// Distances from the map edges to the camera bounds: left, right,
    /// bottom, top, in cells.
    pub camera_bounds_complements: [i32; 4],
    pub playable_width: i32,
    pub playable_height: i32,
    pub flags: u32,
    /// Tileset letter, such as `L` for Lordaeron Summer.
    pub tileset: u8,
    pub loading_screen: Screen,
    pub prologue: Screen,
    /// Frozen Throne only.
    pub game_data_set: Option<i32>,
    /// Frozen Throne only.
    pub environment: Option<Environment>,
    pub players: Vec<Player>,
    pub forces: Vec<Force>,
    pub upgrades: Vec<UpgradeAvailability>,
    pub tech: Vec<TechAvailability>,
    pub random_unit_tables: Vec<RandomUnitTable>,
    /// Frozen Throne only; empty for Reign of Chaos maps.
    pub random_item_tables: Vec<RandomItemTable>,
}

/// Loading or prologue screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    /// Index of a built-in screen, or -1 for none.
    pub preset: i32,
    /// Path of a custom screen model (Frozen Throne only, otherwise empty).
    pub custom_model: String,
    pub text: String,
    pub title: String,
    pub subtitle: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    /// 0 when the map uses no custom terrain fog, otherwise the fog style.
    pub fog_style: i32,
    pub fog_start_z: f32,
    pub fog_end_z: f32,
    pub fog_density: f32,
    pub fog_color: Rgba,
    /// Weather effect for the whole map, or `None` for no weather.
    pub weather: Option<RawCode>,
    pub sound_environment: String,
    /// Tileset letter whose lighting the map uses, or 0 for its own.
    pub light_environment: u8,
    pub water_color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub number: i32,
    /// 1 human, 2 computer, 3 neutral, 4 rescuable.
    pub kind: i32,
    /// 1 human, 2 orc, 3 undead, 4 night elf, 0 selectable.
    pub race: i32,
    pub fixed_start_position: bool,
    pub name: String,
    pub start_x: f32,
    pub start_y: f32,
    /// Bit `n` set: player `n` has low ally priority for this player.
    pub ally_low_priorities: u32,
    pub ally_high_priorities: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Force {
    pub flags: u32,
    /// Bit `n` set: player `n` belongs to the force.
    pub players: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeAvailability {
    pub players: u32,
    pub upgrade: RawCode,
    /// Upgrade level, 0 for the first.
    pub level: i32,
    /// 0 unavailable, 1 available, 2 researched.
    pub availability: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TechAvailability {
    pub players: u32,
    pub tech: RawCode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomUnitTable {
    pub number: i32,
    pub name: String,
    /// What each column holds: 0 unit, 1 building, 2 item.
    pub column_kinds: Vec<i32>,
    pub rows: Vec<RandomUnitRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomUnitRow {
    /// Percentage chance of this row.
    pub chance: i32,
    /// One object per column.
    pub objects: Vec<RawCode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomItemTable {
    pub number: i32,
    pub name: String,
    pub sets: Vec<Vec<RandomItem>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomItem {
    pub chance: i32,
    pub item: RawCode,
}

impl MapInfo {
    pub fn parse(data: &[u8]) -> Result<MapInfo> {
        let mut r = Reader::new(data);
        let format_version = r.u32()?;
        let tft = match format_version {
            VERSION_REIGN_OF_CHAOS => false,
            VERSION_FROZEN_THRONE => true,
            other => return Err(Error::UnsupportedVersion(other)),
        };
        let saves = r.u32()?;
        let editor_version = r.u32()?;
        let name = r.string()?;
        let author = r.string()?;
        let description = r.string()?;
        let recommended_players = r.string()?;
        let camera_bounds = std::array::from_fn::<_, 8, _>(|_| r.f32());
        let camera_bounds = collect(camera_bounds)?;
        let camera_bounds_complements = collect(std::array::from_fn::<_, 4, _>(|_| r.i32()))?;
        let playable_width = r.i32()?;
        let playable_height = r.i32()?;
        let flags = r.u32()?;
        let tileset = r.u8()?;

        let (loading_screen, game_data_set, prologue, environment) = if tft {
            let loading_screen = Screen {
                preset: r.i32()?,
                custom_model: r.string()?,
                text: r.string()?,
                title: r.string()?,
                subtitle: r.string()?,
            };
            let game_data_set = r.i32()?;
            let prologue = Screen {
                preset: -1,
                custom_model: r.string()?,
                text: r.string()?,
                title: r.string()?,
                subtitle: r.string()?,
            };
            let environment = Environment {
                fog_style: r.i32()?,
                fog_start_z: r.f32()?,
                fog_end_z: r.f32()?,
                fog_density: r.f32()?,
                fog_color: rgba(&mut r)?,
                weather: Some(r.raw_code()?).filter(|code| code.0 != [0; 4]),
                sound_environment: r.string()?,
                light_environment: r.u8()?,
                water_color: rgba(&mut r)?,
            };
            (
                loading_screen,
                Some(game_data_set),
                prologue,
                Some(environment),
            )
        } else {
            let campaign_background = r.i32()?;
            let text = r.string()?;
            let title = r.string()?;
            let subtitle = r.string()?;
            let loading_screen = Screen {
                preset: r.i32()?,
                custom_model: String::new(),
                text,
                title,
                subtitle,
            };
            let prologue = Screen {
                preset: campaign_background,
                custom_model: String::new(),
                text: r.string()?,
                title: r.string()?,
                subtitle: r.string()?,
            };
            (loading_screen, None, prologue, None)
        };

        let players = r.list(33, |r| {
            Ok(Player {
                number: r.i32()?,
                kind: r.i32()?,
                race: r.i32()?,
                fixed_start_position: r.u32()? & 1 != 0,
                name: r.string()?,
                start_x: r.f32()?,
                start_y: r.f32()?,
                ally_low_priorities: r.u32()?,
                ally_high_priorities: r.u32()?,
            })
        })?;
        let forces = r.list(9, |r| {
            Ok(Force {
                flags: r.u32()?,
                players: r.u32()?,
                name: r.string()?,
            })
        })?;
        let upgrades = r.list(16, |r| {
            Ok(UpgradeAvailability {
                players: r.u32()?,
                upgrade: r.raw_code()?,
                level: r.i32()?,
                availability: r.i32()?,
            })
        })?;
        let tech = r.list(8, |r| {
            Ok(TechAvailability {
                players: r.u32()?,
                tech: r.raw_code()?,
            })
        })?;
        let random_unit_tables = r.list(13, |r| {
            let number = r.i32()?;
            let name = r.string()?;
            let column_kinds = r.list(4, Reader::i32)?;
            let columns = column_kinds.len();
            let rows = r.list(4 + columns * 4, |r| {
                Ok(RandomUnitRow {
                    chance: r.i32()?,
                    objects: (0..columns).map(|_| r.raw_code()).collect::<Result<_>>()?,
                })
            })?;
            Ok(RandomUnitTable {
                number,
                name,
                column_kinds,
                rows,
            })
        })?;
        let random_item_tables = if tft {
            r.list(9, |r| {
                Ok(RandomItemTable {
                    number: r.i32()?,
                    name: r.string()?,
                    sets: r.list(4, |r| {
                        r.list(8, |r| {
                            Ok(RandomItem {
                                chance: r.i32()?,
                                item: r.raw_code()?,
                            })
                        })
                    })?,
                })
            })?
        } else {
            Vec::new()
        };
        r.finish()?;

        Ok(MapInfo {
            format_version,
            saves,
            editor_version,
            name,
            author,
            description,
            recommended_players,
            camera_bounds,
            camera_bounds_complements,
            playable_width,
            playable_height,
            flags,
            tileset,
            loading_screen,
            prologue,
            game_data_set,
            environment,
            players,
            forces,
            upgrades,
            tech,
            random_unit_tables,
            random_item_tables,
        })
    }
}

fn collect<T: Copy + Default, const N: usize>(values: [Result<T>; N]) -> Result<[T; N]> {
    let mut out = [T::default(); N];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = value?;
    }
    Ok(out)
}

fn rgba(r: &mut Reader) -> Result<Rgba> {
    Ok(Rgba {
        r: r.u8()?,
        g: r.u8()?,
        b: r.u8()?,
        a: r.u8()?,
    })
}

#[cfg(test)]
mod tests;
