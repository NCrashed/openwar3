---
id: SPEC-0019
title: Basic orders and the approach to an attack target
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

Movement itself (heading, `turnRate`, `propWin`, speed, arrival) is
SPEC-0018. Terms used here: an "update" is the unit's step every 6
ticks; `H` is the heading of SPEC-0018. All tests on flat open ground
with no other units in the way.

### Issuing orders from scripts

The order is taken at once: `GetUnitCurrentOrder` called right after
the `Issue...Order` native already returns the new order.

| Call | Returns | `GetUnitCurrentOrder` after the call |
|---|---|---|
| `IssuePointOrder(u, "move", x, y)` | true | 851986 |
| `IssuePointOrder(u, "patrol", x, y)` | true | 851991 |
| `IssuePointOrder(u, "attack", x, y)` | true | 851983 |
| `IssueTargetOrder(u, "attack", t)` | true | 851983 |
| `IssuePointOrder(u, "smart", x, y)` | **false** | 851971 (and the unit walks) |
| `IssuePointOrder(u, "bogus", x, y)` | false | 0, nothing happens |
| `IssueImmediateOrder(u, "stop")` | true | 0 |
| `IssueImmediateOrder(u, "holdposition")` | true | 0 |

### Stop, hold position and new orders while walking

- **`stop` and `holdposition`** end the motion at once: the position
  does not change after the call (order given at an arbitrary tick, also
  between two updates). The current order reads 0 afterwards in both
  cases.
- **A new `move` while walking** takes effect at the unit's next update.
  The unit goes on walking until then; at that update it does not move
  for one tick and then follows the start rule of SPEC-0018 from its
  current heading. Re-issuing the same `move` costs just that one tick.
  A footman walking east that got a `move` to a point behind it
  (180 degrees) stood for 25 ticks while turning, one to a point at about
  92 degrees for 7 ticks.
- **A new `move` to a point within the arrival distance** ends the order
  at that next update, without motion.
- A `move` after `holdposition` behaves like a `move` given to an idle
  unit.

### Patrol

`patrol` to a point walks there like `move`, stops at the arrival
distance of SPEC-0018, turns round on the spot by the start rule and
walks back towards the point where the unit was when the order was
given, and so on. The current order stays 851991 throughout. A footman
patrolling to a point 600 east stood 24 ticks at each end (the turn by
180 degrees).

### Attack-move without enemies

`attack` to a point with no enemy in reach behaves exactly like `move`
(the same positions tick by tick) and ends with order 0. `smart` to an
empty point behaves the same.

### Approach to an attack target

`IssueTargetOrder(u, "attack", target)` with the target out of range:

1. The unit walks towards the target by SPEC-0018.
2. It stops at the first update at which the distance between the two
   centres is at most `rangeN1 + c_a + c_t`, with `c_a` and `c_t` the
   `collision` sizes of attacker and target (`Units\UnitBalance.slk`
   or the map's object data, field `ucol`). The ordinary arrival rule of
   SPEC-0018 does not apply here.
3. The attack begins at that same update; the damage of the first hit is
   done `dmgpt1` later (`dmgpt1 / 0.005` ticks), and the next hits follow
   every `cool1` (SPEC-0016).

Distances at which the attacker stood (start 600 west of the target,
targets that cannot move):

| Attacker (`rangeN1`, `collision`) | Target collision 16 | 32 | 64 |
|---|---|---|---|
| footman (90, 31) | 130.47 (limit 137) | 146.64 (153) | 179.02 (185) |
| knight (100, 32) | 138.28 (148) | 159.24 (164) | 190.52 (196) |
| rifleman (400, 32) | 446.11 (448) | 462.31 (464) | 494.71 (496) |
| footman, collision 8 | 105.98 (114) | 122.39 (130) | 154.75 (162) |
| footman, collision 48 | 146.68 (154) | 162.84 (170) | 195.24 (202) |

Each distance is less than the limit by less than one update of motion
(8.1 for the footmen, 10.5 for the knight, 8.1 for the rifleman).

The first damage came 100 ticks (footman, `dmgpt1` 0.5), 132 ticks
(knight, 0.66) and 34 ticks (rifleman, 0.17) after the update at which
the attacker stopped; then every 270 ticks (footman, `cool1` 1.35 in
the `Custom_V1` data of a custom map) and 300 ticks (knight and
rifleman, 1.5).

### Turning before an attack

The attack begins only at an update at which the heading, before that
update's turn, differs from the direction to the target by **less than
0.2 radians** (11.46 degrees). Until then the unit turns by `turnRate`
per update without moving (when the target is already in range).

Knight (`turnRate` 0.5, 28.65 degrees per update) next to a target at
angle `a` from its heading, first damage in ticks after the order:

| `a` | 0 | 30 | 45 | 60 | 90 | 120 | 135 | 150 | 180 |
|---|---|---|---|---|---|---|---|---|---|
| ticks | 138 | 144 | 150 | 150 | 156 | 162 | 168 | 168 | 174 |

Footman (`turnRate` 0.6, 34.38 degrees per update) with `a` = 34.38 +
`d`: `d` from 9 to 11 gave 112 ticks (attack at update 2), `d` 11.5 to 16
gave 118 (update 3).

### Reaction delay

When a unit takes an attack order it first waits a random **reaction
delay** before it acts on it (walks towards the target or starts the
swing):

- `ReactionDelay` from `Units\MiscData.txt`, section `[Misc]` (0.25 s
  in 1.27b; the file describes it as the upper limit of a random delay
  in seconds).
- One output `r` of game generator 9 (SPEC-0007) gives
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)`, a value in [0, 1),
  as for `GetRandomReal`, and the delay is `mul(x, ReactionDelay)`.
- The unit acts at its first update at or after the end of the delay:
  a knight next to its target hit at `138 + 6 j` ticks after the order
  (6 ticks to its first update, 132 for `dmgpt1`), where `j` is the
  number of whole updates (0.03 s) the delay spans: delays below 6
  ticks gave `j` = 0, 6.8 to 9.2 ticks `j` = 1, 12.4 to 16.1 ticks
  `j` = 2, ... 43.2 to 46.3 ticks `j` = 7.

Verified on 40 seeds (`SetRandomSeed(s)` right before the order, one
knight per order): the delay computed from the first output of
generator 9 after the seed predicts `j` in all 40 cases (two samples
were one tick off because of the test's own counting timer). With seed
7 the delay is 0.0015 s, so `j` = 0; the turning tests above used that
seed. A unit that walks to its target for longer than its delay shows no
extra wait at the swing.

During the delay the unit does nothing: a footman walking east that got
an attack order on a target to the north stopped at once (the position
did not change from the order on) and stood 10 to 53 ticks, depending
on the seed, before it turned towards the target. It does not keep its
old course.

### Range test

Whether a unit is in range of its target is decided in the movement
system's units of 32 (SPEC-0018), in the engine arithmetic of
SPEC-0004: with `R = range / 32 + c_a / 32 + c_t / 32` (at least
0.49) and `d2` the squared distance between the two centres divided by
32, the target is in range if `d2 < R * R` or `|d2 - R * R| < 0.001`.

### Chasing a moving target

The attacker does not steer at the target's current position. It walks
a path (SPEC-0020, SPEC-0021) to a **goal point**, the target's position
at the time the goal was last taken, and takes a new goal only from
time to time:

- **Taking a goal.** The goal is the target's position at that moment.
  The chase keeps two paths: a high-level path of the chase from the
  attacker's position to the goal, and the attacker's own path to the
  current waypoint of that high-level path (chosen by the look-ahead of
  SPEC-0020, step 1), followed as in SPEC-0020 and SPEC-0021. When the
  current waypoint is the goal itself, the attacker's local search
  differs in two points: its goal point is the target's position at the
  time of that local search, and it ends at the first cell removed from
  the heap whose centre is within the attack range `rangeN1 / 32`
  (cells, not counting the collision sizes) of that point; that cell's
  centre is the end of the path. Whether the attacker starts with the
  first point of the path or walks straight to its end is decided as
  usual (SPEC-0020, step 2: straight only if the search met no
  unpassable cell). The target is moving, so it is not an obstacle for
  the search (SPEC-0021).
- **Waypoints of the chase path** are reached by the range test: at an
  update at which the attacker is within `rangeN1 + c_a + c_t` (the
  range test above) of the current waypoint, the next waypoint is taken
  by the look-ahead, the attacker stands still for that update, and it
  gets a new path from its position along the rest of the chase path.
  (Footman behind a wall: the waypoint was taken over at 134.8 from it,
  not at 142.9 one update before.)
- **A new goal and the existing path.** When a new goal is taken and it
  is the same point as before (a target that does not move), nothing
  changes. Otherwise the chase path is searched again; if its current
  waypoint is the same point as the attacker's current waypoint, the
  attacker keeps its own path, else it gets a new one at once.
- **Start in a block that is not free.** When the attacker stands next
  to a wall so that its level-0 block is not free, the high-level search
  started from the free neighbouring block nearest to the attacker (one
  case, open question 6).
- **Schedule.** Right after a goal is taken a countdown is set to
  `round(0.33 * d)` clamped to 16 .. 132 (with 0.33 the real
  `s2r("0.33")`; `d` the distance from the attacker to the new goal in
  units of 32), and it goes down by one at each of the attacker's
  updates. The next goal is taken at the update at which it has run
  out, so for distances below 48 units of 32 (1536) a new goal comes
  every 17 updates, 102 ticks. (Static analysis: 165 is added to the
  countdown in one mode of the mover that was not seen in the tests.)
- **Start of the attack.** At each update, before moving, the attacker
  stops and starts its swing if the target or the goal point is within
  the range test above (`rangeN1 + c_a + c_t`). In 15 swings against a
  goal point the distances at the swing were 129.4 to 134.5 and at the
  update before 137.5 to 142.6 (footman against a peasant, limit 137);
  one swing came an update later than this rule gives (at 136.68 the
  attacker walked on; open question 2). So a swing at a target that walks away starts from
  farther than the static limit: from 178 to 218 behind a target
  walking away at 150 (start distances 200 to 650, 9 tests), 214 at
  180, 239 at 210, 251 at 240 (static limit 152).
- **A started swing always hits:** the damage comes `dmgpt1` after the
  swing started, however far the target walked meanwhile (up to 374
  between the centres at the damage).
- **After the swing** the chase goes on at once (it does not wait for
  the cooldown). One tick after the end of the backswing (`backSw1`
  after the damage) the attacker takes a goal as above, with the
  target's position at that tick for both searches. It moves from its
  next update on, starting from its current facing as its heading
  (SPEC-0018, as for a new order). At its 12th update counted from that
  first one (66 ticks later) it takes a new goal and stands still for
  that update; then the 17-update schedule goes on. This was seen after
  every swing in every test. Behind targets it could not reach (speed
  270) it stood for one update at intervals of 96 to 102 ticks.
- A target moved with `SetUnitPosition` by 40 or more while the attacker
  walked made the attacker go straight to the new place (it took a new
  goal at once); moved by 20 it did not, the attacker stopped near the
  old place and swung from 166.6.
- Distances at which the first swing started, for comparison:

  | Target speed | Walking away | Walking across |
  |---|---|---|
  | 150 | 199.5, 209.9 | 158.9, 195.1 |
  | 180 | 214.2 | 165.8, 209.6 |
  | 210 | 238.7 | 177.1 |
  | 240 | 251.4 | 176.6 |
  | 270 | never caught | 213.9 |
  | 300 | never caught | 234.2 |

### Acquisition by idle units

An idle unit with a weapon attacks an enemy unit on its own:

- **Range.** It acquires an enemy whose centre is at most
  `acquire + c_a + c_t` away (`acquire` from `Units\UnitWeapons.slk`,
  collisions as above): a footman (`acquire` 500, collision 31) took an
  enemy footman copy (collision 31, `Player(1)`) at 562 and not at 563.
  `Player(15)` (neutral passive) is not an enemy of `Player(0)`
  (`IsUnitEnemy` false).
- **Checks.** Each unit with a weapon checks for targets on its own
  schedule: when it is created it draws one output `r` of game generator
  2 (SPEC-0007, the generator of the damage dice), with
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)` as for the reaction
  delay. Its first check is `100 * x` ticks (`0.5 * x` seconds) after
  its creation, and then every 100 ticks (0.5 s). Units without a weapon
  (a farm) draw nothing. The draws are made in the order of creation.
  `EVENT_UNIT_ACQUIRED_TARGET` fires at the check that finds a target:
  in the tick `floor(100 * x)` counted from the creation (60 of 60
  footmen with seeds 7, 8 and 9); an enemy put into range at tick 300
  or 333 was found at the next check of each unit (`floor(100 * x)` +
  300 or + 400).
- **Choice of target.** Of the enemies in range it takes the nearest
  unit, and a unit before a structure even when the structure is
  nearer (a farm 150 away lost against a footman 300 away, in both
  orders of creation; a structure alone in range was not tested). The field `prio` of
  `Units\UnitData.slk` does not matter (a footman copy with `prio` 10 at
  305 lost against a footman, `prio` 6, at 300; one with `prio` 0 at
  295 won). Between enemies at exactly the same distance it takes the
  one with the smaller y, and with equal y the one with the smaller x
  (tested with targets 300 away in the eight directions, independent of
  the order of creation).
- **Reaction delay.** After acquiring, the unit waits the reaction delay
  of attack orders (generator 9, `ReactionDelay`, see above), counted
  from the exact time of the check (`100 * x` ticks after creation, not
  the whole tick), with the draws of generator 9 in the order of the
  acquisitions. It starts to act (turn, walk) at its first update at or
  after the end of the delay: 60 of 60 footmen.
- **Then** it walks to the target and attacks it by the approach rule
  above. `GetUnitCurrentOrder` stays 0 during such an attack.

## Edge cases

- The target of these tests could not move (move speed 0) and belonged
  to `Player(15)`.
- `smart` to a point returns false although the unit obeys it.

## Open questions

1. Whether other orders use the reaction delay as well (acquisition
   does).
2. One chase in which the attacker did not start its swing at a goal
   point 136.68 away (limit 137);
   what makes the attacker take a new goal 66 ticks after a chase
   resumes; when a moved target makes the attacker take a new goal at
   once; earlier observations of an attacker following the target's
   current position (not reproduced in the chases of the verification
   below).
3. Attack-move with enemies, retaliation, `minRange`; acquisition of
   units that are not idle, of air units and by buildings with weapons;
   whether the 0.5 s period depends on any data field.
4. Queued orders, group orders, formation movement.
5. Orders on units of other players, `hold position` with enemies in
   range.
6. How the start block is chosen when the attacker's own block is not
   free (one case seen), and what the attacker does when no free block
   is near.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17 and 2026-10-18, same set-up
  as SPEC-0018; targets were footman copies with 100000 life, move speed
  0 and collision 16, 32 or 64; attackers footman, knight, rifleman and
  footman copies with collision 8 and 48. Damage times from
  `EVENT_UNIT_DAMAGED`, counted by a 0.005 s timer.

## Verification

1. The order table above.
2. Footman walking east, `stop` at tick 100 after the move order: no
   position change after tick 100, rest 126.89 east of the start.
3. The approach table: 15 of 15 attackers stopped within one update of
   motion below the limit, first damage `dmgpt1` after that update.
4. The turning tables: 12 knight and 10 footman cases.
5. **Chases.** Flat open map, `SetRandomSeed(7)` (reaction delay below
   one update), footman attackers (`rangeN1` 90, `collision` 31) ordered
   at tick 0 to attack a peasant-based target of `Player(15)` (100000
   life, `collision` 16) that is ordered at the same tick to walk 2000
   north (or east):

   | Attacker from | Target from, speed, direction | First motion | Swings start (update) | Damage | Resumes (first motion) | Stand |
   |---|---|---|---|---|---|---|
   | (-2500, -3000) | (-2000, -3400), 190, north | 5 | 322, 688 | 427, 788 | 533, 893 | 598, 958 |
   | (-2500, -6000) | (-2000, -6400), 100, north | 5 | 322, 634, 1072 | 421, 733, 1178 | 527, 839 | 592, 904 |
   | (-2500, 0) | (-2000, -400), 150, north | 47 | 358, 676 | 463, 776 | 569, 881 | 634, 946 |
   | (-2500, -6000) | (-2000, -6000), 190, east | 5 | 706 | | | |

   New goals in the first row were taken at updates 4, 106, 208, 310
   (target at (-2000, -3400), (-1998.1, -3309.2), (-1998.4, -3212.3),
   (-1998.6, -3115.4)), after the first swing at tick 528 (target at
   (-1999.1, -2908.4)) and at update 598. The local path read from memory
   at tick 20 ran from (305.875, 290.25) in 14 cell steps to the cell
   centre (319.5, 278.5) (units of 32), 2.14 from the target's position
   (321.5, 277.75). A model following this spec, SPEC-0018 and SPEC-0020
   reproduced the attackers' positions within 0.31 world units at every
   tick for the first, second and fourth rows over the whole test
   (up to 1140 ticks) and for the third row up to tick 1108, where the
   game did not yet start the third swing (open question 2).
6. **Swing start against the goal point.** 13 footmen at (-2500 -
   0.625 k, -10000 + 1600 k), k = 0 .. 12, each attacking a peasant of
   `Player(15)` at (-2000, y) that walks east to (2000, y), all ordered
   at tick 0 (`SetRandomSeed(7)`; the reaction delays differ per
   attacker): first motion and the update of the first swing:

   | k | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
   |---|---|---|---|---|---|---|---|---|---|---|---|---|---|
   | First motion | 5 | 5 | 47 | 17 | 29 | 17 | 5 | 5 | 23 | 17 | 29 | 29 | 47 |
   | Swing (update) | 706 | 706 | after 840 | 802 | 820 | 802 | 712 | 712 | 814 | 802 | 826 | 826 | after 840 |

   A model with the range test as the limit gave all eleven swings at
   the same updates and the positions within 0.1 world units.
7. **Chase round a wall.** Walls of unwalkable cells at
   -2000 <= x < -1968, y0 - 400 <= y < y0 + 400; footman at (-2500, y0)
   attacking a peasant-based target at (-1850, y0): standing (move speed
   0; y0 = -6000), walking east to (150, y0) (y0 = -3000), walking north
   to (-1850, y0 + 2000) (y0 = 0). The standing target was hit at
   tick 908 (swing from update 802), the northbound one at 721 (from
   616); the eastbound one was not caught in 1000 ticks. In the first
   row the attacker stood still at update 388 when it took over the
   waypoint (-2000, -6416) of the chase path. A model following this spec
   reproduced all three attackers within 0.015 world units at every
   tick.
8. **Acquisition.** A footman of `Player(1)` paused with `PauseUnit` at
   (0, 0), created first, and 20 footmen of `Player(0)` on a circle of
   radius 300 round it, created after it, all at map initialisation
   after `SetRandomSeed(7)`. Acquisition ticks of the circle footmen
   (counted from their creation), sorted: 3 8 18 22 26 31 36 44 45 46 48
   68 70 72 81 84 88 88 90 94; with seed 8: 1 5 13 16 19 32 33 33 40 46 67
   69 70 73 79 81 85 89 91 96. The first change of facing (start of the
   turn towards the target) of the footman acquiring at tick 3 (seed 7)
   came at tick 10, of the one at 8 at tick 10.
