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
        game.start();
        game.advance_seconds(30.0);
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
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A two-player melee map: the local player is the first user slot, the
/// other user slot stays empty, `config` defines the start locations, and
/// the trees are destructables.
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
    game.start();
    let state = game.state.borrow();
    let setup = &state.setup;
    assert_eq!(setup.local_player, 0);
    assert_eq!(setup.present(), [0, 12, 13, 14, 15]);
    assert_eq!(setup.enumerable(), [0]);
    assert_eq!(setup.player_count, 2);
    for player in &map.info.players {
        let (x, y) = setup.start_locations[&player.number];
        // `config` passes them as literals, which read like `S2R` and can
        // land a unit in the last place away: "1792.0" gives 1792.0001.
        assert!((f32::from_bits(x) - player.start_x).abs() <= 1e-3);
        assert!((f32::from_bits(y) - player.start_y).abs() <= 1e-3);
    }
    assert!(state.errors.is_empty(), "{:?}", state.errors);
}
