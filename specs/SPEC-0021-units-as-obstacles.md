---
id: SPEC-0021
title: Units as obstacles for path finding and following
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, process memory, test maps)"]
---

## Behaviour

This spec extends SPEC-0020 (path finding around static obstacles) to
other ground units standing or walking in the way. Terms (cells, size
classes, footprints, local search, look-ahead check, seeing a point,
updates, segment goal) are those of SPEC-0020 and SPEC-0018.

### Occupied cells

Every ground unit occupies the cells of its footprint (the table of
SPEC-0020, by the size class of its own `collision`) placed at the cell
of its position. This was read from the pathing grid in memory for
units of collision 0, 8, 16, 24, 31, 32, 40, 48 and 64, each at six
positions with different fractions of a cell: in all 54 cases the
occupied cells were exactly the footprint at the cell of the position.

For a walking unit, the position that counts is its position at its
own last update: a unit that has not had its update yet in the current
tick still occupies the cells of its previous update.

A unit with a nonzero velocity counts as **moving**. A unit that stands
still, also one that waits during a walk, has velocity 0 and counts as
standing.

### What each check sees

- The **high-level search** ignores units.
- The **local search** and **seeing a point** (string pulling) treat the
  occupied cells of standing units as not passable and ignore moving
  units.
- The **look-ahead check** (SPEC-0020, step 3) treats the occupied cells
  of all other units, moving or not, as not passable.
- A unit never blocks itself.

A local search that met an occupied cell counts as having met an
unpassable cell (SPEC-0020, step 2).

### Blocked by a unit

When the look-ahead check of a unit `A` fails, the units occupying the
failing cells are examined in turn. Units that stand are skipped here
(they only make the check fail), and so are units that are themselves
waiting for another unit at that moment. For a moving unit `B`:

- if `B`'s squared speed is at least `A`'s (both as
  `add(mul(vy, vy), mul(vx, vx))` of the velocities at the start of the
  update, SPEC-0004), or `B`'s size class differs from `A`'s, then **`A`
  waits**: it stands still at this update and the next 4 updates
  (5 updates in all), keeping its path, and then goes on;
- otherwise (`B` is slower and of the same class) **`B` is made to
  wait**: `B` stands still for its next 20 updates, keeping its path;
  the examination goes on with the next unit.

If no unit made `A` wait, `A` is blocked as by a static obstacle: it
stands still at this update and searches again at the next one
(SPEC-0020), now seeing the units that stand. A counter of such
consecutive blocks decides when the unit reacts differently; it draws
the script generator (SPEC-0025, "Blocked units: the patience
counter").

During every waiting update the heading still turns as in SPEC-0018:
towards the current target at the first update of a 5-update wait,
towards the segment goal on the later waiting updates.

Speeds that are equal by `spd` differ slightly through the movement
arithmetic of SPEC-0018. In a head-on meeting of two footmen on an
east-west line, the one walking west had the larger squared speed
(72899.961 against 72899.930) and made the eastward one wait 20
updates.

### Path to a point that a unit stands on

If the goal cell of a local search is occupied by a standing unit, the
search cannot reach it and gives the path to the closest node
(SPEC-0020). At the end of such a path the unit stands for one update
and searches again (SPEC-0020, partial path end); by then the other
unit may have left.

### Searches of many units in the same tick

Units whose updates fall in the same tick are handled by the order
they were last given, the most recent order first. Every `move` order
of a single unit, and every group order (SPEC-0025), forms a cluster;
the clusters are handled newest first, and the members of a group
cluster one after the other in the cluster's order. A new order puts
the unit (or the group) at the front. In the tests of this spec the
orders were given in the order of creation, so the last created unit
came first. Measured (test RTK): footmen `A`, `B`, `C` created in this
order, `move` given to `B`, 0.3 s later to `A`, 0.3 s later to `C`:
order of handling `B`, then `A B`, then `C A B`; a new `move` to `B`
0.9 s later gave `B C A`. In test RTJ a group of four ordered with the
mouse after two single walkers had their orders was handled before
both, its members in cluster order, then the single walkers newest
first.

Local searches share a budget of 1100 iterations (heap removals,
SPEC-0020) per window of game ticks. A search may run only when the
iterations already used in the window are at most 1100; a path that
asks for a search while the budget is used up is put at the end of a
queue and its unit stands for that update (turning towards its segment
goal). When the budget is free again the paths of the queue get their
searches in queue order. A path that had to queue also loses the
search-throttle wait of SPEC-0020. In the test below the next window
started at the next update of the units; the exact window length was
not measured (the budget table read from memory has the counter reset
every second call of a per-tick routine).

The throttle of SPEC-0020 compares the iterations of the whole window
(not only the unit's own search) with 64.

### Groups

Members of a group that are neither held nor already walking are
marked while the group moves, and marked units are ignored by local
searches and seeing a point (SPEC-0025, "Members in each other's
searches"). Held members are standing obstacles. The look-ahead check
treats members of the walker's own cluster like any other unit; the
cells it tests are only those of SPEC-0020, step 3. Example
(`c-RSM-2.w3g`, update at movement clock 5.309854): member 1 is at
(383.052, 381.806) cells, its cell is (383, 381), the cell one cell
length ahead towards its target is (383, 380), so the step is to the
south only and the tested cells are (382, 379) and (383, 379). Member
2, at (384.865, 381.832), occupies x 383..384, y 380..381, which is not
tested: the check is free and nobody waits.

## Edge cases

- A unit that is itself waiting for another unit is never made to wait
  by a third one, and does not make it wait either: it is skipped as a
  blocker.
- A faster unit that catches up with a slower one of the same class
  walking in front makes the slower one stand for 20 updates and walks
  round it (footman behind a peasant).
- A unit of another size class is never made to wait; the unit that
  detects it waits instead (footman meeting a knight head-on waited
  twice for 5 updates, the knight walked round it).

## Open questions

1. The length of the budget window and the budgets of the high-level
   search (static analysis: three further budget tables with 800, 300
   and 900 iterations).
2. In one memory read a walking unit occupied the union of the
   footprints at its last update and at its current position; the
   trajectories fit the footprint at the last update only. The exact
   moment the cells change is open.
3. A unit blocked again soon after a block (the counter of SPEC-0020,
   open question 3), and units that keep blocking each other.
4. Units of other players, air units, buildings, and units that are
   pushed aside.

## Sources

- Static analysis of the 1.27b game library and the pathing grid,
  agent records and path objects read from the memory of the running
  game.
- Test maps on 1.27b under Wine, 2026-10-18: flat map without
  obstacles; walkers with `move` orders and units of `Player(15)` or
  `Player(0)` standing or walking in the way; positions logged every
  tick.

## Verification

Units face 0 when created; the orders are given at tick 0, the first
update of every unit is at tick 4 (tick 10 when its search had to
queue). Positions are world coordinates.

**Standing units in the way.** Footman walkers from (-2500, y) to
(-1500, y), a standing unit at (-2000, y + d):

| Standing unit | d | Walker |
|---|---|---|
| footman, `Player(15)` | 0 | detours north, aiming at (-1968, -5936) from tick 10 (y = -6000 row), rests at (-1511.64, -5998.47), last move tick 743 |
| footman, `Player(0)` | 0 | the same; the standing unit does not give way |
| collision 48 copy | 0 | blocked at tick 298 at (-2110.18, -132.36) (row y = -200), heading turns to the order point at that update, next update turn only, moves again from tick 311 |

**Shared budget (test UO2).** 24 units created in this order, in
pairs: for `k = 0 .. 11` first a footman walker at (-2500, `y_k`) with
`move` to (-1500, `y_k`), then a standing unit of `Player(15)` at
(-2000, `y_k + d_k`), with `y_k = 1800 k - 11000`, `d_k` = 0 for even
`k` and 37 for odd `k`. The standing units are footman copies that
differ only in `collision`: 8 (`k` = 0, 1), 16 (2, 3), 32 (4, 5),
48 (6, 7), 64 (8, 9) and 0 (10, 11). All orders at tick 0, flat open
map. Handled from the last created, the first local searches of the
walkers `k = 11, 10, ..., 4` take 33, 63, 143, 256, 143, 256, 61, 156
iterations, 1111 together after the walker `k = 4`; the walkers
`k = 3, 2, 1, 0` queue and start moving at tick 11 instead of 5. Their
trajectories then agree with the single-walker results (largest
difference 0.14 world units for `k` = 0 to 3 when started at tick 10).

**Walking units (one test, four pairs, rows 3000 apart).**

| Pair | What happens |
|---|---|
| footman east from (-2500, -6000) and footman west from (-1500, -6000) | the westward one (handled first, faster by the arithmetic) makes the eastward one wait: it stands from tick 364 to 484 at (-2022.15, -6000.01); the westward one stands from 364, searches again, walks round north |
| footman east from (-2500, -3000), footman north from (-2000, -3500) to (-2000, -2500) | the northward one makes the eastward one wait from tick 352 to 472 at (-2038.34, -3000.03) |
| footman east from (-2500, 0), peasant east from (-2300, 0) to (-1300, 0) | the footman catches up, makes the peasant wait from tick 280 to 400 at (-2043.54, 0), walks round north |
| footman east from (-2500, 3000), knight west from (-1500, 3000) | the footman waits twice for 5 updates (ticks 316 to 376 at (-2079.42, 2994.36)), the knight walks round |

Ticks during which each unit of this test stood still in the game
(first tick without motion .. first tick with motion again; the first
interval is the start, the last one the end of the walk):

| Pair | Unit | Standing intervals |
|---|---|---|
| 1 | eastward footman | 1..11, 364..485, from 869 |
| 1 | westward footman | 1..29 (turning), 364..377, 394..437, 460..467, from 863 |
| 2 | eastward footman | 1..11, 352..473, from 863 |
| 2 | northward footman | 1..11, 352..365 |
| 3 | footman | 1..11, 274..281, from 760 |
| 3 | peasant | 1..11, 280..401, 406..467 |
| 4 | footman | 1..5, 316..377, 760..767, from 803 |
| 4 | knight | 1..29 (turning), 322..341, 352..359, 376..389, from 676 |

The westward footman's stand 394..437 is a block followed by the search
throttle of SPEC-0020 (its previous search took 79 iterations), and
460..467 a turn without motion by `propWin` (SPEC-0018); the knight's
stands after its blocks are such turns as well.

With the movement arithmetic of SPEC-0018 a model following this spec
reproduced the trajectories of the first and fourth pairs within 1.2
world units at every tick (the knight exactly). In the other pairs the
decision between "wait" and "make wait" came down to one unit of the
last place of the squared speeds, which needs the occasional extra
position commits of SPEC-0018 (open there) to be reproduced.
