//! Loading a map: its scripts and the map files the game is set up from.
//!
//! Files are looked up in the map archive first, then in the game archives
//! by priority (SPEC-0008): `War3Patch.mpq`, `War3xLocal.mpq`, `War3x.mpq`,
//! `War3.mpq`. Archives that are missing are skipped. A map may replace
//! `Scripts\common.j` and `Scripts\Blizzard.j`. The world bounds span the
//! whole terrain grid (SPEC-0008), and destructables are the placed
//! doodads whose type is a destructable type (SPEC-0006).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use ow3_formats::binary::RawCode;
use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::info::MapInfo;
use ow3_formats::map::strings::TriggerStrings;
use ow3_formats::map::terrain::Terrain;
use ow3_jass::ast::{Declaration, Script};
use ow3_jass::check::Program;

use crate::data::ObjectData;

/// Size of a terrain cell in world units.
const CELL: f32 = 128.0;

#[derive(Debug)]
pub struct MapError(pub String);

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MapError {}

fn error(context: &str, e: impl fmt::Display) -> MapError {
    MapError(format!("{context}: {e}"))
}

/// The map archive and the game archives, searched by priority.
pub use ow3_formats::mpq::ArchiveChain as Archives;

/// A destructable placed in `war3map.doo`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedDestructable {
    /// The type as a JASS integer, `'LTlt'` and the like.
    pub type_id: i32,
    pub variation: i32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Facing in radians, as stored.
    pub angle: f32,
    pub life_percent: u8,
}

/// An axis-aligned rectangle in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// Everything the game needs from a map before it starts.
pub struct Map {
    pub info: MapInfo,
    pub strings: TriggerStrings,
    /// `common.j`, `Blizzard.j` and the map's `war3map.j`.
    pub scripts: [Script; 3],
    pub destructables: Vec<PlacedDestructable>,
    pub world_bounds: Bounds,
    /// Ground heights (SPEC-0014).
    pub heights: crate::heights::Heights,
    /// Where ground units may walk (SPEC-0020), if the map has a
    /// `war3map.wpm`.
    pub pathing: Option<crate::path::Grid>,
    /// Where amphibious units may move: the same map with that move
    /// kind's mask (hub #14).
    pub amphibious: Option<crate::path::Grid>,
    /// Damage constants (SPEC-0015).
    pub combat: crate::combat::Combat,
    /// Unit, destructable and item types.
    pub data: ObjectData,
    /// The string lists of the interface files, for `GetLocalizedString`.
    pub localized: BTreeMap<String, String>,
}

/// Natives the game provides that neither `common.j` nor `common.ai`
/// declares (SPEC-0001).
const UNDECLARED_NATIVES: [&str; 6] = [
    "DialogSetAsync",
    "SetStackedSound",
    "ClearStackedSound",
    "SetStackedSoundRect",
    "ClearStackedSoundRect",
    "DebugBreak",
];

fn native_names(script: &Script) -> impl Iterator<Item = &str> {
    script.declarations.iter().filter_map(|d| match d {
        Declaration::Native(n) => Some(n.signature.name.as_str()),
        _ => None,
    })
}

/// Fails for the first native `map` declares that neither the `provided`
/// scripts declare nor is one of [`UNDECLARED_NATIVES`].
fn check_natives(map: &Script, provided: &[&Script]) -> Result<(), MapError> {
    let mut names: BTreeSet<&str> = UNDECLARED_NATIVES.into_iter().collect();
    for script in provided {
        names.extend(native_names(script));
    }
    match native_names(map).find(|n| !names.contains(n)) {
        Some(name) => Err(MapError(format!(
            "war3map.j: the game has no native {name}"
        ))),
        None => Ok(()),
    }
}

fn parse_script(name: &str, data: &[u8]) -> Result<Script, MapError> {
    let text = String::from_utf8_lossy(data);
    ow3_jass::parse(&text).map_err(|e| error(name, e))
}

/// The JASS integer of a raw code: its four characters, first one highest.
pub fn raw_code_value(code: RawCode) -> i32 {
    i32::from_be_bytes(code.0)
}

impl Map {
    /// Loads the map at `map` with the game archives in `game_data`.
    pub fn load(game_data: &Path, map: &Path) -> Result<Map, MapError> {
        let mut archives =
            Archives::open(game_data, map).map_err(|e| error(&map.display().to_string(), e))?;
        Map::from_archives(&mut archives)
    }

    pub fn from_archives(archives: &mut Archives) -> Result<Map, MapError> {
        let war3map = archives
            .read_map_file("war3map.j")
            .or_else(|| archives.read_map_file("Scripts\\war3map.j"))
            .ok_or_else(|| MapError("the map has no war3map.j".into()))?;
        let mut library = |name: &str| {
            archives
                .read(name)
                .ok_or_else(|| MapError(format!("{name} not found")))
                .and_then(|data| parse_script(name, &data))
        };
        let common = library("Scripts\\common.j")?;
        let blizzard = library("Scripts\\Blizzard.j")?;
        let script = parse_script("war3map.j", &war3map)?;
        // A map may declare only natives the game provides (SPEC-0001):
        // those of the game's own `common.j` and `common.ai`, whatever
        // file the map uses, and a few undeclared ones.
        let provided = ["Scripts\\common.j", "Scripts\\common.ai"]
            .iter()
            .map(|&name| {
                archives
                    .read_game_file(name)
                    .map(|data| parse_script(name, &data))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let [Some(common_j), Some(common_ai)] = &provided[..] {
            check_natives(&script, &[common_j, common_ai])?;
        }

        let info = archives
            .read_map_file("war3map.w3i")
            .ok_or_else(|| MapError("the map has no war3map.w3i".into()))?;
        let info = MapInfo::parse(&info).map_err(|e| error("war3map.w3i", e))?;
        let strings = match archives.read_map_file("war3map.wts") {
            Some(data) => TriggerStrings::parse(&data).map_err(|e| error("war3map.wts", e))?,
            None => TriggerStrings::default(),
        };
        let terrain = archives
            .read_map_file("war3map.w3e")
            .ok_or_else(|| MapError("the map has no war3map.w3e".into()))?;
        let terrain = Terrain::parse(&terrain).map_err(|e| error("war3map.w3e", e))?;
        let world_bounds = Bounds {
            min_x: terrain.origin_x,
            min_y: terrain.origin_y,
            max_x: terrain.origin_x + (terrain.width.saturating_sub(1)) as f32 * CELL,
            max_y: terrain.origin_y + (terrain.height.saturating_sub(1)) as f32 * CELL,
        };
        let heights = crate::heights::Heights::new(&terrain);
        let pathing = archives
            .read_map_file("war3map.wpm")
            .map(|d| {
                ow3_formats::map::pathing::Pathing::parse(&d).map_err(|e| error("war3map.wpm", e))
            })
            .transpose()?;
        let amphibious = pathing.as_ref().map(crate::path::Grid::amphibious);
        let pathing = pathing.map(|p| crate::path::Grid::new(&p));
        // The melee flag of war3map.w3i chooses the constants (SPEC-0015).
        let combat = crate::combat::Combat::load(archives, info.flags & 0x04 != 0);
        let data = ObjectData::load(archives, info.flags & 0x04 != 0)?;
        let localized = localized_strings(archives);
        let destructables = destructables(archives, &data)?;
        Ok(Map {
            info,
            strings,
            scripts: [common, blizzard, script],
            destructables,
            world_bounds,
            heights,
            pathing,
            amphibious,
            combat,
            data,
            localized,
        })
    }

    pub fn script_refs(&self) -> Vec<&Script> {
        self.scripts.iter().collect()
    }

    /// Checks the scripts, as the game does before it runs them.
    pub fn check(&self) -> Result<Program, MapError> {
        ow3_jass::check::check(&self.script_refs()).map_err(|d| {
            let script = ["common.j", "Blizzard.j", "war3map.j"][d[0].script.min(2)];
            error(script, &d[0])
        })
    }
}

fn destructables(
    archives: &mut Archives,
    data: &ObjectData,
) -> Result<Vec<PlacedDestructable>, MapError> {
    let Some(file) = archives.read_map_file("war3map.doo") else {
        return Ok(Vec::new());
    };
    let doodads = Doodads::parse(&file).map_err(|e| error("war3map.doo", e))?;
    Ok(doodads
        .doodads
        .iter()
        .filter(|d| data.destructables.contains_key(&raw_code_value(d.kind)))
        .map(|d| PlacedDestructable {
            type_id: raw_code_value(d.kind),
            variation: d.variation,
            x: d.x,
            y: d.y,
            z: d.z,
            angle: d.angle,
            life_percent: d.life_percent,
        })
        .collect())
}

/// Files whose string lists `GetLocalizedString` reads (SPEC-0010).
pub const STRING_FILES: [&str; 4] = [
    "UI\\FrameDef\\GlobalStrings.fdf",
    "UI\\FrameDef\\InfoPanelStrings.fdf",
    "UI\\FrameDef\\DateStrings.fdf",
    "UI\\FrameDef\\NetworkStrings.fdf",
];

/// The entries of the string lists in [`STRING_FILES`]. PROVISIONAL: a
/// later definition replaces an earlier one, also across files (SPEC-0010
/// open question 1).
fn localized_strings(archives: &mut Archives) -> BTreeMap<String, String> {
    let mut strings = BTreeMap::new();
    for file in STRING_FILES {
        // A map cannot add or change these strings (SPEC-0010).
        if let Some(data) = archives.read_game_file(file) {
            let text = String::from_utf8_lossy(&data);
            strings.extend(ow3_formats::fdf::string_lists(&text));
        }
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(text: &str) -> Script {
        ow3_jass::parse(text).unwrap()
    }

    /// SPEC-0001: a map may declare natives of `common.j`, of `common.ai`
    /// and the undeclared ones, and fails to load with any other.
    #[test]
    fn maps_declare_provided_natives_only() {
        let common_j = script("native KillUnit takes unit whichUnit returns nothing\n");
        let common_ai = script("native UnitAlive takes unit id returns boolean\n");
        let provided = [&common_j, &common_ai];
        let map = |natives: &str| check_natives(&script(natives), &provided);
        assert!(map("native UnitAlive takes unit id returns boolean\nnative DebugBreak takes integer i returns nothing\nnative DialogSetAsync takes dialog d returns nothing\n").is_ok());
        assert!(map("native KillUnit takes unit u returns nothing\n").is_ok());
        assert!(map("native NoSuchNative takes nothing returns nothing\n").is_err());
    }
}
