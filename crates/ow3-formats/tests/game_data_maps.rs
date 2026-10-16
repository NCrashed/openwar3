//! Parses files of the maps shipped with a Warcraft III 1.27b installation.
//! Expects `game-data/1.27b/Maps/` (the game's `Maps` folder, or a symlink to
//! it); each test passes without checking anything when it is absent.

use std::path::{Path, PathBuf};

use ow3_formats::binary::RawCode;
use ow3_formats::map;
use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::info::MapInfo;
use ow3_formats::map::objects::{ObjectKind, ObjectModifications, Value};
use ow3_formats::map::strings::TriggerStrings;
use ow3_formats::map::terrain::Terrain;
use ow3_formats::map::units::PlacedUnits;
use ow3_formats::mpq::Archive;
use ow3_formats::slk::Table;

fn maps() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/Maps");
    if !root.exists() {
        eprintln!("skipping: {} not found", root.display());
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_map(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn is_map(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("w3m") || e.eq_ignore_ascii_case("w3x"))
}

/// Calls `check` with the contents of `file` in every map that has it.
fn for_each_map_file(file: &str, mut check: impl FnMut(&Path, &[u8])) -> usize {
    let mut count = 0;
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        if let Ok(data) = archive.read(file) {
            check(&path, &data);
            count += 1;
        }
    }
    count
}

#[test]
fn every_map_info_parses_completely() {
    let mut failures = Vec::new();
    let count = for_each_map_file("war3map.w3i", |path, data| {
        if let Err(err) = MapInfo::parse(data) {
            failures.push(format!("{}: {err}", path.display()));
        }
    });
    if maps().is_empty() {
        return;
    }
    eprintln!("parsed war3map.w3i of {count} maps");
    assert!(count > 100, "only {count} maps have war3map.w3i");
    assert!(
        failures.is_empty(),
        "{} of {count} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_map_name_resolves_through_trigger_strings() {
    if maps().is_empty() {
        return;
    }
    let mut checked = 0;
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        let info = MapInfo::parse(&archive.read("war3map.w3i").unwrap()).unwrap();
        let strings = TriggerStrings::parse(&archive.read("war3map.wts").unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for value in [&info.name, &info.author, &info.recommended_players] {
            if value.starts_with("TRIGSTR_") {
                let text = strings
                    .resolve(value)
                    .unwrap_or_else(|| panic!("{}: {value} is not defined", path.display()));
                assert!(!text.is_empty(), "{}: {value} is empty", path.display());
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "only {checked} references checked");
}

#[test]
fn every_terrain_parses_and_matches_map_info() {
    if maps().is_empty() {
        return;
    }
    let mut checked = 0;
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        let info = MapInfo::parse(&archive.read("war3map.w3i").unwrap()).unwrap();
        let terrain = Terrain::parse(&archive.read("war3map.w3e").unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let name = path.display();
        assert_eq!(terrain.tileset, info.tileset, "{name}: tileset");
        // The playable area plus the camera bound margins is the full map.
        let [left, right, bottom, top] = info.camera_bounds_complements;
        assert_eq!(
            terrain.width as i32 - 1,
            info.playable_width + left + right,
            "{name}: width"
        );
        assert_eq!(
            terrain.height as i32 - 1,
            info.playable_height + bottom + top,
            "{name}: height"
        );
        for point in &terrain.points {
            assert!(
                usize::from(point.ground_texture()) < terrain.ground_tilesets.len(),
                "{name}: texture index"
            );
        }
        checked += 1;
    }
    assert!(checked > 100);
}

#[test]
fn every_doodad_file_parses_completely() {
    let mut failures = Vec::new();
    let mut doodads = 0;
    let count = for_each_map_file("war3map.doo", |path, data| match Doodads::parse(data) {
        Ok(parsed) => doodads += parsed.doodads.len(),
        Err(err) => failures.push(format!("{}: {err}", path.display())),
    });
    if maps().is_empty() {
        return;
    }
    eprintln!("parsed {doodads} doodads in {count} maps");
    assert!(count > 100, "only {count} maps have war3map.doo");
    if !failures.is_empty() {
        panic!(
            "{} of {count} failed:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    // Positions must fall on the terrain: doodads in world units (128 per
    // tile from the terrain origin), special doodads in tiles.
    let mut outside = Vec::new();
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        let terrain = Terrain::parse(&archive.read("war3map.w3e").unwrap()).unwrap();
        let parsed = Doodads::parse(&archive.read("war3map.doo").unwrap()).unwrap();
        let max_x = terrain.origin_x + 128.0 * (terrain.width - 1) as f32;
        let max_y = terrain.origin_y + 128.0 * (terrain.height - 1) as f32;
        for d in &parsed.doodads {
            if !(terrain.origin_x..=max_x).contains(&d.x)
                || !(terrain.origin_y..=max_y).contains(&d.y)
            {
                outside.push(format!(
                    "{}: {} at ({}, {})",
                    path.display(),
                    d.kind,
                    d.x,
                    d.y
                ));
            }
        }
        for s in &parsed.special {
            if !(0..terrain.width as i32).contains(&s.x)
                || !(0..terrain.height as i32).contains(&s.y)
            {
                outside.push(format!(
                    "{}: special {} at ({}, {})",
                    path.display(),
                    s.kind,
                    s.x,
                    s.y
                ));
            }
        }
    }
    eprintln!("{} doodads outside the terrain", outside.len());
    assert!(
        outside.is_empty(),
        "{}",
        outside[..outside.len().min(20)].join("\n")
    );
}

#[test]
fn every_object_modification_file_parses_completely() {
    if maps().is_empty() {
        return;
    }
    let mut failures = Vec::new();
    let mut files = 0;
    let mut modifications = 0;
    for kind in ObjectKind::ALL {
        for_each_map_file(kind.file_name(), |path, data| {
            files += 1;
            match ObjectModifications::parse(data, kind) {
                Ok(parsed) => {
                    modifications += parsed
                        .original
                        .iter()
                        .chain(&parsed.custom)
                        .map(|o| o.modifications.len())
                        .sum::<usize>();
                }
                Err(err) => {
                    failures.push(format!("{} {}: {err}", path.display(), kind.file_name()))
                }
            }
        });
    }
    eprintln!("parsed {files} object files with {modifications} modifications");
    assert!(files > 50, "only {files} object files");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Field codes of the game's object metadata table for a kind, with the
/// value of their `type` column.
fn metadata_fields(kind: ObjectKind) -> Option<std::collections::BTreeMap<RawCode, String>> {
    let table = match kind {
        ObjectKind::Units | ObjectKind::Items => "Units\\UnitMetaData.slk",
        ObjectKind::Destructibles => "Units\\DestructableMetaData.slk",
        ObjectKind::Doodads => "Doodads\\DoodadMetaData.slk",
        ObjectKind::Abilities => "Units\\AbilityMetaData.slk",
        ObjectKind::Buffs => "Units\\AbilityBuffMetaData.slk",
        ObjectKind::Upgrades => "Units\\UpgradeMetaData.slk",
    };
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/War3Patch.mpq");
    let data = Archive::open_path(path).ok()?.read(table).ok()?;
    let table = Table::parse(&data).unwrap();
    let type_column = table.column("type").expect("type column");
    let mut fields = std::collections::BTreeMap::new();
    for row in 2..=table.last_row() {
        if let (Some(id), Some(ty)) = (table.get(row, 1), table.get(row, type_column)) {
            // Codes shorter than four characters, such as `Crs`, are stored
            // in object files padded with zero bytes.
            let id = id.raw().as_bytes();
            if id.is_empty() || id.len() > 4 {
                continue;
            }
            let mut bytes = [0u8; 4];
            bytes[..id.len()].copy_from_slice(id);
            fields.insert(RawCode(bytes), ty.raw().to_owned());
        }
    }
    Some(fields)
}

#[test]
fn every_modified_field_exists_in_game_metadata() {
    if maps().is_empty() {
        return;
    }
    let mut unknown = Vec::new();
    let mut types: std::collections::BTreeMap<(String, &'static str), usize> = Default::default();
    for kind in ObjectKind::ALL {
        let fields = metadata_fields(kind).expect("metadata table");
        for_each_map_file(kind.file_name(), |path, data| {
            let parsed = ObjectModifications::parse(data, kind).unwrap();
            for modification in parsed
                .original
                .iter()
                .chain(&parsed.custom)
                .flat_map(|o| &o.modifications)
            {
                let Some(meta_type) = fields.get(&modification.field) else {
                    unknown.push(format!(
                        "{} {}: {}",
                        path.display(),
                        kind.file_name(),
                        modification.field
                    ));
                    continue;
                };
                let value_type = match modification.value {
                    Value::Integer(_) => "integer",
                    Value::Real(_) => "real",
                    Value::Unreal(_) => "unreal",
                    Value::String(_) => "string",
                };
                *types.entry((meta_type.clone(), value_type)).or_default() += 1;
            }
        });
    }
    // Each metadata type is always stored with the same value type.
    let mut stored_as: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for (meta, value) in types.keys() {
        stored_as.entry(meta.as_str()).or_default().push(value);
    }
    let ambiguous: Vec<_> = stored_as
        .iter()
        .filter(|(_, values)| values.len() > 1)
        .collect();
    assert!(
        ambiguous.is_empty(),
        "metadata types stored in several ways: {ambiguous:?}"
    );
    assert_eq!(stored_as["real"], ["real"]);
    assert_eq!(stored_as["unreal"], ["unreal"]);
    assert_eq!(stored_as["int"], ["integer"]);
    assert_eq!(stored_as["string"], ["string"]);
    assert!(
        unknown.is_empty(),
        "{} unknown fields:\n{}",
        unknown.len(),
        unknown[..unknown.len().min(20)].join("\n")
    );
}

#[test]
fn every_unit_placement_parses_and_matches_map_info() {
    if maps().is_empty() {
        return;
    }
    let mut failures = Vec::new();
    let (mut files, mut units) = (0, 0);
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        let Ok(data) = archive.read("war3mapUnits.doo") else {
            continue;
        };
        files += 1;
        let placed = match PlacedUnits::parse(&data) {
            Ok(placed) => placed,
            Err(err) => {
                failures.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        units += placed.units.len();
        // Every player of the map info has its start location placed where
        // the map info says.
        let info = MapInfo::parse(&archive.read("war3map.w3i").unwrap()).unwrap();
        for player in &info.players {
            let found = placed.units.iter().any(|u| {
                u.kind == RawCode(*b"sloc")
                    && u.owner == player.number
                    && u.x == player.start_x
                    && u.y == player.start_y
            });
            if !found {
                failures.push(format!(
                    "{}: no start location for player {}",
                    path.display(),
                    player.number
                ));
            }
        }
    }
    eprintln!("parsed {units} placed units in {files} maps");
    assert!(files > 100, "only {files} unit files");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
}

#[test]
fn every_region_camera_and_sound_file_parses_completely() {
    if maps().is_empty() {
        return;
    }
    let mut failures = Vec::new();
    let (mut regions, mut cameras, mut sounds) = (0, 0, 0);
    let mut sound_names = Vec::new();
    for path in maps() {
        let mut archive = Archive::open_path(&path).unwrap();
        let name = path.display().to_string();
        let mut names_here = std::collections::BTreeSet::new();
        if let Ok(data) = archive.read("war3map.w3s") {
            match map::sounds::parse(&data) {
                Ok(parsed) => {
                    sounds += parsed.len();
                    names_here.extend(parsed.into_iter().map(|s| s.name));
                }
                Err(err) => failures.push(format!("{name} w3s: {err}")),
            }
        }
        if let Ok(data) = archive.read("war3map.w3c") {
            match map::cameras::parse(&data) {
                Ok(parsed) => cameras += parsed.len(),
                Err(err) => failures.push(format!("{name} w3c: {err}")),
            }
        }
        if let Ok(data) = archive.read("war3map.w3r") {
            match map::regions::parse(&data) {
                Ok(parsed) => {
                    regions += parsed.len();
                    for region in &parsed {
                        if region.left > region.right || region.bottom > region.top {
                            failures.push(format!("{name}: region {} is inverted", region.name));
                        }
                        if region.end != 0xFF {
                            failures.push(format!(
                                "{name}: region {} ends with {:#x}",
                                region.name, region.end
                            ));
                        }
                        // An ambient sound refers to a sound of the same map.
                        if !region.ambient_sound.is_empty() {
                            sound_names.push(names_here.contains(&region.ambient_sound));
                        }
                    }
                }
                Err(err) => failures.push(format!("{name} w3r: {err}")),
            }
        }
    }
    let resolved = sound_names.iter().filter(|&&found| found).count();
    eprintln!(
        "parsed {regions} regions, {cameras} cameras, {sounds} sounds; \
         {resolved} of {} region ambient sounds found in w3s",
        sound_names.len()
    );
    assert!(regions > 0 && cameras > 0 && sounds > 0);
    assert_eq!(
        resolved,
        sound_names.len(),
        "region ambient sounds not in w3s"
    );
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Edge case of SPEC-0003: entry 681 of this map is a header followed by
/// `#VALUE!`; it shows as empty text and does not affect the entries after it.
#[test]
fn death_sheep_malformed_entry_matches_spec_0003() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-data/1.27b/Maps/Scenario/(1)TheDeathSheep.w3m");
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return;
    }
    let data = Archive::open_path(&path)
        .unwrap()
        .read("war3map.wts")
        .unwrap();
    let strings = TriggerStrings::parse(&data).unwrap();
    assert_eq!(strings.display("TRIGSTR_681"), "");
    for n in 682..=684 {
        assert!(
            !strings.display(&format!("TRIGSTR_{n}")).is_empty(),
            "entry {n}"
        );
    }
}
