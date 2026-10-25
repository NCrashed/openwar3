//! The turn checksums of SPEC-0024 in the replays of the test maps in
//! `game-data/1.27b/replays`, `c-RSF-1.w3g` being the reference replay of
//! the spec. Passes without checking anything when they are not there.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::checksum::{Turn, time_part, turns};
use ow3_sim::map::Map;
use ow3_sim::replay::{Marks, Playback};

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

/// Replays of the test maps whose play needs what the specs do not give
/// yet, with the reason.
const UNSPECIFIED: &[(&str, &str)] = &[
    (
        "c-SBG-1.w3g",
        "the Thunder Clap probe (slows ending by time, hold position): every turn checksum \
         matches with the object table started as the game has it (tests/objects.rs, \
         thunder_clap_checksums); this test runs without it",
    ),
    (
        "c-SAK-1.w3g",
        "the third hero test (Mountain King, Blademaster with mirror images and wind walk): \
         every turn checksum to its end matches with the object table started as the game \
         has it (tests/objects.rs, heroes_three_checksums); this test runs without it",
    ),
    (
        "c-RTW-3.w3g",
        "player orders (moves, patrol, follow, group and Shift group orders, stop): every \
         turn checksum to its end matches with the object table started as the game has it \
         (tests/objects.rs, orders_checksums); this test runs without it",
    ),
    (
        "c-RXR-1.w3g",
        "the battle reference (melee, rifles, missiles): every turn checksum to its end \
         matches with the object table started as the game has it (tests/objects.rs, \
         battle_checksums); this test runs without it",
    ),
    (
        "c-RYP-1.w3g",
        "peasants harvesting lumber: every turn checksum to its end matches with the object \
         table started as the game has it (tests/objects.rs, lumber_checksums); this test \
         runs without it",
    ),
    (
        "c-RXD-1.w3g",
        "gold harvest with kobolds attacking: every turn checksum to its end matches with \
         the object table started as the game has it (tests/objects.rs, \
         creeps_harvest_checksums); this test runs without it",
    ),
    (
        "c-RZH-1.w3g",
        "the hero test (Mountain King, footmen, creeps): every turn checksum to its end \
         matches with the object table started as the game has it (tests/objects.rs, \
         heroes_one_checksums); this test runs without it",
    ),
    (
        "c-RZJ-1.w3g",
        "the second hero test (Archmage, Paladin, water elementals, an Ogre Lord's camp): \
         every turn checksum to its end matches with the object table started as the game \
         has it (tests/objects.rs, heroes_two_checksums); this test runs without it",
    ),
    (
        "c-RXI-1.w3g",
        "six peasants harvesting gold: every turn checksum to its end matches with the \
         object table started as the game has it (tests/objects.rs, harvest_checksums); \
         this test runs without it",
    ),
];

/// The replays of the test maps (`c-*.w3g`) that can be played, sorted.
fn test_replays() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("c-")
                && name.ends_with(".w3g")
                && !UNSPECIFIED.iter().any(|(n, _)| *n == name)
        })
        .collect();
    paths.sort();
    paths
}

/// A replay played to its end, with its turns.
fn play(path: &std::path::Path) -> (Replay, Vec<Turn>, Vec<Marks>) {
    let game_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let replay = Replay::parse(&std::fs::read(path).unwrap()).unwrap();
    let map_path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data, &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.load_object_ids(path);
    playback.run();
    let turns = turns(&replay);
    (replay, turns, playback.marks)
}

/// In the replays of the test maps the state changes at the same turns in
/// the game and in the simulation (from turn 3 on: in turn 2 the game
/// frees objects the simulation does not have): when the script makes
/// objects or draws random numbers and when a move ends. In a replay with
/// actions the game also changes state the simulation does not have
/// (objects of orders and paths, subgroups), so there every change of the
/// simulation must be among the recorded ones.
#[test]
fn state_changes_at_the_recorded_turns() {
    for path in test_replays() {
        let (replay, turns, marks) = play(&path);
        let recorded: Vec<usize> = (2..turns.len())
            .filter(|&k| turns[k].remainder() != turns[k - 1].remainder())
            .map(|k| k + 1)
            .collect();
        // The object table (`C`, `D`) is left out until it is complete.
        let plain = |m: &Marks| Marks { c: 0, d: 0, ..*m };
        let ours: Vec<usize> = (2..turns.len().min(marks.len()))
            .filter(|&k| plain(&marks[k]) != plain(&marks[k - 1]))
            .map(|k| k + 1)
            .collect();
        let actions = replay.events.iter().any(|e| {
            matches!(e, ow3_formats::replay::Event::TimeSlot { actions, .. } if !actions.is_empty())
        });
        // Buildings and their workers change state the simulation does not
        // have (objects of the work, freed after it).
        let builds = path
            .file_name()
            .is_some_and(|n| n == "c-RTI-1.w3g" || n == "c-RTM-1.w3g");
        if actions || builds {
            let missing: Vec<usize> = ours
                .iter()
                .copied()
                .filter(|k| !recorded.contains(k))
                .collect();
            assert!(missing.is_empty(), "{}: {missing:?}", path.display());
        } else {
            assert_eq!(ours, recorded, "{}", path.display());
        }
    }
}

/// The parts of the turn checksums read from the game (`<name>.parts.tsv`:
/// turn, `t`, checksum, `B`, `N`, `C`, `D`, `E`, `R`, `G`): the time, the
/// network sum and the recorded checksum follow from the replay, and the
/// generator sums `E` and `R` of the playback are the game's at every
/// turn listed.
#[test]
fn parts_of_the_turns() {
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
    for path in test_replays() {
        let Ok(parts) = std::fs::read_to_string(path.with_extension("parts.tsv")) else {
            continue;
        };
        let (_, turns, marks) = play(&path);
        let name = path.file_name().unwrap().to_string_lossy();
        for line in parts.lines().filter(|l| !l.starts_with('#')) {
            let w: Vec<&str> = line.split('\t').collect();
            let k: usize = w[0].parse().unwrap();
            let (turn, marks) = (&turns[k - 1], &marks[k - 1]);
            let expected = (hex(w[2]), w[3].parse().unwrap(), hex(w[4]));
            assert_eq!(
                (turn.recorded, time_part(turn.before), turn.n),
                expected,
                "{name} turn {k}"
            );
            assert_eq!(
                (marks.e, marks.r),
                (hex(w[7]), hex(w[8])),
                "{name} turn {k}"
            );
        }
    }
}
