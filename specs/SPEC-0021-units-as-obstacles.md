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

For a walking unit the cells that count are the footprint at its
position at the last moment its cells were moved, which is:

- for a unit alone in its cluster (SPEC-0021, "Searches of many units in
  the same tick"; a single unit's `move`, attack or gathering walk), right
  after its own update: in the rest of that tick and until its next
  update it occupies the footprint at its new position;
- for a member of a group cluster (SPEC-0025), only after the updates of
  all members of the cluster: during those updates every member still
  occupies the footprint of its previous update, also the members that
  have already had their update in this tick, and it also still counts
  as moving or standing by its velocity of its previous update (replay
  `c-RTW-3` tick 5473: footman 918 started to walk at its update, which
  came before 900's in the same cluster; at 900's check 918 still had
  velocity 0, counted as standing, and 900 was blocked instead of
  waiting).

(The grid lists a moved unit also on the cells of its previous
footprint until its next move, but the checks of this spec look only at
its current footprint.) Read in the game, the pathing grid at every
look-ahead check: `c-RXI-1` tick 853, peasant 2 (alone in its cluster)
moved from cell (346, 385) to (347, 385) at its update; at the next
unit's check of that tick the grid listed it on x 345..347, and at
peasant 3's check in 859 the cell (345, 385) tested by that check was
not counted (no wait, as in the game), while x 346..347 counted.
Replay `c-RTW-3` tick 4885, a group of seven footmen: footman 900 had
its update before footman 918, and at 918's check and at every later
check of the group in that tick 900 still occupied x 376..377,
y 376..377 (its footprint of 4879), so 918 waited 5 updates for it as
in the game.

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

- if `B` is in the same cluster as `A` (SPEC-0025), **`A` waits** as
  below, whatever the speeds and classes (replay `c-RTW-3`, ticks 4855
  to 5317: eleven such cases in a group of footmen, `A` waited in all,
  also when `A` was faster by the last digit, 71.1893 against 71.1892
  per update in cell units at 4885). A flag of the cluster turns this
  off, then the rules below apply to members as well; when it is set
  is open (it was clear in all cases measured);
- if `B`'s squared speed is at least `A`'s (both as
  `add(mul(vy, vy), mul(vx, vx))` of the velocities at the start of the
  update, SPEC-0004), or `B`'s owner field differs from `A`'s (below), then **`A`
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

During every waiting update the heading still turns as in SPEC-0018
(at most `turnRate`, the angle function of SPEC-0018 from the committed
position): at the update that starts the wait towards the local path
point it was about to step to, on the later waiting updates towards the
member's place (the cluster's waypoint plus the member's offset,
SPEC-0025). Read in the game: `c-RXR-1` unit 8 at 307 (wait start) turned
from 0 to 0.6, a full step towards its path point (366.5, 386.5)
(direction 0.6435; its place (386.5, 386.5) lies at 0.0443), and at 313
to 0.044278, the place's direction; `c-RXD-1` creep 7 at 4603 (wait
start) a full 0.5 step to 2.753035, at 4609 to the direction of its
place (399.5, 401.5), 2.7184253, not to that of its path point (415.5,
401.5), 2.4266458.

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

Units whose updates fall in the same tick are handled cluster by
cluster, the newest cluster first, and the members of a group cluster
one after the other in the cluster's order. A unit gets a new cluster,
and so moves to the front, each time it **takes a new goal**:

- when it is given an order: a `move` of a single unit, a group order
  (SPEC-0025), by the player or a script, and also the orders the game
  gives itself, such as the `move` home at the end of
  `GuardReturnTime` (SPEC-0030);
- when an attack after an acquisition or a call for help (SPEC-0019)
  starts, which is at the **end of the unit's reaction delay**, not at
  the acquisition;
- when a unit that is already attacking acquires another target: in
  the tick of that acquisition;
- after each swing of a chase (SPEC-0019, "After the swing"): one tick
  after the end of the backswing, and again at the 12th update counted
  from the first update after it (the update at which the attacker
  takes a new goal and stands).

A new cluster made before the updates of the tick (an order, or an
engine timer: end of the reaction delay, end of the backswing, the
guard return, an acquisition check) counts in that tick already: the
unit is handled first in it. A cluster made during the unit's own
update (the goal at the 12th update) counts from the next tick. When
several units take new goals in the same tick, the last one is first;
engine timers that expire in the same tick fire in the order they were
started.

Measured (test RTK): footmen `A`, `B`, `C` created in this order,
`move` given to `B`, 0.3 s later to `A`, 0.3 s later to `C`: order of
handling `B`, then `A B`, then `C A B`; a new `move` to `B` 0.9 s later
gave `B C A`. In test RTJ a group of four ordered with the mouse after
two single walkers had their orders was handled before both, its
members in cluster order, then the single walkers newest first.

In `c-RXD-1` (units 6, 7, 8 `nkob`, 9 `nkot`) the four creeps acquired
or were called in tick 1557 in the order 7, 8, 6, 9 (SPEC-0019, "Order
of the responders"). Their reaction delays ended in ticks 1565 (9),
1569 (8), 1581 (7) and 1607 (6), so from tick 1607 they were handled
in the order 6, 7, 8, 9. Other new goals of that game: acquisitions of
another target in ticks 1836, 1857, 2007, 2031 and 2207; ends of
backswing (damage tick + 120, `backSw1` 0.6 s) in ticks 2033, 2075,
2081, 2087, 2321, 2405 and 2693; the goal at the 12th update in ticks
2107, 2149, 2155, 2161, 2395 and 2479 (front from the next tick). The
guard checks of the four ran in the order they entered the fight (7, 8,
6, 9); all four were outside at the check of tick 3557, and the `move`
orders home of tick 4557 came in that order, so from tick 4560 they
were handled 9, 6, 8, 7. These rules give the order of every tick in
which two or more creeps updated from tick 1557 to their return home
(tick 5918).

Local searches share a budget of 1100 iterations (heap removals,
SPEC-0020) per window of game ticks. A search may run only when the
iterations already used in the window are at most 1100; a path that
asks for a search while the budget is used up is put at the end of a
queue and its unit stands for that update (turning towards its segment
goal). When the budget is free again the paths of the queue get their
searches in queue order. A path that had to queue also loses the
search-throttle wait of SPEC-0020.

**The window is two update cycles.** The count of used iterations is
set to 0 before every second update cycle: before the cycles with an
odd number `m` (the update cycle `m` of SPEC-0018, ticks `6 m + 1` while
the updates have not drifted), so a window holds the updates of cycles
`m` and `m + 1` for odd `m`. A search that hits the iteration limit
costs 701 (the limit 700 plus the last removal). Read in the game (the
budget table at every search, `c-RXR-1`: ten units given attack-move in
tick 301, all searching first in tick 307, cycle 51): the counts went
0, 87, 228, 400, 542, 1243 in 307, the next five paths were refused
there and again in 313 (still 1243), the count was 0 in 319 (two of
them searched, 701 each, 1402, three refused), 1402 in 325, 0 in 331
(two more), 1402 in 337, 0 in 343 (the last one). So the units started
in pairs two updates apart (first motion 308, 320, 332, 344), the order
of the queue being the order of their first refusals.

The throttle of SPEC-0020 compares the iterations of the whole window
(not only the unit's own search) with 64.

**High-level budgets.** The high-level searches of SPEC-0020 have
budget tables of their own, kept the same way (iterations used, a
budget, an iteration limit): limit 5000 / budget 800, 2000 / 300 and
400 / 900. A path whose high-level search finds the budget used up is
refused like a local one, for the whole tick. Read in replay
`c-RYP-1` (six peasants ordered to harvest, first searches in tick 307,
cycle 51): the 2000 / 300 table went 0 -> 5 -> 16 -> 2532 for peasants
5, 4 and 3 (peasant 3's high-level search cost 2516) and the other
three peasants were refused although the local table stood at 299 of
1100; the 400 / 900 table took 3 per search. In tick 313 (cycle 52)
the 2000 / 300 table was back at 0 and the other three searched (6,
13, 16), while the local table was not reset (299 -> 689), as above.

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
- A unit with another value of the field below is never made to wait;
  the unit that detects it waits instead (footman meeting a knight
  head-on waited twice for 5 updates, the knight walked round it). The
  value compared is a 4-bit field of each unit's path state, NOT the
  size class of SPEC-0020. In `c-RZH-1` it was the same for all the units
  read, foot units of size classes 1 and 2 alike. At tick 343 of
  `c-RZH-1`, footman 4 (collision 31, class 1) caught up with the
  Mountain King (collision 32, class 2), which walked in front, slower
  by one unit of the last place in squared speed. The field was equal
  for both, so the hero was made to wait 20 updates and footman 4 was
  blocked. The field reads as the unit's owning player. In `c-SBJ-1` it
  was 0 for all four heroes of player 0 (death knight `foot`, lich
  `hover`, crypt lord, dreadlord) and 0xC for the ogre and all
  kobolds, which belong to Neutral Hostile (player 12), whatever
  their size. So `foot` and `hover` are not told apart. That the knight of
  the head-on case had another owner than the footman was not
  checked, and no replay yet has units of two players blocking each
  other.
- A unit that dies stops occupying its cells in the tick of its death:
  from that moment no check counts it, and its cells leave the grid in
  the next tick (replay `c-RXR-1`, footman 4 killed by a troll's hit in
  tick 2360 of the log: its cells were marked as not counting at the
  start of the next tick handler and gone one tick later).

## Open questions

1. The high-level budgets: which search uses which table, when each
   is reset (the 2000 / 300 one was reset between cycles 51 and 52),
   and how one high-level search can cost 2516 with a limit of 2000.
2. When exactly a group cluster moves its members' cells (after its
   last member's update, or later in the tick), and the cells of a unit
   that starts or stops walking.
3. A unit blocked again soon after a block (the counter of SPEC-0020,
   open question 3), and units that keep blocking each other.
4. Units of other players, air units, buildings, and units that are
   pushed aside.
5. When the cluster flag that makes members of one cluster compare
   speeds (instead of the examining member always waiting) is set.

## Sources

- Static analysis of the 1.27b game library and the pathing grid,
  agent records and path objects read from the memory of the running
  game.
- Test maps on 1.27b under Wine, 2026-10-18: flat map without
  obstacles; walkers with `move` orders and units of `Player(15)` or
  `Player(0)` standing or walking in the way; positions logged every
  tick.
- Replay `c-RXD-1.w3g` (creeps chasing harvesting peasants), 2026-10-20:
  creation of the movement clusters, the order of the updates and the
  local paths read in the running game during playback.

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

**Crowd chase (`c-RXD-1`).** Six peasants harvesting a mine 4096 from
their town hall, four creeps (three `nkob`, one `nkot`, all speed 270,
collision 31) attacking them; no input, 45 s. `c-RXD-1.events.tsv`
lists, by game tick, every update of every unit with its position and
every wait, made-to-wait and block that the game decided; a model of
this spec, SPEC-0019 and SPEC-0029 run on the replay should give the
same events at the same ticks. Counts up to tick 9200: 59 waits of 5
updates, 17 made-to-wait of 20 updates, 54 blocks. Most of them are
between the creeps themselves (41 of the 5-update waits and 13 of the
20-update waits are a creep waiting for another creep); a creep waited
for a peasant only twice. Of the creeps' time standing after their
first hit, 8007 ticks were swings (31 swings of about 200 ticks:
`dmgpt1` 0.38 s, then `backSw1` 0.6 s) and 6390 ticks other stands,
mostly single updates (57 of 6 ticks) and 5-update waits (14 of 30
ticks); the creeps hit 37 times in 45 s.

**Seeing past a standing unit (`c-RXD-1`, tick 1585).** Creep 8 has
the local path of its search at its first update (tick 1573), made
towards the position peasant 5 had when the creep's reaction delay
ended (tick 1569, SPEC-0019), from its start (260, 520) through (240, 496), (208, 464),
(176, 432), (176, 400), (144, 400), (112, 400), (80, 368), ... (world
units, start first). The search met the cells of kobold 6, which stands
at (100, 450) until its own delay ends (tick 1607) and occupies cells
x 386..387, y 397..398. So the target is (240, 496) (SPEC-0020, step
2). At tick 1585 the creep is at (250.528, 507.040), within 0.49 cells
of it, and the farthest-point rule runs: (208, 464), (176, 432),
(176, 400) and (144, 400) can be seen; (112, 400) cannot (the fourth
sample enters cell (388, 397) with a step to the lower left, and the
corner rule of size class 1 needs cell (387, 397), occupied by kobold
6). The new target is (144, 400), which lies on the same diagonal as
(208, 464) and (176, 432), so the creep walks straight down the
diagonal. Read in the game: the target was the first point after the
start at tick 1573 and the fifth from tick 1585. Without kobold 6 every
point of the path could be seen and the target would have been the
last point of the list.
