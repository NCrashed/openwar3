---
id: SPEC-0018
title: Unit movement on open ground (move order, part 1)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps, static analysis of the binary)"]
---

## Behaviour

This part covers one ground or air unit given a `move` point order on
flat open ground with no other units, doodads or cliffs in the way:
speed, turning, when the unit starts walking, how its position advances
and when it arrives. Path finding around obstacles and units is not
covered (see Open questions).

Values come from the unit's data (`Units\UnitBalance.slk`,
`Units\UnitData.slk`, or the map's object data): `spd` (move speed,
units per second), `turnRate` (radians, see below), `propWin`
(propulsion window, degrees), `orientInterp` (SPEC-0013).

The unit is updated every 6 ticks (0.03 s, SPEC-0013). "Update `k`" is
the `k`-th update after the order.

**Phase.** There is one update cycle for all units: every unit is
updated in the same ticks, every 6th tick of the game, whenever it was
created. Units created 1, 4, 7, ... 19 ticks apart and ordered in the
same tick all had their update in the same tick. Within a tick the unit
updates came before the script timers of these tests (see "Order
within an update tick" below), so an order given from a timer is
taken at the next update tick after the order's tick, also when the
order's tick is itself an update tick:

| Order given at timer tick | 0 | 1 | 2 | 3 | 4 | 5 .. 9 | 10 |
|---|---|---|---|---|---|---|---|
| First update at timer tick | 4 | 4 | 4 | 4 | 10 | 10 | 16 |

(Timer ticks counted by a 0.005 s periodic timer started at map
initialisation, with tick 0 the moment of the first orders.) An order
0.5 s after that moment (timer tick 100, itself an update tick) is
taken at tick 106, 6 ticks later, which is the case of the tests below.

**Phase from the start of the game.** Number the simulation ticks of
the game from 1 (tick 1 is the first tick after `main`; a 0.005 s
periodic timer started in `main` fires for the `k`-th time in tick `k`,
and a 0 s timer started in `main` fires in tick 1 before it). Then:

- the unit updates are in ticks 7, 13, 19, ... (`6 m + 1`, `m >= 1`);
  tick 1 is not an update tick;
- a 0.5 s timer started in `main` fires in tick 101 and a 1.0 s timer
  in tick 201 (the sum of 100 tick lengths stays just below 0.5,
  SPEC-0002), before the periodic timer of that tick;
- an order given in `main`, from the 0 s timer (tick 1) or in tick 6 is
  taken at the update of tick 7, and the unit is first seen moved in
  tick 8; an order given from a timer in tick 7 is taken at tick 13; an
  order from the 0.5 s timer (tick 101), or from a timer in tick 100, at
  tick 103;
- a footman created in `main` with `SetUnitFacing(u, 90)` first changes
  its facing in tick 7; one created in tick 47 in tick 49.

The timer ticks of the table above are these ticks minus 3.

**Order within an update tick.** The unit updates of a tick are not
always before its script timers. The update cycle has its own time
`U`: it is a periodic timer of the game's timer queue (SPEC-0002) with
the period `P = 0x3CF5C290` (0.0300000012, one unit in the last place
above the float nearest to 0.03), so `U1 = P` for the update of tick 7
and `U(m+1) = add(U(m), P)` (SPEC-0004) for each following update
tick; while the updates run, the queue's time is `U(m)`; `U(m)` lies between
`clock(k)` and `clock(k + 1)` of its tick `k = 6 m + 1`. Inside the tick
the updates (and what they cause: a construction start and its trigger
event, SPEC-0026) come at time `U` among the timers that fire in the
tick, ordered with their expiries as timers are ordered with each
other (SPEC-0002): after a timer whose expiry is below `U`, before one
whose expiry is above. The 0.005 sum of the clock and the 0.03 sum lose
different amounts to truncation, so `U` moves ahead of the clock of
its tick: `U - clock(k)` is +0.0000005 in tick 307 (1.53 s), +0.0000935
in tick 943 (4.71 s), +0.0001259 in tick 1045 (5.22 s), +0.0005331 in
tick 1891 (9.45 s), +0.0005474 in tick 1909 and +0.0012770 in tick 2827
(14.13 s). When `U` has moved a whole tick ahead, the updates fall one tick
later: an update runs in the tick `k` with `clock(k) < U(m) <=
clock(k + 1)` (the rule of every timer of the queue, SPEC-0019,
"Acquisition by idle units"), which is tick `6 m + 1` up to `m` = 1157,
`6 m + 2` from `m` = 1158 (34.74 s, tick 6950), `6 m + 3` from `m` =
1486 (44.58 s, tick 8919) and `6 m + 4` from `m` = 1813 (54.39 s, tick
10882); the updates of `c-RXI-1.events.tsv`, read in the game, fall in
exactly these ticks.

A timer with timeout 0 started inside a timer callback expires 0.0001
later (the minimum timeout of SPEC-0002), so a 0.005 s periodic timer
started from such a timer (as in the test maps of these specs, whose
actions run from a 0 s timer of a 1.0 s timer of `main`) fires in tick
`k` with expiry `clock(k) + 0.0001`. Against that timer the update came
first up to 4.71 s and after it from 5.22 s on (tests RTI, RTM and
`c-RSR-1` of SPEC-0026). Measured through `TimerGetElapsed` of a
reference timer read in construction start events (test RTM): with
the queue's time equal to `U(m)` for `m` = 51, 174, 324 and 487, the
reads `sub(30.0, sub(e, U(m)))` (`e = add(add(1.0, 0.0001), 30.0)`, the
expiry of the reference timer) give exactly the values printed,
0.529901504, 4.219886768, 8.719846720 and 13.609804096 (`R2SW` with 9
decimals); the period and the expiry `U(m)` were read from the timer's
entry in memory.
The orders of the tables above were given early in the game, where the
updates still come first. How the cycle goes on once `U` has moved by
a whole tick (after roughly 200 s at this rate) was not tested.

### Two angles: heading and facing

A unit has a **heading** `H`, the direction it moves in, and a separate
**facing** `F`, the angle that `GetUnitFacing` returns and the model is
drawn with.

- At each update the heading turns towards the direction of the target
  by at most `turnRate` radians (footman 0.6 rad = 34.38 degrees per
  update). If the remaining difference is smaller, it becomes equal to
  the target direction. It turns the shorter way.
- At an update that stops the unit (velocity 0), the stored heading is
  the heading turned in this way, wrapped into `[0, 2 pi)` by the
  `fmod` of SPEC-0004 (`2 pi` = `40c90fdb`, then `+ 2 pi` if negative);
  it is not taken from the velocity. In replay `c-RXI-1` peasant 0
  stopped at the mine in tick 1069: heading before `40c2b7cc`
  (6.084936), turned by its `turnRate` 0.6 to `40d5eaff` (6.684936),
  stored `3ecdb265` (a plain `sub(x, 2 pi)` would give `3ecdb240`).
- The facing follows the heading by the facing turn rule of SPEC-0013
  (`step = min(w + s, m, B(r))` per update, with the `orientInterp`
  values and the braking table there). It is the same rule as for
  `SetUnitFacing`, applied to a heading that may itself be turning.
- Example: with a slow `turnRate` (0.1 rad, 5.73 degrees per update) the
  footman's facing went 4.0107, 10.6987, 16.3289, 22.0670, 27.7958,
  33.5255 degrees: two acceleration steps, then the brake holds it at a
  constant 0.85 degrees behind the heading, where `B(0.85 + 5.73)`
  equals the heading's 5.73 degrees per update. The rule gives these six
  values to 0.0001 degrees.

So `turnRate` sets how fast a unit can change its direction of motion,
and `orientInterp` only how fast its model turns. (This fits the
SPEC-0013 observation that `turnRate` did not change the turn speed of
`SetUnitFacing`.)

### When the unit starts to move

Let `D` be the direction from the unit to the target and `H_k` the
heading after update `k` (`H_0` is the unit's facing when the order is
given). If `|D - H_k| < propWin` (in degrees), the unit moves from
update `k + 1` on. Until then it turns on the spot.

This rule holds at every update of a walk, not only after the order:
whenever the target changes by more than `propWin` from the heading
(for instance when string pulling of SPEC-0020 switches to a point at
a sharp angle), the unit stands for that update and only turns. Example
(SPEC-0020, test PF2, footman): at tick 502 its heading was 87.1
degrees and the new target lay at 15.3 degrees; it turned to 52.7
without moving and walked on from tick 508.

Footman (`turnRate` 0.6, `propWin` 60), heading 0, target at angle `a`:

| `a` (degrees) | Headings after updates 1, 2, ... | First update with motion |
|---|---|---|
| 30, 45 | 34.4 | 1 |
| 60, 70, 80, 90, -90 | 34.4, 68.8 | 2 |
| 100, 110, 120 | 34.4, 68.8, 103.1 | 3 |
| 135, 150 | ..., 137.5 | 4 |
| 165, 180 | ..., 171.9 | 5 |

The first step of motion goes along the heading of that update, which
may still differ from `D`: for `a` = 70, 80 and 90 the first step was at
68.8 degrees; for 110 and 120 at 103.1. A unit that has to turn round
therefore walks a short curve.

The rule was checked on 40 cases: footman with `propWin` 10, 30, 60 and
90 and `turnRate` 0.1, 0.2, 0.3, 0.6, 0.8, 1.0 and 3.0 (custom types
based on the footman), target angles from 30 to 180 degrees. A
target set at 60 degrees with `propWin` 60 did not start the motion at
update 1; see "Movement arithmetic" for why: the target point was
written with three decimals, (x + 250, y + 433.013) for x + 500 cos 60,
y + 500 sin 60, and its angle in the engine arithmetic is `0x3F860AD9`,
72 units of the last place above `propWin`.

### Speed and position

- The unit moves at `spd` from its first step: there is no acceleration
  or deceleration. Measured distances per update (0.03 s) along a
  straight line: footman (270) 8.0996, knight (350) 10.4991, peasant
  (190) 5.6993, mortar team (220) 6.5996, gyrocopter (400) 11.9991.
- The position changes **every tick**, the heading only at updates.
  During one update the position advances along the heading at the
  constant velocity `spd * (cos H, sin H)`.
- The position is stored relative to the world's lower left corner
  (SPEC-0013), and the per-tick positions are computed from the
  position at the start of the update and the elapsed time, in the
  engine arithmetic of SPEC-0004 (which truncates): for a footman going
  east from x = -3000 on a map with `minX = -12288`, the stored
  coordinate is near 9288, where one unit of the last place is 1/1024.
  The positions after 1..6 ticks were 1382, 2764, 4147, 5529, 6911 and
  8294 1024ths from the start, the truncated values of `n * 1.35`
  (`n * 1382.4`, a little less since `cos H` is just below 1). Going west
  from x = 3000 (stored coordinate near 15288, decreasing), each update
  covered 8295 1024ths: truncation then lengthens the step.
- `GetUnitX`/`GetUnitY` read the position of the current tick.

### Arrival

The movement system works in cells of 32 world units (positions,
distances and radii are divided by 32 before use). A point move has an
arrival radius of 0.49 cells, that is 15.68 world units (the game's
minimum radius; a radius given by the order, such as an attack range,
replaces it when larger). The unit stops at the first update at which
its distance to the target is within that radius. The order then ends
at once:
`GetUnitCurrentOrder` returns 0 from that update on (851986, the `move`
order id, before). The unit stays where it is, so it comes to rest 8 to
16 short of the target point, depending on how the steps fall:

- footman 1000 east: rest at 11.93 before the target; 1000 north
  12.67; 1000 diagonal 11.9; knight 13.1; peasant 13.9 to 14.0; abomination
  11.9 to 12.3; mortar team 10.0 to 10.1; gyrocopter 4.1 (12 per update).
- With the target 8.1 + `d` ahead of a footman (one update of motion,
  then `d` left): `d` up to 15.65 stopped after that update, `d` 15.7
  and more walked one more update. For a peasant the boundary was not
  monotonic in `d` (15.6 walked on, 15.65 stopped), so the test is not
  the plain distance; see Open questions.
- A move order to the unit's own position ends at the first update
  without any motion.

After stopping, the facing goes on turning towards the last heading
by the `orientInterp` rule.

### Movement arithmetic

The rules above describe the motion; this section gives the exact
arithmetic, which makes the small heading changes on straight walks
(up to about 0.33 degrees either way, so that a footman walking 1000
east drifts up to 0.05 sideways and back) come out as in the game. All
operations are those of SPEC-0004 (`add`, `sub`, `mul`, `div`) and
SPEC-0011 (`sqrt`, `acos`, `sin`, `cos`, the internal functions behind
the natives of the same names, checked to be identical on 3000 random
inputs each). Positions `P` are stored relative to the map's lower left
corner in world units (SPEC-0013); velocities `v` are in world units
per second.

**Game clock.** The movement system has its own clock `T`: 0 when the
game starts, and at every tick `T = add(T, 0.005)` (`0x3BA3D70A`), so
it lags behind the exact time (after 305 ticks it is `1.5249985`). With
the ticks numbered as in "Phase from the start of the game", the clock
in tick `k` is the result of `k - 1` such additions (in tick 205 it is
`1.0199990`, in tick 301 `1.4999986`, read from memory). The elapsed
time between two clock values is `e = sub(T1, T0)`, taken as 0 when
`|e| < 0.0001` (`0x38D1B717`). (The clock is kept as a count of
300-second periods plus this value; the change of period was not
observed.)

**Units of storage.** The game keeps positions and velocities in cells
of 32 world units (position relative to the lower left corner, divided
by 32; velocity in cells per second; `spd / 32` as the speed limit).
Dividing by 32 is exact, and all results below came out with the same
bits whether computed in cells or in world units, so the formulas are
written in world units.

**Position.** The game keeps for each unit a committed position `P`,
the velocity `v` and the clock value `Tb` of the commit. The position
reported at any tick (`GetUnitX`, `GetUnitY`, and what the path code
uses) is `add(P, mul(v, e))` per coordinate with `e = sub(T, Tb)`. At
each update the position is committed first: `P = add(P, mul(v, e))`,
`Tb = T`.

**Angle of a vector.** For a vector `(x, y)` with length
`len = sqrt(add(mul(x, x), mul(y, y)))`: if `|len| <= 0.00001`
(`0x3727C5AC`) the angle is 0; otherwise `a = acos(div(x, len))`, the
angle is 0 if `|a| <= 0.00001`, `sub(2 pi, a)` if `y < 0`, and `a`
otherwise, with `2 pi = 0x40C90FDB`. Angles are in `[0, 2 pi)`.

**Angle difference.** For a target angle `A` and a current angle `H`
(both as produced by the angle function above): `t = sub(H, A)`; then

- if `t < 0` and `-t > pi`: `d = sub(-2 pi, t)`;
- if `t >= 0` and `t > pi`: `d = sub(2 pi, t)`;
- otherwise `d = -t` (sign flip, exact).

`pi = 0x40490FDB`, `2 pi = 0x40C90FDB`. This is not the same as
computing `sub(A, H)` and adding or subtracting `2 pi` afterwards: the
order of the operations changes the last bits when one angle is near 0
and the other near `2 pi`, which happens on every walk along the x axis
(the heading wobbles around 0).

**Update.** At each update of a walking unit, after the commit:

1. The current heading `H` is the angle of `v`, or the stored heading
   when the speed `sqrt(add(mul(vx, vx), mul(vy, vy)))` is below
   `0.0000004` (`0x34D6BF95`), for instance when the unit stood still.
   The heading is therefore not kept from one update to the next: it
   is recomputed from the velocity, and this round trip through `cos`,
   `sin` and `acos` is what makes it wander.
2. `A` = angle of `sub(target, P)` (per coordinate), `d` = the angle
   difference from `H` to `A` (above); `d = 0` if `|d| < 0.0000002`
   (`0x3456BF95`).
3. The unit moves in this update if `|d| < propWin` and
   nothing else stops it (SPEC-0019, SPEC-0020); its speed is then
   `spd` (no acceleration was observed), otherwise 0. `propWin` is
   compared in radians, `mul(propWin, DEG)` with `DEG` of SPEC-0011:
   the game held `0x3F860A91` for `propWin` 60 (read from memory), one
   unit of the last place below the nearest float to pi / 3. The test
   is strict: `|d|` equal to `propWin` does not move.
4. The new heading is `H = add(H, d)` if `|d| <= turnRate`, else
   `add(H, turnRate)` or `add(H, -turnRate)` with the sign of `d`. (It
   may leave `[0, 2 pi)` by a little; `sin` and `cos` are periodic.)
5. With speed `s > 0` the wanted velocity is
   `w = (mul(s, cos H), mul(s, sin H))` and the new velocity is
   `v = add(v, sub(w, v))` per coordinate. Then the velocity is limited
   to the unit's speed: with `q = add(mul(vx, vx), mul(vy, vy))`, if
   `|q| < 0.0000002` (`0x3456BF95`) the velocity becomes `(0, 0)`;
   otherwise, if `L = sqrt(q)` is greater than `spd` (strictly),
   `k = mul(spd, div(1.0, L))` and `v = (mul(vx, k), mul(vy, k))`.
   Because `cos` and `sin` of SPEC-0011 are approximations,
   `cos^2 + sin^2` is often slightly above 1 (a footman's `|w|` came
   out as 270.0040 for a heading of 20.56 degrees), and this step then
   shortens the velocity a little and turns it by up to about 0.001
   degrees. With speed 0 the velocity is `(0, 0)`.
6. The stored heading becomes the angle of the new velocity (angle
   function above, with `len = sqrt(q)`), or `H` brought into
   `[0, 2 pi)` when the velocity is 0. The facing turns towards the
   stored heading (SPEC-0013, "Facing turn rule").

So a unit standing still (at an update where it must not move) still
turns its heading towards the target of that update.

Example, footman at (-3000, -2000) facing 0, `move` to (-2000, -2000),
first update 6 ticks after the order: the first target angle is not 0
but 0.31338 degrees (`sqrt` of `mul(x, x)` is not exactly `x`), and the
stored positions relative to the corner at ticks 12, 18, 24, 30 are
(9296.099609, 10288.043945), (9304.199219, 10288.016602),
(9312.298828, 10287.977539), (9320.398438, 10287.990234) exactly.

The position is committed only at the updates (and when an order is
given). An earlier version of this spec assumed extra commits between
updates to explain differences of 1/1024; they were the velocity limit
of step 5 and the angle difference: traced in the running game, a
walking footman's position was committed only at its updates.

## Edge cases

- `move` to a point behind the unit: the unit first turns on the spot,
  then walks a curve (see the table); a footman turning by 180 degrees
  started at update 5, about 0.15 s after a footman turning by 45.
- A unit with `turnRate` far above `propWin` (3.0 rad) behaves like one
  that turns instantly: motion from update 1 or 2.

## Open questions

1. The exact arithmetic of the arrival test: with the radius 15.68 a
   footman stopped at 15.65 and walked on at 15.7 as expected, but a
   peasant walked on at 15.6 and stopped at 15.65 and 15.85, so the
   distance is not the plain Euclidean one in world units (it is taken
   in cells, from positions that change continuously between updates).
2. (Resolved: see Movement arithmetic, including the clock origin;
   there are no extra commits between updates.)
3. Path finding: SPEC-0020 covers static obstacles of the pathing map;
   detours around units, buildings, doodads and cliffs are open, and so
   is the long-distance case where a unit walking 5000 first headed 1.7
   degrees off and joined a line 32 off the start line (another unit
   crossed its way in that test).
4. Collision between moving units, formation of groups, `patrol`,
   `attack` move, `stop`, `holdposition`, and orders given while moving.
5. Air units (`movetp` `fly`), move height, and units on slopes.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17: flat map (all points at
  one height and layer), pathing map without obstacles, no preplaced
  units or doodads, melee initialisation removed; footman, knight,
  peasant, grunt, abomination, mortar team, gyrocopter, and footman
  copies with changed `turnRate` (`umvr`) and `propWin` (`uprw`).
  Position and facing read with `GetUnitX`, `GetUnitY` and
  `GetUnitFacing` every tick from a 0.005 s timer, as bit patterns.
- Static analysis of the 1.27b game library: the `move` order is handed
  to the movement ability, positions are advanced by a velocity between
  updates.

## Verification

1. Footman at (-3000, -2000), facing 0, `move` to (-2000, -2000), order
   from a timer in tick 295: first motion at update 1, rest at
   (-2011.9326171875, -1999.99609375) reached at tick 738 after the
   order (update 123), `GetUnitCurrentOrder` 0 from tick 739. With the
   movement arithmetic above, the positions of all 732 logged ticks are
   exact to the bit, and so are the position and velocity at all 122
   updates read from memory. (Without the velocity limit of step 5 the
   rest point is -2011.870; an implementation aiming straight at the
   target in ordinary floating point got -2011.848.)
2. The start table above for `propWin` 10, 30, 60, 90 and the `turnRate`
   values listed: first motion at the predicted update in all 40 cases.
3. Facing of a footman with `turnRate` 0.1 turning to 180 degrees:
   4.0107, 10.6987, 16.3289, 22.0670, 27.7958, 33.5255 at updates 1..6.
4. **Phase from the start of the game** (two runs, same results): six
   footmen created in `main`, `move` orders given in `main`, from a 0 s
   timer of `main`, from a 0.005 s periodic timer of `main` in its 6th,
   7th and 100th call, and from a 0.5 s timer of `main`: first motion in
   ticks 8, 8, 8, 14, 104, 104.
5. **Diagonal walk, velocity limit and facing** (test map RSF): footman
   created at (0, 0) facing 0 in tick 201 (a 1.0 s timer of `main`),
   `move` to (800, 300) given in the same tick. First update with motion
   in tick 205 (clock `1.0199990`): heading `0x3EB7B2A2`, wanted
   velocity (252.8071, 94.8080) with length 270.0040, limited velocity
   (`0x437CCDA7`, `0x42BD9CF4`) = (252.80333, 94.80655), stored heading
   `0x3EB7B64C`. Facings after updates 1 to 9: `0x3D8F5C29`,
   `0x3E570A3D`, `0x3EB246E1`, `0x3EB4E335`, `0x3EB60B7C`,
   `0x3EB6A397`, `0x3EB6FC1D`, `0x3EB7314F`, `0x3EB754CC`, then the snap
   to the stored heading of update 10, `0x3EB7B96E`. Rest at
   (788.6640625, 295.75), 12.1064 from the target, after 104 updates.
   Position, velocity and facing at every update match the game's
   memory bit for bit.
