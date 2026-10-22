//! The path agents of `c-RXI-1` (six peasants harvesting) and `c-RTW-3`
//! (footmen walking in groups): every position commit read in the game
//! (`<replay>.agents.tsv`), compared bit for bit with the simulation's.
//! c-RXI-1 and c-RTW-3 must match; c-RYP-1 reports the first commit of each unit
//! that differs. Skipped without the game data.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

/// (tick, unit) -> (x, y, vx, vy, heading), in path cells.
type Commits = BTreeMap<(u64, i32), [u32; 7]>;

fn game_commits(name: &str) -> Option<Commits> {
    let text =
        std::fs::read_to_string(game_data().join(format!("replays/{name}.agents.tsv"))).ok()?;
    let hex = |s: &str| u32::from_str_radix(s, 16).ok();
    let mut game = Commits::new();
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = l.split('\t').collect();
        let (Some(tick), Some(unit)) = (w[0].parse().ok(), w[2].parse().ok()) else {
            continue;
        };
        let mut v: Vec<u32> = w[3..8].iter().filter_map(|s| hex(s)).collect();
        v.extend(w[8..10].iter().map(|s| hex(s.trim()).unwrap_or(0)));
        if v.len() == 7 {
            // The first commit of a tick: the update.
            game.entry((tick, unit))
                .or_insert([v[0], v[1], v[2], v[3], v[4], v[5], v[6]]);
        }
    }
    Some(game)
}

/// Plays replay `name` up to `until` and gives its commits, units named
/// by `label` (handle, units at the end, object ids).
fn our_commits(
    name: &str,
    until: u64,
    label: impl Fn(u32, &[ow3_sim::game::UnitView], &[(u32, i32)]) -> i32,
) -> Commits {
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    let ids = std::fs::read_to_string(dir.join(format!("{name}.units.txt")))
        .map(|t| ow3_sim::replay::read_object_ids(&t))
        .unwrap_or_default();
    if !ids.is_empty() {
        playback.set_object_ids(ids.clone());
    }
    playback.game.state.borrow_mut().agent_trace = Some(Vec::new());
    while playback.game.ticks() < until && playback.step_tick() {}
    let units = playback.game.units();
    let cells = |v: u32| (f32::from_bits(v) / 32.0).to_bits();
    let state = playback.game.state.borrow();
    let mut ours = Commits::new();
    for &(t, h, p, v, heading, aim) in state.agent_trace.as_deref().unwrap_or(&[]) {
        ours.entry((t, label(h, &units, &ids))).or_insert([
            cells(p[0]),
            cells(p[1]),
            cells(v[0]),
            cells(v[1]),
            heading,
            cells(aim[0]),
            cells(aim[1]),
        ]);
    }
    ours
}

/// The first differing commit of each unit, printed with the commits
/// before it.
fn report(game: &Commits, ours: &Commits) -> BTreeMap<i32, u64> {
    // OW3_AGENT="unit from to" lists both sides' commits of one unit.
    if let Ok(v) = std::env::var("OW3_AGENT") {
        let w: Vec<i64> = v
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [u, from, to] = w[..] {
            let f = |a: &[u32; 7]| a.map(f32::from_bits);
            for t in from as u64..=to as u64 {
                let (g, o) = (game.get(&(t, u as i32)), ours.get(&(t, u as i32)));
                if g.is_some() || o.is_some() {
                    eprintln!("{t}: game {:?}\n{t}: ours {:?}", g.map(f), o.map(f));
                }
            }
        }
    }
    let mut first: BTreeMap<i32, u64> = BTreeMap::new();
    for (&(t, u), g) in game {
        if first.contains_key(&u) {
            continue;
        }
        match ours.get(&(t, u)) {
            Some(o) if o[..4] == g[..4] => {}
            // The commits at orders and stops are not traced.
            None => {}
            Some(o) => {
                let f = |a: &[u32; 7]| a.map(f32::from_bits);
                let before: Vec<_> = game
                    .range(..(t, u))
                    .filter(|((_, v), _)| *v == u)
                    .rev()
                    .take(4)
                    .collect();
                for (&(s, _), h) in before.into_iter().rev() {
                    eprintln!(
                        "  {s}: game {:?}\n  {s}: ours {:?}",
                        f(h),
                        ours.get(&(s, u)).map(f)
                    );
                }
                eprintln!(
                    "unit {u} tick {t}: game {g:08x?} {:?}\n  ours {o:08x?} {:?}",
                    f(g),
                    f(o)
                );
                first.insert(u, t);
            }
        }
    }
    // Stored headings, which only show in the velocity later.
    let mut headings: BTreeMap<i32, u64> = BTreeMap::new();
    for (&(t, u), g) in game {
        if let Some(o) = ours.get(&(t, u))
            && o[4] != g[4]
            && !headings.contains_key(&u)
        {
            eprintln!("heading of {u} at {t}: game {:08x} ours {:08x}", g[4], o[4]);
            headings.insert(u, t);
        }
    }
    first
}

#[test]
fn harvest_agents_report() {
    let Some(game) = game_commits("c-RXI-1") else {
        return;
    };
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    // Peasants by their order among the units.
    let ours = our_commits("c-RXI-1", 12000, |h, units, _| {
        units
            .iter()
            .filter(|u| u.type_id == code(b"hpea"))
            .position(|u| u.handle == h)
            .map_or(-1, |i| i as i32)
    });
    // Every commit of the path agents is the game's, bit for bit.
    let first = report(&game, &ours);
    assert!(first.is_empty(), "first differences: {first:?}");
}

#[test]
fn group_agents_report() {
    let Some(game) = game_commits("c-RTW-3") else {
        return;
    };
    // Units by their creation numbers: the nth unit of its type.
    let ours = our_commits("c-RTW-3", 6000, |h, units, ids| {
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
    });
    // Every commit of the path agents is the game's, bit for bit.
    let first = report(&game, &ours);
    assert!(first.is_empty(), "first differences: {first:?}");
}

#[test]
fn lumber_agents_report() {
    let Some(game) = game_commits("c-RYP-1") else {
        return;
    };
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    // Peasants by their order among the units.
    let ours = our_commits("c-RYP-1", 12205, |h, units, _| {
        units
            .iter()
            .filter(|u| u.type_id == code(b"hpea"))
            .position(|u| u.handle == h)
            .map_or(-1, |i| i as i32)
    });
    eprintln!("first differences: {:?}", report(&game, &ours));
}

#[test]
fn creep_agents_report() {
    let Some(game) = game_commits("c-RXD-1") else {
        return;
    };
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    let kinds = [code(b"hpea"), code(b"nkob"), code(b"nkot")];
    // Peasants and creeps by their order of creation.
    let ours = our_commits("c-RXD-1", 8762, |h, units, _| {
        units
            .iter()
            .filter(|u| kinds.contains(&u.type_id))
            .position(|u| u.handle == h)
            .map_or(-1, |i| i as i32)
    });
    eprintln!("first differences: {:?}", report(&game, &ours));
}
