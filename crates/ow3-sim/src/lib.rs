//! Deterministic game simulation.
//!
//! Time follows SPEC-0002: game time advances in ticks of 0.005 s, timers
//! live in a queue with its own time, and sleeping script threads are timed
//! on a separate sleep clock.

pub mod clock;
pub mod data;
pub mod game;
pub mod map;
pub mod sleep;
pub mod timers;
