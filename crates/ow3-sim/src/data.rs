//! Object types the simulation needs from the game data: units,
//! destructables and items, with the map's custom types.
//!
//! Standard types come from `Units\unitUI.slk` (the unit's string id such
//! as `footman`, column `name`), `Units\UnitBalance.slk` (`HP`),
//! `Units\DestructableData.slk` (`HP`) and `Units\ItemData.slk` (`HP`).
//! A custom type of the map (`war3map.w3u`, `w3b`, `w3t`) copies its base
//! type and takes the hit points it sets (`uhpm`, `bhps`, `ihtp`).
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: a custom unit keeps the
//! string id of its base type, and when several types share a string id,
//! the lowest raw code is the one `UnitId` finds.

use std::collections::BTreeMap;

use ow3_formats::binary::RawCode;
use ow3_formats::map::objects::{ObjectKind, ObjectModifications, Value};
use ow3_formats::slk::Table;

use crate::map::{Archives, MapError, raw_code_value};

#[derive(Clone, Debug, PartialEq)]
pub struct UnitType {
    /// String id such as `footman`, used by `UnitId` and
    /// `GroupEnumUnitsOfType`.
    pub name: Option<String>,
    /// Hit points, as a bit pattern of a real.
    pub hp: u32,
    /// A building (`isbldg` in `UnitBalance.slk`).
    pub building: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectData {
    pub units: BTreeMap<i32, UnitType>,
    /// Hit points of destructable and item types, as bit patterns.
    pub destructables: BTreeMap<i32, u32>,
    pub items: BTreeMap<i32, u32>,
    unit_names: BTreeMap<String, i32>,
}

/// A row of an SLK table: the raw code and the text of the columns asked
/// for.
type Row = (i32, Vec<Option<String>>);

/// Rows of an SLK table by the raw code in `key`, with the text of
/// `columns` (`None` where a cell is missing).
fn rows(
    archives: &mut Archives,
    path: &str,
    key: &str,
    columns: &[&str],
) -> Result<Vec<Row>, MapError> {
    let Some(data) = archives.read(path) else {
        return Ok(Vec::new());
    };
    let table = Table::parse(&data).map_err(|e| MapError(format!("{path}: {e}")))?;
    let Some(key) = table.column(key) else {
        return Ok(Vec::new());
    };
    let columns: Vec<Option<u32>> = columns.iter().map(|c| table.column(c)).collect();
    let mut out = Vec::new();
    for row in 2..=table.last_row() {
        let Some(id) = table.get(row, key) else {
            continue;
        };
        let Ok(bytes) = <[u8; 4]>::try_from(id.raw().as_bytes()) else {
            continue;
        };
        let values = columns
            .iter()
            .map(|c| {
                c.and_then(|c| table.get(row, c))
                    .map(|v| v.raw().to_owned())
            })
            .collect();
        out.push((raw_code_value(RawCode(bytes)), values));
    }
    Ok(out)
}

fn real(text: Option<&String>) -> u32 {
    text.and_then(|t| t.parse::<f32>().ok())
        .unwrap_or(0.0)
        .to_bits()
}

/// The map's custom types of `kind`: (new id, base id, hit points if set).
fn custom(
    archives: &mut Archives,
    kind: ObjectKind,
    hp_field: &[u8; 4],
) -> Result<Vec<(i32, i32, Option<u32>)>, MapError> {
    let Some(data) = archives.read_map_file(kind.file_name()) else {
        return Ok(Vec::new());
    };
    let mods = ObjectModifications::parse(&data, kind)
        .map_err(|e| MapError(format!("{}: {e}", kind.file_name())))?;
    Ok(mods
        .custom
        .iter()
        .filter_map(|d| {
            let id = d.id?;
            let hp = d
                .modifications
                .iter()
                .rev()
                .find(|m| m.field.0 == *hp_field)
                .and_then(|m| match m.value {
                    Value::Integer(i) => Some((i as f32).to_bits()),
                    Value::Real(r) | Value::Unreal(r) => Some(r.to_bits()),
                    Value::String(_) => None,
                });
            Some((raw_code_value(id), raw_code_value(d.base), hp))
        })
        .collect())
}

impl ObjectData {
    pub fn load(archives: &mut Archives) -> Result<ObjectData, MapError> {
        let mut data = ObjectData::default();
        for (id, values) in rows(archives, "Units\\unitUI.slk", "unitUIID", &["name"])? {
            data.units.insert(
                id,
                UnitType {
                    name: values[0].clone().filter(|n| !n.is_empty()),
                    hp: 0,
                    building: false,
                },
            );
        }
        for (id, values) in rows(
            archives,
            "Units\\UnitBalance.slk",
            "unitBalanceID",
            &["HP", "isbldg"],
        )? {
            if let Some(unit) = data.units.get_mut(&id) {
                unit.hp = real(values[0].as_ref());
                unit.building = values[1].as_deref() == Some("1");
            }
        }
        for (id, values) in rows(
            archives,
            "Units\\DestructableData.slk",
            "DestructableID",
            &["HP"],
        )? {
            data.destructables.insert(id, real(values[0].as_ref()));
        }
        for (id, values) in rows(archives, "Units\\ItemData.slk", "itemID", &["HP"])? {
            data.items.insert(id, real(values[0].as_ref()));
        }
        for (id, base, hp) in custom(archives, ObjectKind::Units, b"uhpm")? {
            let mut unit = data.units.get(&base).cloned().unwrap_or(UnitType {
                name: None,
                hp: 0,
                building: false,
            });
            unit.hp = hp.unwrap_or(unit.hp);
            data.units.insert(id, unit);
        }
        for (id, base, hp) in custom(archives, ObjectKind::Destructibles, b"bhps")? {
            let base_hp = data.destructables.get(&base).copied().unwrap_or(0);
            data.destructables.insert(id, hp.unwrap_or(base_hp));
        }
        for (id, base, hp) in custom(archives, ObjectKind::Items, b"ihtp")? {
            let base_hp = data.items.get(&base).copied().unwrap_or(0);
            data.items.insert(id, hp.unwrap_or(base_hp));
        }
        for (&id, unit) in &data.units {
            if let Some(name) = &unit.name {
                data.unit_names.entry(name.clone()).or_insert(id);
            }
        }
        Ok(data)
    }

    /// The unit type with string id `name`, as `UnitId` finds it.
    /// PROVISIONAL: the comparison ignores ASCII case.
    pub fn unit_id(&self, name: &str) -> Option<i32> {
        self.unit_names.get(&name.to_ascii_lowercase()).copied()
    }

    /// The string id of a unit type, as `UnitId2String` gives it.
    pub fn unit_name(&self, id: i32) -> Option<&str> {
        self.units.get(&id)?.name.as_deref()
    }
}
