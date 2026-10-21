---
id: SPEC-0020
title: Path finding and path following around static obstacles (part 1)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, process memory, test maps)"]
---

## Behaviour

This part covers one ground unit with a `move` order on a map whose
obstacles come from the pathing map (`war3map.wpm`) only: no other
units, buildings, doodads or cliffs in the way. Movement between path
points (heading, `turnRate`, `propWin`, speed, arrival radius 0.49 cells)
is SPEC-0018; this spec says which point the unit walks to at each
update and when it stands still.

Path finding has two levels. A **high-level search** over a quadtree of
free blocks gives a coarse list of waypoints for the whole route. A
**local search** over single cells gives the exact path to a point a few
waypoints ahead; the unit walks that path, shortcutting it where the way
is free, and searches again when it gets to its end.

All arithmetic of this spec is integer arithmetic or the engine
arithmetic of SPEC-0004, as stated. Coordinates are relative to the
map's lower left corner (`minX`, `minY` of the camera bounds as in
SPEC-0013, for the test maps -12288).

### Units of length

- A **cell** is 32 world units. Cell coordinates are `(x - minX) / 32`
  as real numbers; cell `(i, j)` is the square `i <= cx < i + 1`,
  `j <= cy < j + 1`. "The cell of a point" is the pair of floors.
- A **block** is 2 x 2 cells (64 world units). Block coordinates are
  cell coordinates divided by 2; block `(u, v)` holds cells `2u, 2u+1`
  by `2v, 2v+1`.
- The pathing grid has the size of the `.wpm` (the test maps: 768 x
  768 cells). Points outside the grid are not passable.

### Pathing cells

For a ground unit a cell counts as passable:

- in the **high-level search** (the block states below) if its `.wpm`
  byte has neither 0x02 (not walkable) nor 0x04 (not flyable) set;
- in the **local search**, in **seeing a point** and in the
  **look-ahead check** if its `.wpm` byte does not have 0x02 set; 0x04
  alone does not matter there.

(The flags of the grid were checked against process memory for all 256
byte values; the test walls used `.wpm` byte 0xCA. A wall of cells with
only 0x04 set made footmen take a high-level detour while their local
paths crossed those cells: test BC2 of SPEC-0022, both walkers within
0.01 world units of a model with this rule.)

### Size classes and footprints

The unit's collision size `c` (`collision` of `UnitBalance.slk` or
`ucol`) gives `r = c / 32` and a size class: `r < 0.5` class 0,
`r < 1.0` class 1, `r < 1.5` class 2, otherwise class 3 (footman 31 ->
1, peasant 16 -> 1, knight 32 -> 2, abomination 48 -> 3, collision 8
-> 0).

A unit standing in cell `(x, y)` covers a square **footprint** of
`k + 1` by `k + 1` cells for class `k`:

| Class | Footprint x range | Footprint y range |
|---|---|---|
| 0 | x | y |
| 1 | x-1 .. x | y-1 .. y |
| 2 | x-1 .. x+1 | y-1 .. y+1 |
| 3 | x-2 .. x+1 | y-2 .. y+1 |

A **step** from cell `p` to a neighbouring cell `q` (one of the 8
neighbours) is allowed if every cell that the footprint at `q` covers
and the footprint at `p` does not is passable; for a diagonal step the
same must also hold for the footprints at the two cells `(q.x, p.y)` and
`(p.x, q.y)` (no cutting of corners). Cells of the footprint at `p`
itself are not checked again.

### Local search

Input: start point `s`, goal point `g` (cell coordinates), size class,
an iteration limit (700 for the local search of a moving unit).

- The start cell is the cell of `s`, the goal cell that of `g`. If they
  are the same, the path is `s -> g` at once.
- A* over cells. A node's cost `G` is the sum of step costs, 15 for a
  straight and 21 for a diagonal step. The estimate `h` of a cell
  `(x, y)` to the goal cell `(gx, gy)` is computed in unsigned integers:

      a = 15 |x - gx|,  b = 15 |y - gy|
      big = max(a, b),  small = min(a, b)
      if small > (big >> 2):
          small = small + (small >> 1)
          h = big + (big >> 6) + (small >> 2) + (small >> 7) - (big >> 4)
      else:
          h = big

  (`>>` is the logical shift; the result is taken modulo 2^32.)
- The open list is a binary min-heap on `G + h`, kept in an array from
  index 1, with these tie rules, which decide the path when keys are
  equal:
  - insert: put the entry at the end and move it up while its parent's
    key is greater than **or equal to** its own;
  - remove the minimum: take the entry at index 1, take the last entry
    out and sift it down from index 1: of the two children the right
    one is chosen if its key is less than or equal to the left one's;
    the entry stops when its key is less than or equal to the chosen
    child's key.
- A cell is in one of three states: not yet seen, open, closed. When a
  node is expanded its neighbours are tried in this order:
  `(-1,-1), (0,-1), (+1,-1), (-1,0), (+1,0), (-1,+1), (0,+1), (+1,+1)`
  (dx, dy). A neighbour outside the grid or not reachable by an allowed
  step is skipped. For the others, with `G' = G + step cost`:
  - not yet seen: it becomes open with `G'`; if its squared cell
    distance to the goal cell is strictly less than the best so far,
    it becomes the **closest node** (the start cell is the first);
  - open or closed: only if `G'` is strictly less than its `G`; then it
    gets the new cost and parent and becomes open again (closed cells
    are reopened). The old heap entry stays in the heap and is skipped
    when it comes out.
- Each removal from the heap counts one **iteration**, skipped entries
  included. The search stops when the goal cell is removed (found),
  when the heap is empty, or when the iteration count reaches the
  limit. If the goal was not found, the path ends at the closest node.
- The path is the chain of parents from the end cell back to the start
  cell, each cell given by its centre `(x + 0.5, y + 0.5)`; the start
  cell's point is replaced by `s`, and if the goal was found the goal
  cell's point is replaced by `g`.

### High-level search

**Block states.** For each block (level 0) and each larger aligned
square of 2 x 2 (level 1), 4 x 4 (level 2) and 8 x 8 (level 3) blocks,
the game keeps a state: **free** (all passable), **blocked** or
**mixed**:

- level 0: count the cells of the block that are not passable; 0 ->
  free, 4 -> blocked, 1 to 3 -> mixed;
- level `L + 1` from its four level-`L` children: mixed if any child is
  mixed or if 1 to 3 children are blocked, blocked if all four are,
  free otherwise.

States are built for blocks `0 .. W/2` of level 0 (inclusive, so one
block beyond the grid edge on a 768-cell map, whose cells count as not
passable) and `0 .. (W/2) >> L` of level `L`; squares beyond that count
as free. This was checked against process memory: no difference on any
level.

Below, "`F(u, v)`" means "level-0 block `(u, v)` is free".

**Nodes.** A node is a free square, found from a query block `(u, v)`
and a start level `L`: look at the level-`L` square holding `(u, v)`;
if it is blocked there is no node; if it is mixed there is no node at
level 0 and the query is to be split (see the side scan) at higher
levels; if it is free, go up while the next larger square is free (at
most to level 3). The node is that square. Its **position** is the
query block of the first query that found it during this search; later
queries that find the same square do not change it.

**Granularity.** Units of size class 0 and 1 use granularity 1, classes
2 and 3 granularity 2. With granularity 2 a node at block `(u, v)`
stands for a unit that covers blocks `u, u+1` by `v, v+1`, and more
checks apply (marked "gran 2" below).

**Costs.** Between nodes at positions `(u1, v1)` and `(u2, v2)` the cost
is `isqrt((24 |u1 - u2|)^2 + (24 |v1 - v2|)^2)`, with `isqrt` the
integer floor of the square root; the estimate to the goal is the same
formula to the goal's query block. The search is A* with the same
heap, the same node states and the same "closest node" rule as the
local search, and an iteration limit (see Open questions).

**Start and goal.** The start node is found from the block of the
start point at level 0; if there is none (the start block is not free)
the search gives no list (not tested). The goal node is found from the
block of the goal point. If both are the same node, the list is just
`goal, start`.

**Trees in the local search.** Trees are not part of the static
pathing map: their cells carry no static closing bits. Each tree is
held as three occupant entries (layers) over its 4 x 4 cells (the
occupied cells of SPEC-0021), in this order in each cell's list: one
that never closes a cell for a ground unit; one of the kind that closes
cells only for the searches of workers in the gathering cycle (the kind
of the marks of workers waiting at a mine, SPEC-0029); and its
footprint layer, which closes cells for ground units and is the layer a
search to that tree takes as its target (the target tree's own
footprint layer does not close). A cell test walks the cell's list from
the front and stops at the first closing entry, so the target is
touched (which ends a search to a target, as for chases and buildings)
only if its entry comes before any closing entry of that cell. So a
worker on its first walk touches its tree in the tree's own cells,
while a worker in the gathering cycle stops at the tree's second layer
and does not touch it by the search. Read in the game: `c-RYP-1`
peasant 0's search of 313 (first walk, to tree 2's point (354, 382))
touched in cell (352, 380), the lower left cell of the tree; peasant
5's search of 1315 (gathering cycle, tree 27 at x 358..361, y
388..391) was refused at every step towards row 392 (each step's
footprint reached row 391), ran to its limit and ended at its closest
cell (360, 393). Such a worker reaches its tree through the arrival
test with the order's reach (SPEC-0019) and the touch or patience
cases of SPEC-0025.

**Two searches, and a target's footprint.** A path to a target runs
the high-level search twice in a row: first the cluster's search with
iteration limit 5000, from the start to the target point, then the
unit's own search with limit 400, from its position to the segment goal
(step 1) taken on the cluster's list; their removals are counted in
different budget tables (SPEC-0021, "High-level budgets"). The unit
then walks the list of its own 400 search: its segment goals for the
local searches are taken on that list, not on the cluster's. Replay
`c-RXD-1`, creep 7 at 4561 (open ground): the cluster's list was start,
(215.75, 193.75), (207.75, 193.75), (199.75, 200.75), goal (191.0625,
200.125); its segment goal (199.75, 200.75) (legs 1.29, 8.0, 10.63);
the unit's 400 search to that point went through (207.75, 200.75)
instead (4 removals), and its local search went to (207.75, 200.75)
(legs 1.29, 10.63 on its own list), (415.5, 401.5) in cells.
For a worker going to a tree, the goal point is the tree's position,
and for the search the 3 x 3 level-0 blocks centred on the block of
that point count as free, whatever covers them; the larger squares are
taken from this changed level 0 by the usual rule. This was read in
the state tables as each search saw them (`c-RYP-1` tick 307, the tree
block covering blocks x 176..181, y 188..195, all blocked before):
peasant 5 (tree at block (180, 195)): blocks x 179..181, y 194..195
free, level-1 square (90, 97) free, (89, 97) mixed; its 5000-limit
search went (176, 196) level 2, (179, 195) level 0, goal (180, 195)
level 1 in 5 removals. Peasant 4 (block (180, 190)): blocks x 179..181,
y 189..191 free; goal (180, 190) level 1 in 11 removals. Peasant 3
(block (179, 192), inside the tree block): blocks x 178..180,
y 191..193 free but enclosed; its search ran until its heap was empty,
2516 removals. The 400-limit searches took 3 removals each, and
peasant 3's path list came from its 400 search. All removals:
`game-data/1.27b/replays/c-RYP-1.hlsearch-307.tsv`. Whether the 3 x 3
depends on the target's size (both trees here are alike) was not
tested.

**Neighbours of a level-0 node** at position `(x, y)`: try the side
neighbours in the order below (south, east, north, west), each found as
a node from that block at level 0; then each diagonal only if both of
its adjacent side neighbours were accepted.

| Neighbour block | Accepted if (gran 1: always, when the node exists) gran 2 also needs |
|---|---|
| S `(x, y-1)` | `F(x+1, y-1)`, and if `x` is odd and the found node is above level 0: `F(x+1, y-3)` or `F(x-1, y-1)` |
| E `(x+1, y)` | `F(x+2, y)` and `F(x+2, y+1)` |
| N `(x, y+1)` | `F(x, y+2)` and `F(x+1, y+2)` |
| W `(x-1, y)` | `F(x-1, y+1)`, and if `y` is odd and the found node is above level 0: `F(x-3, y+1)` or `F(x-1, y-1)` |
| SE `(x+1, y-1)`, after S and E | `F(x+2, y-1)` |
| NE `(x+1, y+1)`, after E and N | `F(x+2, y+2)` |
| NW `(x-1, y+1)`, after N and W | `F(x-1, y+2)` |
| SW `(x-1, y-1)`, after W and S | nothing more |

**Neighbours of a node above level 0.** Let the node's square have
level `L`, size `S = 2^L` blocks, lower left block `(x0, y0)`, and let
`(x, y)` be its position; with gran 2, `x` and `y` are first lowered to
at most `x0 + S - 2` and `y0 + S - 2`. The four sides are scanned in
the order bottom, right, top, left, from these query blocks:

| Side | Query block | Runs along |
|---|---|---|
| bottom | `(x, y0 - 1)` | x |
| right | `(x0 + S, y)` | y |
| top | `(x, y0 + S)` | x |
| left | `(x0 - 1, y)` | y |

A side scan at level `L` from query block `q`:

1. Look up the level-`L` square holding `q`. Blocked or outside: nothing
   (no flag). Free: go to step 3.
2. Mixed: split it into its two halves along the side (level `L - 1`).
   Let `m` be the first block of the upper half (along the side). If
   `q` lies in the upper half, first scan the lower half from the query
   block `m - 1` when `L - 1` is 0 and `m - granularity` otherwise, then
   continue with `q` in the upper half; if `q` lies in the lower half,
   first scan the lower half from `q`, then continue with `m` in the
   upper half. Each half is scanned the same way at level `L - 1`
   (splitting again while mixed). At level 0 a mixed square gives
   nothing.
3. A free square found at level `l` (the level reached by splitting,
   before the climb to larger free squares) gives the node found from
   the query block. Gran 2 adds a check on the query block `(qx, qy)`;
   with `e` the last block of the level-`l` square along the side:

   | Side | query before `e` | query at `e` |
   |---|---|---|
   | bottom | `F(qx+1, qy)` | `F(qx+1, qy)`, `F(qx+1, qy+1)` and (`F(qx+1, qy+2)` or `F(qx-1, qy)`) |
   | right | `C(qx, qy)` | `C(qx, qy)` and (`F(qx, qy-1)` and `F(qx+1, qy-1)`, or `F(qx-2, qy+1)` and `F(qx-1, qy+1)`, or `F(qx-1, qy+1)` and `F(qx, qy-1)`) |
   | top | `C(qx, qy)` | `C(qx, qy)` and (`F(qx-1, qy)` and `F(qx-1, qy+1)`, or `F(qx+1, qy-2)` and `F(qx+1, qy-1)`, or `F(qx+1, qy-1)` and `F(qx-1, qy)`) |
   | left | `F(qx, qy+1)` | `F(qx, qy+1)`, `F(qx+1, qy+1)` and (`F(qx+2, qy+1)` or `F(qx, qy-1)`) |

   with `C(a, b)` = `F(a+1, b)` and `F(a, b+1)` and `F(a+1, b+1)`.
   If accepted, the node is relaxed as a neighbour, and if `l` is above
   0 the piece sets the side's **end flags**: the low-end flag if it is
   the piece at the low end of the side (the lower half at every
   split), the high-end flag if it is the piece at the high end. A
   square found without splitting sets both.

After the four sides, the corners are tried, each only if both
adjoining side ends were flagged:

| Corner | Query block | Needs | Gran 2 also needs |
|---|---|---|---|
| bottom right | `(x0 + S, y0 - 1)` | bottom high end, right low end | `F(qx+1, qy)` |
| top right | `(x0 + S, y0 + S)` | right high end, top high end | `C(qx, qy)` |
| top left | `(x0 - 1, y0 + S)` | top low end, left high end | `F(qx, qy+1)` |
| bottom left | `(x0 - 1, y0 - 1)` | left low end, bottom low end | nothing more |

A corner query looks up the level-`L` square holding the query block
and, while it is mixed, the next lower level at the same block; mixed at
level 0 or blocked gives nothing.

**Waypoints.** The list runs from the end node back to the start node
through the parents. Each node gives the point `(px + o, py + o)` in
block coordinates, `o = 0.75` for gran 1 and `1.25` for gran 2, where
`(px, py)` is its position; with gran 2 and a node above level 0, a
coordinate equal to the last block of the node's square is lowered by
one first. The start node's point is replaced by the start point and,
if the goal node was reached, the goal node's point by the goal point.
With gran 1 a waypoint is the centre of the upper right cell of its
block.

### Following the path

When a unit gets a `move` order, at its first update (SPEC-0018) it runs
the high-level search from its position to the order point. Then:

1. **Segment goal.** Starting at the current waypoint (at first, the
   start), walk along the high-level list towards the goal, adding up
   the lengths of the legs in block units. The first waypoint at which
   the sum reaches 10.0 (640 world units) is the segment goal and
   becomes the current waypoint. If the sum reaches 10.0 only at the
   last point of the list, or never, the segment goal is the order
   point. This matters when the high-level search failed and its list
   ends at the closest node: the segment goal is then still the order
   point, not the closest node (replay `c-RYP-1`, peasant 3 at 1141:
   legs 2.06, 4.0, 2.24, 2.26 to the closest node (182.5, 193.5), sum
   10.55 only there; local search to the tree's point (358, 384) cells,
   which failed and ended at its closest cell (351, 384), point (351.5,
   384.5)).
2. **Local path.** A local search from the unit's position to the
   segment goal (class of the unit, limit 700). If any cell the search
   tested on its way was not passable (for a step or a ring check of an
   expanded cell), the unit's **target** is the first point of the path
   after the start point, and it is not shortcut yet. If the search met
   no unpassable cell at all, the target is the last point of the path
   (the unit walks straight to it).
3. **Each update**, before moving:
   - **End of a segment at a waypoint** (checked first): when the local
     path ends at a waypoint of the unit's high-level list (not at the
     order point), the segment is done as soon as the unit's position
     after this update's step, in high-level units (cells / 2), is
     within 0.49 of that waypoint (squared distance at most `0.49 *
     0.49`, that is 0.98 cells); then in the same update the next
     segment goal is taken (step 1 from that waypoint) and searched
     (step 2), and the unit walks on without standing. Replay
     `c-RXD-1`, creep 7 walking home, local path ending at the waypoint
     (207.75, 200.75): at 5209 its position after the step (416.3111,
     400.7957) cells = (208.1556, 200.3979), squared distance 0.2885:
     no; at 5215 (416.1200, 400.9616) = (208.0600, 200.4808), 0.1686:
     the segment ended, local search from there to the next segment
     goal (399.5, 401.5) in the same update, and the step of 5215 went
     on (read in the game).
   - **Arrival at the target**: if the squared distance in cells from
     the unit to the target is at most `0.49 * 0.49` (engine
     arithmetic): if the target is not the last point of the local
     path, the target becomes the farthest later point that can be
     **seen** from the unit's position (below; it is the next point if
     none farther can be seen). If it is the last point and the segment
     goal is a waypoint, the end of the segment above normally comes
     first; this older reading (the unit **stands still for this
     update** and searches at the next) is kept only for the case that
     the waypoint check did not fire (not seen since). If
     it is the order point, the order ends (SPEC-0018 arrival). If it is
     the end of a local path that did not reach its goal (the search
     ended at the closest node), or the unit has not arrived at its
     place there (SPEC-0019), it counts as blocked: the patience counter
     of SPEC-0025 ("Blocked units") applies, the unit stands still for
     this update and searches again from its position at the next
     update (`c-RXD-1` creep 9 at 5719).
   - **Look-ahead check**: take the cell of the unit, `c0` (of its
     position at this update, that is the position stored at its
     previous update plus its velocity times the elapsed time,
     SPEC-0018), and the cell of the point one cell length ahead
     towards the target (the target itself when it is closer than one
     cell), `c1`. The cells of the
     footprint at `c1` that a step from `c0` to `c1` newly covers (as for
     a search step; if `c1 = c0`, the whole footprint) must be passable.
     If not, the unit is **blocked**: it stands still for this update,
     drops its local path and at the next update searches again from
     its position to the same segment goal (step 2).
   - Otherwise it turns and moves towards the target by SPEC-0018.
   - **At every update at which the unit stands still** because of one
     of the rules above, its heading still turns towards a point by the
     turning rule of SPEC-0018, without motion: towards the segment goal
     after a block, a throttled search or a partial path end, and
     towards the reached last point of the local path at the end of a
     segment.
4. **Search throttle.** The game counts unit update cycles (one every 6
   ticks). After a search that took 64 or more iterations, the same
   unit cannot search again until 10 cycles (60 ticks) after it; until
   then it stands still at each update at which it would search.
   After a search with fewer iterations there is no wait. (Seen: a
   search of 66 iterations made a blocked unit stand 3 updates instead
   of 1; searches of 3, 5 and 53 iterations caused no wait.)

**Seeing a point.** A point `p` can be seen from position `s` if the
following walk finds no step that is not allowed for the unit's size
class. Let `d` be the vector from `s` to `p` in cells and `len` its
length; the direction is `d / len` (or `d` itself if `len` is at most
1). Sample the points `s + t * direction` for `t = 1, 2, ...` while `t <
len`. Whenever the cell of a sample differs from the cell of the
previous sample, check the step from the previous sample's cell to the
new one as in the search (only the newly covered cells, including the
corner rule for diagonal changes). The "previous sample" before the
first one is the cell `(0, 0)`, so the first sampled cell is always
checked as a step in the direction `(+1, +1)`. The farthest-point rule
tries the points from the one after the target onwards and stops at the
first one that cannot be seen.

## Edge cases

- A unit whose segment goal is the order point and whose start and
  order point are in the same free square walks the local path from
  the start (no waypoints).
- The look-ahead check can block a unit on a line that the "seeing"
  walk accepted, because it samples from the unit's current position:
  in the tests this happened when a unit cut close past the corner of a
  wall (footman, knight, collision 8, collision 64, abomination).
- A unit with collision 0 walked straight to the first segment goal
  from its first update: its search met no unpassable cell (the
  straight-walk rule of step 2).

## Open questions

1. The two high-level searches (limits 5000 and 400, above): which
   one's list is used when both succeed, and why the second one found
   a list for peasant 3 in 3 removals where the first failed.
2. (Resolved: collision 0, see step 2.)
3. What happens when a unit is blocked again soon after a block: the
   game keeps a per-path counter, set at the first block to 2 or to 7
   or 8 (from a random number, so the block may draw from a random
   generator; which one is not known), and does something not yet
   observed when it runs down. Also which point decides between 2 and
   7/8 (a distance of 12 cells is involved).
4. (Moved: the budget shared between units is SPEC-0021.)
5. Arrival at a high-level waypoint itself (as opposed to the end of a
   local path) was never seen to matter.
6. Buildings and doodads as obstacles, cliffs and water, unreachable
   targets, iteration limits being hit, warps. Units as obstacles are
   SPEC-0021.
7. (Resolved: the abomination that turned only 11 degrees after a
   block had already turned towards its segment goal at the update of
   the block, see "At every update at which the unit stands still".)

## Sources

- Static analysis of the 1.27b game library (path finder, quadtree,
  path following) and reading the path objects, the pathing grid and
  the quadtree from the memory of the running game (lists and states
  compared one to one with an independent model).
- Test maps on 1.27b under Wine, 2026-10-18: the flat map of SPEC-0018
  with walls written into `war3map.wpm` (byte 0xCA), units walking past
  them, positions logged every tick.

## Verification

All walls are rectangles of unpassable cells, given as half-open world
ranges `x0 <= x < x1, y0 <= y < y1` widened to whole cells (each cell
touching the range is unpassable). Units are created at the start
point facing 0 and get the `move` order at once; ticks count from the
order (first update at tick 4). Positions are world coordinates.

**Test PF1.** For each row `y0` = -6000, -3000, 0, 3000 a wall
`-2000 <= x < -1968`, `y0 - 400 <= y < y0 + 400`; a unit at `(-2500, y0)`
ordered to `(-1500, y0)`: footman, peasant, knight, abomination.

- Footman: high-level list (world) `(-2500,-6000)`, `(-2512,-6160)`,
  `(-2000,-6416)`, `(-1744,-6352)`, `(-1500,-6000)` (start first). The
  first target is the cell centre `(-2480,-6032)`; at the update of
  tick 22 the target becomes `(-2032,-6416)` by shortcutting. The unit
  stands still from tick 484 to 490 at `(-2009.15,-6414.25)` (end of
  the first local path), then goes `(-1904,-6416)`, `(-1872,-6384)`,
  `(-1500,-6000)` and rests at `(-1507.54,-6007.75)`, last moving at
  tick 989.
- Knight: stands at `(-1977.04,522.90)` from tick 436 to 448 (end of
  the first local path, then 1 update turning without motion by the
  `propWin` rule), rests at `(-1503.87,4.38)`, last moving at tick 851.
- Abomination: blocked at tick 478 at `(-2065.35,3467.98)`, walks the
  cell path round the wall corner and rests at `(-1507.72,3010.23)`,
  last moving at tick 1127.

**Test PF2.** For each `k = 0 .. 9`, row `y0 = 2000 k - 9000`, a wall
`-2000 <= x < -1968`, `y0 - 400 <= y < y0 + 400`, a unit at `(-2500,
y0)` ordered to `(-1500, y0 + 100)`. Units: footman copies with
collision 8, 16, 24, 32, 40, 48, 64, 0, then footman and knight.

| Unit | Events (ticks) | Rest point, last moving tick |
|---|---|---|
| collision 8 | end of first local path 502; blocked at 550 at `(-1968.48,-8561.35)`, throttled at 556 and 562, moves again from 569 | `(-1506.70,-8894.89)`, 995 |
| collision 16 | end of first local path 508 | `(-1509.09,-6892.18)`, 983 |
| collision 32 | end of first local path 556 | `(-1507.10,-2893.71)`, 1031 |
| collision 48 | end of first local path 556 | `(-1507.68,1107.82)`, 1031 |
| collision 64 | blocked 478; turn only at 502 (new target at 20.5 degrees, heading 94.3); end of local path 580 | `(-1507.75,3108.23)`, 1067 |
| footman | blocked at 478 at `(-2035.00,7439.00)`, new path north; target switched east at 502, turn only at 502 (`propWin`, SPEC-0018), end of local path 526 at `(-2009.08,7469.67)` | `(-1509.22,7108.35)`, 1007 |
| knight | end of first local path 424; blocked 460 at `(-1941.28,9459.56)` | `(-1505.94,9105.19)`, 791 |

Path lists read from the game for the collision 8 unit of PF2 (same
test run alone): at tick 546 its local path (cells, goal first) was
`(337.125,105.875) (336.5,106.5) (335.5,107.5) (334.5,108.5)
(333.5,108.5) (332.5,108.5) (331.5,109.5) (330.5,109.5) (329.5,110.5)
(328.5,111.5) (327.5,112.5) (326.5,113.5) (325.5,114.5) (324.5,115.5)
(323.5,116.5) (322.5,116.5) (321.1450,117.1653)`, searched at tick 508
from its position to the order point in 66 iterations, and its first
high-level list (blocks, goal first) `(168.5625,52.9375) (164.75,55.75)
(162.75,57.75) (162.75,58.75) (160.75,58.75) (152.75,56.75)
(152.9375,51.375)`.

An implementation following this spec and the movement arithmetic of
SPEC-0018 reproduced these trajectories within 0.1 world units at
every tick (collision 8, 40 and 0 run alone: largest differences 0.013,
0.030 and 0.016). With straight-line movement in ordinary floating
point instead, the differences stayed within 1.5 except for the
collision 40 unit, whose block at tick 604 depends on 0.05 world units
next to a cell boundary.
