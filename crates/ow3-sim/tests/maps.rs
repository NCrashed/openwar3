//! Loads and starts every map of the installation in `game-data/1.27b/Maps`
//! (except the analysts' test maps) and runs it for 30 s of game time.
//! Passes without checking anything when the game data is absent.
//!
//! It takes a while, so it is ignored by default:
//! `cargo test --release -p ow3-sim --test maps -- --ignored --nocapture`.
//! `OW3_MAP=<part of a file name>` runs only the matching maps.

use std::path::{Path, PathBuf};

use ow3_sim::game::Game;
use ow3_sim::map::Map;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

fn maps(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // `Test` and `Test-ow3` hold the analysts' test maps.
            if name != "Test" && name != "Test-ow3" {
                maps(&path, found);
            }
        } else if (name.ends_with(".w3m") || name.ends_with(".w3x"))
            && !name.starts_with("ow3-")
            && std::env::var("OW3_MAP")
                .ok()
                .is_none_or(|m| name.contains(&m))
        {
            found.push(path);
        }
    }
}

#[test]
#[ignore]
fn every_map_loads_and_runs() {
    let mut found = Vec::new();
    maps(&game_data().join("Maps"), &mut found);
    found.sort();
    let mut failures = Vec::new();
    let mut stubs: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
    for path in &found {
        let name = path
            .strip_prefix(game_data())
            .unwrap()
            .display()
            .to_string();
        let map = match Map::load(&game_data(), path) {
            Ok(map) => map,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let program = match map.check() {
            Ok(program) => program,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut game = Game::load_map(&program, &map).unwrap();
        if let Err(e) = game.start() {
            failures.push(format!("{name}: {e}"));
            continue;
        }
        game.advance_seconds(30.0);
        for (native, calls) in game.vm.stub_calls() {
            let entry = stubs.entry(native.to_owned()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += calls;
        }
        let state = game.state.borrow();
        eprintln!(
            "{name}: {} destructables, {} lines, {} thread errors{}",
            map.destructables.len(),
            state.output.len(),
            state.errors.len(),
            state
                .errors
                .first()
                .map_or(String::new(), |e| format!(" (first: {e})")),
        );
    }
    eprintln!("{} maps", found.len());
    // Natives that are still stubs, by the number of maps that call them.
    let mut by_maps: Vec<(&String, &(usize, u64))> = stubs.iter().collect();
    by_maps.sort_by_key(|(name, (maps, calls))| {
        (std::cmp::Reverse(*maps), std::cmp::Reverse(*calls), *name)
    });
    for (name, (maps, calls)) in by_maps {
        eprintln!("stub {name}: {maps} maps, {calls} calls");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A two-player melee map (SPEC-0008): the local player in slot 0 and a
/// computer in slot 1 play, the other slots are empty with team -1,
/// `config` defines the start locations, and the trees are destructables.
#[test]
fn melee_map_setup() {
    let path = game_data().join("Maps/(2)BootyBay.w3m");
    if !path.exists() {
        return;
    }
    let map = Map::load(&game_data(), &path).unwrap();
    assert_eq!(map.info.players.len(), 2);
    assert!(map.destructables.len() > 100, "{}", map.destructables.len());
    let program = map.check().unwrap();
    let mut game = Game::load_map(&program, &map).unwrap();
    game.start().unwrap();
    let state = game.state.borrow();
    let setup = &state.setup;
    assert_eq!(setup.local_player, 0);
    assert_eq!(setup.present(), [0, 1, 12, 13, 14, 15]);
    assert_eq!(setup.enumerable(), [0, 1]);
    assert_eq!(setup.slots[1].name, "Computer (Normal)");
    assert_eq!(setup.slots[2].team, -1);
    assert_eq!(setup.map_flags & 32768, 32768);
    assert_eq!(setup.player_count, 2);
    for player in &map.info.players {
        // `config` passes them as literals (SPEC-0005), exact here.
        let (x, y) = setup.start_locations[&player.number];
        assert_eq!(
            (f32::from_bits(x), f32::from_bits(y)),
            (player.start_x, player.start_y)
        );
    }
    assert!(state.errors.is_empty(), "{:?}", state.errors);
}

/// Unit, destructable and item types from the game data.
#[test]
fn object_data() {
    let path = game_data().join("Maps/(2)BootyBay.w3m");
    if !path.exists() {
        return;
    }
    let map = Map::load(&game_data(), &path).unwrap();
    let data = &map.data;
    let hfoo = i32::from_be_bytes(*b"hfoo");
    assert_eq!(data.unit_id("footman"), Some(hfoo));
    assert_eq!(data.unit_id("Footman"), Some(hfoo));
    assert_eq!(data.unit_name(hfoo), Some("footman"));
    assert_eq!(f32::from_bits(data.units[&hfoo].hp), 420.0);
    // Heroes start with realHP (SPEC-0009).
    let hp = |code: &[u8; 4]| f32::from_bits(data.units[&i32::from_be_bytes(*code)].hp);
    assert_eq!(hp(b"Hpal"), 650.0);
    assert_eq!(hp(b"Ucrl"), 675.0);
    assert_eq!(data.unit_id("FOOTMAN"), Some(hfoo));
    assert_eq!(data.unit_name(i32::from_be_bytes(*b"zzzz")), None);
    assert!(data.units.len() > 500, "{}", data.units.len());
    assert!(
        data.destructables
            .contains_key(&i32::from_be_bytes(*b"LTlt"))
    );
    assert!(data.items.contains_key(&i32::from_be_bytes(*b"ratc")));
}

/// SPEC-0010: one key from each interface string file is found.
#[test]
fn localized_strings() {
    let path = game_data().join("Maps/(2)BootyBay.w3m");
    if !path.exists() {
        return;
    }
    let map = Map::load(&game_data(), &path).unwrap();
    for key in [
        "GAMEOVER_DEFEAT_MSG",
        "AGILITY",
        "MONTH_01",
        "ERROR_ID_INVALIDPARAMS",
    ] {
        assert!(
            map.localized.get(key).is_some_and(|t| !t.is_empty()),
            "{key}"
        );
    }
    assert!(!map.localized.contains_key("gameover_defeat_msg"));
    assert!(map.localized.get("HIGH_VOL").is_some_and(|t| !t.is_empty()));
}
