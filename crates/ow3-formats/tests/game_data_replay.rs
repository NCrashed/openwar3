//! Parses the replays saved by the game in `game-data/1.27b/replays`.
//! Passes without checking anything when there are none.

use std::path::PathBuf;

use ow3_formats::replay::{Event, Replay};

#[test]
fn every_replay_parses() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/replays");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipping: {} not found", dir.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let data = std::fs::read(&path).unwrap();
        // The game leaves an empty TempReplay.w3g behind.
        if data.is_empty() {
            continue;
        }
        let replay = Replay::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        eprintln!(
            "{}: {:?}, {} players, map {:?}, seed {:#x}, {} events",
            path.display(),
            replay.header,
            replay.players.len(),
            replay.settings.map_path,
            replay.seed,
            replay.events.len()
        );
        let slots = replay
            .events
            .iter()
            .filter(|e| matches!(e, Event::TimeSlot { .. }))
            .count();
        let checksums = replay
            .events
            .iter()
            .filter(|e| matches!(e, Event::Checksum(_)))
            .count();
        // In 1.27b replays a checksum follows every time slot of the game.
        assert!(slots > 0, "{}", path.display());
        assert!(
            checksums.abs_diff(slots) <= 2,
            "{checksums} checksums, {slots} slots"
        );
        assert!(
            replay
                .settings
                .map_path
                .to_ascii_lowercase()
                .ends_with(".w3m")
                || replay
                    .settings
                    .map_path
                    .to_ascii_lowercase()
                    .ends_with(".w3x")
        );
    }
}
