//! Parses files of the maps shipped with a Warcraft III 1.27b installation.
//! Expects `game-data/1.27b/Maps/` (the game's `Maps` folder, or a symlink to
//! it); each test passes without checking anything when it is absent.

use std::path::{Path, PathBuf};

use ow3_formats::map::info::MapInfo;
use ow3_formats::map::strings::TriggerStrings;
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
