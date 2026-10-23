---
id: SPEC-0002
title: Game time, game speed, timers and script sleeps
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary)", "Anton Gushcha (black-box test maps only)"]
---

## Behaviour

### Simulation tick

The simulation advances in ticks of 0.005 game seconds. The tick length is
the single-precision float nearest to 0.005 (bit pattern `0x3BA3D70A`).

### Real time and game speed

The game converts elapsed real time into ticks in chunks of 25 ms of real
time. Each chunk runs a number of ticks that depends on the game speed:

| Speed index | `common.j` constant | Lobby name | Ticks per 25 ms | Game seconds per real second |
|---|---|---|---|---|
| 0 | `MAP_SPEED_SLOWEST` | Slow | 3 | 0.6 |
| 1 | `MAP_SPEED_SLOW` | Normal | 4 | 0.8 |
| 2 | `MAP_SPEED_NORMAL` | Fast | 5 | 1.0 |

The lobby names are inferred from the default speed (see Verification,
item 5) and still need a stopwatch check for indexes 0 and 1.

- `SetGameSpeed` takes the speed index. Values above 2
  (`MAP_SPEED_FAST`, `MAP_SPEED_FASTEST`) are treated as 2.
- `GetGameSpeed` returns the current index (0, 1 or 2).
- In some game modes `SetGameSpeed` does nothing (open question 1).
- A game started from the lobby with default settings runs at index 2.
- **Falling behind.** Real time that has not yet been turned into ticks is
  kept, but at most 4 s of it. After a stall of the game (for example a
  script that runs for several seconds of real time in one tick), the
  game runs extra ticks to make up at most 4 s; any longer stall is lost
  as game time. Test CU: after stalls of about 2.3, 6.9 and more than 10 s,
  a 10 s sleep saw 10.1, 7.2 and 4.2 s of game time, matching
  `min(stall, 4) + 10 - stall` (and only the 4 s of catch-up once the sleep
  had already expired during the stall).

### Timers

Timers live in a queue with its own time `q`, measured in game seconds.
All arithmetic on timer times below is the engine arithmetic of SPEC-0004
(`add`, `sub`); comparisons are ordinary float comparisons.

- `q` starts at 0.0 when the game starts.
- **Start.** `TimerStart(t, timeout, periodic, func)` takes `timeout`,
  replaces it by 0.0001 if it is less than 0.0001 (this includes 0 and
  negative values), and sets the expiry `e = add(q, timeout)`. Each start
  also takes the next number of a start counter.
- **Long timers.** The queue itself holds only timeouts below 300 s (a
  larger value given to it directly becomes `sub(300.0, 0.001)`), so a
  timer started with `timeout` is split:
  `n = trunc(timeout) / 120` (integer division of the integer parts) and
  `R = fmod(timeout, 120.0)` (SPEC-0004). If `n = 0` the timer is an
  ordinary entry with `timeout`. Otherwise it runs `n` parts of 120.0 s,
  each started from the expiry of the previous one (`add(e, 120.0)`), and
  then a last part of `R` s (at least 0.0001), after which it fires. A
  periodic long timer starts the same sequence again. Because `fmod` is
  inexact, a long timer can fire slightly early: 310 s is 2 parts and
  `R = 69.99998`, so it fires just before a timer due at exactly 310 s.
  `TimerGetTimeout` and `TimerGetRemaining` report the full timeout.
  Tested: 299, 299.999, 300, 301, 310 and 450 s all fire at the requested
  second.
- **Order.** Pending timers are ordered by expiry `e`; equal expiries are
  ordered by the start counter, so the earlier start fires first. A
  periodic timer's re-arm takes no new number: it keeps the number of
  its first start. `c-SAK-1`: the script's 0.5 s timer (first started
  at 1.5 s) and a 20 s buff timer started at 2.0 s both expired at
  22.0; the script's timer fired first.
- **Tick.** At each simulation tick the queue computes
  `q1 = add(q, 0x3BA3D70A)` (the tick length). Then, while the earliest
  pending timer has `e <= q1`: that timer is removed, `q` is set to its `e`,
  and it fires (its callback runs, and a periodic timer is started again
  with its period, so the next expiry is `add(e, period)`, which agrees
  with test T1c). Finally `q` is set to `q1`.
- **Wrap at 300 s.** If `q1 >= 300.0`, the queue first fires every timer
  with `e <= 300.0` as above (with `q = 300.0` at the end), then replaces
  every pending expiry `e` by `sub(e, 300.0)` and continues with
  `q1 = sub(q1, 300.0)`, using 0.0 instead if that result is smaller than
  8e-7 in magnitude (`0x3556BF95`). So timer times never exceed 300.

Constants used above, as stored in the game: 0.0001 is `0x38D1B717`,
0.001 is `0x3A83126F` (both the nearest floats), 300.0 is `0x43960000`,
120.0 is `0x42F00000`, the tick length is `0x3BA3D70A` and the wrap
threshold 8e-7 is `0x3556BF95`.

Reading a timer (`timeout` here is the value after the limits above):

- `TimerGetTimeout(t)`: the timeout given to the last `TimerStart`.
- `TimerGetRemaining(t)` of a running timer: `sub(e, q)`.
- `TimerGetElapsed(t)` of a running timer: `sub(timeout, sub(e, q))`.
- Of a paused timer, `TimerGetRemaining` is the value saved when it was
  paused and `TimerGetElapsed` is `sub(TimerGetTimeout(t), remaining)`.
- A timer never started reads 0 for timeout, remaining and elapsed. An
  expired one-shot timer reads remaining 0 and elapsed equal to its
  timeout.

Periodic timers, pause and resume:

- A periodic timer is started again after its callback has run: inside
  the callback `TimerGetRemaining` is 0 and `TimerGetElapsed` is the
  period. The next expiry is one period after the expiry that fired.
- `TimerStart` on the expired timer inside its own callback replaces the
  period and the callback: a 0.5 s periodic timer restarted with 0.3 s in
  its second callback then fired every 0.3 s.
- `DestroyTimer` on the expired timer inside its own callback stops it
  without error.
- `PauseTimer` keeps the remaining time; `ResumeTimer` continues from it,
  also for long timers: a 130 s timer paused after 5.1 s for 10.1 s fired
  at 130 + 10.1 s after its start. After `ResumeTimer`, `TimerGetElapsed`
  counts from the resume (2 s after resuming it read 2.17 with 122.73
  remaining, 124.90 having been left at the pause).

Consequences that scripts can see:

- While a timer callback runs, the game time seen through other timers is
  the expiry of the firing timer, not the tick time.
- A period shorter than a tick makes a timer fire several times within one
  tick.
- Because `q` is a sum of tick lengths computed with the engine addition,
  it drifts slightly below the exact multiple of 0.005 (about 2 ms after 20
  s on a reference timer).
- `TriggerRegisterTimerEvent(trig, timeout, periodic)` follows the same
  rules: a 1.0 s event registered at game start fires at exactly 1.0.

### Script sleeps

`TriggerSleepAction(d)` does **not** measure game time. Sleeping threads
are timed on a separate **sleep clock** that advances with real time in
steps of 25 ms (the chunks of the game loop) and is not scaled by the game
speed. Threads are resumed only at sleep checks, which happen every 0.1 s
of that clock.

Consequences:

- At game speed index 2 (1.0) and while the game keeps up with real time,
  the sleep clock and game time advance together, and the checks fall on
  game times 0.1, 0.2, 0.3, ... In this case a thread that calls
  `TriggerSleepAction(d)` at a check resumes at the first later check at
  which more than `d + 0.05` seconds have passed, that is after
  `round(10 * d) + 1` checks with halves rounded up:

  | d | 0 to 0.0499 | 0.05 to 0.1499 | 0.15 to 0.2499 | 0.25 | 1.0 |
  |---|---|---|---|---|---|
  | checks waited | 1 | 2 | 3 | 4 | 11 |
  | game seconds | 0.1 | 0.2 | 0.3 | 0.4 | 1.1 |

- At lower speeds a sleep lasts fewer game seconds, scaled by the speed
  factor: `TriggerSleepAction(1.0)` started at a check lasted 1.1 s at speed
  index 2 (220 ticks), 0.88 s at index 1 (176 ticks) and 0.66 s at index 0
  (132 ticks), five runs each (test SQ).
- Resumes happen at the end of a 25 ms chunk of real time, so a sleep can
  last one chunk longer than the rule says: one run in five took 225, 180
  or 135 ticks. A chunk is 5, 4 or 3 ticks at speed index 2, 1 or 0.
- In a LAN game both players resume the thread at the same tick (the resume
  is distributed with the network turns), with the same durations as in
  single player, including the occasional extra chunk.
- Rule including sleeps that start between checks, in sleep clock time
  (equal to game time at speed 1.0 while the game keeps up): a thread that
  starts sleeping at time `t` resumes at the first sleep check `T` with
  `T - 0.05 > t + d`. For starts at a check this is the formula above.
  In the T2d test (starts 0.003-0.097 s after a check, `d` 0, 0.05, 0.1)
  it predicts 16 of 18 resume times; the two others were within 0.02 s of
  a boundary, which the sleep clock (driven by real frame times) does not
  resolve.
- An implementation cannot reproduce the original exactly for sleeps that
  start between checks, because the original depends on real frame timing.
  Using game time divided by the speed factor as the sleep clock and the
  rule above gives the behaviour of an unloaded single-player game.
- When the game falls behind real time, the sleep clock runs ahead of game
  time. Sleep checks then fall between the 0.1 s steps of game time, in
  multiples of 0.025 s, and resume times measured in game time become
  irregular. This explains the irregular results of sleeps started between
  checks: on a slow test machine, 136 sleeps started at offsets of 0 to
  0.099 s after a check waited between 0.25 and 7.7 tenths of a game second
  for the same d.

- While the game is paused with `PauseGame(true)`, game time and timers
  stop, but the sleep clock keeps running, so a sleeping thread can resume
  during the pause and unpause the game. The pause takes effect at the end
  of the current 25 ms chunk: the remaining ticks of that chunk still run
  (between 0 and 4 ticks were observed). A thread that paused at game time
  1.0 and slept 2.0 s resumed at 1.025; a 0.5 s timer started just before
  the pause fired at 1.5, after the unpause.

`PolledWait` from `Blizzard.j` measures the remaining time with a timer
(game time) and sleeps in steps of `TriggerSleepAction(0.1)`. It ends
within a few tenths of a second after the requested game time at every
speed: `PolledWait(1.0)` lasted 1.17-1.27 game seconds at speed 2 and
1.03-1.06 at speeds 0 and 1.

## Edge cases

- `TriggerSleepAction(0)` still suspends the thread until the next check.
- A timeout below 0.0001, including 0, behaves exactly as 0.0001.
- Two timers with equal timeouts started in the same tick fire in start
  order, regardless of their handle ids.

## Open questions

1. Under which conditions does `SetGameSpeed` do nothing? It works in
   single player and in a LAN game (both players see index 0 after
   `SetGameSpeed(MAP_SPEED_SLOWEST)`). The game ignores it when a flag of the
   game settings is set; that flag is not the map flag 0x4000 and was not
   set in a LAN game. Battle.net games were not tested.
2. Replays: how sleep resumes are reproduced when a replay is watched.
3. The exact number of ticks that run after `PauseGame(true)`.

## Sources

- Analysis of the 1.27b game library (method kept in the analyst
  workspace): tick length, speed table, minimum timer timeout.
- `Scripts\common.j` (speed constants) and `Scripts\Blizzard.j`
  (`PolledWait`) from the user's `War3Patch.mpq`.
- Test maps on 1.27b under Wine, single player and a two-player LAN game
  between two game instances, 2026-10-16 and 2026-10-17.

## Verification

All test maps are a 12-player melee map without victory conditions. A
reference clock is a one-shot timer of 1000 s started on the first line of
`main`; "time" below is `TimerGetElapsed` of that clock. Do not use a much
longer reference timer: near 100000 s the float step is 1/128 s and hides
the behaviour.

1. **Periodic timer (T1c).** At game time 1.0 start a periodic 0.0001 s
   timer. Expected: consecutive fires 0.0001 s apart (shown as 0.00006 or
   0.00012 because of the clock resolution); 1000 fires span 0.0999-0.1000 s.
2. **Timer order (T3c).** At 1.0 start timers in this order: three of 0.5 s
   (labelled 2, 0, 1), one of 0.501, one of 0.499, one of 0.0. Expected
   firing order and times: 0.0 at 1.0001; 0.499 at 1.499; the three 0.5 s
   timers at 1.5 in start order 2, 0, 1; 0.501 at 1.501.
3. **Sleeps (T2c).** From a trigger action that starts at 1.0, call
   `TriggerSleepAction(d)` three times in a row for each d in 0, 0.001, 0.01,
   0.05, 0.0999, 0.1, 0.1001, 0.15, 0.2, 0.25 and record start and end in
   5 ms ticks. Expected: every call starts and ends on a multiple of 20
   ticks; durations 20, 20, 20, 40, 40, 40, 40, 60, 60, 80 ticks.
4. **Sleeps and game speed (SP).** In a trigger action: for speed indexes
   2, 0, 1, 2 in turn, `SetGameSpeed`, `TriggerSleepAction(0.5)`, then
   measure `TriggerSleepAction(1.0)` and `PolledWait(1.0)` in game time.
   Expected: the sleep lasts about 1.2, 0.75, 0.92, 1.2 game seconds;
   `PolledWait` lasts 1.0-1.3 game seconds at every speed.
5. **Pause (PZ).** As described under Script sleeps: game time 1.0 at
   the pause, 1.025 when the 2 s sleep ends, the 0.5 s timer at 1.5.
6. **Game speed (T4).** In a default lobby game `GetGameSpeed` gives index
   2. Pending: measure with a stopwatch the real time between game times 10
   and 60 at each lobby speed; expected 50 s, 62.5 s and 83.3 s.
7. **Long timers (LT).** At game time 1.0 start one-shot timers of 299,
   299.999, 300, 301, 310 and 450 s and a periodic 1 s counter. Each fires
   in the second it asks for; the 310 s timer fires before the counter's
   310th tick.
8. **Catching up (CU).** Stall the game for several seconds of real time
   inside one tick (many `ExecuteFunc` calls of a function that runs until
   the operation limit), then measure game time over
   `TriggerSleepAction(10.0)`. Expected: 10.1 for a stall under 4 s, about
   `14 - stall` for longer stalls, about 4.2 when the stall exceeds 10 s.
9. **Timer readings (RD).** A never-started timer reads 0, 0, 0; a 0.5 s
   one-shot timer read 0.5 s after it expired gives remaining 0, elapsed
   0.5, timeout 0.5.
10. **Sleep and speed, repeated (SQ).** Five `TriggerSleepAction(1.0)` in a
    row at each speed index: 220 220 220 225 220, 176 176 180 176 176 and
    135 132 132 135 132 ticks.
11. **LAN (MP).** Two players; the sleep durations of item 3 measured by
    both: identical tick numbers on both machines (20, 40, 40, 80-85 and
    220-225 ticks for d = 0, 0.05, 0.1, 0.25, 1.0); `SetGameSpeed(SLOWEST)`
    takes effect for both.
