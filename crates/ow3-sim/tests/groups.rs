//! Group orders played from replays against the state of their members
//! read from the game at every cluster update (`<name>.members.tsv`,
//! SPEC-0025). Passes without checking anything when the replays are not
//! there.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

/// A member at a cluster update: its tick, its index among the units of
/// the order, its target (world), committed position (relative to the
/// world corner) and velocity, as bits.
struct Row {
    tick: u64,
    member: usize,
    target: [u32; 2],
    position: [u32; 2],
    velocity: [u32; 2],
}

fn rows(text: &str) -> Vec<Row> {
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let fields: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
        .collect();
    let agents: Vec<&str> = fields[..4].iter().map(|r| r[8]).collect();
    let world = |x: u32| (f32::from_bits(x) * 32.0).to_bits();
    fields
        .iter()
        .map(|r| {
            let clock = f32::from_bits(hex(r[0]));
            let target = [hex(r[13]), hex(r[14])].map(|x| {
                if x == 0 {
                    0
                } else {
                    (f32::from_bits(x) * 32.0 - 12288.0).to_bits()
                }
            });
            Row {
                // The clock of tick k is the sum of k - 1 ticks.
                tick: (clock / 0.005).round() as u64 + 1,
                member: agents.iter().position(|a| *a == r[8]).unwrap(),
                target,
                position: [hex(r[15]), hex(r[16])].map(world),
                velocity: [hex(r[17]), hex(r[18])].map(world),
            }
        })
        .collect()
}

/// Plays replay `name` and returns the first rows of its members file
/// that differ from the simulation, at most `limit`.
fn mismatches(name: &str, limit: usize) -> Option<Vec<String>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let path = dir.join(format!("replays/{name}.w3g"));
    let text = std::fs::read_to_string(path.with_extension("members.tsv")).ok()?;
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let map_path = dir.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&dir, &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.load_object_ids(&path);
    let rows = rows(&text);
    while playback.game.ticks() < rows[0].tick - 1 {
        playback.step_tick();
    }
    let units: Vec<u32> = playback.game.units().iter().map(|u| u.handle).collect();
    let mut found = Vec::new();
    for row in rows {
        while playback.game.ticks() < row.tick - 1 {
            playback.step_tick();
        }
        // The slots whose turn has come are applied before the updates of
        // the next tick.
        playback.begin_due_slots();
        let (o, v, t) = playback
            .game
            .motion_of(units[row.member])
            .unwrap_or_default();
        let target_differs = row.target != [0, 0] && t != row.target;
        if (o, v) != (row.position, row.velocity) || target_differs {
            found.push(format!(
                "tick {} member {}: position {:?} for {:?}, velocity {:?} for {:?}, target {:?} for {:?}",
                row.tick,
                row.member,
                o.map(f32::from_bits),
                row.position.map(f32::from_bits),
                v.map(f32::from_bits),
                row.velocity.map(f32::from_bits),
                t.map(f32::from_bits),
                row.target.map(f32::from_bits)
            ));
            if found.len() == limit {
                break;
            }
        }
    }
    Some(found)
}

/// `c-RSP-1`: a peasant, a rifleman, a knight and a footman. All four
/// walk to the game's places, start at the same updates (the knight and
/// the footman at the first, the rifleman held to the 19th, the peasant to
/// the 35th) and move bit for bit as in the game throughout: committed
/// positions and velocities at every update.
#[test]
fn mixed_group_follows_the_game() {
    if let Some(found) = mismatches("c-RSP-1", 5) {
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// `c-RSM-2`: four footmen ordered twice. They block each other, wait,
/// count their patience and search round each other, and move bit for bit
/// as in the game at every update of both orders.
#[test]
fn group_of_four_follows_the_game() {
    if let Some(found) = mismatches("c-RSM-2", 5) {
        assert!(found.is_empty(), "{found:#?}");
    }
}
