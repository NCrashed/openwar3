//! A group order played from the replay `c-RSP-1.w3g` against the state
//! of its members read from the game at every cluster update
//! (`c-RSP-1.members.tsv`, SPEC-0025). Passes without checking anything
//! when the replay is not there.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

/// The first cluster update of the replay (its README).
const FIRST_UPDATE: u64 = 1051;

/// A member at a cluster update: its target (world), committed position
/// (relative to the world corner) and velocity, as bits.
type Row = ([u32; 2], [u32; 2], [u32; 2]);

/// The rows of the members file by update, each with the member's index
/// among the units of the order.
fn updates(text: &str) -> Vec<Vec<(usize, Row)>> {
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
        .collect();
    let agents: Vec<&str> = rows[..4].iter().map(|r| r[8]).collect();
    let world = |x: u32| (f32::from_bits(x) * 32.0).to_bits();
    let mut updates: Vec<Vec<(usize, Row)>> = Vec::new();
    let mut clock = "";
    for r in &rows {
        if r[0] != clock {
            clock = r[0];
            updates.push(Vec::new());
        }
        let member = agents.iter().position(|a| *a == r[8]).unwrap();
        let target = [hex(r[13]), hex(r[14])];
        let row = (
            target.map(|x| {
                if x == 0 {
                    0
                } else {
                    (f32::from_bits(x) * 32.0 - 12288.0).to_bits()
                }
            }),
            [hex(r[15]), hex(r[16])].map(world),
            [hex(r[17]), hex(r[18])].map(world),
        );
        updates.last_mut().unwrap().push((member, row));
    }
    updates
}

/// The members walk to the places of the game at every update, start at
/// the same updates (the knight and the footman at the first, the
/// rifleman held to the 19th, the peasant to the 35th), and the knight and
/// the rifleman move bit for bit as in the game throughout.
///
/// PROVISIONAL: the peasant and the footman take another first step: the
/// game found their first local paths free of obstacles, the simulation
/// does not (it sees the members standing next to them, SPEC-0021), so
/// their positions are not compared.
#[test]
fn group_members_follow_the_game() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let path = dir.join("replays/c-RSP-1.w3g");
    let Ok(text) = std::fs::read_to_string(path.with_extension("members.tsv")) else {
        return;
    };
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let map_path = dir.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(&dir, &map_path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    playback.load_object_ids(&path);
    while playback.game.ticks() < FIRST_UPDATE - 1 {
        playback.step_tick();
    }
    let units: Vec<u32> = playback.game.units().iter().map(|u| u.handle).collect();
    let mut started = [None; 4];
    let mut game_started = [None; 4];
    for (k, update) in updates(&text).iter().enumerate() {
        while playback.game.ticks() < FIRST_UPDATE + 6 * k as u64 - 1 {
            playback.step_tick();
        }
        for &(member, (target, position, velocity)) in update {
            let (o, v, t) = playback.game.motion_of(units[member]).unwrap_or_default();
            if k > 0 {
                assert_eq!(t, target, "update {k} member {member}: target");
            }
            if v != [0, 0] && started[member].is_none() {
                started[member] = Some(k);
            }
            if velocity != [0, 0] && game_started[member].is_none() {
                game_started[member] = Some(k);
            }
            if member == 1 || member == 2 {
                assert_eq!((o, v), (position, velocity), "update {k} member {member}");
            }
        }
    }
    assert_eq!(started, game_started);
    assert_eq!(game_started, [Some(35), Some(19), Some(1), Some(1)]);
}
