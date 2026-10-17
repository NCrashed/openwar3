//! Starts a map and prints what its scripts show to the local player.
//!
//! `cargo run --release -p ow3-sim --example run_map -- <map> [seconds]`
//!
//! The game archives are read from `game-data/1.27b`, or from the
//! directory in `OW3_GAME_DATA`. The map runs at full speed for the given
//! seconds of real time (10 by default).

use std::path::PathBuf;
use std::process::ExitCode;

use ow3_sim::game::Game;
use ow3_sim::map::Map;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: run_map <map> [seconds]");
        return ExitCode::FAILURE;
    };
    let seconds: f64 = match args.next().map(|s| s.parse()) {
        None => 10.0,
        Some(Ok(seconds)) => seconds,
        Some(Err(_)) => {
            eprintln!("seconds must be a number");
            return ExitCode::FAILURE;
        }
    };
    let game_data = std::env::var_os("OW3_GAME_DATA")
        .map_or_else(|| PathBuf::from("game-data/1.27b"), PathBuf::from);
    let map = match Map::load(&game_data, path.as_ref()) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let program = match map.check() {
        Ok(program) => program,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut game = match Game::load_map(&program, &map) {
        Ok(game) => game,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = game.start() {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    game.advance_seconds(seconds);
    for line in game.output() {
        println!("{line}");
    }
    for error in &game.state.borrow().errors {
        eprintln!("thread ended: {error}");
    }
    ExitCode::SUCCESS
}
