//! Object types the simulation needs from the game data: units,
//! destructables and items, with the map's custom types (SPEC-0009).
//!
//! Standard types come from `Units\unitUI.slk` (the unit's string id such
//! as `footman`, column `name`), `Units\UnitBalance.slk` (`HP`, plus 25
//! per point of `STR` for heroes), `Units\DestructableData.slk` (`HP`) and
//! `Units\ItemData.slk` (`HP`). A custom type of the map (`war3map.w3u`,
//! `w3b`, `w3t`) copies its base type and takes the hit points it sets
//! (`uhpm`, `ustr`, `bhps`, `ihtp`); a custom unit has the string id
//! `custom_<raw code>`, a custom hero none.
//!
//! A type is a hero when its raw code starts with an upper-case letter.
//! PROVISIONAL, listed in specs/QUESTIONS.md: when several types share a
//! string id, the lowest raw code is the one `UnitId` finds.

use std::collections::BTreeMap;

use ow3_formats::binary::RawCode;
use ow3_formats::map::objects::{ObjectKind, ObjectModifications, Value};
use ow3_formats::slk::Table;

use crate::map::{Archives, MapError, raw_code_value};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitType {
    /// String id such as `footman`, used by `UnitId` and
    /// `GroupEnumUnitsOfType`.
    pub name: Option<String>,
    /// Hit points, as a bit pattern of a real.
    pub hp: u32,
    /// A building (`isbldg` in `UnitBalance.slk`).
    pub building: bool,
    /// Max mana and the mana a new unit has.
    pub max_mana: u32,
    pub start_mana: u32,
    /// Life regeneration per second under `regen_type`, and for heroes the
    /// part from strength, which always applies (SPEC-0013).
    pub life_regen: u32,
    pub regen_type: RegenType,
    pub hero_life_regen: u32,
    /// Mana regeneration per second.
    pub mana_regen: u32,
    /// How the unit turns (`orientInterp` of `UnitData.slk`, SPEC-0013).
    pub orient_interp: u8,
}

/// When a type regenerates life (`regenType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RegenType {
    #[default]
    Always,
    Night,
    Day,
    Blight,
    Never,
}

impl RegenType {
    fn parse(text: &str) -> RegenType {
        match text.trim().to_ascii_lowercase().as_str() {
            "night" => RegenType::Night,
            "day" => RegenType::Day,
            "blight" => RegenType::Blight,
            "none" => RegenType::Never,
            _ => RegenType::Always,
        }
    }
}

/// The balance numbers of a unit type that its starting state comes
/// from, as bit patterns of reals.
#[derive(Clone, Debug, Default, PartialEq)]
struct Balance {
    hp: u32,
    strength: u32,
    intelligence: u32,
    mana: u32,
    start_mana: u32,
    life_regen: u32,
    mana_regen: u32,
    regen_type: RegenType,
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

/// A custom type: its id, its base type's id and the values of the fields
/// asked for that it sets.
type Custom = (i32, i32, Vec<Option<u32>>);

/// The map's custom types of `kind`: new id, base id and the values of
/// `fields` it sets.
fn custom(
    archives: &mut Archives,
    kind: ObjectKind,
    fields: &[&[u8; 4]],
) -> Result<Vec<Custom>, MapError> {
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
            let values = fields
                .iter()
                .map(|field| {
                    d.modifications
                        .iter()
                        .rev()
                        .find(|m| m.field.0 == **field)
                        .and_then(|m| match m.value {
                            Value::Integer(i) => Some((i as f32).to_bits()),
                            Value::Real(r) | Value::Unreal(r) => Some(r.to_bits()),
                            Value::String(_) => None,
                        })
                })
                .collect();
            Some((raw_code_value(id), raw_code_value(d.base), values))
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
                    ..UnitType::default()
                },
            );
        }
        for (id, values) in rows(archives, "Units\\UnitData.slk", "unitID", &["orientInterp"])? {
            if let Some(unit) = data.units.get_mut(&id) {
                unit.orient_interp = values[0]
                    .as_deref()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
            }
        }
        // The balance numbers of every type (SPEC-0009, SPEC-0013).
        let mut balance = BTreeMap::new();
        for (id, values) in rows(
            archives,
            "Units\\UnitBalance.slk",
            "unitBalanceID",
            &[
                "HP",
                "STR",
                "INT",
                "manaN",
                "mana0",
                "regenHP",
                "regenMana",
                "regenType",
                "isbldg",
            ],
        )? {
            let b = Balance {
                hp: real(values[0].as_ref()),
                strength: real(values[1].as_ref()),
                intelligence: real(values[2].as_ref()),
                mana: real(values[3].as_ref()),
                start_mana: real(values[4].as_ref()),
                life_regen: real(values[5].as_ref()),
                mana_regen: real(values[6].as_ref()),
                regen_type: RegenType::parse(values[7].as_deref().unwrap_or("")),
            };
            if let Some(unit) = data.units.get_mut(&id) {
                apply(unit, id, &b);
                unit.building = values[8].as_deref() == Some("1");
            }
            balance.insert(id, b);
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
        // A custom unit has its own string id, "custom_" and its raw code,
        // or none for a hero, and takes its balance numbers from its base
        // unless it sets them (SPEC-0009, SPEC-0013). PROVISIONAL: a
        // custom regeneration type (`uhrt`) is not read.
        let fields: [&[u8; 4]; 7] = [
            b"uhpm", b"ustr", b"uint", b"umpm", b"umpi", b"uhpr", b"umpr",
        ];
        for (id, base, values) in custom(archives, ObjectKind::Units, &fields)? {
            let mut unit = data.units.get(&base).cloned().unwrap_or_default();
            unit.name = (!is_hero(id))
                .then(|| format!("custom_{}", String::from_utf8_lossy(&id.to_be_bytes())));
            let mut b = balance.get(&base).cloned().unwrap_or_default();
            let slots = [
                &mut b.hp,
                &mut b.strength,
                &mut b.intelligence,
                &mut b.mana,
                &mut b.start_mana,
                &mut b.life_regen,
                &mut b.mana_regen,
            ];
            for (slot, value) in slots.into_iter().zip(&values) {
                if let Some(v) = value {
                    *slot = *v;
                }
            }
            apply(&mut unit, id, &b);
            data.units.insert(id, unit);
        }
        for (id, base, values) in custom(archives, ObjectKind::Destructibles, &[b"bhps"])? {
            let base_hp = data.destructables.get(&base).copied().unwrap_or(0);
            data.destructables.insert(id, values[0].unwrap_or(base_hp));
        }
        for (id, base, values) in custom(archives, ObjectKind::Items, &[b"ihtp"])? {
            let base_hp = data.items.get(&base).copied().unwrap_or(0);
            data.items.insert(id, values[0].unwrap_or(base_hp));
        }
        for (&id, unit) in &data.units {
            if let Some(name) = &unit.name {
                data.unit_names
                    .entry(name.to_ascii_lowercase())
                    .or_insert(id);
            }
        }
        Ok(data)
    }

    /// The unit type with string id `name`, as `UnitId` finds it.
    /// The comparison ignores ASCII case (SPEC-0009).
    pub fn unit_id(&self, name: &str) -> Option<i32> {
        self.unit_names.get(&name.to_ascii_lowercase()).copied()
    }

    /// The string id of a unit type, as `UnitId2String` gives it.
    pub fn unit_name(&self, id: i32) -> Option<&str> {
        self.units.get(&id)?.name.as_deref()
    }
}

/// A type is a hero exactly when its raw code starts with `A`..`Z`,
/// whatever its data (SPEC-0009).
pub fn is_hero(id: i32) -> bool {
    id.to_be_bytes()[0].is_ascii_uppercase()
}

/// Max life of a new unit of type `id`: `hp`, plus 25 per point of
/// `strength` for a hero (SPEC-0009). Both are bit patterns of whole
/// numbers.
fn life(id: i32, hp: u32, strength: u32) -> u32 {
    if is_hero(id) {
        let bonus = 25.0 * f32::from_bits(strength);
        (f32::from_bits(hp) + bonus).to_bits()
    } else {
        hp
    }
}

/// 0.05: life per second per point of strength, and mana per second per
/// point of intelligence, of a hero.
const PER_POINT_REGEN: u32 = 0x3D4C_CCCD;

/// The starting state of a unit of type `id` from its balance numbers
/// (SPEC-0009, SPEC-0013). A hero has 25 life and 15 mana per point of
/// strength and intelligence, starts with full mana, regenerates 0.05 life
/// per second per point of strength on top of `regenHP`, and 0.05 mana
/// per point of intelligence instead of `regenMana`.
fn apply(unit: &mut UnitType, id: i32, b: &Balance) {
    let hero = is_hero(id);
    unit.hp = life(id, b.hp, b.strength);
    unit.life_regen = b.life_regen;
    unit.regen_type = b.regen_type;
    if hero {
        let points = |bits: u32| f32::from_bits(bits) as i32;
        unit.max_mana = (f32::from_bits(b.mana) + 15.0 * f32::from_bits(b.intelligence)).to_bits();
        unit.start_mana = unit.max_mana;
        unit.hero_life_regen = ow3_math::mul(ow3_math::i2r(points(b.strength)), PER_POINT_REGEN);
        unit.mana_regen = ow3_math::mul(ow3_math::i2r(points(b.intelligence)), PER_POINT_REGEN);
    } else {
        unit.max_mana = b.mana;
        unit.start_mana = if f32::from_bits(b.start_mana) < f32::from_bits(b.mana) {
            b.start_mana
        } else {
            b.mana
        };
        unit.hero_life_regen = 0;
        unit.mana_regen = b.mana_regen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(c: &[u8; 4]) -> i32 {
        i32::from_be_bytes(*c)
    }

    /// The table of SPEC-0009 "Heroes".
    #[test]
    fn hero_life_and_codes() {
        let bits = |v: f32| v.to_bits();
        // The paladin: HP 100, strength 22.
        assert_eq!(life(code(b"Hpal"), bits(100.0), bits(22.0)), bits(650.0));
        // A non-hero code on paladin data gets no strength bonus.
        assert_eq!(life(code(b"h003"), bits(100.0), bits(22.0)), bits(100.0));
        // A hero code on footman data: strength 0.
        assert_eq!(life(code(b"Hf00"), bits(420.0), 0), bits(420.0));
        // uhpm 200 on the paladin, ustr 10 on a footman-based hero.
        assert_eq!(life(code(b"H001"), bits(200.0), bits(22.0)), bits(750.0));
        assert_eq!(life(code(b"Hf01"), bits(420.0), bits(10.0)), bits(670.0));
        for (c, hero) in [
            (0x40, false),
            (0x41, true),
            (0x5A, true),
            (0x5B, false),
            (0x30, false),
            (0x61, false),
            (0x7A, false),
            (0xC0, false),
            (0x20, false),
            (0x00, false),
        ] {
            assert_eq!(
                is_hero(i32::from_be_bytes([c, b'z', b'z', b'z'])),
                hero,
                "{c:#x}"
            );
        }
    }
}
