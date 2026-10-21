//! The crowd chase `c-RXD-1` (SPEC-0021, "Crowd chase"): six peasants
//! harvesting and four creeps attacking them, no input. The replay is
//! played tick by tick with the path decisions traced, and compared with
//! the game's `c-RXD-1.log.tsv` (positions) and `c-RXD-1.events.tsv`
//! (updates, waits, made-to-waits and blocks). Skipped without the game
//! data.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::game::follow::{PathEvent, PathEventKind};
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

/// An event of the game: tick, kind, unit a, unit b, position.
type Logged = (u64, String, i32, Option<i32>, [f32; 2]);

fn events(path: &std::path::Path) -> Vec<Logged> {
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("tick"))
        .filter_map(|l| {
            let w: Vec<&str> = l.split('\t').collect();
            let num = |s: Option<&&str>| s.and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            Some((
                w.first()?.parse().ok()?,
                (*w.get(1)?).to_owned(),
                w.get(2)?.parse().ok()?,
                w.get(3).and_then(|s| s.parse().ok()),
                [num(w.get(4)), num(w.get(5))],
            ))
        })
        .collect()
}

/// The positions of the log: unit, tick, (x, y) after the tick.
fn positions(path: &std::path::Path) -> BTreeMap<i32, Vec<(u64, [f32; 2])>> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out: BTreeMap<i32, Vec<(u64, [f32; 2])>> = BTreeMap::new();
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = l.split('\t').collect();
        if w.get(1) != Some(&"p") {
            continue;
        }
        let (Some(tick), Some(unit), Some(x), Some(y)) = (
            w.first().and_then(|s| s.parse().ok()),
            w.get(2).and_then(|s| s.parse().ok()),
            w.get(4).and_then(|s| s.parse().ok()),
            w.get(5).and_then(|s| s.parse().ok()),
        ) else {
            continue;
        };
        out.entry(unit).or_default().push((tick, [x, y]));
    }
    out
}

/// What the simulation gives: the trace with units as indices of the log,
/// and each unit's position at every tick.
struct Ours {
    events: Vec<Logged>,
    at: BTreeMap<i32, BTreeMap<u64, [f32; 2]>>,
}

fn play_replay(name: &str, until: u64) -> Option<Ours> {
    let dir = game_data().join("replays");
    let path = dir.join(format!("{name}.w3g"));
    if !path.exists() {
        return None;
    }
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.game.state.borrow_mut().path_trace = Some(Vec::new());
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    let kinds = [code(b"hpea"), code(b"nkob"), code(b"nkot")];
    let mut index: BTreeMap<u32, i32> = BTreeMap::new();
    let mut at: BTreeMap<i32, BTreeMap<u64, [f32; 2]>> = BTreeMap::new();
    while playback.game.ticks() < until && playback.step_tick() {
        let tick = playback.game.ticks();
        for u in playback.game.units() {
            if !kinds.contains(&u.type_id) {
                continue;
            }
            let n = index.len() as i32;
            let i = *index.entry(u.handle).or_insert(n);
            at.entry(i).or_default().insert(tick, [u.x, u.y]);
        }
    }
    let state = playback.game.state.borrow();
    let [x0, y0, _, _] = state.world_bounds.map(f32::from_bits);
    let unit = |h: u32| index.get(&h).copied().unwrap_or(-1);
    let events = state
        .path_trace
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|e: &PathEvent| {
            let kind = match e.kind {
                PathEventKind::Update => "update",
                PathEventKind::Wait => "wait4",
                PathEventKind::MadeWait => "made_wait20",
                PathEventKind::Blocked => "blocked",
            };
            let [x, y] = e.at.map(f32::from_bits);
            (
                e.tick,
                kind.to_owned(),
                unit(e.a),
                e.b.map(unit),
                [x + x0, y + y0],
            )
        })
        .collect();
    Some(Ours { events, at })
}

/// `c-RXD-1` played up to `until`.
fn play(until: u64) -> Option<Ours> {
    play_replay("c-RXD-1", until)
}

/// A report of where the simulation of replay `name`, played up to
/// `until`, first leaves the game: per unit, the first logged position
/// it misses, and the first events that differ. The tick of that event
/// (`u64::MAX` when none differs), or `None` without the game data.
fn report(name: &str, until: u64) -> Option<u64> {
    let ours = play_replay(name, until)?;
    let dir = game_data().join("replays");
    let game = events(&dir.join(format!("{name}.events.tsv")));
    let logged = positions(&dir.join(format!("{name}.log.tsv")));
    #[allow(non_snake_case)]
    let UNTIL = until;
    // The tick offset: the first move of unit 0 in the log and in ours.
    let first_move = |list: &[(u64, [f32; 2])]| list.get(1).map(|p| p.0);
    let log_first = first_move(&logged[&0]).unwrap();
    let ours_list: Vec<(u64, [f32; 2])> = ours.at[&0].iter().map(|(&t, &p)| (t, p)).collect();
    let start = ours_list[0].1;
    let ours_first = ours_list
        .iter()
        .find(|p| p.1 != start)
        .map(|p| p.0)
        .unwrap();
    let shift = log_first as i64 - ours_first as i64;
    eprintln!("first move: log {log_first}, ours {ours_first}, shift {shift}");
    for (unit, list) in &logged {
        let Some(track) = ours.at.get(unit) else {
            eprintln!("unit {unit}: not found");
            continue;
        };
        let miss = list
            .iter()
            .filter(|(t, _)| (*t as i64 - shift) < UNTIL as i64)
            .find(|(t, p)| {
                let t = (*t as i64 - shift) as u64;
                track
                    .get(&t)
                    .is_none_or(|q| (q[0] - p[0]).abs() > 0.01 || (q[1] - p[1]).abs() > 0.01)
            });
        if let Some((t, p)) = miss {
            let q = track.get(&((*t as i64 - shift) as u64));
            eprintln!("unit {unit}: first miss at log tick {t}: game {p:?}, ours {q:?}");
        }
    }
    let mut ours_events: Vec<Logged> = ours
        .events
        .iter()
        // The path events take the game's ticks as they are; the
        // positions of the log come one tick later here.
        .map(|e| (e.0, e.1.clone(), e.2, e.3, e.4))
        .collect();
    ours_events.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    let mut game_events: Vec<Logged> = game.into_iter().filter(|e| e.0 <= UNTIL).collect();
    game_events.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    let same = |a: &Logged, b: &Logged| {
        a.0 == b.0
            && a.1 == b.1
            && a.2 == b.2
            && (a.1 == "update" || a.3 == b.3)
            // Only updates carry a position in the game's file.
            && (a.1 != "update"
                || ((a.4[0] - b.4[0]).abs() < 0.04 && (a.4[1] - b.4[1]).abs() < 0.04))
    };
    let first = game_events
        .iter()
        .zip(&ours_events)
        .position(|(a, b)| !same(a, b));
    eprintln!(
        "events: game {}, ours {}; first difference at {first:?}",
        game_events.len(),
        ours_events.len()
    );
    if let Some(i) = first {
        for k in i.saturating_sub(3)..(i + 8).min(game_events.len()).min(ours_events.len()) {
            eprintln!("game {:?}\nours {:?}", game_events[k], ours_events[k]);
        }
    }
    Some(first.map_or(u64::MAX, |i| game_events[i].0))
}

/// `c-RXD-1`: the creeps' chase follows the game.
///
/// PROVISIONAL: the events are the game's up to tick 2317 (looked at next).
#[test]
fn crowd_chase_report() {
    let Some(first) = report("c-RXD-1", 9300) else {
        return;
    };
    assert!(first >= 2317, "first difference at tick {first}");
}

/// `c-RXI-1`: the workers' walks between the mine and the town hall.
///
/// PROVISIONAL: the events are the game's up to tick 1327 (looked at next).
#[test]
fn gathering_walks_report() {
    let Some(first) = report("c-RXI-1", 11300) else {
        return;
    };
    assert!(first >= 1327, "first difference at tick {first}");
}

/// Each creep takes its first path update in the tick of the game: the
/// acquisition checks as engine timers and the responders in the order of
/// the grid (SPEC-0019).
#[test]
fn crowd_chase_creeps_start() {
    let Some(ours) = play(2000) else {
        return;
    };
    let game = events(&game_data().join("replays/c-RXD-1.events.tsv"));
    for unit in 6..10 {
        let g = game.iter().find(|e| e.2 == unit && e.1 == "update");
        let o = ours.events.iter().find(|e| e.2 == unit && e.1 == "update");
        eprintln!("creep {unit}: game {g:?}\n         ours {o:?}");
        assert_eq!(g.map(|e| e.0), o.map(|e| e.0), "creep {unit}");
    }
}

/// The units are handled in the game's order within each tick (SPEC-0021,
/// "Searches of many units in the same tick"): the creeps by the ends of
/// their reaction delays, the peasants by their orders, newest first.
#[test]
fn crowd_chase_order_of_updates() {
    let Some(ours) = play(1840) else {
        return;
    };
    let game = events(&game_data().join("replays/c-RXD-1.events.tsv"));
    let order = |list: &[Logged]| {
        let mut by_tick: BTreeMap<u64, Vec<i32>> = BTreeMap::new();
        for e in list.iter().filter(|e| e.1 == "update" && e.0 < 1837) {
            by_tick.entry(e.0).or_default().push(e.2);
        }
        by_tick
    };
    assert_eq!(order(&ours.events), order(&game));
}

/// `c-RTW-3` after the group `smart` of turn 233 (ticks 4806 to 5600):
/// the path events of the footmen against the game's
/// `c-RTW-3.events.tsv`, units by their creation numbers. The tick of the
/// first event that differs.
///
/// PROVISIONAL: the events are the game's up to the tick asserted (looked
/// at next).
#[test]
fn group_walks_report() {
    let dir = game_data().join("replays");
    let path = dir.join("c-RTW-3.w3g");
    if !path.exists() {
        return;
    }
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    let ids = ow3_sim::replay::read_object_ids(
        &std::fs::read_to_string(dir.join("c-RTW-3.units.txt")).unwrap(),
    );
    playback.set_object_ids(ids.clone());
    playback.game.state.borrow_mut().path_trace = Some(Vec::new());
    while playback.game.ticks() < 5600 && playback.step_tick() {}
    // The creation number of each handle: the nth unit of its type.
    let units = playback.game.units();
    let creation = |h: u32| -> i32 {
        let Some(u) = units.iter().find(|u| u.handle == h) else {
            return -1;
        };
        let nth = units
            .iter()
            .filter(|v| v.type_id == u.type_id)
            .position(|v| v.handle == h)
            .unwrap();
        let mut of_type: Vec<u32> = ids
            .iter()
            .filter(|&&(_, t)| t == u.type_id)
            .map(|&(n, _)| n)
            .collect();
        of_type.sort_unstable();
        of_type.get(nth).map_or(-1, |&n| n as i32)
    };
    let state = playback.game.state.borrow();
    let [x0, y0, _, _] = state.world_bounds.map(f32::from_bits);
    let kinds = ["update", "wait4", "made_wait20", "blocked"];
    let mut ours: Vec<Logged> = state
        .path_trace
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|e| e.tick >= 4806)
        .map(|e: &PathEvent| {
            let kind = match e.kind {
                PathEventKind::Update => "update",
                PathEventKind::Wait => "wait4",
                PathEventKind::MadeWait => "made_wait20",
                PathEventKind::Blocked => "blocked",
            };
            let [x, y] = e.at.map(f32::from_bits);
            (
                e.tick,
                kind.to_owned(),
                creation(e.a),
                e.b.map(creation),
                [x + x0, y + y0],
            )
        })
        .collect();
    let mut game: Vec<Logged> = events(&dir.join("c-RTW-3.events.tsv"))
        .into_iter()
        .filter(|e| kinds.contains(&e.1.as_str()))
        .collect();
    ours.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    game.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    let same = |a: &Logged, b: &Logged| {
        a.0 == b.0
            && a.1 == b.1
            && a.2 == b.2
            && (a.1 == "update" || a.3 == b.3)
            && (a.1 != "update"
                || ((a.4[0] - b.4[0]).abs() < 0.04 && (a.4[1] - b.4[1]).abs() < 0.04))
    };
    let first = game.iter().zip(&ours).position(|(a, b)| !same(a, b));
    eprintln!(
        "events: game {}, ours {}; first difference at {first:?}",
        game.len(),
        ours.len()
    );
    if let Some(i) = first {
        for k in i.saturating_sub(3)..(i + 8).min(game.len()).min(ours.len()) {
            eprintln!("game {:?}\nours {:?}", game[k], ours[k]);
        }
    }
    let tick = first.map_or(u64::MAX, |i| game[i].0);
    assert!(tick >= 5371, "first difference at tick {tick}");
}
