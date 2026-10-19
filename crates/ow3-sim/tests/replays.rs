//! Plays the replays saved by the game in `game-data/1.27b/replays` to
//! their end, twice, and checks that both runs end in the same state. A
//! replay with a log of the game (`<name>.log.txt` next to `<name>.w3g`)
//! is also checked against it. Passes without checking anything when
//! there are no replays.

use std::path::{Path, PathBuf};

use ow3_formats::replay::{Event, Replay};
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

/// A unit at the end of a run: handle, position, facing and life bits.
type End = (u32, u32, u32, u32, u32);

/// A run of a replay: its ticks, its units at the end and its `Preload`
/// lines.
struct Run {
    ticks: u64,
    units: Vec<End>,
    preload: Vec<String>,
}

fn play(game_data: &Path, replay: &Replay) -> Run {
    let path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(game_data, &path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, replay).unwrap();
    playback.run();
    let units = playback
        .game
        .units()
        .iter()
        .map(|u| {
            (
                u.handle,
                u.x.to_bits(),
                u.y.to_bits(),
                u.facing.to_bits(),
                u.life.to_bits(),
            )
        })
        .collect();
    Run {
        ticks: playback.game.ticks(),
        units,
        preload: playback.game.preload(),
    }
}

/// The replays in the game directory, with their paths.
fn replays() -> Vec<(PathBuf, Replay)> {
    let Ok(entries) = std::fs::read_dir(game_data().join("replays")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "w3g"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let data = std::fs::read(&path).unwrap();
            // The game leaves an empty TempReplay.w3g behind.
            (!data.is_empty()).then(|| (path, Replay::parse(&data).unwrap()))
        })
        .collect()
}

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

#[test]
fn replays_play_the_same_twice() {
    for (path, replay) in replays() {
        let first = play(&game_data(), &replay);
        // The slots of about 100 ms add up to the replay's game time.
        let slots: u64 = replay
            .events
            .iter()
            .map(|e| match e {
                Event::TimeSlot { ms, .. } => u64::from(*ms),
                _ => 0,
            })
            .sum();
        assert_eq!(first.ticks, slots / 5, "{}", path.display());
        let second = play(&game_data(), &replay);
        assert_eq!(second.units, first.units, "{}", path.display());
        assert_eq!(second.preload, first.preload, "{}", path.display());
    }
}

/// The `@@P n x y facing random` lines of a log, up to its `@@OW3 END`.
fn positions<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<[f64; 4]> {
    lines
        .take_while(|l| !l.contains("@@OW3 END"))
        .filter_map(|l| {
            let rest = &l[l.find("@@P ")? + 4..];
            let words: Vec<f64> = rest
                .trim_end_matches(['"', ')', ' '])
                .split(' ')
                .map(|w| w.parse().unwrap())
                .collect();
            Some([words[1], words[2], words[3], words[4]])
        })
        .collect()
}

/// The positions logged by the script while the replay plays
/// are those of the game's log within 0.02, and the facings within 0.05
/// degrees (the turn at the start differs by 0.04). The random values are not
/// compared: the generators' state at the start of a game is not specified
/// (SPEC-0007, open question 1).
#[test]
fn replays_follow_the_game_logs() {
    for (path, replay) in replays() {
        let Ok(log) = std::fs::read_to_string(path.with_extension("log.txt")) else {
            continue;
        };
        let expected = positions(log.lines());
        let run = play(&game_data(), &replay);
        let got = positions(run.preload.iter().map(String::as_str));
        assert!(!expected.is_empty(), "{}", path.display());
        assert_eq!(got.len(), expected.len(), "{}", path.display());
        for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
            for k in 0..3 {
                assert!(
                    (g[k] - e[k]).abs() <= if k == 2 { 0.05 } else { 0.02 },
                    "{} line {}: {g:?} against {e:?}",
                    path.display(),
                    i + 1
                );
            }
        }
    }
}
