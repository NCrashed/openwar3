//! The object table of `c-SAK-1` (SPEC-0024): every taking and freeing of
//! an entry read in the game (`c-SAK-1.objects.tsv`), compared in order
//! with the simulation's. Report only while the table is being built up
//! mechanic by mechanic. Skipped without the game data.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::game::table::ObjectTable;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

/// (tick, new, entry, creation, tag); the creation is `None` for an object
/// made before the list starts.
type Row = (u64, bool, u32, Option<u32>, String);

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
            let entry = w.get(4)?.parse().ok()?;
            let creation = w
                .get(5)?
                .parse::<i64>()
                .ok()
                .and_then(|c| u32::try_from(c).ok());
            let tag = w.get(6).map_or("", |t| t.trim()).to_string();
            Some((tick, new, entry, creation, tag))
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
    playback
        .game
        .set_table(ObjectTable::new(821, vec![818, 805, 806], 821), true);
    let until = game.last().map_or(0, |r| r.0);
    while playback.game.ticks() <= until && playback.step_tick() {}
    let ours = playback.game.table_log();
    let first = game
        .iter()
        .zip(&ours)
        .position(|(g, o)| {
            g.0 != o.tick || g.1 != o.new || g.2 != o.entry || g.3.is_some_and(|c| c != o.creation)
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
