//! The object table of `c-SAK-1` (SPEC-0024): every taking and freeing of
//! an entry read in the game (`c-SAK-1.objects.tsv`), compared in order
//! with the simulation's: all 11848 rows match. Skipped without the game
//! data.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::checksum::{Parts, Statics, time_part, turn_checksum, turns};
use ow3_sim::game::table::ObjectTable;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

/// (tick, new, entry, creation, tag, world clock); the creation is `None`
/// for an object made before the list starts. The list's tick column runs
/// up to two ahead of our tick count late in the game, so rows are
/// compared by the world clock of the tick they are counted in, within a
/// tick (an event at a tick's boundary may be counted in either;
/// `OW3_SLACK` sets another bound in seconds).
type Row = (u64, bool, u32, Option<u32>, String, f32);

fn game_rows(name: &str) -> Option<Vec<Row>> {
    let text =
        std::fs::read_to_string(game_data().join(format!("replays/{name}.objects.tsv"))).ok()?;
    let rows = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let w: Vec<&str> = l.split('\t').collect();
            let tick = w.first()?.parse().ok()?;
            let new = *w.get(3)? == "new";
            // Objects of the second pool (`p2:n`) are not in the table.
            let entry = w.get(4)?.parse().ok()?;
            let creation = w
                .get(5)?
                .parse::<i64>()
                .ok()
                .and_then(|c| u32::try_from(c).ok());
            let tag = w.get(6).map_or("", |t| t.trim()).to_string();
            let clock = w.get(2)?.parse().ok()?;
            Some((tick, new, entry, creation, tag, clock))
        })
        .collect();
    Some(rows)
}

/// The object table as the game has it at the start: 821 entries, the
/// free list empty (`C` = 821 at the first turn), then 818, 805 and 806
/// freed before 0.1 s (the first takings of the list at 1.0 s come from
/// the free list 806, 805, 818, then it grows from 821). The time of day
/// holds 767, 647 and 819 (two watches, the rate).
fn start_table() -> ObjectTable {
    let mut table = ObjectTable::new(821, Vec::new(), 821);
    for e in [818, 805, 806] {
        table.free_at(1, e, "map start");
    }
    table.day = vec![767, 647, 819];
    table
}

#[test]
fn heroes_three_objects() {
    if let Some(first) = compare_objects("c-SAK-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// The hero test `c-RZH-1` (Mountain King, footmen, creeps).
#[test]
fn heroes_one_objects() {
    if let Some(first) = compare_objects("c-RZH-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBG-1`: the Thunder Clap probe (two ogres and a paladin slowed, the
/// slows ending by time, units on hold position).
#[test]
fn thunder_clap_objects() {
    if let Some(first) = compare_objects("c-SBG-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBG-1`: every turn checksum, with the object table as the game
/// starts it.
#[test]
fn thunder_clap_checksums() {
    if let Some(first) = turn_parts("c-SBG-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RZJ-1`: the second hero test (Archmage, Paladin, water elementals,
/// an Ogre Lord's camp): report only while its table is being built up.
#[test]
fn heroes_two_checksums_report() {
    turn_parts("c-RZJ-1");
}

/// The rows of replay `name`'s object list against the simulation's: the
/// first that differs, `None` when all match; nothing without the list.
fn compare_objects(name: &str) -> Option<Option<usize>> {
    let game = game_rows(name)?;
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.game.set_table(start_table(), true);
    // (Some ticks past the list's last, whose tick column runs ahead.)
    // OW3_UNTIL: play to that tick instead.
    let until = std::env::var("OW3_UNTIL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(game.last().map_or(0, |r| r.0) + 10);
    while playback.game.ticks() <= until && playback.step_tick() {}
    // Not the objects the game makes at its end, after the replay's last
    // turn (c-RZH-1 at 63.0 s, the tick of the list's first row marked so),
    // which no turn checksum sees.
    let end = game
        .iter()
        .find(|r| r.4.contains("after the last turn"))
        .map_or(u64::MAX, |r| r.0);
    let game: Vec<_> = game.into_iter().filter(|r| r.0 < end).collect();
    // (The frees of the map start, before the list's first row.)
    let first_tick = game.first().map_or(0, |r| r.0);
    let ours: Vec<_> = playback
        .game
        .table_log()
        .into_iter()
        .filter(|o| o.tick + 10 >= first_tick)
        .collect();
    // OW3_OBJECTS="from to": our rows in that range of positions.
    if let Ok(range) = std::env::var("OW3_OBJECTS") {
        let w: Vec<usize> = range
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        for (k, o) in ours.iter().enumerate().take(w[1]).skip(w[0]) {
            eprintln!(
                "{k}: {} {} {} {} {}",
                o.tick, o.new, o.entry, o.creation, o.tag
            );
        }
    }
    // The world clock of the tick each of our rows is counted in.
    let clocks: std::collections::BTreeMap<u64, f32> = playback
        .game
        .table_clocks()
        .into_iter()
        .map(|(t, c)| (t, f32::from_bits(c)))
        .collect();
    let slack = std::env::var("OW3_SLACK")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0055);
    let first = game
        .iter()
        .zip(&ours)
        .position(|(g, o)| {
            clocks.get(&o.tick).is_none_or(|&c| (g.5 - c).abs() > slack)
                || g.1 != o.new
                || g.2 != o.entry
                || g.3
                    .is_some_and(|c| o.creation != u32::MAX && c != o.creation)
        })
        .or((game.len() != ours.len()).then(|| game.len().min(ours.len())));
    eprintln!(
        "objects: game {}, ours {}; first difference at {first:?}",
        game.len(),
        ours.len()
    );
    let at = first.unwrap_or(game.len().min(ours.len()));
    for k in at.saturating_sub(4)..(at + 8) {
        let g = game.get(k).map(|g| {
            format!(
                "{} {} {} {} {:?} {}",
                g.0,
                g.5,
                if g.1 { "new " } else { "free" },
                g.2,
                g.3,
                g.4
            )
        });
        let o = ours.get(k).map(|o| {
            format!(
                "{} {} {} {} {} {}",
                o.tick,
                clocks.get(&o.tick).copied().unwrap_or(0.0),
                if o.new { "new " } else { "free" },
                o.entry,
                o.creation,
                o.tag
            )
        });
        eprintln!("{k}: game {g:?}\n{k}: ours {o:?}");
    }
    Some(first)
}

/// The turn checksums of `c-SAK-1` against the parts read in the game
/// (`c-SAK-1.parts.tsv`: turn, `t`, checksum, `B`, `N`, `C`, `D`, `E`,
/// `R`, `G`): `B`, `N`, `C`, `D`, `E` and `R` of the playback are the
/// game's at every turn, and so is `G`; every recorded checksum follows,
/// up to the one constant of `K1`, `K2`, `K3` (SPEC-0024).
#[test]
fn heroes_three_checksums() {
    if let Some(first) = turn_parts("c-SAK-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RZH-1`: every turn checksum, with the object table as the game
/// starts it.
#[test]
fn heroes_one_checksums() {
    if let Some(first) = turn_parts("c-RZH-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// The turns of replay `name` against its parts file: the first turn
/// whose recorded checksum, parts or G differ, `None` when all match;
/// nothing without the file.
fn turn_parts(name: &str) -> Option<Option<usize>> {
    let text =
        std::fs::read_to_string(game_data().join(format!("replays/{name}.parts.tsv"))).ok()?;
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.game.set_table(start_table(), false);
    playback.run();
    let turns = turns(&replay);
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let mut constant = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split('\t').collect();
        let k: usize = w[0].parse().unwrap();
        let (turn, marks) = (&turns[k - 1], &playback.marks[k - 1]);
        let ours = (
            turn.recorded,
            time_part(turn.before),
            turn.n,
            marks.c,
            marks.d,
            marks.e,
            marks.r,
            marks.g,
        );
        let game = (
            hex(w[2]),
            w[3].parse().unwrap(),
            hex(w[4]),
            w[5].parse().unwrap(),
            w[6].parse().unwrap(),
            hex(w[7]),
            hex(w[8]),
            hex(w[9]),
        );
        let parts = Parts {
            b: ours.1,
            n: ours.2,
            c: ours.3,
            d: ours.4,
            e: ours.5,
            r: ours.6,
            g: ours.7,
        };
        let rest = turn.recorded ^ turn_checksum(&Statics::default(), &parts);
        if ours != game || *constant.get_or_insert(rest) != rest {
            eprintln!("{name}: turn {k} differs\n  ours {ours:x?}\n  game {game:x?}");
            return Some(Some(k));
        }
    }
    eprintln!("{name}: all turns match");
    Some(None)
}
