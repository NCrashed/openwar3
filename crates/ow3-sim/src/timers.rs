//! The timer queue of SPEC-0002.
//!
//! All arithmetic on timer times uses the engine operations of SPEC-0004;
//! comparisons are ordinary float comparisons. Times are bit patterns.

use std::collections::{BTreeMap, HashMap};

use ow3_math::{add, sub};

/// Length of a simulation tick, 0.005 s (SPEC-0002).
pub const TICK: u32 = 0x3BA3_D70A;
/// 300.0: timer times never exceed it.
const WRAP: u32 = 0x4396_0000;
/// Smallest timeout, the float nearest to 0.0001.
pub(crate) const MIN_TIMEOUT: u32 = 0x38D1_B717;
/// The float nearest to 0.001, subtracted from 300 for the largest timeout.
const MAX_TIMEOUT_MARGIN: u32 = 0x3A83_126F;
/// After the wrap, a queue time smaller than this in magnitude becomes 0.
const WRAP_EPSILON: u32 = 0x3556_BF95;

fn real(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// The timeout a timer is queued with: at least 0.0001 and at most just
/// under 300 s (SPEC-0002).
pub fn clamp_timeout(timeout: u32) -> u32 {
    if real(timeout) >= real(WRAP) {
        sub(WRAP, MAX_TIMEOUT_MARGIN)
    } else if real(timeout) < real(MIN_TIMEOUT) || real(timeout).is_nan() {
        MIN_TIMEOUT
    } else {
        timeout
    }
}

/// 120.0, the length of the parts a long timer is split into.
pub const LONG_PART: u32 = 0x42F0_0000;

/// The timeout `TimerStart` works with: at least 0.0001 (SPEC-0002).
pub fn raise_timeout(timeout: u32) -> u32 {
    if real(timeout) < real(MIN_TIMEOUT) || real(timeout).is_nan() {
        MIN_TIMEOUT
    } else {
        timeout
    }
}

/// Splits a timeout (already raised) into `n = trunc(timeout) / 120` parts
/// of 120 s and a last part `R = fmod(timeout, 120)`, at least 0.0001
/// (SPEC-0002, SPEC-0004). With `n = 0` the timer is queued whole.
pub fn split_timeout(timeout: u32) -> (u32, u32) {
    let n = (ow3_math::r2i(timeout).max(0) / 120) as u32;
    if n == 0 {
        return (0, timeout);
    }
    (n, raise_timeout(ow3_math::fmod(timeout, LONG_PART)))
}

/// Identifies one start of a timer. A timer started again gets a new id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(pub u64);

/// A timer that has expired, removed from the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fired<T> {
    pub id: TimerId,
    /// The expiry, which is also the queue time while it is handled.
    pub expiry: u32,
    pub payload: T,
}

/// A tick in progress: timers expiring up to `limit` fire, and when the
/// queue wraps at 300 s, the rest of the tick up to `after_wrap`.
#[derive(Debug)]
pub struct Tick {
    limit: u32,
    after_wrap: Option<u32>,
}

/// Pending timers ordered by expiry, then by start (SPEC-0002).
///
/// Expiries are positive, so ordering their bit patterns as unsigned
/// integers orders them as numbers.
#[derive(Clone, Debug)]
pub struct TimerQueue<T> {
    time: u32,
    next_start: u64,
    pending: BTreeMap<(u32, u64), T>,
    expiries: HashMap<TimerId, u32>,
}

impl<T> Default for TimerQueue<T> {
    fn default() -> Self {
        TimerQueue {
            time: 0,
            next_start: 0,
            pending: BTreeMap::new(),
            expiries: HashMap::new(),
        }
    }
}

impl<T> TimerQueue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The queue time `q`. While a timer is handled it is that timer's
    /// expiry.
    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Starts a timer that expires `timeout` seconds after the queue time.
    /// The timeout is raised to 0.0001 and limited to just under 300 s.
    /// A periodic timer is started again by its owner when it fires; the
    /// queue time is then its expiry, so periods do not drift.
    pub fn start(&mut self, timeout: u32, payload: T) -> TimerId {
        self.start_from(self.time, timeout, payload)
    }

    /// Starts a timer as [`start`](Self::start) does, as if the queue time
    /// were `time`: for what happened at that time and is handled later.
    /// An expiry before the queue time fires first in the next tick.
    pub fn start_from(&mut self, time: u32, timeout: u32, payload: T) -> TimerId {
        let expiry = add(time, clamp_timeout(timeout));
        let id = TimerId(self.next_start);
        self.next_start += 1;
        self.pending.insert((expiry, id.0), payload);
        self.expiries.insert(id, expiry);
        id
    }

    /// Starts a timer as [`start`](Self::start) does, under the start
    /// number `id` of an earlier start: a periodic timer started again keeps
    /// the number of its first start (SPEC-0002, "Order").
    pub fn start_as(&mut self, timeout: u32, payload: T, id: TimerId) -> TimerId {
        let expiry = add(self.time, clamp_timeout(timeout));
        self.pending.insert((expiry, id.0), payload);
        self.expiries.insert(id, expiry);
        id
    }

    /// Removes a pending timer and returns its payload.
    pub fn cancel(&mut self, id: TimerId) -> Option<T> {
        let expiry = self.expiries.remove(&id)?;
        self.pending.remove(&(expiry, id.0))
    }

    /// Expiry of a pending timer.
    pub fn expiry(&self, id: TimerId) -> Option<u32> {
        self.expiries.get(&id).copied()
    }

    /// Advances the queue by one tick and calls `on_fire` for every timer
    /// that expires in it, in order. `on_fire` may start and cancel timers;
    /// one that expires within the same tick fires in it too.
    pub fn tick(&mut self, mut on_fire: impl FnMut(&mut Self, Fired<T>)) {
        let mut tick = self.begin_tick();
        while let Some(fired) = self.next_due(&mut tick) {
            on_fire(self, fired);
        }
        self.end_tick(tick);
    }

    /// Starts a tick. Take the timers that expire in it with
    /// [`TimerQueue::next_due`] until it returns `None`, then call
    /// [`TimerQueue::end_tick`]. The queue may be changed in between.
    pub fn begin_tick(&mut self) -> Tick {
        let until = add(self.time, TICK);
        if real(until) >= real(WRAP) {
            Tick {
                limit: WRAP,
                after_wrap: Some(until),
            }
        } else {
            Tick {
                limit: until,
                after_wrap: None,
            }
        }
    }

    /// The next timer that expires within the tick, removed from the queue,
    /// with the queue time set to its expiry.
    pub fn next_due(&mut self, tick: &mut Tick) -> Option<Fired<T>> {
        loop {
            if let Some(entry) = self.pending.first_entry() {
                let (expiry, start) = *entry.key();
                if real(expiry) <= real(tick.limit) {
                    let payload = entry.remove();
                    let id = TimerId(start);
                    self.expiries.remove(&id);
                    self.time = expiry;
                    return Some(Fired {
                        id,
                        expiry,
                        payload,
                    });
                }
            }
            // Timers up to 300 s have fired: shift everything back and go
            // on with the rest of the tick.
            let until = tick.after_wrap.take()?;
            self.time = WRAP;
            self.shift_back();
            let until = sub(until, WRAP);
            tick.limit = if real(until).abs() < real(WRAP_EPSILON) {
                0
            } else {
                until
            };
        }
    }

    /// The next timer that expires within the tick and before `before`
    /// (strictly), as [`TimerQueue::next_due`] takes it. None in a tick in
    /// which the queue wraps at 300 s.
    pub fn next_due_before(&mut self, tick: &mut Tick, before: u32) -> Option<Fired<T>> {
        if tick.after_wrap.is_some() {
            return None;
        }
        let (expiry, _) = *self.pending.first_key_value()?.0;
        if real(expiry) < real(before) {
            self.next_due(tick)
        } else {
            None
        }
    }

    /// Moves the queue time on to `time` within a tick, for what happens
    /// at that time between the timers (the unit updates, SPEC-0018).
    pub fn move_to(&mut self, time: u32) {
        if real(time) > real(self.time) {
            self.time = time;
        }
    }

    pub fn end_tick(&mut self, tick: Tick) {
        debug_assert!(tick.after_wrap.is_none(), "next_due was not drained");
        self.time = tick.limit;
    }

    /// Moves the queue and every pending expiry back by 300 s.
    fn shift_back(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for ((expiry, start), payload) in pending {
            let shifted = sub(expiry, WRAP);
            self.pending.insert((shifted, start), payload);
            self.expiries.insert(TimerId(start), shifted);
        }
        self.time = sub(self.time, WRAP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(value: f32) -> u32 {
        value.to_bits()
    }

    /// The spec's times were read from a 1000 s reference timer, whose
    /// resolution near 1000 is one unit in the last place, 2^-14 s.
    const CLOCK_RESOLUTION: f32 = 1.0 / 16384.0;

    fn assert_measured(got: f32, measured: f32) {
        assert!(
            (got - measured).abs() <= CLOCK_RESOLUTION,
            "{got} is not {measured} within the reference clock's resolution"
        );
    }

    /// Test T3c of SPEC-0002: firing order and times. The timers are
    /// started from a timer callback at 1.0, as in the test map.
    #[test]
    fn spec_0002_timer_order() {
        let mut queue: TimerQueue<&str> = TimerQueue::new();
        queue.start(bits(1.0), "start");
        let mut log = Vec::new();
        while real(queue.time()) < 1.6 {
            queue.tick(|q, fired| {
                if fired.payload == "start" {
                    for (timeout, label) in [
                        (0.5, "2"),
                        (0.5, "0"),
                        (0.5, "1"),
                        (0.501, "0.501"),
                        (0.499, "0.499"),
                        (0.0, "0.0"),
                    ] {
                        q.start(bits(timeout), label);
                    }
                } else {
                    log.push((fired.payload, real(fired.expiry)));
                }
            });
        }
        let order: Vec<&str> = log.iter().map(|(label, _)| *label).collect();
        assert_eq!(order, ["0.0", "0.499", "2", "0", "1", "0.501"]);
        for ((_, got), measured) in log.iter().zip([1.0001, 1.499, 1.5, 1.5, 1.5, 1.501]) {
            assert_measured(*got, measured);
        }
    }

    /// A periodic timer started again keeps the number of its first start:
    /// with the same expiry it fires before a timer started after that
    /// first start (c-SAK-1: the script's 0.5 s timer before Wind Walk's
    /// buff timer at 22.0).
    #[test]
    fn a_periodic_restart_keeps_its_first_start() {
        let mut queue = TimerQueue::new();
        let periodic = queue.start(bits(0.5), "periodic");
        queue.start(bits(1.0), "one-shot");
        let mut log = Vec::new();
        while log.len() < 3 {
            queue.tick(|q, fired| {
                log.push(fired.payload);
                if fired.payload == "periodic" {
                    q.start_as(bits(0.5), "periodic", periodic);
                }
            });
        }
        assert_eq!(log, ["periodic", "periodic", "one-shot"]);
    }

    /// Test T1c of SPEC-0002: a periodic 0.0001 s timer fires several times
    /// per tick, 0.0001 s apart, and 1000 fires span about 0.1 s.
    #[test]
    fn spec_0002_short_periodic_timer() {
        let mut queue = TimerQueue::new();
        queue.start(bits(1.0), false);
        let mut fires = Vec::new();
        while fires.len() < 1001 {
            queue.tick(|q, fired| {
                if fired.payload || fires.is_empty() {
                    fires.push(fired.expiry);
                    q.start(bits(0.0001), true);
                }
            });
        }
        // Measured: 0.0999 to 0.1000 s. Each period is added with the
        // engine's truncating addition, so the span stays a little short.
        let span = real(fires[1000]) - real(fires[0]);
        assert!(
            (0.0999 - CLOCK_RESOLUTION..=0.1).contains(&span),
            "1000 fires span {span}"
        );
        let per_tick = fires
            .iter()
            .filter(|&&e| real(e) > real(fires[0]) && real(e) <= real(fires[0]) + 0.005)
            .count();
        assert!(per_tick >= 49, "{per_tick} fires in the first tick");
    }

    #[test]
    fn queue_time_drifts_below_exact_ticks() {
        let mut queue: TimerQueue<()> = TimerQueue::new();
        for _ in 0..4000 {
            queue.tick(|_, _| {});
        }
        // 4000 exact ticks are 20 s; the engine sum stays below (SPEC-0002).
        let drift = 20.0 - real(queue.time());
        assert!(drift > 0.0 && drift < 0.01, "drift {drift}");
    }

    #[test]
    fn timeouts_are_clamped_and_cancel_works() {
        let mut queue = TimerQueue::new();
        let short = queue.start(bits(-5.0), 1);
        assert_eq!(queue.expiry(short), Some(MIN_TIMEOUT));
        let long = queue.start(bits(1000.0), 2);
        assert_eq!(queue.expiry(long), Some(sub(WRAP, MAX_TIMEOUT_MARGIN)));
        assert_eq!(queue.cancel(long), Some(2));
        assert_eq!(queue.cancel(long), None);
        let mut fired = Vec::new();
        queue.tick(|_, f| fired.push(f.payload));
        assert_eq!(fired, [1]);
        assert!(queue.is_empty());
    }

    /// SPEC-0002 long timers: 310 s is two parts of 120 s and 69.99998 s.
    #[test]
    fn long_timeouts_are_split() {
        assert_eq!(split_timeout(bits(119.5)), (0, bits(119.5)));
        let (n, rest) = split_timeout(bits(310.0));
        assert_eq!(n, 2);
        assert!(real(rest) < 70.0 && real(rest) > 69.9999, "{}", real(rest));
        assert_eq!(split_timeout(bits(450.0)), (3, bits(90.0)));
        assert_eq!(split_timeout(bits(240.0)), (2, MIN_TIMEOUT));
        assert_eq!(raise_timeout(bits(-1.0)), MIN_TIMEOUT);
    }

    /// A timer started at 299.5 s for 1 s expires after the queue wraps:
    /// its expiry is moved back by 300 s along with the queue time.
    #[test]
    fn wraps_at_300_seconds() {
        let mut queue = TimerQueue::new();
        queue.start(bits(299.5), "a");
        let mut fired = Vec::new();
        let mut wrapped = false;
        while fired.len() < 2 {
            let before = real(queue.time());
            queue.tick(|q, f| {
                fired.push((f.payload, real(f.expiry)));
                if f.payload == "a" {
                    q.start(bits(1.0), "b");
                }
            });
            wrapped |= real(queue.time()) < before;
            assert!(real(queue.time()) <= 300.0);
        }
        assert!(wrapped);
        assert_eq!(fired[0].0, "a");
        assert_measured(fired[0].1, 299.5);
        assert_eq!(fired[1].0, "b");
        assert!((0.4..0.6).contains(&fired[1].1), "{fired:?}");
    }
}
