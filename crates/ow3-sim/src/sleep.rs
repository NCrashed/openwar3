//! Sleeping script threads and the sleep clock (SPEC-0002).
//!
//! `TriggerSleepAction` is timed on a sleep clock that follows real time,
//! not game time. Sleeping threads resume only at sleep checks, every 0.1 s
//! of that clock. A thread that starts sleeping at `t` for `d` seconds
//! resumes at the first check `T` with `T - 0.05 > t + d`.
//!
//! Sleep clock times are counted in units of 1/60 ms, so that tick lengths
//! at every game speed (5, 6.25 and 8.33 ms of sleep clock) are whole
//! numbers of units.

use std::collections::BTreeMap;

/// Sleep clock units per second.
pub const UNITS_PER_SECOND: u64 = 60_000;
/// Interval between sleep checks, 0.1 s.
pub const CHECK_INTERVAL: u64 = UNITS_PER_SECOND / 10;
/// The half interval in the resume rule, 0.05 s.
const HALF_CHECK: u64 = CHECK_INTERVAL / 2;

/// Threads waiting for a sleep check, in the order they resume.
#[derive(Clone, Debug)]
pub struct SleepQueue<T> {
    next_sleep: u64,
    sleepers: BTreeMap<(u64, u64), T>,
}

impl<T> Default for SleepQueue<T> {
    fn default() -> Self {
        SleepQueue {
            next_sleep: 0,
            sleepers: BTreeMap::new(),
        }
    }
}

/// The sleep check at which a sleep of `duration` seconds (a real's bit
/// pattern) started at sleep clock time `start` ends: the first check `T`
/// after `start` with `T - 0.05 > start + duration`.
pub fn resume_check(start: u64, duration: u32) -> u64 {
    let duration = f64::from(f32::from_bits(duration));
    let duration = if duration.is_nan() { 0.0 } else { duration };
    // Exact: a float times 60000 fits in a double's 53-bit mantissa.
    let threshold = start as f64 + HALF_CHECK as f64 + duration * UNITS_PER_SECOND as f64;
    let next_after_start = (start / CHECK_INTERVAL + 1) * CHECK_INTERVAL;
    if threshold < next_after_start as f64 {
        return next_after_start;
    }
    // The smallest multiple of the interval strictly above the threshold.
    let checks = (threshold / CHECK_INTERVAL as f64).floor() as u64 + 1;
    checks * CHECK_INTERVAL
}

impl<T> SleepQueue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.sleepers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sleepers.is_empty()
    }

    /// Puts a thread to sleep and returns the check at which it resumes.
    pub fn sleep(&mut self, start: u64, duration: u32, thread: T) -> u64 {
        let check = resume_check(start, duration);
        self.sleepers.insert((check, self.next_sleep), thread);
        self.next_sleep += 1;
        check
    }

    /// Removes the threads due at the check at `time`, in the order they
    /// went to sleep.
    pub fn due(&mut self, time: u64) -> Vec<T> {
        let later = self.sleepers.split_off(&(time + 1, 0));
        std::mem::replace(&mut self.sleepers, later)
            .into_values()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks of 5 ms at full speed, in sleep clock units.
    const TICK_UNITS: u64 = UNITS_PER_SECOND / 200;

    /// Test T2c of SPEC-0002: sleeps started at a check, three in a row for
    /// each duration, last 20, 40, 60 or 80 ticks.
    #[test]
    fn spec_0002_sleeps_started_at_a_check() {
        let mut time = 10 * CHECK_INTERVAL; // game time 1.0
        for (d, ticks) in [
            (0.0, 20),
            (0.001, 20),
            (0.01, 20),
            (0.05, 40),
            (0.0999, 40),
            (0.1, 40),
            (0.1001, 40),
            (0.15, 60),
            (0.2, 60),
            (0.25, 80),
        ] {
            for _ in 0..3 {
                let end = resume_check(time, f32::to_bits(d));
                assert_eq!((end - time) / TICK_UNITS, ticks, "d = {d}");
                assert_eq!(end % CHECK_INTERVAL, 0);
                time = end;
            }
        }
    }

    /// `round(10 * d) + 1` checks for a start at a check (SPEC-0002).
    #[test]
    fn checks_waited_from_a_check() {
        for (d, checks) in [(0.0499, 1), (0.05, 2), (0.1499, 2), (0.25, 4), (1.0, 11)] {
            let start = 7 * CHECK_INTERVAL;
            let end = resume_check(start, f32::to_bits(d));
            assert_eq!((end - start) / CHECK_INTERVAL, checks, "d = {d}");
        }
    }

    #[test]
    fn sleeps_between_checks_and_negative_durations() {
        // Started 0.07 s after a check: 0 s waits for the check after next
        // only if more than 0.05 s must pass (SPEC-0002 rule).
        let check = 3 * CHECK_INTERVAL;
        let start = check + 7 * UNITS_PER_SECOND / 100;
        assert_eq!(resume_check(start, 0), check + 2 * CHECK_INTERVAL);
        let start = check + 3 * UNITS_PER_SECOND / 100;
        assert_eq!(resume_check(start, 0), check + CHECK_INTERVAL);
        // A negative duration still waits for the next check.
        assert_eq!(
            resume_check(check, (-5.0f32).to_bits()),
            check + CHECK_INTERVAL
        );
    }

    #[test]
    fn due_threads_resume_in_sleep_order() {
        let mut queue = SleepQueue::new();
        let a = queue.sleep(0, 0.2f32.to_bits(), "a");
        let b = queue.sleep(0, 0.15f32.to_bits(), "b");
        queue.sleep(0, 1.0f32.to_bits(), "c");
        assert_eq!(a, b);
        assert!(queue.due(a - CHECK_INTERVAL).is_empty());
        assert_eq!(queue.due(a), ["a", "b"]);
        assert_eq!(queue.len(), 1);
    }
}
