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
updates come before script timers, so an order given from a timer is
taken at the next update tick after the order's tick, also when the
order's tick is itself an update tick:

| Order given at timer tick | 0 | 1 | 2 | 3 | 4 | 5 .. 9 | 10 |
|---|---|---|---|---|---|---|---|
| First update at timer tick | 4 | 4 | 4 | 4 | 10 | 10 | 16 |

(Timer ticks counted by a 0.005 s periodic timer started at map
initialisation, with tick 0 the moment of the first orders.) An order
0.5 s after that moment (timer tick 100, itself an update tick) is
taken at tick 106, 6 ticks later, which is the case of the tests below.

### Two angles: heading and facing

A unit has a **heading** `H`, the direction it moves in, and a separate
**facing** `F`, the angle that `GetUnitFacing` returns and the model is
drawn with.

- At each update the heading turns towards the direction of the target
  by at most `turnRate` radians (footman 0.6 rad = 34.38 degrees per
  update). If the remaining difference is smaller, it becomes equal to
  the target direction. It turns the shorter way.
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
it lags behind the exact time (after 305 ticks it is `1.5249985`). The
elapsed time between two clock values is `e = sub(T1, T0)`, taken as 0
when `|e| < 0.0001` (`0x38D1B717`). (The clock is kept as a count of
300-second periods plus this value; the change of period was not
observed.)

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

**Update.** At each update of a walking unit, after the commit:

1. The current heading `H` is the angle of `v`, or the stored heading
   when the speed `sqrt(add(mul(vx, vx), mul(vy, vy)))` is below
   `0.0000004` (`0x34D6BF95`), for instance when the unit stood still.
   The heading is therefore not kept from one update to the next: it
   is recomputed from the velocity, and this round trip through `cos`,
   `sin` and `acos` is what makes it wander.
2. `A` = angle of `sub(target, P)` (per coordinate), `d = sub(A, H)`;
   if `d >= 0` and `d > pi` then `d = sub(d, 2 pi)`; if `d < 0` and
   `-d > pi` then `d = add(d, 2 pi)` (`pi = 0x40490FDB`); `d = 0` if
   `|d| < 0.0000002` (`0x3456BF95`).
3. The unit moves in this update if `|d| < propWin` and
   nothing else stops it (SPEC-0019, SPEC-0020); its speed is then
   `spd` (no acceleration was observed), otherwise 0. `propWin` is
   compared in radians, `mul(propWin, DEG)` with `DEG` of SPEC-0011:
   the game held `0x3F860A91` for `propWin` 60 (read from memory), one
   unit of the last place below the nearest float to pi / 3. The test
   is strict: `|d|` equal to `propWin` does not move.
4. The new heading is `H = add(H, d)` if `|d| <= turnRate`, else
   `add(H, turnRate)` or `add(H, -turnRate)` with the sign of `d`. It is
   also stored. (It may leave `[0, 2 pi)` by a little; `sin` and `cos`
   are periodic.)
5. With speed `s > 0` the wanted velocity is
   `w = (mul(s, cos H), mul(s, sin H))` and the new velocity is
   `v = add(v, sub(w, v))` per coordinate; with speed 0 it is `(0, 0)`.

So a unit standing still (at an update where it must not move) still
turns its heading towards the target of that update.

Example, footman at (-3000, -2000) facing 0, `move` to (-2000, -2000),
first update 6 ticks after the order: the first target angle is not 0
but 0.31338 degrees (`sqrt` of `mul(x, x)` is not exactly `x`), and the
stored positions relative to the corner at ticks 12, 18, 24, 30 are
(9296.099609, 10288.043945), (9304.199219, 10288.016602),
(9312.298828, 10287.977539), (9320.398438, 10287.990234) exactly.

Rarely the game commits the position also between two updates (one
case at tick 115 of this example); the trigger was not found (it is
not every crossing of a cell or half-cell line). Such a commit can
shorten a step by one unit of the last place (1/1024 here); see Open
questions.

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
2. (Resolved: see Movement arithmetic.) Open: the trigger of the
   occasional extra position commits between updates, and where the
   game clock starts relative to the map script (the per-tick positions
   of the example came out the same for any clock value at the order
   between 0 and about 3 seconds).
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

1. Footman at (-3000, -2000), facing 0, `move` to (-2000, -2000): first
   motion at update 1, per-tick positions as above, rest at
   x = -2011.9326 reached at tick 738 after the order (update 123),
   `GetUnitCurrentOrder` 0 from tick 739. With the movement arithmetic
   above the positions of every tick up to tick 116 are exact; the
   occasional extra commits of the game then cost up to about 0.06 in x
   by the end, so compare the update count and the tick of arrival
   exactly, x within 0.1 and y within 0.05. (An implementation aiming
   straight at the target in ordinary floating point got -2011.848.)
2. The start table above for `propWin` 10, 30, 60, 90 and the `turnRate`
   values listed: first motion at the predicted update in all 40 cases.
3. Facing of a footman with `turnRate` 0.1 turning to 180 degrees:
   4.0107, 10.6987, 16.3289, 22.0670, 27.7958, 33.5255 at updates 1..6.
