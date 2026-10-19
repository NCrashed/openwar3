
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
use std::path::PathBuf;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
use ow3_formats::replay::Replay;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
use ow3_sim::checksum::{Turn, time_part, turns};
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
use ow3_sim::map::Map;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
use ow3_sim::replay::{Marks, Playback};
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// The turns of the reference replay, if it is there.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn reference() -> Option<Vec<Turn>> {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let path =
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays/c-RSF-1.w3g");
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let data = std::fs::read(path).ok()?;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    Some(turns(&Replay::parse(&data).unwrap()))
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// Turn, `t`, `N` and the checksum of each row of `SPEC-0024-vectors.tsv`.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn vectors() -> Vec<(usize, u64, u32, u32)> {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    include_str!("../../../specs/SPEC-0024-vectors.tsv")
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .lines()
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .map(|l| {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let w: Vec<&str> = l.split('\t').collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let hex = |s| u32::from_str_radix(s, 16).unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            (
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                w[0].parse().unwrap(),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                w[1].parse().unwrap(),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                hex(w[3]),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                hex(w[9]),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            )
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        })
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .collect()
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// SPEC-0024, verification 2: the network sum and the time follow from
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// the replay, and the recorded checksums are those of the vectors.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
#[test]
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn network_sums_of_the_reference_replay() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let Some(turns) = reference() else {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        return;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    };
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    for (turn, ms, n, checksum) in vectors() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let t = &turns[turn - 1];
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        assert_eq!((t.after, t.n, t.recorded), (ms, n, checksum), "turn {turn}");
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    }
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// SPEC-0024, verification 4: from turn 12 on, the remainder changes
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// only at turn 41, when the footman's move ends.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
#[test]
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn remainders_of_the_reference_replay() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let Some(turns) = reference() else {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        return;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    };
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let changes: Vec<usize> = (12..turns.len())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .filter(|&k| turns[k].remainder() != turns[k - 1].remainder())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .map(|k| k + 1)
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    assert_eq!(changes, [41]);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// Replays of the test maps whose play needs what the specs do not give
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// yet, with the reason.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
const UNSPECIFIED: &[(&str, &str)] = &[(
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    "c-RSM-1.w3g",
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    "members walking next to each other see each other in their local \
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
         paths and look-ahead otherwise than in the game (SPEC-0021, SPEC-0025 \
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
         open question 2)",
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
)];
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// The replays of the test maps (`c-*.w3g`) that can be played, sorted.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn test_replays() -> Vec<PathBuf> {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays");
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let Ok(entries) = std::fs::read_dir(dir) else {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        return Vec::new();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    };
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let mut paths: Vec<PathBuf> = entries
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .flatten()
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .map(|e| e.path())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .filter(|p| {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let name = p.file_name().unwrap().to_string_lossy();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            name.starts_with("c-")
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                && name.ends_with(".w3g")
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                && !UNSPECIFIED.iter().any(|(n, _)| *n == name)
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        })
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        .collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    paths.sort();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    paths
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// A replay played to its end, with its turns.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn play(path: &std::path::Path) -> (Replay, Vec<Turn>, Vec<Marks>) {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let game_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let replay = Replay::parse(&std::fs::read(path).unwrap()).unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let map_path = game_data.join(replay.settings.map_path.replace('\\', "/"));
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let map = Map::load(&game_data, &map_path).unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let program = map.check().unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    playback.load_object_ids(path);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    playback.run();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let turns = turns(&replay);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    (replay, turns, playback.marks)
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// In the replays of the test maps the state changes at the same turns in
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// the game and in the simulation (from turn 3 on: in turn 2 the game
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// frees objects the simulation does not have): when the script makes
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// objects or draws random numbers and when a move ends. In a replay with
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// actions the game also changes state the simulation does not have
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// (objects of orders and paths, subgroups), so there every change of the
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// simulation must be among the recorded ones.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
#[test]
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn state_changes_at_the_recorded_turns() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    for path in test_replays() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let (replay, turns, marks) = play(&path);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let recorded: Vec<usize> = (2..turns.len())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .filter(|&k| turns[k].remainder() != turns[k - 1].remainder())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .map(|k| k + 1)
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let ours: Vec<usize> = (2..turns.len().min(marks.len()))
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .filter(|&k| marks[k] != marks[k - 1])
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .map(|k| k + 1)
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            .collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let actions = replay.events.iter().any(|e| {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            matches!(e, ow3_formats::replay::Event::TimeSlot { actions, .. } if !actions.is_empty())
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        });
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        if actions {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let missing: Vec<usize> = ours
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                .iter()
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                .copied()
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                .filter(|k| !recorded.contains(k))
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                .collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            assert!(missing.is_empty(), "{}: {missing:?}", path.display());
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        } else {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            assert_eq!(ours, recorded, "{}", path.display());
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        }
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    }
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",

        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// The parts of the turn checksums read from the game (`<name>.parts.tsv`:
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// turn, `t`, checksum, `B`, `N`, `C`, `D`, `E`, `R`, `G`): the time, the
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// network sum and the recorded checksum follow from the replay, and the
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// generator sums `E` and `R` of the playback are the game's at every
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
/// turn listed.
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
#[test]
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
fn parts_of_the_turns() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    for path in test_replays() {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let Ok(parts) = std::fs::read_to_string(path.with_extension("parts.tsv")) else {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            continue;
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        };
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let (_, turns, marks) = play(&path);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        let name = path.file_name().unwrap().to_string_lossy();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        for line in parts.lines().filter(|l| !l.starts_with('#')) {
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let w: Vec<&str> = line.split('\t').collect();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let k: usize = w[0].parse().unwrap();
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let (turn, marks) = (&turns[k - 1], &marks[k - 1]);
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            let expected = (hex(w[2]), w[3].parse().unwrap(), hex(w[4]));
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            assert_eq!(
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                (turn.recorded, time_part(turn.before), turn.n),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                expected,
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                "{name} turn {k}"
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            );
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            assert_eq!(
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                (marks.e, marks.r),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                (hex(w[7]), hex(w[8])),
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
                "{name} turn {k}"
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
            );
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
        }
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
    }
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
}
        "the members block each other at other updates than in the game: the \
         game draws the script generator for patience at turns 65, 67, 76 and \
         77, the simulation at 62 and 79 (SPEC-0025)",
