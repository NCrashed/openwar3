//! The object table of `c-SAK-1` (SPEC-0024): every taking and freeing of
//! an entry read in the game (`c-SAK-1.objects.tsv`), compared in order
//! with the simulation's: all 11848 rows match. Skipped without the game
//! data.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::checksum::{ICECROWN_TEST, Parts, Statics, time_part, turn_checksum, turns};
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
/// The replays whose table is counted from the map's load (IceCrown).
const INIT_TABLES: [&str; 2] = ["c-SCC-1", "c-SEA-1"];

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
/// an Ogre Lord's camp): every turn checksum.
#[test]
fn heroes_two_checksums() {
    if let Some(first) = turn_parts("c-RZJ-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RZJ-1`'s object table.
#[test]
fn heroes_two_objects() {
    if let Some(first) = compare_objects("c-RZJ-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-RXR-1`: the battle reference (melee, riflemen, missiles): every
/// turn checksum.
#[test]
fn battle_checksums() {
    if let Some(first) = turn_parts("c-RXR-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RXR-1`'s object table.
#[test]
fn battle_objects() {
    if let Some(first) = compare_objects("c-RXR-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-RXI-1`: six peasants harvesting gold: every turn checksum.
#[test]
fn harvest_checksums() {
    if let Some(first) = turn_parts("c-RXI-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RXI-1`'s object table.
#[test]
fn harvest_objects() {
    if let Some(first) = compare_objects("c-RXI-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-RTW-3`: player orders to footmen (moves, patrol, follow, group and
/// Shift group orders, stop): every turn checksum.
#[test]
fn orders_checksums() {
    if let Some(first) = turn_parts("c-RTW-3") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RTW-3`'s object table.
#[test]
fn orders_objects() {
    if let Some(first) = compare_objects("c-RTW-3") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-RYP-1`: peasants harvesting lumber: every turn checksum.
#[test]
fn lumber_checksums() {
    if let Some(first) = turn_parts("c-RYP-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RYP-1`'s object table.
#[test]
fn lumber_objects() {
    if let Some(first) = compare_objects("c-RYP-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-RXD-1`: gold harvest with kobolds attacking: every turn checksum.
#[test]
fn creeps_harvest_checksums() {
    if let Some(first) = turn_parts("c-RXD-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-RXD-1`'s object table.
#[test]
fn creeps_harvest_objects() {
    if let Some(first) = compare_objects("c-RXD-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBI-1`: Orc hero spells (Chain Lightning, War Stomp, Healing Wave):
/// every turn checksum.
#[test]
fn orc_spells_checksums() {
    if let Some(first) = turn_parts("c-SBI-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBI-1`'s object table.
#[test]
fn orc_spells_objects() {
    if let Some(first) = compare_objects("c-SBI-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBJ-1`: Undead spells (Death Coil, Frost Nova, Impale, Carrion
/// Swarm): every turn checksum.
#[test]
fn undead_spells_checksums() {
    if let Some(first) = turn_parts("c-SBJ-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBJ-1`'s object table.
#[test]
fn undead_spells_objects() {
    if let Some(first) = compare_objects("c-SBJ-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBH-1`: Flame Strike (its point mover, the area's two phases and
/// its timer of random delays): every turn checksum.
#[test]
fn flame_strike_checksums() {
    if let Some(first) = turn_parts("c-SBH-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBH-1`'s object table.
#[test]
fn flame_strike_objects() {
    if let Some(first) = compare_objects("c-SBH-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBK-1`: Night Elf spells (Mana Burn, Fan of Knives, Entangling
/// Roots): every turn checksum.
#[test]
fn night_elf_spells_checksums() {
    if let Some(first) = turn_parts("c-SBK-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBK-1`'s object table.
#[test]
fn night_elf_spells_objects() {
    if let Some(first) = compare_objects("c-SBK-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBL-1`, `c-SBM-1`, `c-SBN-1` and `c-SBO-1`: a hero of each race
/// levelling from creep kills: every turn checksum and the object table.
#[test]
fn levelling_checksums() {
    for name in ["c-SBL-1", "c-SBM-1", "c-SBN-1", "c-SBO-1"] {
        if let Some(first) = turn_parts(name) {
            assert_eq!(first, None, "{name}: the turns part at turn {first:?}");
        }
    }
}

#[test]
fn levelling_objects() {
    for name in ["c-SBL-1", "c-SBM-1", "c-SBN-1", "c-SBO-1"] {
        if let Some(first) = compare_objects(name) {
            assert_eq!(
                first, None,
                "{name}: the object tables part at row {first:?}"
            );
        }
    }
}

/// `c-SBS-1`: a level-3 revive and six heroes picking up items at different
/// distances: every turn checksum.
#[test]
fn pickups_checksums() {
    if let Some(first) = turn_parts("c-SBS-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBS-1`'s object table.
#[test]
fn pickups_objects() {
    if let Some(first) = compare_objects("c-SBS-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBR-1`: a level-2 hero revived, a salve of three charges used, dropped
/// and picked up again: every turn checksum.
#[test]
fn salve_revive_checksums() {
    if let Some(first) = turn_parts("c-SBR-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBR-1`'s object table.
#[test]
fn salve_revive_objects() {
    if let Some(first) = compare_objects("c-SBR-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBQ-1`: a hero revived at an altar, items picked up and used: every
/// turn checksum.
#[test]
fn altar_items_checksums() {
    if let Some(first) = turn_parts("c-SBQ-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBQ-1`'s object table.
#[test]
fn altar_items_objects() {
    if let Some(first) = compare_objects("c-SBQ-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBP-1`: auras of units and heroes killed while they run: every turn
/// checksum.
#[test]
fn dead_auras_checksums() {
    if let Some(first) = turn_parts("c-SBP-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBP-1`'s object table.
#[test]
fn dead_auras_objects() {
    if let Some(first) = compare_objects("c-SBP-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
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
    let mut playback = if INIT_TABLES.contains(&name) {
        let mut table = ObjectTable::new(640, Vec::new(), 640);
        table.log = Some(Vec::new());
        table.clocks = Some(Vec::new());
        Playback::with_table(&program, &map, &replay, Some(table)).unwrap()
    } else {
        let mut playback = Playback::new(&program, &map, &replay).unwrap();
        playback.load_object_ids(&dir.join(format!("{name}.w3g")));
        playback.game.set_table(start_table(), true);
        playback
    };
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
    // IceCrown's table from the map's load (`compare_init`).
    let mut playback = if INIT_TABLES.contains(&name) {
        let table = ObjectTable::new(640, Vec::new(), 640);
        Playback::with_table(&program, &map, &replay, Some(table)).unwrap()
    } else {
        let mut playback = Playback::new(&program, &map, &replay).unwrap();
        playback.load_object_ids(&dir.join(format!("{name}.w3g")));
        playback.game.set_table(start_table(), false);
        playback
    };
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

/// The turns of IceCrown replay `name` against the checksums recorded in
/// the replay: the first turn that differs, `None` when all match; nothing
/// without the replay.
fn turn_checksums(name: &str, through: Option<usize>) -> Option<Option<usize>> {
    let dir = game_data().join("replays");
    let data = std::fs::read(dir.join(format!("{name}.w3g"))).ok()?;
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    let turns = turns(&replay);
    while playback.step_slot() {
        let k = playback.marks.len() - 1;
        let (turn, marks) = (&turns[k], &playback.marks[k]);
        let parts = Parts {
            b: time_part(turn.before),
            n: turn.n,
            c: marks.c,
            d: marks.d,
            e: marks.e,
            r: marks.r,
            g: marks.g,
        };
        let ours = turn_checksum(&ICECROWN_TEST, &parts);
        if ours != turn.recorded {
            eprintln!(
                "{name}: turn {} differs at {} ms\n  ours {ours:08x}\n  game {:08x}\n  parts {parts:x?}",
                k + 1,
                turn.before,
                turn.recorded,
            );
            for error in playback.game.state.borrow().errors.iter().take(8) {
                eprintln!("  script error: {error}");
            }
            for (player, native, count) in playback.game.ai_stub_calls() {
                eprintln!("  AI stub p{player}: {native} x{count}");
            }
            return Some(Some(k + 1));
        }
        if through == Some(k + 1) {
            eprintln!("{name}: the first {} turns match", k + 1);
            return Some(None);
        }
    }
    if turns.len() != playback.marks.len() {
        return Some(Some(turns.len().min(playback.marks.len()) + 1));
    }
    eprintln!("{name}: all turns match");
    Some(None)
}

/// `c-SBT-1`: ten Human buildings made by the script, their Blight Dispels
/// ending at 0.88 s and 1.2 s: every turn checksum.
#[test]
fn buildings_checksums() {
    if let Some(first) = turn_parts("c-SBT-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBT-1`'s object table.
#[test]
fn buildings_objects() {
    if let Some(first) = compare_objects("c-SBT-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBU-1`: a hero revived twice with its default place by the altar
/// taken: every turn checksum.
#[test]
fn revive_place_checksums() {
    if let Some(first) = turn_parts("c-SBU-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBU-1`'s object table.
#[test]
fn revive_place_objects() {
    if let Some(first) = compare_objects("c-SBU-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBV-1`: the Mountain King's Avatar and the Paladin's Resurrection at
/// level 6: every turn checksum.
#[test]
fn avatar_resurrection_checksums() {
    if let Some(first) = turn_parts("c-SBV-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBV-1`'s object table.
#[test]
fn avatar_resurrection_objects() {
    if let Some(first) = compare_objects("c-SBV-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBW-1`: the Blademaster's Bladestorm and the Far Seer's Earthquake
/// at level 6: every turn checksum.
#[test]
#[ignore = "not matched yet"]
fn bladestorm_earthquake_checksums() {
    if let Some(first) = turn_parts("c-SBW-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBW-1`'s object table.
#[test]
fn bladestorm_earthquake_objects() {
    if let Some(first) = compare_objects("c-SBW-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBX-1`: the Death Knight's Animate Dead and the Lich's Death and
/// Decay at level 6: every turn checksum.
#[test]
fn animate_dead_decay_checksums() {
    if let Some(first) = turn_parts("c-SBX-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBX-1`'s object table.
#[test]
fn animate_dead_decay_objects() {
    if let Some(first) = compare_objects("c-SBX-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBY-1`: the Priestess of the Moon's Starfall and the Keeper of the
/// Grove's Tranquility at level 6: every turn checksum.
#[test]
fn starfall_tranquility_checksums() {
    if let Some(first) = turn_parts("c-SBY-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBY-1`'s object table.
#[test]
fn starfall_tranquility_objects() {
    if let Some(first) = compare_objects("c-SBY-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SBZ-1`: power building, peasants helping on a farm and a barracks:
/// every turn checksum.
#[test]
fn power_building_checksums() {
    if let Some(first) = turn_parts("c-SBZ-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SBZ-1`'s object table.
#[test]
fn power_building_objects() {
    if let Some(first) = compare_objects("c-SBZ-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SCA-1`: a Giant Sea Turtle lured by a build and its cancel at night,
/// then shot from high ground (hub #14): every turn checksum.
#[test]
#[ignore = "not matched yet"]
fn turtle_checksums() {
    if let Some(first) = turn_parts("c-SCA-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SCA-1`'s object table.
#[test]
fn turtle_objects() {
    if let Some(first) = compare_objects("c-SCA-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SCC-1`: the melee start of four races on (12)IceCrown: every turn
/// checksum.
#[test]
fn melee_start_checksums() {
    if let Some(first) = turn_parts("c-SCC-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}

/// `c-SEA-1`: three computer players running the standard melee AI on
/// (12)IceCrown for 180 seconds: every turn checksum.
#[test]
#[ignore = "not matched yet"]
fn melee_ai_checksums() {
    if let Some(first) = turn_checksums("c-SEA-1", None) {
        assert_eq!(first, None, "the first differing turn {first:?}");
    }
}

/// `c-SEA-1`'s synchronous initialization, first 0.100 s resumption of
/// the three standard melee AI scripts, first paid-worker placement and
/// the queued ordinary-mine state and the first entangled-mine arrivals:
/// the first ten turn checksums.
#[test]
fn melee_ai_start_checksums() {
    if let Some(first) = turn_checksums("c-SEA-1", Some(10)) {
        assert_eq!(first, None, "the first differing turn {first:?}");
    }
}

/// The supplied race scripts resume at the 0.100 world-clock boundary and
/// start their player-bound worker-assignment threads in player order.
#[test]
fn melee_ai_scripts_resume() {
    let dir = game_data().join("replays");
    let Ok(data) = std::fs::read(dir.join("c-SEA-1.w3g")) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..3 {
        assert!(playback.step_slot());
    }
    let starts = playback
        .game
        .table_log()
        .iter()
        .filter(|event| event.new && event.call_name == "StartThread")
        .count();
    assert_eq!(starts, 12);
}

/// The main-table activity of the three AI batches at world clock 0.100,
/// from the first `StartThread` object through the turn-3 checksum.
#[test]
fn melee_ai_turn3_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn3-objects.tsv")) else {
        return;
    };
    let expected: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .map(|words| {
            (
                words[4] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let replay = Replay::parse(&std::fs::read(dir.join("c-SEA-1.w3g")).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..2 {
        assert!(playback.step_slot());
    }
    let log = playback.game.table_log();
    let start = log
        .iter()
        .position(|event| event.new && event.creation == 12102)
        .unwrap();
    let ours = &log[start..];
    let first = expected
        .iter()
        .zip(ours)
        .position(|(expected, ours)| *expected != (ours.new, ours.entry, ours.creation))
        .or((expected.len() != ours.len()).then(|| expected.len().min(ours.len())));
    assert_eq!(first, None, "the first differing table event {first:?}");
}

/// The four main-table events after `c-SEA-1`'s turn-3 checkpoint through
/// turn 4: a paid worker's old placement records go, then its two new
/// placement records take their entries.
#[test]
fn melee_ai_turn4_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn4-events.tsv")) else {
        return;
    };
    let expected: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[3] == "take" || words[3] == "free")
        .map(|words| {
            (
                words[3] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let replay = Replay::parse(&std::fs::read(dir.join("c-SEA-1.w3g")).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..2 {
        assert!(playback.step_slot());
    }
    let start = playback.game.table_log().len();
    assert!(playback.step_slot());
    let log = playback.game.table_log();
    let ours: Vec<_> = log[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected);
}

/// The paid acolyte's assignment walk becomes stale when it joins the
/// Haunted Gold Mine and is released at its first later update. The path is
/// freed before the cluster, leaving the latter at the head of the table.
#[test]
fn melee_ai_turn5_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn5-events.tsv")) else {
        return;
    };
    let expected: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[4] == "take" || words[4] == "free")
        .map(|words| {
            (
                words[4] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let Ok(data) = std::fs::read(dir.join("c-SEA-1.w3g")) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..3 {
        assert!(playback.step_slot());
    }
    let start = playback.game.table_log().len();
    assert!(playback.step_slot());
    let log = playback.game.table_log();
    let ours: Vec<_> = log[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected);
}

/// No main-table object changes between the turn-5 and turn-6 checkpoints.
/// The first aura pass starts on the first queue step after turn 6, before
/// either assigned peon reaches the mine.
#[test]
fn melee_ai_turn6_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn6-events.tsv")) else {
        return;
    };
    let expected = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .find(|words| words[3] == "post-checkpoint" && words[5] != "-")
        .map(|words| (true, words[5].parse().unwrap(), words[6].parse().unwrap()))
        .unwrap();
    let Ok(data) = std::fs::read(dir.join("c-SEA-1.w3g")) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..4 {
        assert!(playback.step_slot());
    }
    let start = playback.game.table_log().len();
    assert!(playback.step_slot());
    assert_eq!(&playback.game.table_log()[start..], []);
    assert!(playback.step_tick());
    let log = playback.game.table_log();
    let ours: Vec<_> = log[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours.first(), Some(&expected));
}

/// The first aura sweep and both Orc peon arrivals between the turn-6 and
/// turn-7 checkpoints. The peons' walks are released only after the latter
/// checkpoint, so those four frees are not part of this vector.
#[test]
fn melee_ai_turn7_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn7-events.tsv")) else {
        return;
    };
    let expected: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[7] == "yes" && (words[3] == "take" || words[3] == "free"))
        .map(|words| {
            (
                words[3] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let expected_after: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[7] == "no" && (words[3] == "take" || words[3] == "free"))
        .map(|words| {
            (
                words[3] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let Ok(data) = std::fs::read(dir.join("c-SEA-1.w3g")) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..5 {
        assert!(playback.step_slot());
    }
    let start = playback.game.table_log().len();
    assert!(playback.step_slot());
    let ours: Vec<_> = playback.game.table_log()[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected);
    let start = playback.game.table_log().len();
    assert!(playback.step_tick());
    let ours: Vec<_> = playback.game.table_log()[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected_after);
}

/// The Night Elf wisp's arrival at its entangled mine between turns 9 and 10
/// checkpoints. The Orc peons do not change in this interval, and the wisp's
/// walk is released only after the turn-10 checkpoint.
#[test]
fn melee_ai_turn10_objects() {
    let dir = game_data().join("replays");
    let Ok(text) = std::fs::read_to_string(dir.join("c-SEA-1.ai-turn10-events.tsv")) else {
        return;
    };
    let expected: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[7] == "yes" && (words[3] == "take" || words[3] == "free"))
        .map(|words| {
            (
                words[3] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let expected_after: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("seq"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|words| words[7] == "no" && (words[3] == "take" || words[3] == "free"))
        .map(|words| {
            (
                words[3] == "take",
                words[5].parse().unwrap(),
                words[6].parse().unwrap(),
            )
        })
        .collect();
    let Ok(data) = std::fs::read(dir.join("c-SEA-1.w3g")) else {
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    for _ in 0..8 {
        assert!(playback.step_slot());
    }
    let start = playback.game.table_log().len();
    assert!(playback.step_slot());
    let ours: Vec<_> = playback.game.table_log()[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected);
    let start = playback.game.table_log().len();
    assert!(playback.step_tick());
    let ours: Vec<_> = playback.game.table_log()[start..]
        .iter()
        .map(|event| (event.new, event.entry, event.creation))
        .collect();
    assert_eq!(ours, expected_after);
}

/// `c-SCC-1`'s object table.
#[test]
#[ignore = "not matched yet"]
fn melee_start_objects() {
    if let Some(first) = compare_objects("c-SCC-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// `c-SCC-1`'s object table from the map's loading to 1.0 s: the load's
/// 640 entries, then IceCrown's script (its units, InitBlizzard, the melee
/// start) and the first ticks, against the game's (main table only).
#[test]
fn melee_init_objects() {
    if let Some(first) = compare_init("c-SCC-1") {
        assert_eq!(first, None, "the object tables part at row {first:?}");
    }
}

/// The rows of replay `name`'s table from the map's loading (its
/// `.objects-init.tsv`) against ours, the load's part as the table
/// starts: the first that differs, `None` when all match; nothing
/// without the list.
fn compare_init(name: &str) -> Option<Option<usize>> {
    let dir = game_data().join("replays");
    let text = std::fs::read_to_string(dir.join(format!("{name}.objects-init.tsv"))).ok()?;
    // (kind, entry, creation) of the main table's rows after the load.
    let game: Vec<(bool, u32, u32)> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|w| w[7] != "load" && !w[4].starts_with("p2:"))
        .map(|w| (w[1] == "new", w[4].parse().unwrap(), w[5].parse().unwrap()))
        .collect();
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    // The load takes 640 entries, in order, before the script (hub #15).
    let table = ObjectTable::new(640, Vec::new(), 640);
    let mut playback = Playback::with_table(&program, &map, &replay, Some(table)).unwrap();
    while playback.game.ticks() < 200 && playback.step_tick() {}
    let ours: Vec<(bool, u32, u32)> = playback
        .game
        .table_log()
        .into_iter()
        .map(|o| (o.new, o.entry, o.creation))
        .collect();
    let first = game.iter().zip(&ours).position(|(g, o)| g != o);
    Some(first.or((game.len() != ours.len()).then(|| game.len().min(ours.len()))))
}

/// `c-SCD-1`: the end of a melee game on (12)IceCrown (the players'
/// buildings killed, the victory): every turn checksum.
#[test]
#[ignore = "not matched yet"]
fn melee_end_checksums() {
    if let Some(first) = turn_parts("c-SCD-1") {
        assert_eq!(first, None, "the turns part at turn {first:?}");
    }
}
