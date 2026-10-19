//! The turn checksums of SPEC-0024 in the replays of the test maps in
//! `game-data/1.27b/replays`, `c-RSF-1.w3g` being the reference replay of
//! the spec. Passes without checking anything when they are not there.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::checksum::{Turn, turns};
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

/// The turns of the reference replay, if it is there.
fn reference() -> Option<Vec<Turn>> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays/c-RSF-1.w3g");
    let data = std::fs::read(path).ok()?;
    Some(turns(&Replay::parse(&data).unwrap()))
}

/// Turn, `t`, `N` and the checksum of each row of `SPEC-0024-vectors.tsv`.
fn vectors() -> Vec<(usize, u64, u32, u32)> {
    include_str!("../../../specs/SPEC-0024-vectors.tsv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let w: Vec<&str> = l.split('\t').collect();
            let hex = |s| u32::from_str_radix(s, 16).unwrap();
            (
                w[0].parse().unwrap(),
                w[1].parse().unwrap(),
                hex(w[3]),
                hex(w[9]),
            )
        })
        .collect()
}

/// SPEC-0024, verification 2: the network sum and the time follow from
/// the replay, and the recorded checksums are those of the vectors.
#[test]
fn network_sums_of_the_reference_replay() {
    let Some(turns) = reference() else {
        return;
    };
    for (turn, ms, n, checksum) in vectors() {
        let t = &turns[turn - 1];
        assert_eq!((t.after, t.n, t.recorded), (ms, n, checksum), "turn {turn}");
    }
}

/// SPEC-0024, verification 4: from turn 12 on, the remainder changes
/// only at turn 41, when the footman's move ends.
#[test]
fn remainders_of_the_reference_replay() {
    let Some(turns) = reference() else {
        return;
    };
    let changes: Vec<usize> = (12..turns.len())
        .filter(|&k| turns[k].remainder() != turns[k - 1].remainder())
        .map(|k| k + 1)
        .collect();
    assert_eq!(changes, [41]);
}

/// Turns from 3 on whose state changed, as the recorded checksums show
/// them and as the simulation's marks do. Turn 2 is left out: the game
/// changes something there that the simulation does not have.
fn changed_turns(path: &std::path::Path) -> (Vec<usize>, Vec<usize>) {
    let game_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let replay = Replay::parse(&std::fs::read(path).unwrap()).unwrap();
    let map_path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data, &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.run();
    let turns = turns(&replay);
    let recorded = (2..turns.len())
        .filter(|&k| turns[k].remainder() != turns[k - 1].remainder())
        .map(|k| k + 1)
        .collect();
    let marks = &playback.marks;
    let ours = (2..turns.len().min(marks.len()))
        .filter(|&k| marks[k] != marks[k - 1])
        .map(|k| k + 1)
        .collect();
    (recorded, ours)
}

/// In the replays of the test maps (`c-*.w3g`) the state changes at the
/// same turns in the game and in the simulation: when the script makes
/// objects or draws random numbers and when a move ends.
#[test]
fn state_changes_at_the_recorded_turns() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("c-") && name.ends_with(".w3g")
        })
        .collect();
    paths.sort();
    for path in paths {
        let (recorded, ours) = changed_turns(&path);
        assert_eq!(ours, recorded, "{}", path.display());
    }
}
