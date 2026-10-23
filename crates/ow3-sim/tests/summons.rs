//! The summon events of a replay (SPEC-0044): the summons the map's script
//! logs while the replay plays are those of the game's log.

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;
use std::path::PathBuf;

/// `c-RZJ-1`: the two Water Elementals, with their type, summoner and
/// place (the `s` rows of the log, `@@s` lines of the script).
#[test]
fn water_elementals_fire_summon_events() {
    let gd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let (Ok(bytes), Ok(text)) = (
        std::fs::read(gd.join("replays/c-RZJ-1.w3g")),
        std::fs::read_to_string(gd.join("replays/c-RZJ-1.log.tsv")),
    ) else {
        return;
    };
    let replay = Replay::parse(&bytes).unwrap();
    let map = Map::load(&gd, &gd.join(replay.settings.map_path.replace('\\', "/"))).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, &replay).unwrap();
    while playback.game.ticks() < 5000 && playback.step_tick() {}
    // unit, summoner, x, y
    let game: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|p| p.len() > 7 && p[1] == "s")
        .map(|p| format!("{} {} {} {}", p[2], p[5], p[6], p[7]))
        .collect();
    let ours: Vec<String> = playback
        .game
        .preload()
        .iter()
        .filter_map(|l| l.strip_prefix("@@s "))
        .map(|l| {
            let p: Vec<&str> = l.split(' ').collect();
            format!("{} {} {} {}", p[1], p[3], p[4], p[5])
        })
        .collect();
    assert_eq!(ours, game);
}
