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
- expires at `now + timeout`. The addition uses the engine's own float
  addition, which differs from IEEE addition (open question 2);
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

`TriggerSleepAction(d)` suspends the calling thread. Suspended threads are
resumed only at **sleep checks**, which happen every 0.1 game seconds,
counted from the start of the game: at game times 0.1, 0.2, 0.3, ...

A thread that calls `TriggerSleepAction(d)` at a sleep check resumes at the
first later check at which more than `d + 0.05` seconds have passed. In
other words it waits `round(10 * d) + 1` checks, with halves rounded up:

| d | 0 to 0.0499 | 0.05 to 0.1499 | 0.15 to 0.2499 | 0.25 | 1.0 |
|---|---|---|---|---|---|
| checks waited | 1 | 2 | 3 | 4 | 11 |
| game seconds | 0.1 | 0.2 | 0.3 | 0.4 | 1.1 |

For a call made between two checks no rule is known yet (open question
3). Observed resume times, relative to the check C before the call made at
C + o:

| o | 0.003 | 0.03 | 0.049 | 0.051 | 0.07 | 0.097 |
|---|---|---|---|---|---|---|
| `TriggerSleepAction(0)` | C+0.1 | C+0.1 | C+0.1 | C+0.1 | C+0.2 | C+0.2 |
| `TriggerSleepAction(0.05)` | C+0.2 | C+0.2 | C+0.2 | C+0.2 | C+0.2 | C+0.2 |
| `TriggerSleepAction(0.1)` | C+0.2 | C+0.3 | C+0.2 | C+0.3 | C+0.3 | C+0.3 |

The last row is not monotonic in o, so the resume time depends on more than
the call time and d. Each value comes from one run; an implementation
should not rely on this table until the rule is found.

Sleep checks also drift against timer time: read on a reference timer, the
checks fall 0.02 ms early at 3 s and 2.4 ms early at 20 s.

`PolledWait` from `Blizzard.j` is built on a timer and `TriggerSleepAction`
and inherits these rules: `PolledWait(1.0)` started at a check lasted
about 1.2 s (measured with a coarse clock, to be repeated).

## Edge cases

- `TriggerSleepAction(0)` still suspends the thread until the next check.
- A timeout below 0.0001, including 0, behaves exactly as 0.0001.
- Two timers with equal timeouts started in the same tick fire in start
  order, regardless of their handle ids.

## Open questions

1. Under which conditions does `SetGameSpeed` do nothing (multiplayer,
   ladder, replays, a map flag)?
2. Exact definition of the engine's float addition and subtraction used for
   timer times. Needed for bit-exact expiry times; planned as a separate
   spec on deterministic arithmetic.
3. Resume time of a sleep started between two checks (see the table in
   Script sleeps), and the cause of the drift between sleep checks and timer
   time.
4. Are sleep checks tied to game time in multiplayer as well, or to network
   turns? All tests so far are single player.
5. What happens when the game falls behind real time (catch-up limit,
   dropped time)?
6. Do timers fire while the game is paused (`PauseGame`, menu in single
   player)?

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
4. **PolledWait (T2, to be repeated with the 1000 s clock).**
   `PolledWait(1.0)` called at a sleep check returns about 1.2 s later.
5. **Game speed (T4).** In a default lobby game `GetGameSpeed` gives index
   2. Pending: measure with a stopwatch the real time between game times 10
   and 60 at each lobby speed; expected 50 s, 62.5 s and 83.3 s.
