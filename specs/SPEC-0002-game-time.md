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

### Timers

Timers live in a queue with its own time `q`, measured in game seconds.
All arithmetic on timer times below is the engine arithmetic of SPEC-0004
(`add`, `sub`); comparisons are ordinary float comparisons.

- `q` starts at 0.0 when the game starts.
- **Start.** `TimerStart(t, timeout, periodic, func)` takes `timeout`,
  replaces it by `sub(300.0, 0.001)` if it is 300 or more (open question 6)
  and by 0.0001 if it is less than 0.0001 (this includes 0 and negative
  values), and sets the expiry `e = add(q, timeout)`. Each start also takes
  the next number of a start counter.
- **Order.** Pending timers are ordered by expiry `e`; equal expiries are
  ordered by the start counter, so the earlier start fires first.
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

- At lower speeds a sleep lasts fewer game seconds:
  `TriggerSleepAction(1.0)` called 0.5 s after a speed change lasted 1.2,
  0.92 and 0.75 game seconds at speed indexes 2, 1 and 0. The ratios
  (0.77, 0.625) match the speed factors 0.8 and 0.6.
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
  stop, but the sleep clock keeps running: a thread that pauses the game at
  game time 1.0 and calls `TriggerSleepAction(2.0)` resumes at game time
  1.025 and can unpause the game. A 0.5 s timer started just before the
  pause fires at game time 1.5, after the unpause.

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

1. Under which conditions does `SetGameSpeed` do nothing? In single
   player it works (test: 2 -> `SLOWEST` gives 0, `FASTEST` gives 2,
   `SLOW` gives 1). The game ignores it when a game setting flag is set;
   which lobby or map setting sets that flag is not known yet.
2. `TimerGetRemaining` and `TimerGetElapsed`: which operations derive them
   from `e`, `q` and the timeout.
3. The sleep clock in detail: sleeping threads are timed on a second timer
   queue that is advanced with the real time of each frame; the 0.1 s
   checks appear to be the network turns of single player (100 ms). Turn
   length and alignment in multiplayer and in replays are not known.
4. Multiplayer: all tests so far are single player.
5. Catching up after a slowdown. Analysis of the game library shows that
   real time not yet converted to ticks is accumulated and capped at 4000 ms,
   so a lag longer than 4 s is not made up; not yet confirmed by a test.
6. Timers of 300 s or more: the queue limits a timeout to just under 300 s;
   whether `TimerStart` with a longer timeout fires late (after splitting)
   or early (at 299.999 s) is not tested.

## Sources

- Analysis of the 1.27b game library (method kept in the analyst
  workspace): tick length, speed table, minimum timer timeout.
- `Scripts\common.j` (speed constants) and `Scripts\Blizzard.j`
  (`PolledWait`) from the user's `War3Patch.mpq`.
- Test maps on 1.27b under Wine, single player, 2026-10-16.

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
