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
the `k`-th update after the order. In all tests below the order was
given from a timer 0.5 s after the units were created, and update 1 came
6 ticks after it; how the first update depends on the phase of the
unit's update cycle was not tested.

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
difference of exactly 60 degrees with `propWin` 60 did not start the
motion.

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

### Straight lines are not exact

On a straight walk the heading does not point exactly at the target:
it changes at every update by up to about 0.33 degrees either way, so a
footman walking 1000 east drifts up to 0.05 sideways and back. The
pattern is deterministic (the same for the same start, target and
seed, also at another place with the same relative geometry) but depends
on the absolute position. Its rule is open; an implementation that aims
straight at the target will differ from the game by these small
amounts.

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
2. The rule of the small heading changes on straight walks.
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
   `GetUnitCurrentOrder` 0 from tick 739. Until the heading changes on
   straight walks are specified, compare with tolerances: the update
   count and the tick of arrival exactly, x within 0.15 and y within
   0.05. (The truncation to 1/1024 of the stored coordinate makes a step
   whose heading is slightly off the line lose up to one unit of the
   last place, about 0.001 per update, so over 123 updates x can differ
   by about 0.1; an implementation aiming straight at the target got
   -2011.848.)
2. The start table above for `propWin` 10, 30, 60, 90 and the `turnRate`
   values listed: first motion at the predicted update in all 40 cases.
3. Facing of a footman with `turnRate` 0.1 turning to 180 degrees:
   4.0107, 10.6987, 16.3289, 22.0670, 27.7958, 33.5255 at updates 1..6.
