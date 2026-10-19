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
  velocity times the elapsed time);
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

### Members that wait for their rank to pass

At every update of a cluster with more than one member, before the
members move, each member's "hold" mark is cleared and then set again
by the following rule (when the rule is not suspended, see below). A
held member does not walk at this update (speed 0); it still turns
towards its target.

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
2. if `P = 1`, the unit gives up waiting: it searches its path again
   (SPEC-0020) and `P` stays 1 until it is cleared;
3. otherwise `P = P - 1` and the unit stands still at this update.

`P` is cleared to 0 at any update at which the look-ahead is free again.
So `P = 2` means "stand one update, then search again" (the rule of
SPEC-0021), and 7 or 8 mean "stand up to 6 or 7 updates while still
blocked, then search again". The counter is in updates of the unit
(every 6 ticks).

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
`c-RSP-1.members.tsv` it is 1 at the second update). An arrived member
normally leaves at once; but if it is also marked as blocked, the
cluster has more than one member, the stall count is below 20 and its
squared distance from its place is above 256 (more than 16 cells), it
stays and searches its path again instead (read from the code, not
tested).

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
  `c-RSM-1.w3g` and `c-RSP-1.w3g` (with the members' state at every
  cluster update in `c-RSP-1.members.tsv`).

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
