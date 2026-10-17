//! Loading a map: its scripts and the map files the game is set up from.
//!
//! Files are looked up in the map archive first, then in the game archives
//! by priority (SPEC-0008): `War3Patch.mpq`, `War3xLocal.mpq`, `War3x.mpq`,
//! `War3.mpq`. Archives that are missing are skipped. A map may replace
//! `Scripts\common.j` and `Scripts\Blizzard.j`. The world bounds span the
//! whole terrain grid (SPEC-0008), and destructables are the placed
//! doodads whose type is a destructable type (SPEC-0006).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use ow3_formats::binary::RawCode;
use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::info::MapInfo;
use ow3_formats::map::strings::TriggerStrings;
use ow3_formats::map::terrain::Terrain;
use ow3_jass::ast::Script;
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
    /// Damage constants (SPEC-0015).
    pub combat: crate::combat::Combat,
    /// Unit, destructable and item types.
    pub data: ObjectData,
    /// The string lists of the interface files, for `GetLocalizedString`.
    pub localized: BTreeMap<String, String>,
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
        // The melee flag of war3map.w3i chooses the constants (SPEC-0015).
        let combat = crate::combat::Combat::load(archives, info.flags & 0x04 != 0);
        let data = ObjectData::load(archives)?;
        let localized = localized_strings(archives);
        let destructables = destructables(archives, &data)?;
        Ok(Map {
            info,
            strings,
            scripts: [common, blizzard, script],
            destructables,
            world_bounds,
            heights,
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
