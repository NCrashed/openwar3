//! Loading a map: its scripts and the map files the game is set up from.
//!
//! Files are looked up in the map archive first, then in the game archives
//! from the newest to the oldest: `War3Patch.mpq`, `War3xlocal.mpq`,
//! `War3x.mpq`, `War3local.mpq`, `War3.mpq`. Archives that are missing are
//! skipped.
//!
//! PROVISIONAL, listed in specs/QUESTIONS.md: a map may replace
//! `Scripts\common.j` and `Scripts\Blizzard.j` by its own copies; the world
//! bounds span the whole terrain grid; destructables are the placed
//! doodads whose type is in `Units\DestructableData.slk` or is a custom
//! destructable of the map.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ow3_formats::binary::RawCode;
use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::info::MapInfo;
use ow3_formats::map::objects::{ObjectKind, ObjectModifications};
use ow3_formats::map::strings::TriggerStrings;
use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::Archive;
use ow3_formats::slk::Table;
use ow3_jass::ast::Script;
use ow3_jass::check::Program;

/// Game archives by priority, highest first.
pub const GAME_ARCHIVES: [&str; 5] = [
    "War3Patch.mpq",
    "War3xlocal.mpq",
    "War3x.mpq",
    "War3local.mpq",
    "War3.mpq",
];

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

/// The map archive and the game archives, searched in order.
pub struct Archives {
    archives: Vec<Archive<BufReader<File>>>,
}

impl Archives {
    /// Opens `map` and the game archives found in `game_data`.
    pub fn open(game_data: &Path, map: &Path) -> Result<Archives, MapError> {
        let mut archives =
            vec![Archive::open_path(map).map_err(|e| error(&map.display().to_string(), e))?];
        for name in GAME_ARCHIVES {
            let path = game_data.join(name);
            if path.exists() {
                archives.push(Archive::open_path(&path).map_err(|e| error(name, e))?);
            }
        }
        Ok(Archives { archives })
    }

    /// The first copy of `name`, or `None` if no archive has it.
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.archives
            .iter_mut()
            .find(|a| a.contains(name))
            .and_then(|a| a.read(name).ok())
    }

    /// `name` from the map archive only.
    pub fn read_map_file(&mut self, name: &str) -> Option<Vec<u8>> {
        let map = &mut self.archives[0];
        map.contains(name).then(|| map.read(name).ok()).flatten()
    }
}

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
        let mut archives = Archives::open(game_data, map)?;
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
        let destructables = destructables(archives)?;
        Ok(Map {
            info,
            strings,
            scripts: [common, blizzard, script],
            destructables,
            world_bounds,
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

/// Destructable types: the standard ones and the map's custom ones.
fn destructable_types(archives: &mut Archives) -> Result<BTreeSet<RawCode>, MapError> {
    let mut types = BTreeSet::new();
    if let Some(data) = archives.read("Units\\DestructableData.slk") {
        let table = Table::parse(&data).map_err(|e| error("DestructableData.slk", e))?;
        if let Some(column) = table.column("DestructableID") {
            for row in 2..=table.last_row() {
                if let Some(id) = table.get(row, column)
                    && let Ok(bytes) = <[u8; 4]>::try_from(id.raw().as_bytes())
                {
                    types.insert(RawCode(bytes));
                }
            }
        }
    }
    let kind = ObjectKind::Destructibles;
    if let Some(data) = archives.read_map_file(kind.file_name()) {
        let mods = ObjectModifications::parse(&data, kind).map_err(|e| error("war3map.w3b", e))?;
        types.extend(mods.custom.iter().filter_map(|d| d.id));
    }
    Ok(types)
}

fn destructables(archives: &mut Archives) -> Result<Vec<PlacedDestructable>, MapError> {
    let Some(data) = archives.read_map_file("war3map.doo") else {
        return Ok(Vec::new());
    };
    let doodads = Doodads::parse(&data).map_err(|e| error("war3map.doo", e))?;
    let types = destructable_types(archives)?;
    Ok(doodads
        .doodads
        .iter()
        .filter(|d| types.contains(&d.kind))
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
