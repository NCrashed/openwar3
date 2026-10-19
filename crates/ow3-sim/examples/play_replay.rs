//! Plays a replay in the simulation and prints what could not be played
//! and the state at the end.
//!
//! `cargo run --release -p ow3-sim --example play_replay -- <replay.w3g> [--units]`
//!
//! The map is the one the replay names, under the game directory
//! (`game-data/1.27b`, or the directory in `OW3_GAME_DATA`). At the end it
//! prints a hash of the units (handle, owner, type, position, facing,
//! life), which two runs of the same replay must share; with `--units` it
//! lists them.

use std::path::PathBuf;
use std::process::ExitCode;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: play_replay <replay.w3g> [--units]");
        return ExitCode::FAILURE;
    };
    let list = args.iter().any(|a| a == "--units");
    let game_data = std::env::var_os("OW3_GAME_DATA")
        .map_or_else(|| PathBuf::from("game-data/1.27b"), PathBuf::from);
    let replay = match std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|d| Replay::parse(&d).map_err(|e| e.to_string()))
    {
        Ok(replay) => replay,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let map_path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = match Map::load(&game_data, &map_path) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("{}: {e}", map_path.display());
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
    let mut playback = match Playback::new(&program, &map, &replay) {
        Ok(playback) => playback,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    playback.run();
    for note in &playback.notes {
        println!("{note}");
    }
    let units = playback.game.units();
    // FNV-1a over the units' state.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut add = |word: u32| {
        for b in word.to_le_bytes() {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for u in &units {
        add(u.handle);
        add(u.owner as u32);
        add(u.type_id as u32);
        add(u.x.to_bits());
        add(u.y.to_bits());
        add(u.facing.to_bits());
        add(u.life.to_bits());
        if list {
            let code = String::from_utf8_lossy(&u.type_id.to_be_bytes()).into_owned();
            println!(
                "{:#x} {code} of {} at ({}, {}) life {}",
                u.handle, u.owner, u.x, u.y, u.life
            );
        }
    }
    println!(
        "{} ms played, {} ticks, {} units, state {hash:016x}",
        playback.ms(),
        playback.game.ticks(),
        units.len()
    );
    ExitCode::SUCCESS
}
