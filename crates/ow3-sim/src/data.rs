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
    /// The type has the data of a hero: it is a hero type of the game or
    /// a map's type based on one (SPEC-0009, skill points).
    pub hero_data: bool,
    /// Max mana and the mana a new unit has.
    pub max_mana: u32,
    pub start_mana: u32,
    /// Life regeneration per second under `regen_type`, and for heroes the
    /// part from strength, which always applies (SPEC-0013).
    pub life_regen: u32,
    pub regen_type: RegenType,
    pub hero_life_regen: u32,
    /// A hero's starting strength in points; 0 for others.
    pub strength: i32,
    /// Mana regeneration per second.
    pub mana_regen: u32,
    /// How the unit turns (`orientInterp` of `UnitData.slk`, SPEC-0013).
    pub orient_interp: u8,
    /// Armor value (`def`), armor type (`defType`, an index into
    /// [`crate::combat::ARMOR_TYPES`]) and a hero's agility (`AGI`),
    /// SPEC-0015.
    pub armor: u32,
    pub armor_type: Option<usize>,
    pub agility: u32,
    /// A hero's primary attribute (`Primary`: STR, AGI or INT), as a
    /// real; 0 for others.
    pub primary: u32,
    /// Weapon 1 (SPEC-0016), if the type has one.
    pub weapon: Option<Weapon>,
    /// Movement (SPEC-0018): speed (`spd`, units per second), turn rate
    /// (`turnRate`, radians per update of 0.03 s) and propulsion window
    /// (`propWin`, degrees), as reals.
    pub speed: u32,
    pub turn_rate: u32,
    pub prop_window: u32,
    /// The collision size (`collision`), as a real (SPEC-0019).
    pub collision: u32,
    /// How far the unit looks for enemies on its own (`acquire`, SPEC-0019).
    pub acquire: u32,
    /// The pathing texture of a building (`pathTex` of `UnitData.slk`,
    /// SPEC-0022), as a key of [`ObjectData::path_textures`].
    pub path_texture: Option<String>,
}

/// The pathing of a destructable type (`DestructableData.slk`,
/// SPEC-0022): textures alive and dead, as keys of
/// [`ObjectData::path_textures`], and `fixedRot` in degrees as a real
/// (negative: the facing given at creation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DestructablePath {
    pub alive: Option<String>,
    pub dead: Option<String>,
    pub fixed_rotation: u32,
}

/// A pathing texture: one pixel per cell, rows from the bottom; red
/// pixels close a cell for walking, green ones for flying (SPEC-0022).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathTexture {
    pub width: i32,
    pub height: i32,
    pub red: Vec<bool>,
    pub green: Vec<bool>,
}

impl PathTexture {
    fn from_tga(tga: &ow3_formats::tga::Tga) -> PathTexture {
        let pixels: Vec<[u8; 4]> = (0..tga.height)
            .flat_map(|y| (0..tga.width).map(move |x| (x, y)))
            .map(|(x, y)| tga.at(x, y))
            .collect();
        PathTexture {
            width: tga.width as i32,
            height: tga.height as i32,
            red: pixels.iter().map(|p| p[0] == 255).collect(),
            green: pixels.iter().map(|p| p[1] == 255).collect(),
        }
    }
}

/// The archive path of a pathing texture named in a table: `None` for
/// `_`, `none` or nothing; `.tga` added when the name has no extension.
fn path_texture_name(text: Option<&str>) -> Option<String> {
    let name = text?.trim();
    if name.is_empty() || name == "_" || name.eq_ignore_ascii_case("none") {
        return None;
    }
    let name = if name.to_ascii_lowercase().ends_with(".tga") {
        name.to_owned()
    } else {
        format!("{name}.tga")
    };
    Some(name.to_ascii_lowercase())
}

/// Weapon 1 of a unit type (`Units\UnitWeapons.slk`, SPEC-0016).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Weapon {
    /// `dmgplus1`, `dice1`, `sides1`.
    pub damage: i32,
    pub dice: i32,
    pub sides: i32,
    /// `cool1` and `dmgpt1` in seconds, `rangeN1`, as reals.
    pub cooldown: u32,
    pub damage_point: u32,
    /// `backSw1`: seconds from the damage to the end of the swing, as a
    /// real (SPEC-0019).
    pub backswing: u32,
    pub range: u32,
    /// The JASS attack type of `atkType1` (1 for `normal`, 2 `pierce`,
    /// ..., 0 `spells`).
    pub attack_type: i32,
}

/// The JASS attack type of an `atkType1` name, or `None` for no weapon.
fn attack_type(name: &str) -> Option<i32> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "spells" => 0,
        "normal" => 1,
        "pierce" => 2,
        "siege" => 3,
        "magic" => 4,
        "chaos" => 5,
        "hero" => 6,
        _ => return None,
    })
}

/// The index of an armor type name, or `None` for a name outside the
/// lists, such as `Wood` (SPEC-0015).
fn armor_type(name: &str) -> Option<usize> {
    let name = name.trim().to_ascii_lowercase();
    crate::combat::ARMOR_TYPES.iter().position(|t| *t == name)
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
    /// Hit points of destructable and item types, as bit patterns, and
    /// their armor types (`armor`, see [`UnitType::armor_type`]).
    pub destructables: BTreeMap<i32, u32>,
    pub items: BTreeMap<i32, u32>,
    pub destructable_armor: BTreeMap<i32, Option<usize>>,
    pub item_armor: BTreeMap<i32, Option<usize>>,
    /// The pathing of destructable types and the textures that buildings
    /// and destructables use, by lower-case archive path (SPEC-0022).
    pub destructable_paths: BTreeMap<i32, DestructablePath>,
    pub path_textures: BTreeMap<String, PathTexture>,
    unit_names: BTreeMap<String, i32>,
}

/// A row of an SLK table: the raw code and the text of the columns asked
/// for.
type Row = (i32, Vec<Option<String>>);

/// Rows of an SLK table by the raw code in `key`, with the text of
/// `columns` (`None` where a cell is missing).
fn rows(
    archives: &mut Archives,
    melee: bool,
    path: &str,
    key: &str,
    columns: &[&str],
) -> Result<Vec<Row>, MapError> {
    // PROVISIONAL: the map's own copy comes first, as for MiscGame.txt
    // (SPEC-0015); SPEC-0016 does not say.
    let data = archives
        .read_map_file(path)
        .or_else(|| {
            (!melee)
                .then(|| archives.read_game_file(&format!("Custom_V1\\{path}")))
                .flatten()
        })
        .or_else(|| archives.read_game_file(path));
    let Some(data) = data else {
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

/// A type the map changes or adds, with the values it sets (the last one
/// for a field set twice).
struct Definition {
    id: i32,
    base: i32,
    /// A new type of the map, rather than a change to a standard one.
    custom: bool,
    values: BTreeMap<[u8; 4], Value>,
}

impl Definition {
    /// A number field as a real.
    fn number(&self, field: &[u8; 4]) -> Option<u32> {
        match self.values.get(field)? {
            Value::Integer(i) => Some((*i as f32).to_bits()),
            Value::Real(r) | Value::Unreal(r) => Some(r.to_bits()),
            Value::String(_) => None,
        }
    }

    fn text(&self, field: &[u8; 4]) -> Option<&str> {
        match self.values.get(field)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// The map's changes to standard types of `kind`, then its own types, in
/// the order of the file.
fn definitions(archives: &mut Archives, kind: ObjectKind) -> Result<Vec<Definition>, MapError> {
    let Some(data) = archives.read_map_file(kind.file_name()) else {
        return Ok(Vec::new());
    };
    let mods = ObjectModifications::parse(&data, kind)
        .map_err(|e| MapError(format!("{}: {e}", kind.file_name())))?;
    let definition = |d: &ow3_formats::map::objects::ObjectDefinition, custom: bool| {
        let base = raw_code_value(d.base);
        let id = if custom { raw_code_value(d.id?) } else { base };
        let values = d
            .modifications
            .iter()
            .map(|m| (m.field.0, m.value.clone()))
            .collect();
        Some(Definition {
            id,
            base,
            custom,
            values,
        })
    };
    Ok(mods
        .original
        .iter()
        .filter_map(|d| definition(d, false))
        .chain(mods.custom.iter().filter_map(|d| definition(d, true)))
        .collect())
}

impl ObjectData {
    /// The types of a map from `archives`; `melee` maps read the `Units\`
    /// tables of the game, others their `Custom_V1` copies where they
    /// exist (SPEC-0016). A table imported into the map comes first.
    pub fn load(archives: &mut Archives, melee: bool) -> Result<ObjectData, MapError> {
        let mut data = ObjectData::default();
        for (id, values) in rows(archives, melee, "Units\\unitUI.slk", "unitUIID", &["name"])? {
            data.units.insert(
                id,
                UnitType {
                    name: values[0].clone().filter(|n| !n.is_empty()),
                    hero_data: is_hero(id),
                    ..UnitType::default()
                },
            );
        }
        for (id, values) in rows(
            archives,
            melee,
            "Units\\UnitData.slk",
            "unitID",
            &["orientInterp", "turnRate", "propWin", "pathTex"],
        )? {
            if let Some(unit) = data.units.get_mut(&id) {
                unit.orient_interp = values[0]
                    .as_deref()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                unit.turn_rate = real(values[1].as_ref());
                unit.prop_window = real(values[2].as_ref());
                unit.path_texture = path_texture_name(values[3].as_deref());
            }
        }
        // Weapon 1 (SPEC-0016). PROVISIONAL: reals are read like the other
        // numbers of the tables; how the game reads them is not specified.
        for (id, values) in rows(
            archives,
            melee,
            "Units\\UnitWeapons.slk",
            "unitWeapID",
            &[
                "dmgplus1", "dice1", "sides1", "cool1", "dmgpt1", "rangeN1", "atkType1", "acquire",
                "backSw1",
            ],
        )? {
            let Some(unit) = data.units.get_mut(&id) else {
                continue;
            };
            unit.acquire = real(values[7].as_ref());
            let int = |v: &Option<String>| v.as_deref().and_then(|t| t.trim().parse().ok());
            unit.weapon = values[6]
                .as_deref()
                .and_then(attack_type)
                .map(|attack_type| Weapon {
                    damage: int(&values[0]).unwrap_or(0),
                    dice: int(&values[1]).unwrap_or(0),
                    sides: int(&values[2]).unwrap_or(0),
                    cooldown: real(values[3].as_ref()),
                    damage_point: real(values[4].as_ref()),
                    backswing: real(values[8].as_ref()),
                    range: real(values[5].as_ref()),
                    attack_type,
                });
        }
        // The balance numbers of every type (SPEC-0009, SPEC-0013).
        let mut balance = BTreeMap::new();
        for (id, values) in rows(
            archives,
            melee,
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
                "def",
                "defType",
                "AGI",
                "Primary",
                "spd",
                "collision",
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
                unit.armor = real(values[9].as_ref());
                unit.armor_type = armor_type(values[10].as_deref().unwrap_or(""));
                unit.agility = real(values[11].as_ref());
                unit.speed = real(values[13].as_ref());
                unit.collision = real(values[14].as_ref());
                unit.primary = match values[12].as_deref() {
                    Some("STR") => b.strength,
                    Some("AGI") => unit.agility,
                    Some("INT") => b.intelligence,
                    _ => 0,
                };
            }
            balance.insert(id, b);
        }
        for (id, values) in rows(
            archives,
            melee,
            "Units\\DestructableData.slk",
            "DestructableID",
            &["HP", "armor", "pathTex", "pathTexDeath", "fixedRot"],
        )? {
            data.destructables.insert(id, real(values[0].as_ref()));
            data.destructable_paths.insert(
                id,
                DestructablePath {
                    alive: path_texture_name(values[2].as_deref()),
                    dead: path_texture_name(values[3].as_deref()),
                    fixed_rotation: real(values[4].as_ref()),
                },
            );
            data.destructable_armor
                .insert(id, armor_type(values[1].as_deref().unwrap_or("")));
        }
        for (id, values) in rows(
            archives,
            melee,
            "Units\\ItemData.slk",
            "itemID",
            &["HP", "armor"],
        )? {
            data.items.insert(id, real(values[0].as_ref()));
            data.item_armor
                .insert(id, armor_type(values[1].as_deref().unwrap_or("")));
        }
        // A custom unit has its own string id, "custom_" and its raw code,
        // or none for a hero, and takes its balance numbers from its base
        // unless it sets them (SPEC-0009, SPEC-0013).
        // Changes to standard types come first, then the map's own types.
        // PROVISIONAL: a custom type starts from its base as the map has
        // changed it.
        for def in definitions(archives, ObjectKind::Units)? {
            let mut unit = data.units.get(&def.base).cloned().unwrap_or_default();
            if def.custom {
                unit.name = (!is_hero(def.id))
                    .then(|| format!("custom_{}", String::from_utf8_lossy(&def.id.to_be_bytes())));
            }
            let mut b = balance.get(&def.base).cloned().unwrap_or_default();
            let slots: [(&[u8; 4], &mut u32); 7] = [
                (b"uhpm", &mut b.hp),
                (b"ustr", &mut b.strength),
                (b"uint", &mut b.intelligence),
                (b"umpm", &mut b.mana),
                (b"umpi", &mut b.start_mana),
                (b"uhpr", &mut b.life_regen),
                (b"umpr", &mut b.mana_regen),
            ];
            for (field, slot) in slots {
                if let Some(v) = def.number(field) {
                    *slot = v;
                }
            }
            if let Some(name) = def.text(b"uhrt") {
                b.regen_type = RegenType::parse(name);
            }
            apply(&mut unit, def.id, &b);
            unit.armor = def.number(b"udef").unwrap_or(unit.armor);
            unit.agility = def.number(b"uagi").unwrap_or(unit.agility);
            if let Some(name) = def.text(b"udty") {
                unit.armor_type = armor_type(name);
            }
            unit.speed = def.number(b"umvs").unwrap_or(unit.speed);
            unit.collision = def.number(b"ucol").unwrap_or(unit.collision);
            unit.acquire = def.number(b"uacq").unwrap_or(unit.acquire);
            if let Some(name) = def.text(b"upat") {
                unit.path_texture = path_texture_name(Some(name));
            }
            unit.turn_rate = def.number(b"umvr").unwrap_or(unit.turn_rate);
            unit.prop_window = def.number(b"uprw").unwrap_or(unit.prop_window);
            if let Some(orient) = def.number(b"uori") {
                unit.orient_interp = f32::from_bits(orient) as u8;
            }
            unit.primary = match def.text(b"upra") {
                Some(p) if p.eq_ignore_ascii_case("STR") => b.strength,
                Some(p) if p.eq_ignore_ascii_case("AGI") => unit.agility,
                Some(p) if p.eq_ignore_ascii_case("INT") => b.intelligence,
                Some(_) => 0,
                None => unit.primary,
            };
            // The weapon: a map can give one, change it or take it away.
            let attack = match def.text(b"ua1t") {
                Some(name) => attack_type(name),
                None => unit.weapon.as_ref().map(|w| w.attack_type),
            };
            unit.weapon = attack.map(|attack_type| {
                let mut weapon = unit.weapon.clone().unwrap_or_default();
                weapon.attack_type = attack_type;
                let int =
                    |field, old: i32| def.number(field).map_or(old, |v| f32::from_bits(v) as i32);
                weapon.damage = int(b"ua1b", weapon.damage);
                weapon.dice = int(b"ua1d", weapon.dice);
                weapon.sides = int(b"ua1s", weapon.sides);
                weapon.cooldown = def.number(b"ua1c").unwrap_or(weapon.cooldown);
                weapon.damage_point = def.number(b"udp1").unwrap_or(weapon.damage_point);
                weapon.backswing = def.number(b"ubs1").unwrap_or(weapon.backswing);
                weapon.range = def.number(b"ua1r").unwrap_or(weapon.range);
                weapon
            });
            balance.insert(def.id, b);
            data.units.insert(def.id, unit);
        }
        for def in definitions(archives, ObjectKind::Destructibles)? {
            let base_hp = data.destructables.get(&def.base).copied().unwrap_or(0);
            data.destructables
                .insert(def.id, def.number(b"bhps").unwrap_or(base_hp));
            let armor = data.destructable_armor.get(&def.base).copied().flatten();
            data.destructable_armor.insert(def.id, armor);
            let mut path = data
                .destructable_paths
                .get(&def.base)
                .cloned()
                .unwrap_or_default();
            if let Some(name) = def.text(b"bptx") {
                path.alive = path_texture_name(Some(name));
            }
            if let Some(name) = def.text(b"bptd") {
                path.dead = path_texture_name(Some(name));
            }
            path.fixed_rotation = def.number(b"bfxr").unwrap_or(path.fixed_rotation);
            data.destructable_paths.insert(def.id, path);
        }
        // The pathing textures these types use (SPEC-0022).
        let names: std::collections::BTreeSet<String> = data
            .units
            .values()
            .filter(|u| u.building)
            .filter_map(|u| u.path_texture.clone())
            .chain(
                data.destructable_paths
                    .values()
                    .flat_map(|p| [p.alive.clone(), p.dead.clone()])
                    .flatten(),
            )
            .collect();
        for name in names {
            if let Some(tga) = archives
                .read(&name)
                .and_then(|d| ow3_formats::tga::Tga::parse(&d).ok())
            {
                data.path_textures.insert(name, PathTexture::from_tga(&tga));
            }
        }
        for def in definitions(archives, ObjectKind::Items)? {
            let base_hp = data.items.get(&def.base).copied().unwrap_or(0);
            data.items
                .insert(def.id, def.number(b"ihtp").unwrap_or(base_hp));
            let armor = data.item_armor.get(&def.base).copied().flatten();
            data.item_armor.insert(def.id, armor);
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

/// A hero's life regeneration per second from `strength` points.
pub fn strength_regen(strength: i32) -> u32 {
    ow3_math::mul(ow3_math::i2r(strength), PER_POINT_REGEN)
}

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
        unit.strength = points(b.strength);
        unit.hero_life_regen = strength_regen(unit.strength);
        unit.mana_regen = ow3_math::mul(ow3_math::i2r(points(b.intelligence)), PER_POINT_REGEN);
    } else {
        unit.max_mana = b.mana;
        unit.start_mana = if f32::from_bits(b.start_mana) < f32::from_bits(b.mana) {
            b.start_mana
        } else {
            b.mana
        };
        unit.hero_life_regen = 0;
        unit.strength = 0;
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
