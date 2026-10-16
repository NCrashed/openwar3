//! Object modification files: changes a map makes to the game's units,
//! items, destructibles, doodads, abilities, buffs and upgrades, and the
//! new objects it derives from them.
//!
//! | File | Kind | Levels |
//! |---|---|---|
//! | `war3map.w3u` | units | no |
//! | `war3map.w3t` | items | no |
//! | `war3map.w3b` | destructibles | no |
//! | `war3map.w3d` | doodads | yes |
//! | `war3map.w3a` | abilities | yes |
//! | `war3map.w3h` | buffs | no |
//! | `war3map.w3q` | upgrades | yes |
//!
//! Kinds with levels store, for every modification, the level (or doodad
//! variation) and a data column. Layout reference: the community W3X/W3M
//! format notes (originally by Zepir, revised on Hive Workshop).

use crate::binary::{Error, RawCode, Reader, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Units,
    Items,
    Destructibles,
    Doodads,
    Abilities,
    Buffs,
    Upgrades,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 7] = [
        ObjectKind::Units,
        ObjectKind::Items,
        ObjectKind::Destructibles,
        ObjectKind::Doodads,
        ObjectKind::Abilities,
        ObjectKind::Buffs,
        ObjectKind::Upgrades,
    ];

    pub fn file_name(self) -> &'static str {
        match self {
            ObjectKind::Units => "war3map.w3u",
            ObjectKind::Items => "war3map.w3t",
            ObjectKind::Destructibles => "war3map.w3b",
            ObjectKind::Doodads => "war3map.w3d",
            ObjectKind::Abilities => "war3map.w3a",
            ObjectKind::Buffs => "war3map.w3h",
            ObjectKind::Upgrades => "war3map.w3q",
        }
    }

    pub fn has_levels(self) -> bool {
        matches!(
            self,
            ObjectKind::Doodads | ObjectKind::Abilities | ObjectKind::Upgrades
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectModifications {
    pub format_version: u32,
    /// Changes to objects of the game.
    pub original: Vec<ObjectDefinition>,
    /// New objects, each derived from an object of the game.
    pub custom: Vec<ObjectDefinition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectDefinition {
    /// The game object this one changes or derives from.
    pub base: RawCode,
    /// Code of the new object; `None` in the table of original objects.
    pub id: Option<RawCode>,
    pub modifications: Vec<Modification>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modification {
    /// Field code, such as `uhpm` for a unit's maximum hit points.
    pub field: RawCode,
    /// Level or doodad variation; only for kinds with levels.
    pub level: Option<i32>,
    /// Data column (`DataA` = 1, `DataB` = 2, ...); only for kinds with
    /// levels.
    pub data_column: Option<i32>,
    pub value: Value,
    /// Four bytes stored after the value. Current editors write 0 or the
    /// object's code; files from older editors may hold the code of an
    /// unrelated object. Kept as stored; its effect, if any, is unknown.
    pub end_marker: RawCode,
}

/// A field value. Reals are kept as stored.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Real(f32),
    /// A real limited to 0..1 by the editor.
    Unreal(f32),
    String(String),
}

impl ObjectModifications {
    pub fn parse(data: &[u8], kind: ObjectKind) -> Result<ObjectModifications> {
        let mut r = Reader::new(data);
        let format_version = r.u32()?;
        if !(1..=2).contains(&format_version) {
            return Err(Error::UnsupportedVersion(format_version));
        }
        let original = read_table(&mut r, kind)?;
        let custom = read_table(&mut r, kind)?;
        r.finish()?;
        Ok(ObjectModifications {
            format_version,
            original,
            custom,
        })
    }
}

fn read_table(r: &mut Reader, kind: ObjectKind) -> Result<Vec<ObjectDefinition>> {
    // Field, type, value and end marker; the smallest value is an empty
    // string of one byte.
    let min_modification = if kind.has_levels() { 21 } else { 13 };
    r.list(12, |r| {
        let base = r.raw_code()?;
        let id = Some(r.raw_code()?).filter(|code| code.0 != [0; 4]);
        let modifications = r.list(min_modification, |r| {
            let field = r.raw_code()?;
            let type_offset = r.position();
            let value_type = r.u32()?;
            let (level, data_column) = if kind.has_levels() {
                (Some(r.i32()?), Some(r.i32()?))
            } else {
                (None, None)
            };
            let value = match value_type {
                0 => Value::Integer(r.i32()?),
                1 => Value::Real(r.f32()?),
                2 => Value::Unreal(r.f32()?),
                3 => Value::String(r.string()?),
                _ => {
                    return Err(Error::Invalid {
                        what: "value type",
                        offset: type_offset,
                    });
                }
            };
            Ok(Modification {
                field,
                level,
                data_column,
                value,
                end_marker: r.raw_code()?,
            })
        })?;
        Ok(ObjectDefinition {
            base,
            id,
            modifications,
        })
    })
}

#[cfg(test)]
mod tests;
