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

A timer started with `TimerStart(t, timeout, periodic, func)` at game time
`now`:

- uses `timeout`, raised to 0.0001 if it is smaller (this includes 0 and
  negative values);
- expires at `now + timeout`, computed with the engine's addition
  (SPEC-0004), which differs from IEEE addition (open question 2);
- a periodic timer expires again one period after its previous expiry
  time, so periods do not drift and do not snap to ticks. A period shorter
  than a tick makes the timer fire several times within one tick.

Firing:

- All timers that expire within a tick fire during that tick, in order of
  expiry time. Timers with the same expiry time fire in the order in which
  they were started.
- While a timer callback runs, the game time seen by scripts (for example
  through `TimerGetElapsed` of another running timer) is the expiry time of
  the firing timer, not the start or end of the tick.
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
2. Bit-exact timer times: addition and subtraction are specified in
   SPEC-0004. Still open: which operations combine the timer time, the
   timeout and the tick length, and in which order.
3. The exact definition of the sleep clock: is it the network turn time
   (same for all players in multiplayer), and how the 0.1 s checks are
   aligned to it.
4. Multiplayer: all tests so far are single player.
5. Catching up after a slowdown. Analysis of the game library shows that
   real time not yet converted to ticks is accumulated and capped at 4000 ms,
   so a lag longer than 4 s is not made up; not yet confirmed by a test.

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
