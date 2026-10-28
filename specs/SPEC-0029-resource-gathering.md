---
id: SPEC-0029
title: Gathering gold and lumber (Human peasant)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps logged every tick)"]
---

## Behaviour

This spec covers a Human peasant (`hpea`) gathering gold from a gold
mine (`ngol`) and lumber from trees (summer tree wall `LTlt`), and
bringing it to a town hall (`htow`) or a lumber mill (`hlum`). Ticks
and the update cycle are those of SPEC-0018. It is a first version: the
cycle, its timing and the numbers are given; the exact geometry of the
approach is given by measured examples (open questions).

### Data

| Value | Field | Peasant |
|---|---|---|
| gold carried per trip | `DataC` of the harvest ability `Ahar` | 10 |
| lumber per hit and per trip | `DataA`, `DataB` of `Ahar` | 1 and 10 |
| time between two hits on a tree | `Dur` of `Ahar` | 1.1 s |
| reach to a tree | `Rng` of `Ahar` | 116 |
| radius to look for another tree | `Area` of `Ahar` | 900 |
| gold of a new mine | `DataA` of the mine's ability `Agld` | 12500 |
| units inside a mine at a time | (measured) | 1 |
| time inside the mine | (measured) | 1.0 s |

A building can take resources back when it has a return ability: the
town hall `Argl` (gold and lumber), the lumber mill `Arlm` (lumber
only). A tree is a destructable with 50 life; its life is the lumber
left in it.

### Orders

- `harvest` (852018) on a gold mine or a tree: from a script
  `IssueTargetOrder(u, "harvest", target)`; from the interface the
  gather button (`Ahar`, at (3,1)) or a right click on a mine or tree.
- While the peasant carries resources back, its current order is
  `resumeharvesting` (852017); when the resources are delivered it
  becomes `harvest` (852018) again and the peasant goes back to the same
  mine or tree.
- `returnresources` (852020) is the order of the interface's return
  button (not studied).

### Reaching a building or a tree

- **A building** (gold mine, town hall, lumber mill) is reached through
  the end of the worker's path (below, "The path to a building"): that
  path ends where its search touched the building's red cells, short of
  its goal, so at the update at which the worker arrives at its last
  point (within 0.49 cells, SPEC-0020, step 3) it stands still for that
  update as at the end of a local path that did not reach its goal, and
  at its next update it enters the mine or gives its gold. In `c-RXI-1`
  each of the 47 entries and deliveries after a walk had this stand
  6 to 9 ticks before the change of order the script read (the stands
  are the `blocked` events of `c-RXI-1.events.tsv`; the script reads
  the order up to a few ticks late). Peasant 5
  was 0.496 cells from the end of its path at its update of tick 859
  (not arrived; it walked on), 0.318 at 865 (arrived: stand) and
  entered at 871, although its footprint was already next to a red cell
  at 859. A worker that arrives while another is in the mine stands
  there with no further update of its path and no search until the
  mine is free (peasant 4: arrived at 865, entered at 1071, in the tick
  peasant 5 left).
  The step after the stand needs the walk's heading (SPEC-0018) turned
  to the building's point. At the update of the stand the heading turns
  towards the direction from the worker to the building's point by at
  most `turnRate` (0.6). At each following update the remaining angle is
  tested: within a small tolerance `T` the worker has arrived (it goes
  in, starts to wait or delivers); otherwise it turns again and stands
  for that update. Never in the update of the stand itself. Read in the
  game: `c-RXI-1` peasant 5, 0.2748 rad off at the stand (q 4.319925),
  0 at the next update (4.349923), went in; peasant 4, 0.0202 off at
  the same stand, arrived at the next update; peasant 0, 0.6596 off at
  the stand of 5.339851, 0.0596 left at 5.369849 and arrived there;
  `c-RXD-1` peasant (950, 953) at the mine, 0.971 off at the stand of
  21.749657, 0.371 at 21.779655 (not arrived, turned again), 0 at
  21.809654 (arrived, started to wait). So `T` lies in [0.0596, 0.371)
  (its value not read). The visible facing is not tested.
  The red cells are the inner 8 x 8 of the gold
  mine's `16x16Goldmine` (the outer ring blocks building only), the
  inner 12 x 12 of the town hall's `16x16Simple` and the whole 10 x 10
  of the lumber mill's `10x10SimpleSolid`, placed by SPEC-0022
  (snapping and placement). Distances from the building's centre at
  which the peasant entered or delivered in the tests of this spec:
  for a mine at (cx, cy) 190.9 from the east and 190.4 from the north,
  156.8 from the west and 156.4 from the south, 263.3 from the
  north-east and about 218 from the other diagonals; at a town hall
  218.8 / 219.3 coming from the west / south and 252.9 / 253.3 from the
  east / north.
- **The path to a building.** The worker's walk to a mine or a town
  hall searches its path as a chase searches towards its target
  (SPEC-0019, "touches the target"): the goal is the building's
  (snapped) position, and the local search stops at the first expanded
  cell whose neighbour steps test a cell of the building's red
  footprint (for the class-1 worker: the square `x - 2 .. x + 1`,
  `y - 2 .. y + 1` around the expanded cell `(x, y)` shares a cell with
  the red cells). That cell's centre ends the path. The building's own
  red cells do not count as an unpassable cell met by the search, so
  with nothing else in the way the worker walks straight to the end of
  the path from its first update (SPEC-0020, step 2). Measured in
  `c-RXI-1` (mine at (-896, 0), red cells x 352..359, y 380..387; read
  in the running game): the six first searches in tick 307 stopped at
  the expanded cells (351, 385) or (351, 383), world (-1040, 48) and
  (-1040, -16); five met no unpassable cell and their workers walked
  straight east from their updates of tick 307; the sixth (peasant 3)
  met the cells of the standing peasant beside it at the start and
  walked to the first point of its path. The high-level search of such a walk
  (SPEC-0020) does not see the building it goes to either: its
  footprint is free for that search, so on open ground the high-level
  path runs straight to the building's centre and the first local
  search is already the one that touches the building (`c-RXI-1`: the
  high-level paths of the six were the start, a point 1.4 to 3.4 blocks
  from it and the mine's centre). For every other walk the building is
  an obstacle of both searches (SPEC-0022): a footman ordered to `move`
  from (-1500, 0) to (-300, 0) through the same mine (test RXK) got a
  high-level path to the north side of the mine and a local path along
  it that met the red cells.
- **A tree** is reached at the first update at which the distance
  between the peasant's position and the tree's is at most
  `Rng + 16` = 132 (the reach of `Ahar` plus the peasant's collision
  size 16): in 16 approaches from 8 directions the distance at the
  first hit was between 128.0 and 132.0.

### Gold

1. The peasant walks to the mine (SPEC-0018, SPEC-0020). When it
   reaches it and the mine is free, it enters: it is hidden
   (`IsUnitHidden` true) and stays where it stood. In test RUF (mine at
   (-4096, -5120), the peasant coming from the north) it entered at
   (-4080.40, -4932.93), 187.7 from the mine's centre.
2. It stays inside for 1.0 s: a timer of the main queue (SPEC-0002) is
   started at the moment of the entry (its own update when it walks
   in; for a worker taken from the queue of a busy mine, the moment the
   leaving worker's timer fires), and it comes out in the tick in which
   the queue reaches that time plus 1.0, so 200 or 201 ticks (in
   `c-RXI-1` peasant 4 was taken in at q 53.8084335, in tick 10765, and
   came out in tick 10966), then appears again at the
   same position, carrying 10 gold, with order 852017; at that moment
   the mine's gold (`GetResourceAmount`) drops by 10.
3. It walks to the nearest building that takes gold and delivers when
   it reaches it: the player's gold grows by 10 and its order becomes
   852018 (in RUF at (-4080.02, -4317.42), 221.4 from the centre of the
   town hall at (-4096, -4096)). Then it walks back to the mine.

The cycles of one peasant on that layout repeated with the same
numbers: entering the mine in ticks 568, 2135, 3485, 4835, 6185, 7536
(after the first, every 1350 or 1351 ticks), leaving 200 ticks after
each entry, delivering 682 or 574 ticks after leaving.

**Several peasants on one mine.** Only one peasant is inside at a
time. The others wait where they reached the mine (order 852018, not
hidden); when the one inside leaves, the next one enters in the same
tick, in the order in which they reached the mine (test RUJ: three
peasants from the west, north and east entered in the order of their
arrival, ticks 430, 630, 831). With five peasants ordered in the same
tick (test RUF) the mine lost 10 gold every 200 ticks.

**Where a worker comes out.** With `(cx, cy)` the cell of the point
where the worker entered:

1. If its footprint (SPEC-0020, class 1: cells `cx-1 .. cx`,
   `cy-1 .. cy`) at that cell is free, it comes out exactly at the
   point where it entered.
2. Otherwise rings `n = 1, 2, ...` (up to 32) around that cell are
   tried, with `L = cx - n`, `R = cx + n`, `B = cy - n`, `T = cy + n`,
   in this order: the bottom row `(L + 1, B)` to `(R, B)`, the right
   column `(R, B + 1)` to `(R, T)`, the top row `(R - 1, T)` down to
   `(L, T)`, the left column `(L, T - 1)` down to `(L, B)` (the order
   of SPEC-0027 that starts with the bottom side). The first cell whose
   footprint is free is taken, and the worker comes out at its centre.

A footprint is free when its cells are passable (the mine's red cells
are not) and not covered by the footprint of a unit standing there; a
worker waiting at the mine and the worker inside it (at the place where
it entered) both count. Read in the game (`c-RXI-1`: the searched point,
the order of the sides and the result of the search): peasant 4 entered
at (-1052.06, -17.07) (cell (351, 383)), and when it came out in tick
1271 peasant 1 stood on that cell, peasant 2 on (351, 385) and peasant 0
was inside the mine at (351, 381); ring 1 has no free cell, ring 2
gives (349, 385), and it came out at (-1104, 48). Peasants 5 (tick 2866)
and 3 (tick 5380) entered at cell (351, 385) where the worker that had
just entered the mine stood; ring 1 has no free cell, ring 2 gives
(350, 383) first, and both came out at (-1072, -16). (Test RUF: entered
at (-6162.17, -5957.45), came out at (-6192, -5936).) The units that
make a footprint not free here are those that would close the cells for
a local path search of a ground unit (SPEC-0021): standing and walking
units, a worker on its first walk to the mine and any waiting worker;
not a worker of the gathering cycle that walks (it blocks no local
search, below) and not a worker hidden in the mine. `c-RXD-1` tick
4191: worker 4 came out from its entry cell (443, 385) while worker 2
waited on that cell and worker 0, on its first walk to the mine,
walked past with its cells at (440..441, 381..382): ring 1 had no free
footprint, in ring 2 (442, 383) met worker 0's cell (441, 382) and
(443, 383) was taken: out at (1904, -16). `c-RXI-1` tick 3497: peasant 3
came out on its entry point (351.1182, 385.5313) although peasant 2, in
the gathering cycle and walking to the mine, had its cells in that
footprint.

The footprint is tested cell by cell, rows from the bottom, each row
from the left; the first closed cell ends the test. A unit's cells are
the 2 x 2 cells from `floor(x - 1)` to `floor(x + 1) - 1` (and the same
in y) for its position `(x, y)` at its last commit.

**An empty mine.** When the last gold is taken (the mine had 30 in
test RUJ: three trips), the mine's gold is 0 as the peasant leaves it
and the mine is gone; the peasant delivers its 10 gold and then stands
with order 0.

**Arriving at the tree.** Each time a lumber worker's walk to its tree
ends (the arrival test of SPEC-0019, by reach, by touching the tree or
by the give-up mark of SPEC-0025), the harvest checks the place. The
check passes when either of two tests passes:
- the range test: the distance from the worker to the tree's point
  against `Rng1` of `Ahar` / 32 plus the worker's collision in cells
  (116 / 32 + 0.5 = 4.125 cells; the tree's size does not count);
- the touch test of the path code (SPEC-0025, "Blocked units"): the
  ring of cells around the worker's cell meets a cell of the tree.
The harvest also keeps a "ready" mark, which is clear at every arrival
at a tree (it was clear at each one read: first walks, walks to
another tree, walks back from a delivery). Then:
- passes, mark clear: the retry counter is set to 3, the worker gets
  the walk to the same tree again (this is where its search set changes,
  "Entering the gathering cycle"), the mark is set at once and the
  counter drops to 2. A worker already at its place arrives again at
  its next update (6 ticks later), the check passes again and the work
  starts. So a worker that arrives within range starts the work one
  update after its arrival, never in the arrival update;
- passes, mark set: the work starts. The swing starts at the queue time
  of that update; the first hit (swing point) is 0.433 s later, the
  backswing ends 0.567 s after the hit, the next swing starts 0.1 s
  later, i.e. one hit every 1.1 s (220 ticks) on queue times;
- fails, counter 0: the worker drops the tree, and 0.5 s later it takes
  another tree ("Another tree" below);
- fails, counter above 0 (not seen in the replays read): the walk to
  the tree is given again and the counter drops by one.
Read in the game, `c-RYP-1` (ticks of queue times, Q(k) < e <= Q(k + 1)):
- peasant 0, first walk: arrives at 997 (range passes), walks again,
  check at 1003 (q 5.0099878) passes, the swing starts; hits at q
  5.4429874 = tick 1089, then 1309, 1529, ..., 3069 (ten hits, every
  220 ticks); the tenth hit's backswing ends at q 15.9099751 = tick
  3183, where the walk back is ordered. Second load: arrives at 4789,
  starts at 4795 (q 23.9695663), hits from 4882, walk back at 6976;
- peasant 5: arrives at tree 27 at 1309 by touch, 4.4925 cells from
  the tree's point (range fails, touch passes): walks again (a new
  search at 1315), arrives within range at 1351 and starts there (its
  mark was set at 1309);
- peasant 4: arrives at tree 22 at 1243 by the give-up mark, 4.4871
  cells from the point, both tests fail: drops the tree; 0.5 s later
  it takes tree 14, arrives at 1351 (range passes), starts at 1357;
- peasant 1 as peasant 0: arrives at 1231, starts at 1237;
- peasants 2 and 3 as peasant 4: both tests fail at 1471 and 1807,
  another tree 0.5 s later, arrivals there at 1579 and 1915, starts at
  1585 and 1921.

**Another tree.** When the arrival check fails with the retry
counter at 0 (above), the harvest takes another tree 0.5 s later: among the trees within `Area1` of
`Ahar` (900) of the current tree's point, the current one excluded and
only harvestable ones, the one with the smallest high-level path cost
from the worker. The cost of a candidate is a high-level search
(SPEC-0020) from the worker to it, with the candidate's own cells and
the worker's counted as free and an iteration limit (32 at first, then
after each success the smaller of the limit and that search's removal
count plus 16; a search that fails or hits the limit drops the
candidate); the cost is the length of the found path, the sum over its
legs of `isqrt(4 * (dx^2 + dy^2))` with `dx`, `dy` the difference of
the two nodes' coordinates in the high-level grid, i.e. the integer
square root of the squared leg length in cells (a leg ending at a node
of a special kind counts 2 instead; none occurred in the cases read),
or the same for the start-goal distance when both lie in one node. The
candidates are visited through the grid of 256 x 256 cells of
SPEC-0019 ("Order of the responders"): rows from the bottom, each row
from the left, and in each cell the tree created last first. The first
candidate with the smallest cost is kept (later equal costs do not
replace it), and the worker gets a harvest order on it. `c-RYP-1` peasant 4 (read in the game): arrived at
tree 22 in 1243 out of reach, at 1343 about 30 candidates with costs 4
to 32, the first with cost 4 was the tree at (358, 378); it turned to
it at 1351 and started chopping it at 1357.
Each search runs to the candidate's own point, its cells and the
worker's counted as free, and in every case read it found that point
in one straight leg. So the cost was the straight-run cost from the start
to the tree's point, `isqrt(4 * (dx^2 + dy^2))` with `dx`, `dy` in
high-level grid units (2 cells). `c-RYP-1` peasant 0 after its delivery
at 11578 (search from the felled tree 2's point, high-level (177, 191)):
all 34 candidates had exactly that cost, from tree 1 (177, 190) and
tree 9 (178, 191) with 2 to tree 34 (181, 195) with 11; tree 14 (179,
189) had 5, so tree 9, the first of cost 2 in the visiting order (after
trees 16, 15 and 14), was kept.

**When the tree is gone.** A tree felled while a worker works at it
(by that worker's hit or another's) ends the work of every worker at
it at the end of that worker's own backswing (the hit's backswing,
0.567 s after its hit; the hit itself lands on the tree as usual). At
that moment the worker takes another tree by the rule above, the
search starting from the worker, and gets a harvest order on it: a
walk, ended by the arrival test, then the arrival check with its ready
step as always ("Arriving at the tree"). The swing cycle's 0.1 s pause
started at that backswing end goes on: a worker that has started the
work on the new tree by then (both checks passed at its next updates,
the worker not having to move) swings at the end of the pause as in the
cycle; one that has to walk starts its swing when its work starts (the
swing point 0.433 s later). The load carried so far stays. The tree the
worker returns to after a delivery is not changed by this switch: it is
still the felled one, so after the delivery the worker takes another
tree by the rule above with the search starting from the felled tree's
point (the worker is at the hall), and walks there.
Read in the game, `c-RYP-1` (trees numbered in creation order, from 0):
peasants 0 and 3 worked at tree 2; peasant 3's hit at 10010 felled it.
Peasant 0 (hit at 9996, backswing end 10109): another tree at 10109,
tree 1 (the first of cost 4, from the worker), arrival checks at 10119
(ready step) and 10125 (start), swing at 10129 (the end of the 0.1 s
pause), hit at 10216 on tree 1. Peasant 3 (backswing end 10123): tree
1 too, walked, arrival checks at 10167 and 10197, swing at once at
10197, hit at 10284. Peasant 0 after its delivery at 11578: candidates
costed from tree 2's point (cost 2 for the trees in the next
high-level nodes); the first of cost 2 in the visiting order was tree
9, so it walked to tree 9 (not to tree 1, which it had chopped since
10216). There its arrival check failed (both tests) at 12580, and 0.5 s
later (12680) it took tree 1 again, starting at 12700.

**Entering the gathering cycle.** A gold worker enters the cycle at
its first exit from a mine (below). A lumber worker changes its set of
closing kinds for searches (SPEC-0020, "Trees in the local search") in
two steps: at the first walk again to its tree (the arrival check
passing with the ready mark clear, "Arriving at the tree") it adds the gathering kind to
the ground kind; at the end of its first work at a tree (the walk back
with its load) it changes to the gathering kind only, and it is set to
that again after each delivery. Read in the game, `c-RYP-1`: peasant 5
searched with the first-walk set up to 1081 and with both kinds from
its retry at 1309 (its search of 1315); peasant 0: ground kind until
997, both from its retry at 997, the gathering kind only from 3183
(end of its first work) and again from 3985 (after its delivery);
peasant 4 at 1231 (not arrived) still had the first-walk set.

**Workers in the gathering cycle do not block each other.** A
worker's cells (SPEC-0021, "Occupied cells") block other ground units
until it first comes out of a mine; from that moment on, while it goes
on gathering (walking to the town hall with its load, giving it,
walking back to the mine, waiting at the mine), its cells block only
the placing of buildings and no unit's local search, seeing a point or
look-ahead check. Nor does such a worker see other units in its own
checks: peasant 5 of `c-RXI-1`, out of the mine in tick 1071, had no
wait at its update of 1075 although peasant 2, still on its first walk
(blocking), was next to it, and from 1094 to 1108 it walked through
peasant 2 (down to 2.3 apart, both walking) without any wait or block.
So workers that have been in the mine once walk through each other and
through every other unit's way, with one exception: a worker WAITING at
the mine marks its cells so that the local search of a gathering
worker meets them as unpassable (`c-RXI-1` tick 1279: worker 4,
walking back to the hall, searched along the row past two waiting
peasants; the search touched their cells (350, 383) and (350, 384), so
it took the second-last point of its path first, SPEC-0020 step 2,
instead of walking straight to the end). A lumber worker WORKING at
its tree marks its cells in the same way (the same kind of entry as a
waiting worker at a mine): `c-RYP-1` tick 3537, peasant 2, working at
its tree since 1585, held cells x 350..351, y 387..388 with such an
entry, and peasant 5's search (gathering kind only, SPEC-0020) found
them closed and did not step onto (350, 389) next to them (read in
the game). A worker on its first walk to the mine,
or one waiting there for the first time, still blocks like any unit
(and sees others). Whether a gathering worker sees units other than
workers (creeps, soldiers) was not tested. A worker inside the mine keeps its cells where it
entered (they count when the place of a worker coming out is chosen,
below), but blocks nothing while hidden. Read in the game (`c-RXI-1`,
the bits of the cells' entries for the six peasants every 6 ticks from
tick 850 to 1560 and at 19 s): first walk and first wait blocking;
blocking only buildings from the update after the first exit (peasant 5
out in tick 1071, peasant 4 in 1271, peasant 0 in 1471); all six so at
19 s. In the log, after tick 1003 no worker of `c-RXI-1` waited for or
was blocked by another, and 1745 times two walking workers were closer
than 32 (down to 0.12, a worker going to the mine and one coming back).
What ends this state (another order, a mine that is gone) was not
tested.

**A worker that starts waiting pushes others aside.** A worker that
reaches an occupied mine stands, and at its next update it starts to
wait (its cells now block as those of a standing unit). At that moment
every unit whose footprint overlaps the waiting worker's footprint is
moved to the nearest free place, found by the search of "Where a worker
comes out" (rings around the moved unit's own cell, the centre of the
first free cell). `c-RXI-1`: peasant 2 reached the occupied mine and
stood (its last step in tick 1111), and at its update in tick 1117 it
started to wait on cells x 350..351, y 384..385. Peasant 5 (in the
gathering cycle, walking west with gold, cells x 349..351, y 384..385)
overlapped it and was moved: from its cell (350, 385) the ring 1 cells
(350, 384), (351, 384), (351, 385) (peasant 2), (351, 386), (350, 386)
(a worker waiting at x 350..351, y 386..387) were taken, (349, 386) was
free, and peasant 5 went on from its centre (-1104, 80) (the log shows
the new position in tick 1118). Other workers started waiting in ticks 871 (two),
1075, 1105 and 1177 with nobody on their cells: nothing moved.

This search counts other units differently from the exit search: a
footprint is refused only by the cells of waiting workers (and of the
worker that starts to wait); walking units, first-walk workers
included, and the moved unit itself do not count (standing units of
other kinds were not in the footprints read). `c-RXD-1` tick 4249:
worker 0 started to wait at (443.1479, 383.3138) (cells 442..443,
382..383) and moved worker 4 from its cell (442, 383); every ring-1
footprint meeting x 442 was refused, and (441, 384) was taken (worker 4
put at (1840, 16), its next step to (1834.30, 16.03)) although its cell
(440, 383) held worker 1, walking on its first walk to the mine.

**Going in pushes others aside too.** When a worker goes into the mine,
the units whose footprints overlap its footprint are moved in the same
way. `c-RXI-1`: peasant 3 came out in tick 3497 at cells (351.12,
385.53); peasant 2 reached the free mine standing on the same place
(3499), and when it went in (3505) peasant 3 was moved to (350.5,
383.5). (Found by the implementer in the agents file of that replay,
one case.)

**A worker reaching a busy mine decides at its next update.** A worker
that reaches the mine while another is inside stands; at its next
update it goes in if the mine is free by then, and otherwise starts to
wait; only workers already waiting are taken in when the mine becomes
free. `c-RXI-1`: peasant 4 reached the mine in tick 6001 with peasant 5
inside; peasant 5 came out in 6003, and peasant 4 went in at its update
of 6007, not in 6003. (From the log of that replay, one case.)

**Coming out.** A worker that comes out of the mine starts a new walk
(to the town hall) as for a new order: it stands at its updates while
it turns towards its path (SPEC-0019, "Stop, hold position and new
orders while walking") and then walks; peasant 5 came out in tick 1071
facing the mine, stood at its updates 1075 to 1093 and moved from
1094, already turned by about 52 degrees.

### Speed on gold trips

A worker gathering gold has its movement speed adjusted each time it
enters a mine, so that its trip fits the mine's cycle. Each worker
keeps a 10 s timer, its current speed bonus `b`, the time of its last
trip `W` and the bonus `bW` it had during that trip (all 0 at the
start; the timer not running). With `M` the mine's mining duration
(`DataB` of `Agld`, 1.0) and `K = 5.0`, at the moment the worker goes
into the mine (SPEC-0004 operations):

1. `r` = the remaining time of the timer (0 if it never ran or has
   expired); `t = sub(sub(10, r), M)`: the time since its last entry
   less the time it spent inside.
2. If `r > 0`, `M < 10` and `t > 0`:
   - `s = mul(t, add(b, 1))`, `n = 1.0`;
   - if `W != 0`: `s = add(s, mul(W, add(bW, 1)))`, `n = add(n, 1)`;
   - `q = div(s, mul(n, sub(K, M)))`, clamped to `[0.5, 1.2]`;
   - the new bonus is `b' = sub(q, 1)`; then `W = t`, `bW = b`.

   Otherwise `b' = 0`, `W = 0`, `bW = 0`.
3. The unit's speed changes at once from `spd * (1 + b)` to
   `spd * (1 + b')`, and `b = b'`.
4. The timer is restarted with 10 s.

`s / n` is the average of the last one or two trips, each scaled to
the base speed; `q > 1` means the trips are longer than the `K - M` =
4 s the rule aims at, so far mines make the workers faster (up to 1.2
times `spd`) and near ones slower (down to 0.5 times).

Measured in replay `c-RXI-1` (six peasants, trips of about 8 s), the
values of every entry read in the game: worker 5 entered at 4.350 s
(first entry, `b` 0), 13.319 s (`t` 7.96992, `q` 1.99 -> 1.2: its
speed 190 -> 228, 1.14 per tick from its next walk) and 21.117 s (`s`
= 6.799768 * 1.2 + 7.96992 = 16.12964, `q` 2.016 -> 1.2). A worker
whose last entry was more than 10 s earlier goes back to bonus 0.

### Where the resources go

The peasant takes them to the building that takes that resource and
is nearest **along the path**: with one town hall 896 from the mine
behind a wall and another 1344 from it in the open, the gold went to
the one in the open; without the wall, to the nearer one (test RUJ).

**Upkeep.** The gold a worker delivers (10) is cut by the player's
upkeep at the moment of the delivery, by the food the player uses
(units over the food cap count as well): 10 with 41 food used, 7 with
61, 4 with 81, 101, ... 181 (test RYS: one peasant mining while 10 more
footmen were created every 8 s; deliveries 10, 10, 10, 10, 10, 7, 7, 4,
4, ...). The steps of upkeep (by the
usual table: up to 50 food none, 51 to 80 low, from 81 high) were not
measured at their borders, and lumber was not tested.

### New orders while carrying

A peasant that carries gold and is ordered elsewhere (`move`) keeps the
gold. Given `harvest` on the mine again, it walks to the mine and, when
it reaches it, does not enter: its order becomes 852017 and it first
takes the carried gold back (test RUJ).

### Lumber

1. The peasant walks to the tree and stops within reach of it (in RUF
   at 127.8 from the centre of a tree at (-2048, 4096)).
2. It hits the tree every 1.1 s (220 ticks); each hit takes 1 life from
   the tree and gives the peasant 1 lumber. The first hit came 99 ticks
   after it stopped.
3. After the 10th hit (114 ticks later in the test) its order becomes
   852017 and it carries the 10 lumber to the nearest building that
   takes lumber: with the town hall at (-4096, 4096) and a lumber mill
   at (-2560, 4096) it went to the lumber mill and delivered at
   (-2369.04, 4015.97), 191 from the mill's centre; the player's lumber
   grew by 10.
4. It goes back to the same tree and goes on (next hits 338 and 350
   ticks after the delivery for the two peasants of the test).

**The harvest swing.** The lumber hits follow the timing of an attack
with the worker's second weapon (`Custom_V1\Units\UnitWeapons.slk`,
peasant: `targs2` tree, `cool2` 1.1, `dmgpt2` 0.433), and the walk to the
tree is a chase of SPEC-0019 whose search goal is the tree's centre. The
walk ends at the first update at which the worker's centre is within
`Rng` of `Ahar` (116) plus its collision size (16) of the tree's centre:
in `c-RYP-1` peasants 0, 1 and 5 stopped 128.4, 131.1 and 130.0 from
their trees, the worker's step being 8.4 per update. The first hit
comes 99 ticks after the stop, then one every 220 ticks; after the
10th the worker leaves 114 ticks later with order 852017.

**An ordered tree out of reach.** Three peasants of `c-RYP-1` (and four
of test RYQ, each ordered to the middle tree of a 3 x 3 block) were
ordered to trees inside the block. Each searched towards its tree
again at its updates while blocked at the edge of the block, then,
without a draw of any generator, took another tree of the block and
harvested it (from its place or after a short walk), the first hit 207
to 249 ticks after it had stopped; on its later trips it came back to
that tree. Which tree it takes is open (not always the nearest one:
RYQ peasant 0 took a tree 123 away although three were nearer).

When a tree has no life left it dies; a peasant that is not full yet
goes on at the nearest other tree (test RUJ: a tree with 3 life, then
a tree 320 away, until it carried 10) and then returns.

### Animations

The peasant's model (`units\human\Peasant\Peasant.mdx`) has each of its
`Stand`, `Stand Ready`, `Stand Work`, `Attack` and `Walk` sequences in
three variants: plain, `Gold` and `Lumber`. The variant follows what the
peasant carries: nothing -> plain, gold -> `Gold`, lumber (even 1) ->
`Lumber`. Read from the running game (test RUX, the sequence played by
each peasant's model polled every 20 ms):

- **Walking** to a mine or a tree: `Walk`; back with gold or lumber:
  `Walk Gold`, `Walk Lumber`.
- **Waiting at a busy mine**: the ordinary standing sequences (`Stand`,
  `Stand -2`, ...), as an idle peasant. Inside the mine the peasant is
  hidden and its model is not drawn; as it comes out with the gold it
  shows `Stand Gold` for a moment and then walks with `Walk Gold`.
- **Chopping**: one attack sequence per hit, 1.0 s long, within the
  1.1 s between hits: `Attack` for the first hit when the peasant
  carries no lumber yet, `Attack Lumber` for the next ones. In the rest
  of the cycle it showed `Stand Ready Lumber`, or nothing new before the
  next hit.
- An idle peasant plays `Stand` most of the time and from time to time
  one of the other plain `Stand` sequences.

`Stand Work` (a short loop of 0.6 s) was not seen in gathering.

### Undead: acolytes on a haunted mine

A haunted mine (`ugol`) stands on a gold mine and has its gold
(SPEC-0024 for how it is made). Its ability `Abgm` gives the numbers:
`DataA` 10 (gold per payment), `DataC` 5 (acolytes at a time), `DataD`
200 (radius of the places round it). An acolyte (`uaco`) gathers with
`Aaha`. Acolytes do not go in and carry nothing; the gold goes to the
owner directly. Read in `c-SDA-1`.

1. `harvest` (852018) on the haunted mine: the acolyte walks to it like
   a peasant to a mine (it stops where its footprint reaches the mine,
   in `c-SDA-1` 260 to 268 from the mine's position). In its next
   update (0 to 7 ticks later) it is put at once, without walking, on
   a free place round the mine, and stands there with order 852018,
   not hidden.
2. The places: five points on a circle of radius `DataD` (200), one
   straight up (+y) from the centre and the others every 72 degrees
   (angles 90, 162, 234, 306 and 18). The centre is the mine's
   position plus (16, 16): for the mine at (0, -384) the places are
   (16, -168), (-174.21, -306.20), (-101.56, -529.80),
   (133.56, -529.80) and (206.21, -306.20), the coordinates of
   `c-SDA-1`.
3. The acolyte takes the free place nearest to where it stopped.
   In `c-SDA-1` the arrival order was acolyte 2 (place 90), 0 (162),
   4 (18), 1 (234), 3 (306).
4. When the five places are taken, the order `harvest` on the mine is
   refused (`IssueTargetOrder` returns false; acolyte 5 at log tick
   1700).
5. Payment: the mine pays `DataA` (10) gold every `5 / n` seconds,
   with `n` the acolytes on their places. The mine's gold
   (`GetResourceAmount`) and the owner's gold change in the same tick.
   When `n` changes, the part of the period already done is kept and
   the rest runs at the new rate. Equivalently, each acolyte on its
   place adds 1/5 of a payment per second, and a payment is made each
   time the sum reaches 1. Examples from `c-SDA-1`:
   - with five acolytes the payments come every 200 ticks (ticks 971,
     1171, ..., 3972, with one step of 201 at 3372);
   - acolyte 2 is stopped at tick 4101, 129 ticks after the payment of
     3972; the next payment comes 88 ticks later (4189, the 71 ticks
     left of 200 at 5/4 of that time), then every 250 ticks;
   - acolyte 2 is ordered back at 5301 (from its place, so it does not
     move), 112 ticks after the payment of 5189; the next one comes at
     5412, then every 200 ticks;
   - the first payment: the acolytes reached their places at ticks
     652, 682, 773, 803 and 935, and the first payment came at 971.
     The sum rule gives 969; each acolyte starts adding about 2 ticks
     after it is put on its place.
6. A stopped acolyte stays where it stood on its place (order 0) and
   the place stays its own: ordered to harvest again it does not move.
7. No carrying, so the necropolis takes nothing from the acolytes in
   this cycle; `c-SDA-1` does not test a haunted mine without a
   necropolis.

### Night Elf: entangling a gold mine

The tree of life (`etol`) entangles a gold mine with `Aent`, order
`entangleinstant` (852148) on the mine. Read in `c-SDB-1` and
`c-SDC-1`.

1. A rooted tree of life first uproots: it does not move for 2.5 s
   after the order (ticks 300 to 803 in both tests).
2. It then walks to the mine at 40 per second and stops when its
   centre is about 656 from the mine's position (655.6 in `c-SDB-1`,
   656.9 in `c-SDC-1`).
3. In its next update (7 ticks later) it roots again. It is put on a
   point of the building grid next to where it stopped: (0, 271.57) to
   (0, 256) in `c-SDB-1`, and (70.53, 269.07) to (64, 256) in `c-SDC-1`.
4. 2.5 s after it roots (500 ticks: 1703 to 2203, 1451 to 1951) the
   mine is entangled:
   - the gold mine is hidden;
   - an entangled gold mine (`egol`) appears at its position with the
     gold mine's gold (12500 here);
   - the tree of life's order becomes 0.
   From then on, gathering takes the entangled mine's gold. The hidden
   gold mine's own amount stays as it was.

### Night Elf: wisps on an entangled mine

A wisp (`ewsp`) gathers with `Awha`; the entangled mine holds wisps
with `Aenc` (`DataA` 5) and pays with `Aegm` (`DataA` 10). Read in
`c-SDC-1`.

1. `harvest` on the entangled mine: the wisp walks to it. When it
   reaches it (185 to 200 from the mine's position in `c-SDC-1`) it
   goes in: hidden, at the point where it stopped, with order 852018.
2. At most five wisps are inside. A wisp that reaches the mine while
   five are inside loses its order (order 0) and stands where it
   stopped (wisp 3 at tick 2513).
3. Payment: as for the haunted mine. The mine pays 10 gold every
   `5 / n` seconds, with `n` the wisps inside, and a change of `n`
   keeps the part of the period already done. In `c-SDC-1` the wisps
   went in at ticks 2045, 2273, 2375, 2471 and 2477. The first payment
   came at 2530 (the sum rule gives 2528), then one every 200 ticks.
4. A wisp inside ignores `stop` (its order stays 852018 and the
   payments keep their period). A new `harvest` on the mine is refused
   (`c-SDC-1` ticks 5300 and 6501).

### Night Elf: wisps on trees

1. `harvest` on a tree: the wisp walks towards it. When it reaches it
   (about 118 from the tree in `c-SDC-1`) it is put at once on the
   tree's position. Only one wisp sits on a tree: a wisp whose tree
   is taken is put on another tree nearby (wisp 6, ordered to tree 0
   at (-704, 448) after wisp 7 sat on it, was put on tree 1 at
   (-704, 576), 128 away).
2. From the moment it is put on the tree, the owner gets 5 lumber
   (`Awha` `DataA`) every 8 s (`Dur`): 1600 or 1601 ticks. In
   `c-SDC-1` wisp 7 was put on its tree at tick 1151 and wisp 6 at
   1337; the lumber rose by 5 at 2751, 2937, 4351, 4537, ...
   The wisp carries nothing and goes nowhere.
3. The tree loses no life (both trees stayed at 50).

### Orc: peons

A peon (`opeo`) gathers with the same ability `Ahar` as the peasant
and walks at the same speed (190). A great hall (`ogre`) takes gold
and lumber (`Argl`). Everything above holds for it unchanged except
the timing of the lumber swing. Read in `c-SDE-1`.

1. Gold: as for the peasant. The peon enters the mine one update after
   it stops (6 ticks: stop at 670, hidden at 676), is inside 1.0 s
   (out at 877 with order 852017; the mine loses 10 gold there), and
   delivers 10 gold at the great hall (tick 1355). One peon is inside
   at a time: with six peons on the mine it loses 10 gold every 200
   ticks (7080, 7280, ...).
2. Lumber: 1 life from the tree and 1 lumber per hit, a hit every 1.1 s
   (220 ticks), 10 per trip. The swing follows the peon's second
   weapon (`targs2` tree, `cool2` 1.1, `dmgpt2` 0.5):
   - the first hit comes 106 ticks after the peon stops (the peasant's
     99): one update and then 0.5 s, the swing point `dmgpt2`;
   - after the 10th hit the peon leaves 100 ticks later (the peasant's
     114).
   For both workers the hit and the rest of the swing add up to 1.0 s
   (0.433 + 0.567, 0.5 + 0.5). That is `cool2` less the 0.1 s pause
   between swings.
   `c-SDE-1`: peon 1 stops at 544 and hits tree 0 at 650, 871, ...,
   2631 (life 40), and leaves at 2731; peon 2 stops at 490, hits tree
   1 from 596, and leaves at 2677 after its 10th hit at 2577.

## Edge cases

- Two peasants on two neighbouring trees hit in their own cycles of
  220 ticks (in RUF 36 ticks apart).
- The mine's gold drops when the peasant leaves the mine, not when it
  enters.

## Open questions

1. Whether the rule for trees adds the peasant's collision size or a
   fixed 16.
2. Which peasant enters a busy mine next.
3. How "nearest along the path" is measured (the high-level search of
   SPEC-0020 or another), and the next tree when several are at the
   same distance; a peasant with no mine or tree left nearby.
4. The interface's return button (852020), the automatic gathering
   orders 852021 and 852022, and peasants given a new order while
   carrying.
5. Other races (orc peon and its burrows, undead acolytes and haunted
   mines, night elf wisps and entangled mines).
6. In "Another tree": the second search point when none is found, and
   which nodes count 2.
7. The gold trip speed bonus when the worker stops gathering, and its
   order against the other events of the entry tick.

## Sources

- Test maps RUF and RUG on 1.27b under Wine, 2026-10-20: three
  separate places on a flat map (one peasant on a mine, five peasants
  on another mine, two peasants on trees with a town hall far and a
  lumber mill near), the orders, hidden state, the player's gold and
  lumber, the mines' gold and the trees' life logged at every change,
  and the positions of one gold and one lumber peasant at every tick.
- Test map RUH: names of the order ids by `OrderId2String`.
- Test map RUJ: a busy mine with three peasants, a mine with 30 gold,
  a tree with 3 life and two more trees, two town halls with and
  without a wall in between, a peasant ordered to move while carrying.
- Test map RUX (RUF with an idle peasant added), 2026-10-20: the sequence
  played by the model of each peasant read from the memory of the
  running game every 20 ms for 40 and 70 s.
- Test map RUI: eight mines with a town hall in eight directions and
  eight trees with a lumber mill, a peasant on each; positions at
  every change of order or hidden state.
- `Units\AbilityData.slk`, `Units\UnitAbilities.slk`,
  `Units\DestructableData.slk`.
- Replay `c-SDA-1` (map c-SDA run live), 2026-10-28: six acolytes, a
  necropolis and a haunted mine; positions, orders, the player's gold
  and the mine's gold logged every tick for 44 s.
- Replays `c-SDB-1` and `c-SDC-1` (maps run live), 2026-10-28: a tree
  of life entangling a gold mine (with a long walk in SDB), wisps on the
  entangled mine and on two trees; positions, orders, gold, lumber, the
  mines' gold and the trees' life logged every tick for 59 s.
- Replay `c-SDE-1` (map c-SDE run live), 2026-10-28: a great hall, a
  peon on a gold mine, two peons on two trees, five more peons on the
  mine from 31.5 s; positions, orders, gold, the mine's gold and the
  trees' life logged every tick for 59 s.
- Replay `c-RXI-1` (map c-RXJ run live), 2026-10-21: the timer, the
  trip times, the sums and the bonus of every mine entry to 28 s, and
  the speed of worker 5's path agent, read in the running game.

## Verification

1. **Gold** (RUF, one peasant, order at tick 100): hidden from tick 568
   to 768 at (-4080.40, -4932.93); visible with order 852017 in tick
   769, when the mine drops from 12500 to 12490; delivery (order back to
   852018) in tick 1451.
2. **Lumber** (RUF): the first tree's life goes 50, 49 (tick 379), 48
   (599), ... 40 (2359); order 852017 in tick 2473; lumber 10 -> 20
   when this peasant delivers in tick 2735; the tree's next hit in tick
   3085.
3. **Approach** (RUI): at every one of the 181 entries into a mine and
   deliveries logged, the footprint of the peasant touches a red cell
   of the building (cell distance 1); at the 16 first hits on trees the
   distance is 128.0 to 132.0.
4. **Busy mine** (RUF, five peasants): the mine's gold drops by 10 at
   ticks 769, 969, 1169, 1369, 1569, ...; each entry is in the tick of
   the previous peasant's exit.
5. **Haunted mine** (`c-SDA-1`): the five acolytes stand at
   (16, -168), (-174.21, -306.20), (-101.56, -529.80),
   (133.56, -529.80) and (206.21, -306.20); the sixth acolyte's order
   is refused; the player's gold is 10 at tick 971, 160 at 3972, 170
   at 4189, 210 at 5189, 220 at 5412 and 390 at 8814.
6. **Entangle** (`c-SDC-1`): the tree of life roots at (64, 256) in
   tick 1451, and the entangled mine appears in tick 1951 with 12500
   gold.
7. **Wisps** (`c-SDC-1`): gold 10 at tick 2530 and 210 at 6531; lumber
   5 at 2751, 10 at 2937, 15 at 4351 and 60 at 10940; the trees stay at
   50 life.
8. **Peons** (`c-SDE-1`): peon 0 is hidden from tick 676 to 877 and the
   player's gold is 10 at 1355; tree 0 has 40 life at 2631 and peon 1's
   order is 852017 from 2731; peon 2's from 2677.
