//! Plays the replays saved by the game in `game-data/1.27b/replays` to
//! their end, twice, and checks that both runs end in the same state.
//! Passes without checking anything when there are none.

use std::path::PathBuf;

use ow3_formats::replay::Replay;
use ow3_sim::map::Map;
use ow3_sim::replay::Playback;

/// A unit at the end of a run: handle, position, facing and life bits.
type End = (u32, u32, u32, u32, u32);

/// The ticks of a run and its units at the end.
fn play(game_data: &std::path::Path, replay: &Replay) -> (u64, Vec<End>) {
    let path = game_data.join(replay.settings.map_path.replace('\\', "/"));
    let map = Map::load(game_data, &path).unwrap();
    let program = map.check().unwrap();
    let mut playback = Playback::new(&program, &map, replay).unwrap();
    playback.run();
    let units = playback
        .game
        .units()
        .iter()
        .map(|u| {
            (
                u.handle,
                u.x.to_bits(),
                u.y.to_bits(),
                u.facing.to_bits(),
                u.life.to_bits(),
            )
        })
        .collect();
    (playback.game.ticks(), units)
}

#[test]
fn replays_play_the_same_twice() {
    let game_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let Ok(entries) = std::fs::read_dir(game_data.join("replays")) else {
        return;
    };
    for entry in entries.flatten() {
        let data = std::fs::read(entry.path()).unwrap();
        if data.is_empty() {
            continue;
        }
        let replay = Replay::parse(&data).unwrap();
        let first = play(&game_data, &replay);
        // The slots of about 100 ms add up to the replay's game time.
        let slots: u64 = replay
            .events
            .iter()
            .map(|e| match e {
                ow3_formats::replay::Event::TimeSlot { ms, .. } => u64::from(*ms),
                _ => 0,
            })
            .sum();
        assert_eq!(first.0, slots / 5, "{}", entry.path().display());
        assert!(!first.1.is_empty());
        assert_eq!(
            play(&game_data, &replay),
            first,
            "{}",
            entry.path().display()
        );
    }
}
