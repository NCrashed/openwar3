//! Conversion of real time into game ticks and sleep checks (SPEC-0002).
//!
//! Real time is consumed in chunks of 25 ms. Each chunk runs 3, 4 or 5 game
//! ticks of 0.005 s, depending on the game speed, and none while the game
//! is paused. The sleep clock advances by the chunk's 25 ms in every case,
//! and every 0.1 s of it there is a sleep check. Real time that has not
//! been converted yet is capped at 4 s; anything beyond is dropped.
//!
//! The embedder supplies real time, so a run is deterministic for a given
//! sequence of inputs. Feeding exactly 25 ms per chunk gives the behaviour
//! of an unloaded single-player game, which SPEC-0002 recommends.

use crate::sleep::{CHECK_INTERVAL, UNITS_PER_SECOND};

/// Real time per chunk, in microseconds.
pub const CHUNK_MICROS: u64 = 25_000;
/// Largest backlog of real time not yet converted, in microseconds.
pub const MAX_BACKLOG_MICROS: u64 = 4_000_000;
/// Sleep clock units per chunk.
const CHUNK_UNITS: u64 = UNITS_PER_SECOND * CHUNK_MICROS / 1_000_000;

/// Game speed index as used by `SetGameSpeed` (SPEC-0002). Values above 2
/// are treated as 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    /// Index 0, lobby "Slow": 3 ticks per chunk, 0.6 game s per real s.
    Slowest,
    /// Index 1, lobby "Normal": 4 ticks per chunk, 0.8 game s per real s.
    Slow,
    /// Index 2, lobby "Fast", the default: 5 ticks per chunk.
    Normal,
}

impl Speed {
    pub fn from_index(index: i32) -> Speed {
        match index {
            i32::MIN..=0 => Speed::Slowest,
            1 => Speed::Slow,
            _ => Speed::Normal,
        }
    }

    pub fn index(self) -> i32 {
        self as i32
    }

    pub fn ticks_per_chunk(self) -> u64 {
        match self {
            Speed::Slowest => 3,
            Speed::Slow => 4,
            Speed::Normal => 5,
        }
    }
}

/// What happens next in the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Run one game tick. `sleep_time` is the sleep clock at the end of the
    /// tick, for sleeps that start during it.
    Tick { sleep_time: u64 },
    /// Resume the threads due at this sleep clock time.
    SleepCheck { time: u64 },
}

#[derive(Clone, Debug)]
pub struct FrameClock {
    pub speed: Speed,
    pub paused: bool,
    backlog_micros: u64,
    /// Sleep clock at the start of the next chunk.
    sleep_time: u64,
}

impl Default for FrameClock {
    fn default() -> Self {
        FrameClock {
            speed: Speed::Normal,
            paused: false,
            backlog_micros: 0,
            sleep_time: 0,
        }
    }
}

impl FrameClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// The sleep clock at the start of the next chunk.
    pub fn sleep_time(&self) -> u64 {
        self.sleep_time
    }

    /// Adds elapsed real time. Time beyond the 4 s backlog is dropped.
    pub fn add_real_time(&mut self, micros: u64) {
        self.backlog_micros = (self.backlog_micros + micros).min(MAX_BACKLOG_MICROS);
    }

    /// Takes the next whole chunk from the backlog and returns its steps:
    /// the chunk's ticks, then a sleep check if the chunk ends on one.
    /// Speed and pause are read when the chunk starts. Returns `None` when
    /// less than a chunk of real time is left.
    pub fn next_chunk(&mut self) -> Option<Vec<Step>> {
        if self.backlog_micros < CHUNK_MICROS {
            return None;
        }
        self.backlog_micros -= CHUNK_MICROS;
        let ticks = if self.paused {
            0
        } else {
            self.speed.ticks_per_chunk()
        };
        let start = self.sleep_time;
        let mut steps: Vec<Step> = (1..=ticks)
            .map(|i| Step::Tick {
                sleep_time: start + i * CHUNK_UNITS / ticks,
            })
            .collect();
        self.sleep_time += CHUNK_UNITS;
        if self.sleep_time % CHECK_INTERVAL == 0 {
            steps.push(Step::SleepCheck {
                time: self.sleep_time,
            });
        }
        Some(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sleep::SleepQueue;
    use crate::timers::TimerQueue;

    /// Runs `chunks` chunks of 25 ms and returns the steps.
    fn run(clock: &mut FrameClock, chunks: u64) -> Vec<Step> {
        clock.add_real_time(chunks * CHUNK_MICROS);
        let mut steps = Vec::new();
        while let Some(chunk) = clock.next_chunk() {
            steps.extend(chunk);
        }
        steps
    }

    fn ticks(steps: &[Step]) -> usize {
        steps
            .iter()
            .filter(|s| matches!(s, Step::Tick { .. }))
            .count()
    }

    #[test]
    fn ticks_per_real_second_follow_the_speed() {
        for (speed, per_second) in [
            (Speed::Normal, 200),
            (Speed::Slow, 160),
            (Speed::Slowest, 120),
        ] {
            let mut clock = FrameClock::new();
            clock.speed = speed;
            let steps = run(&mut clock, 40);
            assert_eq!(ticks(&steps), per_second, "{speed:?}");
            let checks = steps.len() - ticks(&steps);
            assert_eq!(checks, 10, "a check every 0.1 s of real time");
        }
    }

    #[test]
    fn at_full_speed_checks_fall_on_tenths_of_game_time() {
        let mut clock = FrameClock::new();
        let steps = run(&mut clock, 8);
        let mut ticks_so_far = 0;
        for step in steps {
            match step {
                Step::Tick { .. } => ticks_so_far += 1,
                Step::SleepCheck { time } => {
                    assert_eq!(ticks_so_far * 5, time / (UNITS_PER_SECOND / 1000), "ms");
                }
            }
        }
    }

    #[test]
    fn pause_stops_ticks_but_not_sleep_checks() {
        let mut clock = FrameClock::new();
        clock.paused = true;
        let steps = run(&mut clock, 8);
        assert_eq!(ticks(&steps), 0);
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn backlog_is_capped_at_four_seconds() {
        let mut clock = FrameClock::new();
        clock.add_real_time(10_000_000);
        let mut chunks = 0;
        while clock.next_chunk().is_some() {
            chunks += 1;
        }
        assert_eq!(chunks, 160);
        assert_eq!(Speed::from_index(7), Speed::Normal);
        assert_eq!(Speed::from_index(-1).index(), 0);
    }

    /// A one-second sleep started at a check, measured in game time at each
    /// speed: the sleep clock is not scaled by the speed (SPEC-0002), so the
    /// sleep lasts 1.1 s of sleep clock and 1.1 times the speed factor in
    /// game time.
    #[test]
    fn sleep_length_in_game_time_scales_with_speed() {
        for (speed, game_seconds) in [
            (Speed::Normal, 1.1),
            (Speed::Slow, 0.88),
            (Speed::Slowest, 0.66),
        ] {
            let mut clock = FrameClock::new();
            clock.speed = speed;
            let mut sleeps = SleepQueue::new();
            let mut timers: TimerQueue<()> = TimerQueue::new();
            let (mut game_ticks, mut started_at, mut ended_at) = (0u64, None, None);
            clock.add_real_time(4 * CHUNK_MICROS * 30);
            while let Some(chunk) = clock.next_chunk() {
                for step in chunk {
                    match step {
                        Step::Tick { .. } => {
                            timers.tick(|_, _| {});
                            game_ticks += 1;
                        }
                        Step::SleepCheck { time } => {
                            if !sleeps.due(time).is_empty() {
                                ended_at = Some(game_ticks);
                            }
                            if started_at.is_none() && time >= 5 * CHECK_INTERVAL {
                                sleeps.sleep(time, 1.0f32.to_bits(), ());
                                started_at = Some(game_ticks);
                            }
                        }
                    }
                }
            }
            let duration = (ended_at.unwrap() - started_at.unwrap()) as f64 * 0.005;
            assert!(
                (duration - game_seconds).abs() < 1e-9,
                "{speed:?}: {duration}"
            );
        }
    }
}
