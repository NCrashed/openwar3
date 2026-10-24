//! The object table of `c-SAK-1` (SPEC-0024): every taking and freeing of
//! an entry read in the game (`c-SAK-1.objects.tsv`), compared in order
//! with the simulation's. Report only while the table is being built up
//! mechanic by mechanic. Skipped without the game data.

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
/// for an object made before the list starts. The list's tick is read from
/// the world clock, which drifts from the tick count late in a game by up
/// to one tick: ticks are compared within one.
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

#[test]
fn heroes_three_objects_report() {
    let name = "c-SAK-1";
    let Some(game) = game_rows(name) else {
        return;
    };
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    // The table as the game has it at 1.0 s (the first takings of the
    // list come from the free list 806, 805, 818, then it grows from 821).
    // The time of day holds 767, 647 and 819 (two watches, the rate).
    let mut table = ObjectTable::new(821, vec![818, 805, 806], 821);
    table.day = vec![767, 647, 819];
    playback.game.set_table(table, true);
    // (A tick or two past the list's last, whose ticks may run one short.)
    let until = game.last().map_or(0, |r| r.0) + 2;
    while playback.game.ticks() <= until && playback.step_tick() {}
    let ours = playback.game.table_log();
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
    let first = game
        .iter()
        .zip(&ours)
        .position(|(g, o)| {
            g.0.abs_diff(o.tick) > 1
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
                "{} {} {} {:?} {}",
                g.0,
                if g.1 { "new " } else { "free" },
                g.2,
                g.3,
                g.4
            )
        });
        let o = ours.get(k).map(|o| {
            format!(
                "{} {} {} {} {}",
                o.tick,
                if o.new { "new " } else { "free" },
                o.entry,
                o.creation,
                o.tag
            )
        });
        eprintln!("{k}: game {g:?}\n{k}: ours {o:?}");
    }
}

/// The turn checksums of `c-SAK-1` with the object table from 1.0 s on:
/// without `B`, `N`, `C`, `D`, `E` and `R` what is left of each recorded
/// checksum depends on the game sum `G` and the constants only, so it
/// changes only where a player's units or selections change. Report the
/// turns where it changes.
#[test]
fn heroes_three_checksum_report() {
    let name = "c-SAK-1";
    if game_rows(name).is_none() {
        return;
    }
    let dir = game_data().join("replays");
    let replay = Replay::parse(&std::fs::read(dir.join(format!("{name}.w3g"))).unwrap()).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    let mut table = ObjectTable::new(821, vec![818, 805, 806], 821);
    table.day = vec![767, 647, 819];
    playback.game.set_table(table, false);
    playback.run();
    let turns = turns(&replay);
    let mut last = None;
    for (k, (turn, marks)) in turns.iter().zip(&playback.marks).enumerate() {
        let parts = Parts {
            b: time_part(turn.before),
            n: turn.n,
            c: marks.c,
            d: marks.d,
            e: marks.e,
            r: marks.r,
            g: 0,
        };
        let rest = turn.recorded ^ turn_checksum(&Statics::default(), &parts);
        if last != Some(rest) {
            eprintln!(
                "turn {} ({} ms): rest {rest:08X}, C {} D {}",
                k + 1,
                turn.after,
                marks.c,
                marks.d
            );
            last = Some(rest);
        }
    }
}
