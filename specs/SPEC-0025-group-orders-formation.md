---
id: SPEC-0025
title: Group orders and formation (part 1: layout)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, memory reads, replays with mouse clicks)"]
---

## Behaviour

A point order given to several selected units at once (replay action
with the group flag, SPEC-0024) does not send every unit to the order
point. The units are split into movement groups ("clusters"); each
cluster walks one path, and every member walks to its own place in a
formation around the cluster's waypoint. This part gives the clusters,
the formation angle, the leader and the exact layout of the places. The
speeds while the formation forms and marches are in part 2 (open
question 1).

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

Each member walks to the cluster's current waypoint plus its offset:
on open ground `(add(Tx, ox_i), add(Ty, oy_i))`. (Checked to about 0.01
in world units; the exact order of the operation was not read.) A member
stops when it is within its arrival radius of that point (SPEC-0018),
so a group comes to rest roughly in the formation's shape around the
order point.

### Waits that draw the script generator

When a member of a cluster with at least two members has to wait for
another unit on its path, the length of the wait is
`7 + ((r >> 22) AND 1)` (7 or 8), with `r` one output of the **script
generator** (SPEC-0007). (The game computes it as the integer part of
`(x - 1) * 2` with `x = (r AND 0x7FFFFF) OR 0x3F800000` taken as a float
in `[1, 2)`, which is exact.) (A unit alone in its
cluster gets 2 under a condition on a distance; otherwise the same
random wait.) This is why `E` of SPEC-0024 changes while a group walks
although the script draws nothing. The unit of the wait (ticks or
updates) and the exact condition were not studied (open question 2).

## Edge cases

- The script generator is drawn by group movement (above), so script
  random numbers after a group order differ from those without it.
- Units with `rmax >= 1.5` cells (collision 48 and more) use the "big"
  rows of the table.

## Open questions

1. Speeds (part 2): in test RSP (peasant, rifleman, knight, footman in a
   row, order 465 to the south) the knight and footman started at once
   at their own speeds (350, 270), the rifleman at update 10, the
   peasant at update 18; from about update 18 on all four walked at
   190, the peasant's speed, until each reached its place. The rules for
   when a member starts and when the group speed applies are not known
   yet. Each unit's own speed limit is not changed by the group.
2. The path wait of a group member: when it happens, its unit, and the
   condition for the wait of 2.
3. When the layout is computed again (new waypoints, members lost or
   added) and how members follow a path with several waypoints.
4. Cluster splitting with distant units, and the path distance used for
   units with hierarchical paths.

## Sources

- Analysis of the 1.27b game library (cluster creation, layout).
- Memory reads in live games (debugger): inputs and results of the layout
  for two groups, and the formation angle before and after the first
  cluster update.
- Test maps RSM, RSO, RSP (groups of four selected with a box and given
  a right click on the ground), replay `c-RSM-1.w3g`.

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
