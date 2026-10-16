//! `war3mapUnits.doo`: units, buildings, items and start locations placed
//! on the map.
//!
//! Supports format version 7 (Reign of Chaos maps) and 8 (Frozen Throne
//! maps up to 1.27). Layout reference: the community W3X/W3M format notes
//! (originally by Zepir, revised on Hive Workshop).

use crate::binary::{Error, RawCode, Reader, Result};

const MAGIC: &[u8; 4] = b"W3do";
pub const VERSION_REIGN_OF_CHAOS: u32 = 7;
pub const VERSION_FROZEN_THRONE: u32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedUnits {
    pub format_version: u32,
    pub subversion: u32,
    pub units: Vec<PlacedUnit>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedUnit {
    /// Unit, building or item code; `sloc` marks a player start location.
    pub kind: RawCode,
    pub variation: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Facing in radians.
    pub angle: f32,
    pub scale: [f32; 3],
    pub flags: u8,
    pub owner: i32,
    /// Two bytes after the owner whose meaning is not documented.
    pub unknown: [u8; 2],
    /// Hit points, or -1 for the default.
    pub hit_points: i32,
    /// Mana, or -1 for the default.
    pub mana: i32,
    /// Frozen Throne only: index of a map random item table, or -1.
    pub item_table: i32,
    /// Item sets dropped on death.
    pub dropped_item_sets: Vec<Vec<DroppedItem>>,
    /// Gold in a gold mine.
    pub gold: i32,
    /// Target acquisition range; -1 normal, -2 camp.
    pub target_acquisition: f32,
    pub hero_level: i32,
    /// Frozen Throne only: strength, agility, intelligence.
    pub hero_attributes: Option<[i32; 3]>,
    pub inventory: Vec<InventoryItem>,
    pub abilities: Vec<AbilityChange>,
    pub random: RandomChoice,
    /// Player colour, or -1 for the owner's.
    pub custom_color: i32,
    /// Region number a waygate leads to, or -1.
    pub waygate_destination: i32,
    /// Number the editor uses to refer to this unit, for example in
    /// triggers.
    pub editor_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DroppedItem {
    pub item: RawCode,
    pub chance: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryItem {
    pub slot: i32,
    pub item: RawCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityChange {
    pub ability: RawCode,
    pub autocast: bool,
    pub level: i32,
}

/// How a random unit or item is chosen. Units that are not random store
/// either the level form or no choice at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RandomChoice {
    /// A kind other than 0, 1 or 2, followed by no data. Reign of Chaos
    /// files use -1; one shipped Frozen Throne map has other values.
    None { kind: i32 },
    /// Any object of a level (-1 for any level) and, for items, of a class.
    ByLevel { level: i32, item_class: u8 },
    /// A column of one of the map's random unit tables.
    FromTable { group: i32, position: i32 },
    /// One of the listed objects, by chance.
    FromList(Vec<(RawCode, i32)>),
}

impl PlacedUnits {
    pub fn parse(data: &[u8]) -> Result<PlacedUnits> {
        let mut r = Reader::new(data);
        if r.bytes(4)? != MAGIC {
            return Err(Error::Invalid {
                what: "unit signature",
                offset: 0,
            });
        }
        let format_version = r.u32()?;
        let tft = match format_version {
            VERSION_REIGN_OF_CHAOS => false,
            VERSION_FROZEN_THRONE => true,
            other => return Err(Error::UnsupportedVersion(other)),
        };
        let subversion = r.u32()?;
        let units = r.list(if tft { 111 } else { 91 }, |r| read_unit(r, tft))?;
        r.finish()?;
        Ok(PlacedUnits {
            format_version,
            subversion,
            units,
        })
    }
}

fn read_unit(r: &mut Reader, tft: bool) -> Result<PlacedUnit> {
    let kind = r.raw_code()?;
    let variation = r.i32()?;
    let (x, y, z) = (r.f32()?, r.f32()?, r.f32()?);
    let angle = r.f32()?;
    let scale = [r.f32()?, r.f32()?, r.f32()?];
    let flags = r.u8()?;
    let owner = r.i32()?;
    let unknown = [r.u8()?, r.u8()?];
    let hit_points = r.i32()?;
    let mana = r.i32()?;
    // Reign of Chaos files have the dropped item sets but no table index.
    let item_table = if tft { r.i32()? } else { -1 };
    let dropped_item_sets = r.list(4, |r| {
        r.list(8, |r| {
            Ok(DroppedItem {
                item: r.raw_code()?,
                chance: r.i32()?,
            })
        })
    })?;
    let gold = r.i32()?;
    let target_acquisition = r.f32()?;
    let hero_level = r.i32()?;
    let hero_attributes = if tft {
        Some([r.i32()?, r.i32()?, r.i32()?])
    } else {
        None
    };
    let inventory = r.list(8, |r| {
        Ok(InventoryItem {
            slot: r.i32()?,
            item: r.raw_code()?,
        })
    })?;
    let abilities = r.list(12, |r| {
        Ok(AbilityChange {
            ability: r.raw_code()?,
            autocast: r.u32()? != 0,
            level: r.i32()?,
        })
    })?;
    // Level in the low three bytes (signed), item class in the fourth.
    let by_level = |raw: u32| RandomChoice::ByLevel {
        level: ((raw << 8) as i32) >> 8,
        item_class: (raw >> 24) as u8,
    };
    let random = match r.i32()? {
        0 => by_level(r.u32()?),
        1 => RandomChoice::FromTable {
            group: r.i32()?,
            position: r.i32()?,
        },
        2 => RandomChoice::FromList(r.list(8, |r| Ok((r.raw_code()?, r.i32()?)))?),
        kind => RandomChoice::None { kind },
    };
    Ok(PlacedUnit {
        kind,
        variation,
        x,
        y,
        z,
        angle,
        scale,
        flags,
        owner,
        unknown,
        hit_points,
        mana,
        item_table,
        dropped_item_sets,
        gold,
        target_acquisition,
        hero_level,
        hero_attributes,
        inventory,
        abilities,
        random,
        custom_color: r.i32()?,
        waygate_destination: r.i32()?,
        editor_id: r.i32()?,
    })
}

#[cfg(test)]
mod tests;
