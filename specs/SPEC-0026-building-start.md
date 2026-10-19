---
id: SPEC-0026
title: Build orders and construction (Human worker)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (replays with mouse clicks, test maps, memory reads)"]
---

## Behaviour

This spec covers a Human worker (peasant, `hpea`) ordered to build a
farm (`hhou`) on open, buildable ground: the order, the costs, the walk,
the moment construction starts, where the worker stands, what the game
draws from the random generators and what changes for the checksum
(SPEC-0024), and then how the building grows until it is finished.
Other races and several workers on one building are not covered (open
questions).

### The order

A build order given with the mouse (build menu key B, then key F for
the farm, then a left click on the ground) is recorded in a replay as
action `0x11` with:

- flags `0x0004`;
- the order id, which for a build order is the rawcode of the building
  type: `0x68686F75` (`hhou`);
- two 32-bit fields `0xFFFFFFFF` (no target object);
- the target point as two floats.

The point in the replay is already on the building grid (SPEC-0022,
"Snapping"): the recorded point was (192, -384), the place where the
farm then stood. Opening the build menu is a separate one-byte action
`0x67`, which changes nothing in the game (no effect on the checksum).
`IssueBuildOrderById(worker, 'hhou', x, y)` from a script behaves the
same way (tests below).

### When the order is accepted

The order takes effect in the turn after its time slot (SPEC-0024,
"When actions take effect"). At that moment:

- the costs are paid at once, before the worker moves: gold 500 -> 420
  and lumber 500 -> 480 for a farm (`goldcost` 80, `lumbercost` 20);
- the worker's current order (`GetUnitCurrentOrder`) becomes the
  building's rawcode as an integer (1751674741 for `hhou`).

### The walk and the start of construction

The worker walks towards the building's position like a `move` order
(SPEC-0018; on open ground in a straight line, at its own speed).
Construction starts at the first update of the worker at which its
distance from the building's position is at most 100 world units
(measured: 99.89 started; 100.14 did not, and the worker started at its
next update at 94.44) and its heading is within 0.2 radians (11.46
degrees) of the direction from the worker to the building's position.
A worker that is close enough but faces farther away first turns on
the spot (SPEC-0013, facing) and starts at the first update at which
the difference is at most 0.2 (measured: 11 degrees started at once,
12 degrees turned first). On a straight walk the worker already faces
the building, so only the distance matters.

At that update:

- the building is created at the target point, in the same tick: script
  timers that expire in that tick already see it (in `c-RSR-1` the
  player's unit count read by a periodic timer is 2 in the creation
  tick). The actions of a trigger on `EVENT_PLAYER_UNIT_CONSTRUCT_START`
  run later in that tick, after the timers of the tick (a timer of that
  tick still saw the variable set by the trigger unset; the trigger
  then logged the same tick number);
- the worker's current order becomes 852024 (`repair`, by
  `OrderId2String`) while it builds;
- if the worker's footprint (SPEC-0020, size classes; a peasant is
  class 1, 2 x 2 cells) overlaps the building's footprint (farm 4 x 4
  cells around its position), the worker is moved at once to a free
  position next to the building, at a cell centre (`32 k + 16`); if it
  does not overlap, the worker stays where it is. Measured, relative to
  the building's position: workers that started at (98.13, 0) to
  (99.89, 0) stayed there; workers that started at (94.44, 0) to
  (95.69, 0) were moved to (112, 0); workers arriving from 30 and 45
  degrees were moved to (112, 16) and (112, 48); a worker arriving from
  the north-west at about (-44, 88) was moved to (-16, 112). The search
  that picks the free position is the general placement of units on
  blocked ground (SPEC-0013, open question 3) and was not studied.

### Construction progress

Times below are game time in seconds as given by the game clock of
SPEC-0018 (the clock of tick `k` is the sum of `k - 1` additions of
0.005, so it drifts slightly from `0.005 (k - 1)`). Arithmetic is that
of SPEC-0004.

With `max` the building's `HP` and `bldtm` its build time (farm 500 and
35, barracks 1500 and 60), the life of a building under construction is

    base = mul(max, 0.1)
    rate = div(mul(max, 0.9), bldtm)
    life(T) = clamp(add(base, mul(rate, sub(T, Tb))), 0, max)

where `T` is the game time at which the life is read and `Tb` the time
growth began. For a farm `base` = 50 and `rate` = 12.857142
(`0x414DB6DB`), for barracks 150 and 22.5. The life is not stepped tick
by tick: every read (`GetUnitState(u, UNIT_STATE_LIFE)`, the checksum,
the display) evaluates the formula at the current time.

Growth begins (`Tb`) at the clock of the second tick after the tick in
which the building is created, when the worker was not moved (it stayed
outside the footprint, see above). Until then the life is `base`. When
the worker was moved out of the footprint, growth began later: 14 ticks
after the creation for a farm and 20 ticks for barracks in every such
test. The rule behind these numbers was not found (open question 1).

Construction is finished in the first tick whose clock, taken at the
next tick, is at least `Tb + bldtm`. Measured from the tick of `Tb`:
7000 ticks for a farm, 12003 ticks for barracks (the drift of the
clock). In that tick:

- the life is set to `max`; from then on it is an ordinary unit life
  (SPEC-0013);
- the player's food cap grows by the type's `fmade` (6 for a farm);
  `GetPlayerState(p, PLAYER_STATE_RESOURCE_FOOD_CAP)` shows the new
  value from the next tick;
- the worker stops building: its current order changes from 852024 to
  0 at its next update (two ticks later in the tests) and it stands
  where it was.

The progress was the same in every run and in playback of the replay.

### Random numbers

When the building is created, its alarm ability (the ability buildings
use to warn of attacks) draws **game generator 21** (SPEC-0007) once:
one step, mapped to a real in `[0, 1)` (read from the code: the same
mapping as `GetRandomReal(0.0, 1.0)`, SPEC-0007, "Natives"; the value
itself was not checked). The value is the delay until the first run
of the ability's periodic check. Nothing else was drawn when the farm
was placed. (The worker itself drew game generator 2 when it was
created, as every unit with a weapon does, SPEC-0007 and SPEC-0019.)

### Checksum (SPEC-0024)

- The turn after the order's slot: `C` and `D` change (the order
  creates internal objects), and `C` again one turn later.
- The turn after construction starts: `C`, `D`, `R` and `G` change
  together: `R` through the draw of generator 21, `G` through the
  player's type record of the building (below), `C` and `D` through the
  new building's objects.

### The player's type record of the building

The player's list of type records (SPEC-0024, "Game sum G") gets a
record for the building type when the first one is started, appended
after the existing records. With three farms under construction the
record was `q0` = 2147483647 (no limit), `q1` = 0, `q2` = 3,
`q3` = 0, `q4` = 0, `q5` = 0, `q6` = 4. For comparison the record of
eight existing peasants was `q0` = 2147483647, `q1` = 8, `q2` = 0,
`q3` = 0, `q4` = 8, `q5` = 0, `q6` = 8. So `q2` counts buildings under
construction, `q1` and `q4` count finished units (their difference is
not known), and `q6` is 4 for a building type and 8 for a unit type.

## Edge cases

- In one test five of twenty script build orders were not executed at
  all (the worker kept order 0 and did not move); all five sites were
  in one column of the test map. The reason (placement check) was not
  found.
- Objects created by a script also take entries of the object table
  (SPEC-0024): in a test whose script created and destroyed a group at
  every tick, the creation counter `D` grew every turn and the farm got
  the creation number 2580.

## Open questions

1. The delay before growth when the worker was moved (14 or 20 ticks
   instead of 2), the growth of the model, the player's type record
   after completion (expected `q2` down by one and `q1`, `q4` up by
   one; not checked), and cancelling.
2. The placement search for the worker, and the exact footprint test.
3. The checks that reject a build order (blocked or unbuildable ground,
   not enough resources, the failed test sites), and what is refunded
   when construction is cancelled.
4. Whether the distance of 100 depends on the building or the worker
   (only farms and peasants were measured), and the exact comparison
   (cells or world units).
5. Other races (Orc and Night Elf workers, Undead summoning) and
   several workers on one building.

## Sources

- Replays with mouse clicks on test maps RSN and RSR (`c-RSN-1.w3g`,
  `c-RSR-1.w3g` with a per-tick log of the worker, the resources and the
  number of units), checksum parts read from memory.
- Test maps RSS (eight peasants approaching from different angles) and
  RST (twenty peasants approaching from the east from 400.0 to 405.7)
  with script build orders, logged every tick.
- Test maps RSU and RSW (farm, life logged every tick with
  `R2SW(x, 1, 7)`), RSX (farm and barracks), RSY (barracks), RSZ and
  RTA (growth delay), RTB (worker positions and facings at the start),
  RTC .. RTF (heading of the worker at the start).
- Debugger on the running game: the random generator calls (generator
  and caller), the type records in memory.

## Verification

1. **c-RSR-1.w3g**: the build order is in slot 81; in the log
   (`c-RSR-1.log.tsv`) gold and lumber drop from 500/500 to 420/480 and
   the order becomes 1751674741 in the tick the order takes effect;
   the peasant walks at 190 towards (192, -384), its distance at its
   last update before construction is 103.51 and the farm appears at
   the next update, where the peasant is placed at (176, -272) with
   order 852024.
2. **RST** (east approach): distances at the update of the start
   98.13 .. 99.89 (worker stays), 94.44 .. 95.69 (worker moved to 112
   east of the building); a worker at 100.14 at an update did not start.
3. **Random numbers**: placing the farm calls game generator 21 once
   and no other generator.
4. **Life during construction** (RSW): the life of the farm read every
   tick follows `life(T)` with `base` 50 and `rate` `0x414DB6DB` to
   within 1e-7 at 80 points spread over the whole construction; it is
   50 in the creation tick and the next one and grows from the second
   tick after the creation.
5. **Completion**: a farm is finished 7000 ticks after the tick of
   `Tb`, barracks 12003 ticks; the life becomes 500 (1500), the food
   cap reads 6 from the next tick (farm), and the worker's order is 0
   two ticks after completion.
6. **Moved worker** (RSZ, RTA): growth starts 14 ticks after the
   creation for a farm and 20 for barracks; 2 ticks when the worker was
   not moved.
7. **Heading** (RTC .. RTF): a worker created 98 units east of the site,
   facing 11 degrees away from it, starts at its first update; facing 12
   degrees away it turns first and starts later.
