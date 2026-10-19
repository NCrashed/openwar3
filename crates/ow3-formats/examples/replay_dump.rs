//! Lists the time slots of a replay that carry actions.
//!
//! `cargo run -p ow3-formats --example replay_dump -- <replay.w3g>`

use ow3_formats::replay::{Event, Replay};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: replay_dump <replay.w3g>");
    let replay = Replay::parse(&std::fs::read(&path).unwrap()).unwrap();
    let (mut turn, mut ms) = (0, 0u64);
    for event in &replay.events {
        if let Event::TimeSlot {
            ms: step, actions, ..
        } = event
        {
            turn += 1;
            for (player, list) in actions {
                for action in list {
                    println!("turn {turn} ({ms} ms, slot {step} ms) player {player}: {action:?}");
                }
            }
            ms += u64::from(*step);
        }
    }
}
