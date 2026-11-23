//! Probes for matching a replay against the game: each test does nothing
//! unless its environment variable is set, and prints to stderr. Run one
//! with `--nocapture`, for example:
//!
//! ```sh
//! OW3_TABLE=9790..9830 cargo test -p ow3-sim --release --test probe table -- --nocapture
//! ```
//!
//! - `OW3_REPLAY`: the replay (default `c-SEA-1`), for all probes.
//! - `table`: `OW3_TABLE=from..to`, the object table's takes and frees
//!   (`TB tick new entry creation tag`) and the clock of each tick (`CLK`)
//!   in those ticks, with the object table of the melee AI tests.
//! - `units`: `OW3_UNITS=tick`, every unit at that tick (`U handle owner
//!   type x y facing order`); with `OW3_TRACK=handle`, also that unit at
//!   every tick from `OW3_FROM` (`T tick x y order`).
//! - `parts`: `OW3_PARTS=game,b,n,c,d,e,r,g` (hex), the game's turn
//!   checksum and our parts: the value each part alone would need to give
//!   the game's checksum, and the C and D pairs near ours that would.
//! - `info`: `OW3_INFO=k`, the replay's map, its number of turns and the
//!   recorded time of turn `k`.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::checksum::{ICECROWN_TEST, Parts, turn_checksum, turns};
use ow3_sim::game::table::ObjectTable;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

fn var<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// The replay named by `OW3_REPLAY`, with its map's program run by `f`.
fn with_replay(f: impl FnOnce(&Replay, &Map, &ow3_jass::check::Program)) {
    let name = std::env::var("OW3_REPLAY").unwrap_or_else(|_| "c-SEA-1".into());
    let Ok(data) = std::fs::read(game_data().join(format!("replays/{name}.w3g"))) else {
        eprintln!("no replay {name}");
        return;
    };
    let replay = Replay::parse(&data).unwrap();
    let map_path = game_data().join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&game_data(), &map_path).unwrap();
    let program = map.check().unwrap();
    f(&replay, &map, &program);
}

#[test]
fn table() {
    let Some(range) = std::env::var("OW3_TABLE").ok() else {
        return;
    };
    let (from, to): (u64, u64) = {
        let mut it = range.split("..").map(|v| v.parse().unwrap());
        (it.next().unwrap(), it.next().unwrap())
    };
    with_replay(|replay, map, program| {
        let table = ObjectTable::new(640, Vec::new(), 640);
        let mut playback = Playback::with_table(program, map, replay, Some(table)).unwrap();
        while playback.game.ticks() <= to && playback.step_tick() {}
        for (t, c) in playback.game.table_clocks() {
            if (from..=to).contains(&t) {
                eprintln!("CLK {t} {:.9}", f32::from_bits(c));
            }
        }
        for e in playback.game.table_log() {
            if (from..=to).contains(&e.tick) {
                eprintln!(
                    "TB {} {} {} {} {}",
                    e.tick, e.new, e.entry, e.creation, e.tag
                );
            }
        }
    });
}

#[test]
fn units() {
    let Some(until) = var::<u64>("OW3_UNITS") else {
        return;
    };
    let track = var::<u32>("OW3_TRACK");
    let from = var::<u64>("OW3_FROM").unwrap_or(0);
    with_replay(|replay, map, program| {
        let mut playback = Playback::new(program, map, replay).unwrap();
        while playback.game.ticks() < until && playback.step_tick() {
            let tick = playback.game.ticks();
            let Some(h) = track.filter(|_| tick >= from) else {
                continue;
            };
            for u in playback.game.units().iter().filter(|u| u.handle == h) {
                eprintln!("T {tick} {:.3} {:.3} order {}", u.x, u.y, u.order);
            }
        }
        for u in playback.game.units() {
            if u.owner <= 3 || u.alive {
                eprintln!(
                    "U {} {} {} {:.3} {:.3} {:.2} order {}",
                    u.handle,
                    u.owner,
                    String::from_utf8_lossy(&u.type_id.to_be_bytes()),
                    u.x,
                    u.y,
                    u.facing.to_degrees(),
                    u.order
                );
            }
        }
    });
}

#[test]
fn parts() {
    let Ok(v) = std::env::var("OW3_PARTS") else {
        return;
    };
    let w: Vec<u32> = v
        .split(',')
        .map(|s| u32::from_str_radix(s.trim(), 16).unwrap())
        .collect();
    let (game, p) = (
        w[0],
        Parts {
            b: w[1],
            n: w[2],
            c: w[3],
            d: w[4],
            e: w[5],
            r: w[6],
            g: w[7],
        },
    );
    let base = turn_checksum(&ICECROWN_TEST, &p);
    // The checksum is linear over xor in each part: the delta of a part
    // that gives the game's checksum, read bit by bit.
    let solve = |f: &dyn Fn(u32) -> Parts| {
        let target = game ^ base;
        let mut delta = 0u32;
        for bit in 0..32 {
            let column = turn_checksum(&ICECROWN_TEST, &f(1 << bit)) ^ base;
            if column.count_ones() == 1 && target & column != 0 {
                delta |= 1 << bit;
            }
        }
        delta
    };
    let parts: [(&str, u32, &dyn Fn(u32) -> Parts); 5] = [
        ("C", p.c, &|x| Parts { c: p.c ^ x, ..p }),
        ("D", p.d, &|x| Parts { d: p.d ^ x, ..p }),
        ("E", p.e, &|x| Parts { e: p.e ^ x, ..p }),
        ("R", p.r, &|x| Parts { r: p.r ^ x, ..p }),
        ("G", p.g, &|x| Parts { g: p.g ^ x, ..p }),
    ];
    for (name, ours, f) in parts {
        let delta = solve(f);
        eprintln!(
            "ONLY {name}: {:08x} (ours {ours:08x}, xor {delta:08x})",
            ours ^ delta
        );
    }
    // C and D together, D within 256 below and above ours.
    for c in 0u32..0x10000 {
        for d in p.d.saturating_sub(0x100)..p.d + 0x100 {
            if turn_checksum(&ICECROWN_TEST, &Parts { c, d, ..p }) == game {
                eprintln!("C {c:x} D {d:x}");
            }
        }
    }
}

#[test]
fn info() {
    let Some(k) = var::<usize>("OW3_INFO") else {
        return;
    };
    with_replay(|replay, _, _| {
        let turns = turns(replay);
        eprintln!(
            "map {} turns {} last {:?} turn {k} after {:?}",
            replay.settings.map_path,
            turns.len(),
            turns.last().map(|t| t.after),
            turns.get(k.saturating_sub(1)).map(|t| t.after)
        );
    });
}
