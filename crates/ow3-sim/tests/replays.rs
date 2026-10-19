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

fn play(game_data: &Path, replay_path: &Path, replay: &Replay) -> Run {
    let path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(game_data, &path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, replay).unwrap();
    playback.load_object_ids(replay_path);
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
        let first = play(&game_data(), &path, &replay);
        // The game runs the whole 25 ms steps of the slots, which are
        // about 100 ms each.
        let slots: u64 = replay
            .events
            .iter()
            .map(|e| match e {
                Event::TimeSlot { ms, .. } => u64::from(*ms),
                _ => 0,
            })
            .sum();
        assert_eq!(first.ticks, slots / 25 * 5, "{}", path.display());
        let second = play(&game_data(), &path, &replay);
        assert_eq!(second.units, first.units, "{}", path.display());
        assert_eq!(second.preload, first.preload, "{}", path.display());
    }
}

/// The `@@` lines of a log up to its `@@OW3 END`, without the `@@`.
fn logged<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines
        .filter_map(|l| {
            let rest = &l[l.find("@@")? + 2..];
            Some(rest.trim_end_matches(['"', ')', ' ']).to_owned())
        })
        .take_while(|l| l != "OW3 END")
        .collect()
}

/// The lines the script logs while the replay plays (positions, facings
/// and random values every 0.1 s) are those of the game's log.
#[test]
fn replays_follow_the_game_logs() {
    for (path, replay) in replays() {
        let Ok(log) = std::fs::read_to_string(path.with_extension("log.txt")) else {
            continue;
        };
        let expected = logged(log.lines());
        let run = play(&game_data(), &path, &replay);
        // Logs in other forms have tests of their own.
        if expected.is_empty() {
            continue;
        }
        assert_eq!(
            logged(run.preload.iter().map(String::as_str)),
            expected,
            "{}",
            path.display()
        );
    }
}

/// SPEC-0026, verification 1: the build order of `c-RSR-1.w3g` against
/// the script's log of every tick (`c-RSR-1.log.tsv`: tick, the peasant's
/// x and y, its order, gold, lumber, units of player 0). The costs are
/// paid, the order changes, the peasant walks and the farm is started in
/// the same ticks as in the game.
///
/// PROVISIONAL: where the peasant is moved off the farm's footprint is
/// not specified (SPEC-0026, open question 2), so its position is not
/// compared once the farm stands. The farm starts at the update of the
/// tick of log line 1708, after the script's timer, so the log shows it
/// from 1709.
#[test]
fn build_order_follows_the_game_log() {
    let path = game_data().join("replays/c-RSR-1.w3g");
    let Ok(log) = std::fs::read_to_string(path.with_extension("log.tsv")) else {
        return;
    };
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let run = play(&game_data(), &path, &replay);
    let ours: Vec<Vec<String>> = run
        .preload
        .iter()
        .filter_map(|l| l.strip_prefix("@@T "))
        .map(|l| l.split(' ').map(str::to_owned).collect())
        .collect();
    let mut started = false;
    let mut rows = 0;
    for (line, ours) in log.lines().filter(|l| !l.starts_with('#')).zip(&ours) {
        let game: Vec<&str> = line.split('\t').collect();
        started |= game[3] == "852024";
        let columns = if started { 3..7 } else { 0..7 };
        for i in columns {
            assert_eq!(ours[i], game[i], "tick {}: column {i}", game[0]);
        }
        rows += 1;
    }
    assert!(started && rows > 5000);
}

/// SPEC-0026, "Construction progress": the farm of `c-RSN-1.w3g`, played on
/// past the end of the replay. Its life is a tenth of 500 until growth
/// begins, then follows `50 + 12.857142 (T - Tb)`; it is finished 35 s of
/// the movement clock after growth began, with 500 life; the player's food cap
/// grows by 6 at the same time, and the peasant's order ends. The ticks
/// are counted from the first tick in which the life is above 50, the
/// one after work began: the tick of the start, or with the peasant
/// moved two updates later.
#[test]
fn a_farm_is_built() {
    let path = game_data().join("replays/c-RSN-1.w3g");
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.load_object_ids(&path);
    let hhou = i32::from_be_bytes(*b"hhou");
    let farm = |p: &Playback| p.game.units().into_iter().find(|u| u.type_id == hhou);
    while farm(&playback).is_none() {
        assert!(playback.step_tick());
    }
    let created = playback.game.ticks();
    let food = playback.game.player_state(0, 4);
    let mut began = None;
    let mut last = 50.0;
    loop {
        let f = farm(&playback).unwrap();
        if began.is_none() && f.life > 50.0 {
            began = Some(playback.game.ticks());
        }
        assert!(f.life >= last, "life fell from {last} to {}", f.life);
        last = f.life;
        if f.life == 500.0 {
            break;
        }
        playback.game.step();
    }
    let finished = playback.game.ticks();
    let began = began.unwrap();
    assert!(
        began - created == 1 || began - created == 13,
        "{created} {began}"
    );
    // The end is a timer of the engine (SPEC-0026): 7002 ticks after the
    // start of work early in a game, 7003 here, where work begins at a
    // clock of 9.46 s and the drift of the clock (SPEC-0018) adds one;
    // counted from the first growth, one less.
    assert_eq!(finished - began, 7002);
    // The food comes with the end, read by a timer of that tick.
    assert_eq!(playback.game.player_state(0, 4), food + 6);
    for _ in 0..6 {
        playback.game.step();
    }
    let hpea = i32::from_be_bytes(*b"hpea");
    let peasant = playback
        .game
        .units()
        .into_iter()
        .find(|u| u.type_id == hpea)
        .unwrap();
    assert_eq!(peasant.order, 0);
}

/// The lines of a script log in the form of the analysts' tables: the
/// `@@` lines of `Preload`, fields joined by tabs, with `fields` choosing
/// and ordering the fields of each kind (`None` drops the line).
fn table(preload: &[String], fields: impl Fn(&[&str]) -> Option<Vec<String>>) -> Vec<String> {
    preload
        .iter()
        .filter_map(|l| l.strip_prefix("@@"))
        .filter_map(|l| fields(&l.split(' ').collect::<Vec<_>>()))
        .map(|f| f.join("\t"))
        .collect()
}

/// The lines of an analysts' log table, without comments and trailing
/// tabs.
fn log_table(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.trim_end_matches('\t').to_owned())
        .collect()
}

/// SPEC-0026 and SPEC-0018 ("Order within an update tick"), against the
/// log of `c-RTI-1.w3g`: four farms whose life a timer reads at every tick
/// near the start and the end of construction and every 1000 ticks
/// (`T b n life`), their start events (`S b n`, before the timer of their
/// tick for the first two farms, after it for the others) and their
/// finish events (`F n`). The log is the game's line for line.
#[test]
fn construction_follows_c_rti_1() {
    let path = game_data().join("replays/c-RTI-1.w3g");
    let Ok(log) = std::fs::read_to_string(path.with_extension("log.tsv")) else {
        return;
    };
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let run = play(&game_data(), &path, &replay);
    let ours = table(&run.preload, |w| {
        let pick = |i: &[usize]| Some(i.iter().map(|&i| w[i].to_owned()).collect());
        match w[0] {
            "T" => pick(&[0, 1, 2, 3]),
            "S" => pick(&[0, 1, 2]),
            "F" => Some(vec!["F".into(), String::new(), w[1].into()]),
            _ => None,
        }
    });
    assert_eq!(ours, log_table(&log));
}

/// SPEC-0018 ("Order within an update tick") against the log of
/// `c-RTM-1.w3g`: the times of 3180 calls of a periodic timer read from a
/// reference timer (`Q n time`), the life of four farms (`T b n life`) and
/// their start events with the time of the reference timer (`S b n
/// time`): the first farm starts before the timer of its tick, the
/// others after it. The log is the game's line for line.
#[test]
fn update_time_follows_c_rtm_1() {
    let path = game_data().join("replays/c-RTM-1.w3g");
    let Ok(log) = std::fs::read_to_string(path.with_extension("log.tsv")) else {
        return;
    };
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let run = play(&game_data(), &path, &replay);
    let ours = table(&run.preload, |w| {
        let pick = |i: &[usize]| Some(i.iter().map(|&i| w[i].to_owned()).collect());
        match w[0] {
            "Q" => pick(&[0, 1, 2]),
            "T" => pick(&[0, 1, 2, 3]),
            "S" => pick(&[0, 1, 2, 5]),
            _ => None,
        }
    });
    let game = log_table(&log);
    assert!(ours.len() >= game.len());
    assert_eq!(ours[..game.len()], game[..]);
}

/// SPEC-0027 against the log of `c-RTP-1.w3g`: a barracks selected with
/// the mouse is ordered twice to train a footman and once to cancel, and
/// given a rally point. The issued order, start, cancel and finish events,
/// the gold, lumber, food and orders, and the footman's place and walk to
/// the rally point at every tick are the game's line for line.
#[test]
fn training_follows_c_rtp_1() {
    let path = game_data().join("replays/c-RTP-1.w3g");
    let Ok(log) = std::fs::read_to_string(path.with_extension("log.txt")) else {
        return;
    };
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let run = play(&game_data(), &path, &replay);
    let ours: Vec<String> = table(&run.preload, |w| {
        Some(w.iter().map(|s| (*s).to_owned()).collect())
    })
    .into_iter()
    .map(|l| l.replace('\t', " "))
    .collect();
    let game = log_table(&log);
    assert!(ours.len() >= game.len());
    assert_eq!(ours[..game.len()], game[..]);
}
