//! Parses files of the maps shipped with a Warcraft III 1.27b installation.
//! Expects `game-data/1.27b/Maps/` (the game's `Maps` folder, or a symlink to
//! it); each test passes without checking anything when it is absent.

use std::path::{Path, PathBuf};

use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::info::MapInfo;
use ow3_formats::map::strings::TriggerStrings;
use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::Archive;

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
