---
id: SPEC-0025
title: Group orders and formation (layout, holds, group speed)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, memory reads, replays with mouse clicks)"]
---

## Behaviour

A point order given to several selected units at once (replay action
with the group flag, SPEC-0024) does not send every unit to the order
point. The units are split into movement groups ("clusters"); each
cluster walks one path, and every member walks to its own place in a
formation around the cluster's waypoint. This spec gives the clusters,
the formation angle, the leader, the exact layout of the places, which
members wait so that the ranks form up in the right order, and the
common speed of the group.

Units in this spec: positions and lengths are in cells of 32 world
units, measured from the lower left corner of the world (SPEC-0018,
"Units of storage"). All arithmetic is that of SPEC-0004 and SPEC-0011
(`add`, `sub`, `mul`, `div`, `sqrt`, `sin`, `cos`); `i2f` is the
conversion of a small integer to float (exact). `angle(x, y)` is the
angle function of SPEC-0018 ("Angle of a vector"), with
`len = sqrt(add(mul(x, x), mul(y, y)))`.

### Clusters

The ordered units (at most 12) are split into clusters: a unit joins the
cluster of another one when their distance is at most 40 cells (90 with
a flag of the order that was not seen set), and so on transitively (a
connected component). The distance is the integer part of the straight
distance in cells for units without hierarchical paths and a path
distance otherwise. Only single clusters were observed; the splitting
is read from the code and not tested.

Each cluster gets the order point as its goal `T`.

**Order of handling.** In a tick, the members of a cluster are handled
one after the other in the cluster's order (the order of the units in
the order, member 0 first), and the clusters among themselves and
among single units newest order first (SPEC-0021, "Searches of many
units in the same tick"). In `c-RSM-2` the members' look-ahead checks
came in the order 0, 1, 2, 3 in every tick, member 0 being the first
created footman.

### Formation angle and leader

- When the order is given, the formation angle is
  `A = angle(sub(Tx, Cx), sub(Ty, Cy))`, with `C` the mean position of
  the members (sum of the positions by `add`, in member order, times
  `div(1.0, i2f(n))`).
- At the cluster's first update the **leader** is chosen: the member
  whose position is nearest to `T` (squared distance; members with
  hierarchical paths are preferred). The cluster's path is searched from
  the leader's position to `T`, and the angle becomes
  `A = angle(sub(Wx, Lx), sub(Wy, Ly))`, with `L` the leader's position
  and `W` the cluster's current waypoint (on open ground the goal `T`).
  The layout below is computed with this angle.

Example (test RSO, four footmen at x = -150, -50, 50, 150, y = 0, order
to (13.79, -464.70)): `C` = (0, 0) gives `A` = `0x4097BEDC` (-88.3
degrees); the leader is the footman at (50, 0) and `A` becomes
`0x40944ED2` (-94.5 degrees).

### Layout

Inputs for each member `i` (in the cluster's member order, which is the
order of the units in the order):

- `p_i`: its current position (SPEC-0018: committed position plus
  velocity times the elapsed time). For the first layout of a cluster,
  made at its first update, this is the position at the moment of the
  group order, not at that update: in replay `c-RTW-3` (tick 5497) the
  leader 918 was walking, and the layout used its position of the order
  (its commit of tick 5491). (Found by the implementer with the agents
  file of that replay; one case.)
- `r_i`: its collision radius in cells (read from memory: footman
  `0x3F780000` = 0.96875, knight and rifleman 1.0, peasant 0.5);
- `k_i`: its formation rank, the `formation` column of
  `Units\UnitData.slk` (0 for most melee units, 2 for most ranged units,
  4 for workers; values up to 15 possible).

With one member, its place is (0, 0). Otherwise:

1. **Projection.** With `s = sin(-A)`, `c = cos(-A)` (the sign bit of `A`
   flipped):
   `u_i = sub(mul(px_i, c), mul(py_i, s))` (forward along `A`) and
   `w_i = add(mul(px_i, s), mul(py_i, c))` (sideways, to the left).
2. **Ranks.** The members are grouped by rank; the ranks are taken in
   increasing order 0, 1, ..., 15, skipping empty ones. Let `R` be the
   number of non-empty ranks.
3. **Width of a rank** with `m` members: `rmax` = the largest `r_i` of
   the rank (start 0); `Dk = add(rmax, rmax)`;
   `big = 1` if `rmax >= 1.5`, else 0; the number of columns is
   `cols = min(m, table[m + 13 * big])`, with the table

   | `m` | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
   |---|---|---|---|---|---|---|---|---|---|---|---|---|
   | `R = 1`, small | 1 | 2 | 3 | 2 | 3 | 3 | 3 | 3 | 3 | 4 | 4 | 4 |
   | `R = 1`, big | 1 | 2 | 3 | 2 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 |
   | `R > 1`, small | 1 | 2 | 3 | 4 | 3 | 3 | 4 | 4 | 4 | 4 | 4 | 4 |
   | `R > 1`, big | 1 | 2 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 |

   With `kf = i2f(cols)` and `Sk = add(Dk, 2.0)`, the rank's width is
   `Wk = add(mul(Dk, kf), mul(Sk, sub(kf, 1.0)))`. `Wmax` is the largest
   `Wk` of all ranks (start 0).
4. **Places.** `depth` starts at 0. For each non-empty rank in order:
   - sort its members by `u` in decreasing order (front first). The
     sort decides ties, so it is given exactly: for `k` from the last
     position down to 1, scan positions `k - 1` down to 0 and remember a
     position whenever its value is strictly smaller than the smallest
     seen so far (starting with the value at `k`); swap the remembered
     position with `k`;
   - take them in that order in rows of `cols` members (the last row
     may be shorter). For a row of `q` members:
     - `q = 1`: the place is `(depth, mul(Wmax, 0.5))`;
     - `q > 1`: sort the row by `w` in increasing order (as above,
       but remembering a position whose value is strictly greater than
       the largest seen so far, so the largest moves to the end); with
       `g = div(sub(Wmax, Dk), i2f(q - 1))` and `b0 = mul(Dk, 0.5)`, the
       `j`-th member of the row (from 0) gets
       `(depth, add(b0, mul(g, i2f(j))))`;
     - after each row except the rank's last,
       `depth = sub(depth, add(Dk, 2.5))`;
   - after the rank, `depth = sub(depth, 5.5000005)` (`0x40B00001`, not
     exactly 5.5).
5. **Centre and turn.** `ma` and `mb` are the means of the first and
   second coordinates of all places (sums by `add` in member order,
   times `div(1.0, i2f(n))`). With `S = sin(A)`, `Co = cos(A)`,
   `da = sub(a_i, ma)`, `db = sub(b_i, mb)`, the member's offset is

       ox_i = sub(mul(da, Co), mul(db, S))
       oy_i = add(mul(da, S), mul(db, Co))

So the first rank stands in front (towards the goal), the later ranks
behind it, each 5.5 cells further back plus the rows inside a rank; the
places of a row are spread evenly over the width of the widest rank,
and a single member stands in the middle.

### Where the members walk

Each member walks to the cluster's current waypoint plus its offset (see
"Following the cluster's path"): on open ground
`(add(Tx, ox_i), add(Ty, oy_i))`. (Checked to about 0.01
in world units; the exact order of the operation was not read.) A member
stops when it is within its arrival radius of that point (SPEC-0018),
so a group comes to rest roughly in the formation's shape around the
order point.

**When a member searches again.** A member's place is recomputed at
every cluster update, but the member does not search a new path each
time. It keeps the place of its last search, and at each of its member
steps it compares the new place with that one in squares of 2 x 2
cells (64 x 64): both points in cell coordinates, each coordinate
truncated to an integer and halved by an arithmetic shift right by 1.
If both squares are equal, nothing changes: the member keeps its path
and its old place (the next comparison is again against the place of
the last search, so small moves add up until one crosses a square
border). If they differ, the member takes the new place and searches
to it (the throttle of SPEC-0020 then applies as usual). A member that
has no place yet (just joined) always searches. Read in `c-RTW-3`,
all member steps of 5521, 5527, 5533, 5539 and 5545 (35): each search
came exactly where the squares differed. For example at 5539 member
936 went from (398.2733, 370.1612) to (398.6408, 369.2842) cells, y
square 185 -> 184, and searched; 864 went from (387.4362, 365.6202) to
(387.8037, 364.7432), squares (193, 182) both times, and kept its path.

The test has a second part that was not exercised in these data: when
the squares differ, the member searches only if two per-path times are
both at least 10 cluster update cycles old; otherwise it keeps its path
and its old place. Both times were 0 in all the steps read, and which
events set them was not read. A change of the cluster's goal serial
(a new goal for the cluster) makes the member search whatever the
squares are.

### Members that wait for their rank to pass

At every update of a cluster with more than one member, before the
members move, each member's "hold" mark is cleared and then set again
by the following rule (when the rule is not suspended, see below). A
held member does not walk at this update (speed 0); it still turns
towards its target. It does not follow its path at this update either:
it makes no search (SPEC-0020), no look-ahead check, and neither waits
nor counts patience (SPEC-0021 and "Blocked units: the patience
counter" below); its path is kept for the update at which it is no
longer held.

1. For every member `i`, with its current position `p_i` (committed
   position plus velocity times the elapsed time, SPEC-0018) and
   `s = sin(-A)`, `c = cos(-A)`:
   `u_i = sub(mul(px_i, c), mul(py_i, s))` (as in the layout),
   `e_i = add(r_i, 1.0)`, `lo_i = sub(u_i, e_i)`, `hi_i = add(u_i, e_i)`.
2. Sort the members by `lo` in decreasing order (the selection sort of
   the layout, step 4).
3. Go through the sorted list from the last entry (smallest `lo`, the
   rearmost) to the first, keeping `low` = the smallest rank seen so far
   (start 16). Members that have reached their place are skipped. For
   the member at position `k` with rank `b`:
   - if `b > low`: it is held (a unit of a rear rank stands ahead of a
     unit of a front rank that is behind it);
   - otherwise (`b <= low`, and `low` becomes `b`): look at the entries
     `k - 1, k - 2, ...` towards the front while `lo` of that entry is
     strictly below `hi_k`; if one of them has not reached its place and
     has a smaller rank than `b`, the member is held.

The rule is suspended (no member is held) while a countdown of the
cluster runs: when, at an update with the countdown at 0, some member
is in a state that excludes it (blocked, searching or waiting on its
path, among others; open question 3), the countdown is set to 66
updates instead.

In test RSP (peasant rank 4, rifleman 2, knight 0, footman 0 side by
side, order to the south) the knight and footman walked at once, the
rifleman was held for 18 updates and the peasant for 34, until the
front ranks had passed them.

### Members in each other's searches

At every update of a cluster with more than one member, while its
members take their steps, each member that is neither held nor already
walking is marked; the marks are removed after the step. A member counts
as walking from the end of its first step with a speed above 0.01; all
members count as walking once the cluster has gone on to a later
waypoint, and after 65 cluster updates on its first path. Local searches and seeing a point (SPEC-0021) ignore
marked units, as they ignore moving units; the look-ahead check sees
them. Held members are not marked and are standing obstacles for the
others.

In `c-RSP-1` at the first cluster update the knight (50, 0) and the
footman (150, 0) were marked, the rifleman (-50, 0) and the peasant
(-150, 0) were held. So the footman's search could meet nobody, while
the rifleman's footprint stood next to the knight (this explains the
clean and the not clean search seen by the implementation; the searches
themselves were not read from memory). The footprint of a standing unit
in a search is the footprint of its size class at the cell of its
position (SPEC-0021, "Occupied cells"): the rifleman (collision 32,
class 2) occupies 3 x 3 cells around the cell of (-50, 0), that is
x from -96 to 0 and y from -32 to 64 in world units; the knight would
occupy x 0 to 96 (same y).

### Group speed

Each member computes its own speed for the update as a single unit
would (SPEC-0018). The cluster then limits it:

- If no member is held and no member has reached its place, the group
  speed is the smallest speed limit (`spd`) of the members; otherwise
  there is no group speed. (A shared limit over several clusters of one
  order also exists; not studied.)
- Each member's speed is `s = min(own speed, group speed)`, and its
  velocity is set from `s` as in SPEC-0018, step 5:
  `w = (mul(s, cos H), mul(s, sin H))`. The limit of step 5 still uses
  the unit's own `spd`, not the group speed, so with a group speed of
  190 the velocity's length is 190 only up to the error of `cos` and
  `sin` (between about 189.9999 and 190.004; in test RSP all 40
  velocities checked were exactly `w` with `s = 190/32` cells per
  second and no further limit).

So while ranks form up, the moving members walk at their own speeds;
once every member walks, all walk at the speed of the slowest; when the
first member reaches its place, the others go on at their own speeds.
In RSP all four walked at 190 (the peasant's speed) from update 35 on;
in RSO the last footman sped up to 270 after the others had stopped.

A further rule, for clusters with a flag that was not seen set, lowers a
member that is faster than the leader to 0.95 times the leader's speed
limit when it is close to the leader.

### Blocked units: the patience counter (script generator)

This rule belongs to path following in general (SPEC-0020, SPEC-0021),
for single units as well as group members; it is given here because it
draws the script generator.

Every unit following a path has a patience counter `P`, 0 when it is
not blocked. The counter is used only when the look-ahead is blocked by
**units**: when the cells ahead are closed by the pathing map (walls,
and as far as tested buildings), the unit searches again at once as in
SPEC-0020 and `P` is not touched. (In the game, a blocked look-ahead
with no unit to wait for first runs a test of the cells ahead against
the static pathing; when that test fails the unit searches again, when
it passes the counter below applies.) At an update at which the unit's
look-ahead is blocked by units and no unit makes it wait by the rules
of SPEC-0021 ("Blocked by a unit"):

1. if `P = 0`, it is set:
   - `P = 2` if the squared distance from the unit's position to its
     path's goal (for a group member: its place, waypoint plus offset)
     is at most 144 (12 cells, 384 world units) **and** the unit's
     cluster has at most one member;
   - otherwise `P = 7 + ((r >> 22) AND 1)` (7 or 8), with `r` one output
     of the **script generator** (SPEC-0007). (The game takes the
     integer part of `(x - 1) * 2` with `x = (r AND 0x7FFFFF) OR
     0x3F800000` as a float in `[1, 2)`, which is exact.)
2. if `P = 1`: the unit gives up on the path: it gets the "within
   reach" mark of the touch case below (it stands, turns towards its
   place, and arrives as soon as it is turned); `P` is not lowered.
   Replay `c-RYP-1`, peasant 4 at its tree: blocked at the end of its
   path in 1225 without touching the tree (`P` 2 -> 1), at 1231 a
   local search to the tree's point that ended at once (the path was
   its own position), blocked again with `P = 1`: marked; turned at
   1231 and 1237, arrived at 1243 (read in the game);
3. otherwise `P = P - 1`.

Except in case 2, the unit then stands still at this update and drops
its local path. At its next update it searches a new local path to the
same segment goal (SPEC-0020, step 2, with the throttle and the shared
budget of SPEC-0020 and SPEC-0021). That search sees the units that
stand in the way (SPEC-0021, "What each check sees"), so it usually
leads round them, and the look-ahead check is then made along the new
path. `P` is cleared to 0 at any update at which the look-ahead is free
again. So `P` counts consecutive updates at which the unit is blocked by
units in spite of a fresh path, each of them followed by a new search;
`P = 2` reaches case 2 at the second such update, 7 or 8 at the seventh
or eighth. The counter is in updates of the unit (every 6 ticks).

The same counter, with the same steps 1 to 3, is used when the unit
comes within 0.49 cells of the last point of its local path (one that
does not end at an intermediate waypoint, SPEC-0020 step 3: complete or
partial) while it has not arrived
(SPEC-0019, "Arrival at the member's place", tested against the place
as it is now). This happens to a member that kept its old path because
its new place lies in the same 2 x 2 square ("When a member searches
again"): it walks to its old place and is then counted as blocked
there. Replay `c-RTW-3`, member 900: at 5539 its place moved from
(394.5697, 363.7980) to (394.9372, 362.9210) cells (same square, old
path kept, its last point the old place); at 5545 it was at (394.3108,
363.7770), 0.2597 from the old place and 1.0607 from the new one: not
arrived, end of path reached, so blocked: `P` set (7 or 8, script
generator; the cluster has more than one member) and lowered by one in
the same update, local path dropped, standing. At 5551 and 5557 it was
blocked again in the same way (`P` 6 -> 5 -> 4), the last search
leading to its old place again.

Before the counter is used (both for units in the way and for the end
of the path), the game tests whether the unit touches its order's
target (the unit's cell against the target's footprint, with a margin
that depends on the pathing class; read in part). If it does, the
counter is not touched: the unit stands at this update, turns towards
its place by its turn rate (SPEC-0018), and from now on counts as
within reach in the arrival test (SPEC-0019, "Arrival at the member's
place") until it arrives; that mark is cleared at the arrival. Replay
`c-RYP-1`, peasant 5 walking to its tree at (360, 390) cells along row
393.5, local path ending at (357.5, 393.5): at 1297 its step took it to
(357.1828, 393.4994), 0.317 from that end and 4.4925 from the tree's
point (reach 4.125, not arrived): blocked, touching, so marked; speed
0, heading 6.2779 -> 5.6779 (0.6 towards the direction 5.3902 of the
tree's point). At 1303 within reach by the mark, angle -0.2877: stood
and turned the rest, heading 5.3902. At 1309 angle 0: arrived (speed 0,
path dropped, mark cleared). At 1315, 4.4925 > 4.125 and no mark: a
new search to the tree's point, local path to (358.5, 393.5).

A partial path ends the same way. `c-RXD-1` creep 9 walking home to
(387.125, 402.75): its local path of 4561 had failed at the iteration
limit and ended at its closest cell (388.5, 402.5). At 5713 its position
after the step was 0.72 from that point; at 5719 0.4666 (arrival test:
1.862 from home, not arrived): blocked, not touching a target, so `P`
set (by the rule 2: one unit, near its goal; the value was not read)
and lowered in the same update, path
dropped, standing; at 5725 a new local search from (388.9479, 402.3693)
to home, 2 points.

Measured (`c-RSM-2.w3g`): member 1 is blocked at the update with clock
5.609832 by member 2, which stands waiting for its 5 updates; `P`
becomes 7 and member 1 stands. At the next update (5.639830) it makes a
local search, its look-ahead is free, `P` is cleared and it walks on
the new path round member 2 (velocity (-2.194, -8.147) cells/s), while
member 2 still waits. The same happens again at 5.729823 and 5.759821.

A unit far from its goal, or any member of a group, draws the script
generator every time it starts to wait this way. This is why `E` of
SPEC-0024 changes while a group walks although the script draws
nothing. Measured (RSO): a footman of the group blocked by another
member at (-8.51, -148.20), 403 from its place (101.99, -542.80), got
`P` = 8, stood one update, and the counter was cleared at the next
update when its way was free.

### Following the cluster's path

The cluster has a current waypoint `W`: a segment goal of the cluster's
own path, which is searched from the leader's position. The segment
goals are chosen exactly as for a single unit (SPEC-0020, "Segment
goal": the first high-level waypoint at which the summed leg lengths
reach 10.0 blocks, or the goal; the same code is used). On open ground
`W` is the goal; in RSQ the first segment goal was (-464, -336). Each
member's place is `W` plus its offset, and a member that is at its place
(within its arrival radius) is marked "arrived".

At every cluster update, unless the cluster is at its last waypoint:

- `near` = the members that have not arrived and whose squared distance
  from their place is below 256 (16 cells; 16, that is 4 cells, for
  clusters with a flag not seen set);
- if at least one member has arrived and every other member has arrived
  or is near, the cluster goes on to the next waypoint. While the
  countdown of "Members that wait for their rank to pass" runs, one
  arrived member is enough;
- if at least one member has arrived but not all others are near, a
  stall count goes up by one per update; when it exceeds 99 (198 or 396
  with flags not seen set), the cluster goes on to the next waypoint
  anyway.

At the last waypoint the stall count is used differently: it goes up by
one at every update (whether or not a member has arrived) and is reset
to 0 at an update in which a member leaves the cluster (in
`c-RSP-1.members.tsv` it is 1 at the second update).

Here "arrived" has a wider sense than "at its place": a member is also
marked arrived when its own path following has ended without reaching
the place, that is when it gave up (a path that cannot be continued,
SPEC-0020, partial path end with no way on). Such a member usually
also carries the "blocked" mark. An arrived member normally leaves the
cluster at once, where it stands. But if it is marked blocked, the
cluster has more than one member, the stall count is below 20 (the
first 20 updates at the last waypoint or since a member last left) and
its squared distance from its place is above 256 (more than 16 cells),
it stays in the cluster and searches its path to its place again. So a
member that got stuck far from its place gets new searches during the
first 20 updates, and after that it leaves the cluster where it got
stuck. A member that stopped within 16 cells of its place leaves at
once. (Read from the code, not tested.)

**After an arrival.** The cluster's own path (its 5000 search) is made
once, with the cluster, and kept. In the tick in which a member
arrives (SPEC-0019, "Arrival at the member's place") it stands and its
own path is dropped; `W` does not change in that tick (the cluster
step comes before the members' steps). At the next cluster step `W`
moves on by the rules above, and in the member's step of that tick its
place is the new `W` plus its offset: having no path, it makes its own
high-level search (limit 400, SPEC-0020 "Two searches") from its
position to that place and then the local search to the segment goal
on its own list. A cluster of one does the same: its `W` moves on at
the first cluster step after its member arrived. Read in the game:
`c-RXD-1` worker 3, `W` (417.5, 391.5) until its arrival in 3361, (448,
384) from the cluster step of 3367, member search to (224, 192) and
local search to (448, 384) in 3367; `c-RYP-1` peasant 4, `W` (353.5,
375.5) until 847, (360, 380) from 853, member search to (180, 190) in
853. No new 5000 search in either.

When the cluster goes on to the next waypoint `W'`, the formation angle
becomes `A = angle(sub(W'x, Wx), sub(W'y, Wy))` (from the old waypoint to
the new one), `W` becomes `W'`, the members' marks and the stall count
are cleared, and the layout is computed again with the members' current
positions. So the formation turns at every corner of the path.

A member that has arrived at its place at the last waypoint leaves the
cluster as described above (the cluster's member count falls); the
remaining members keep their offsets.

Measured (RSQ, the four footmen of RSO with a wall from (-500, -260) to
(600, -196) between them and the goal): first waypoint (-464, -336) at
the west end of the wall, `A` = 3.72059 (213.2 degrees, from the leader
to that waypoint); when the first member reached its place there (the
countdown was running), the cluster went on to the goal with
`A` = 6.01999 (344.9 degrees, from (-464, -336) to the goal); both
layouts matched bit for bit; then the members left the cluster one by
one as they arrived.

## Edge cases

- The script generator is drawn by group movement (above), so script
  random numbers after a group order differ from those without it.
- Units with `rmax >= 1.5` cells (collision 48 and more) use the "big"
  rows of the table.

## Open questions

1. The flagged rule of "Group speed" (0.95 of the leader's speed) and
   the shared limit over several clusters.
2. The member states that start the 66-update countdown, and how the
   look-ahead decides "blocked" for group members (SPEC-0021).
3. How a member's own path to its place is searched (members also
   search their own paths; their targets move with the cluster's
   waypoint), and what happens when units join or leave a cluster other
   than by arriving.
4. Cluster splitting with distant units, and the path distance used for
   units with hierarchical paths.

## Sources

- Analysis of the 1.27b game library (cluster creation, layout).
- Memory reads in live games (debugger): inputs and results of the layout
  for two groups, and the formation angle before and after the first
  cluster update.
- Test maps RSM, RSO, RSP, RSQ (groups of four selected with a box and
  given a right click on the ground; RSQ with a wall), replays
  `c-RSM-1.w3g`, `c-RSM-2.w3g` and `c-RSP-1.w3g` (with the members' state
  at every cluster update in `c-RSM-2.members.tsv` and `c-RSP-1.members.tsv`).

## Verification

1. **Four footmen** (RSO): positions (cells) (379.3125, 384),
   (382.4375, 384), (385.5625, 384), (388.6875, 384), all `r` 0.96875,
   rank 0, `A = 0x40944ED2`. Offsets (cells, bits):
   (`C04676C1`, `BFFDEDF6`), (`40306646`, `C01C2D3F`),
   (`C0306647`, `401C2D3F`), (`404676C1`, `3FFDEDF5`): two rows of two.
   The member at (379.3125, 384) gets (-3.101, -1.984), its walk target
   (-85.4, -528.2) in world units.
2. **Mixed group** (RSP): peasant (r 0.5, rank 4), rifleman (1.0, 2),
   knight (1.0, 0), footman (0.96875, 0) at the same positions, same `A`.
   Offsets: (`3F08BCA7`, `40DB55C2`), (`3DDAC775`, `3FAF77D1`),
   (`C053EE1D`, `C0784904`), (`402AE8B8`, `C08B0F32`): knight and footman
   in front, rifleman 5.5 cells behind, peasant another 5.5 behind.
3. Both formation angles of the example in "Formation angle and leader"
   follow from the angle function bit for bit.
4. **Holds** (RSP): the rule of "Members that wait for their rank to
   pass", computed with the engine operations from the members'
   committed positions, velocities and commit times read from memory,
   gave the game's hold marks at all 86 cluster updates of the test.
   The rifleman was held for 18 updates, the peasant for 34; from then
   on all four walked with speed 190 (`c-RSP-1.members.tsv`).
