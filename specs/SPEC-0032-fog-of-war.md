---
id: SPEC-0032
title: Fog of war - what a unit reveals
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps, memory of the running game)"]
---

## Behaviour

### The fog grid and its states

Visibility is kept per player on a grid of **fog cells** of 128 x 128,
the terrain tiles of `war3map.w3e`: cell `(i, j)` covers
`x0 + 128 i <= x < x0 + 128 (i + 1)` and likewise in y, with `(x0, y0)`
the lower left corner of the map (the offset in `war3map.w3e`; -12288,
-12288 on the 192 x 192 test maps). A point has the state of the cell it
lies in: `IsVisibleToPlayer`, `IsFoggedToPlayer` and `IsMaskedToPlayer`
sampled every 16 units changed only at multiples of 128.

A cell is in one of three states for a player:

- **masked** (black): never seen;
- **visible**: revealed at the last fog update;
- **fogged**: seen before, not visible now (cells left behind by a
  walking unit became fogged, test RWH).

### Updates

The fog is recomputed every **80 ticks** (0.4 s), at the same ticks for
all units: a peasant walking east revealed the cells ahead of it, and
the cells behind it became fogged, only at ticks 80 apart (test RWG:
script ticks 921, 1081, 1161, 1321, ..., all equal modulo 80). At each
update, each unit reveals the cells of its mask (below) placed on the
cell it stands in at that tick.

### Radius

A unit's radius in fog cells is its sight (`sight` of
`Units\UnitBalance.slk` by day, `nsight` at night) divided by 128 and
rounded to the nearest integer, halves up, and **at most 14**:

| Sight | Radius | Seen in |
|---|---|---|
| 350 | 3 | `ncrb` |
| 600 | 5 | `hpea` at night |
| 800 | 6 | `hpea` by day, `Hamg`, `hfoo` at night, `ehpr` at night |
| 900 | 7 | `Ekee` at night, `hgry` at night |
| 1000 | 8 | `espv` at night, `eden` by day |
| 1100 | 9 | `hgyr` at night |
| 1200 | 9 | `hmtm` at night |
| 1400 | 11 | `hfoo`, `hmtm` by day |
| 1600 | 13 | `nwad` |
| 1800 | 14 | `Hamg` by day |
| 1900 | 14 | `etrp`, `ushd`, `ncap` |
| 2200 | 14 | `now3` |

(1200 / 128 = 9.375 gives 9.) The radius does not depend on where the
unit stands inside its cell (six positions tried). Buildings reveal
around the cell of their centre after placement (an `eden` created at
(-6040, -1964) revealed around the cell one up and one right of the cell
of that point).

### The circle

`S(d)`, the half width of the circle of radius `r` at row distance
`d = 0 .. r`, comes from Bresenham's midpoint circle. Starting with
`x = 0`, `y = r` and the decision value `e = 3 - 2r`, take the point
`(x, y)`; then, if `e < 0`, add `4x + 6` to `e`, else add
`4(x - y) + 10` and decrease `y` by one; increase `x` by one; repeat
while `x <= y`. The circle's points are these points and their mirror
images `(y, x)`; `S(d)` is the largest first coordinate among the
points whose second coordinate is `d`. From `S(0)` up to `S(r)`:

| r | S(0), S(1), ..., S(r) |
|---|---|
| 3 | 3, 3, 2, 1 |
| 6 | 6, 6, 6, 5, 4, 3, 2 |
| 8 | 8, 8, 8, 7, 7, 6, 5, 4, 2 |
| 11 | 11, 11, 11, 11, 10, 10, 9, 8, 8, 6, 5, 3 |
| 14 | 14, 14, 14, 14, 13, 13, 13, 12, 11, 11, 10, 9, 7, 6, 3 |

**Flying units** (seen: `hgyr`, `hgry`, `ehpr`, `now3`) reveal the whole
circle: every cell `(dx, dy)` with `|dy| <= r` and `|dx| <= S(|dy|)`
around their cell, with no blocking.

### The mask of a ground unit

A ground unit (and a hovering one, `ushd`) reveals only the cells of its
**mask**, a subset of the circle that is not mirror symmetric, and only
those not hidden by higher ground or trees (next section). For
`r >= 3` the mask is:

- the 3 x 3 cells around the unit's cell (`|dx| <= 1`, `|dy| <= 1`);
- for every ring `k = 1 .. r`, with `L = min(k, S(k))`: the cells
  `(k, b)` for `b = 0 .. L` and `(k, -b)` for `b = 1 .. L - 1`, and the
  same two sets turned by 90, 180 and 270 degrees about the unit's cell
  (a turn by 90 degrees maps `(dx, dy)` to `(-dy, dx)`).

So the mask is unchanged by a turn of 90 degrees but is not mirror
symmetric. For `r = 6` the mask
has 121 cells, rows from `dy = 6` down to `dy = -6`:
`dx` -2..1, -3..2, -4..4, -4..5, -5..6, -6..6, -6..6, -6..6, -6..5,
-5..4, -4..4, -2..3, -1..2. `SPEC-0032-masks.tsv` lists the mask for
every radius 1 to 14 (radius 1: the 3 x 3 cells; radius 2: the 3 x 3
cells and the four cells two away on the axes; these two were read
from the code and not measured).

### Blocking: heights and parents

Each fog cell has a **height**: `128 x` the highest cliff level (layer)
of the four corners of its tile in `war3map.w3e` (the layer height in the
low four bits of each corner's last byte), plus the occlusion
height `occH` of a destructable standing in the cell (`LTlt` trees:
230, so 486 on ground of level 2). Read from memory on the IceCrown
terrain: 128, 256, 384 and 512 by cliff level, 486 in the ten cells
with a tree.

The **eye** of a ground unit is the height of the cell it stands in.
A cell **blocks** when its height is larger than the eye.

Each cell of the mask outside the 3 x 3 centre has two **parents** one
ring closer to the unit:

- on an axis (`dy = 0` or `dx = 0`): the next cell on the axis towards
  the unit, twice;
- on a diagonal (`|dx| = |dy|`): the next cell on the diagonal, twice;
- otherwise, with `|dx| > |dy|`: `(dx - sx, dy)` and `(dx - sx, dy - sy)`
  (`sx`, `sy` the signs of `dx`, `dy`); with `|dy| > |dx|`:
  `(dx, dy - sy)` and `(dx - sx, dy - sy)`.

A cell is **hidden** when one of its parents is hidden or blocks; the
cells of the 3 x 3 centre are never hidden. A cell that is not hidden is
revealed, also when it blocks itself: the edge of a cliff and a tree are
seen, the cells behind them are not.

Measured: a peasant on flat ground of level 2 with a wall of five trees
3 cells east of it, a tree 4 cells north and one 3 cells south-west
(test RWD): the 93 cells revealed are exactly those of this rule. A
peasant west of a block raised by one cliff level (test RWE) saw the
block's edge cells and nothing behind them (111 cells); one on top of the
block (test RWF) saw all 121 cells of its mask, the low ground included.

### The minimap

The minimap shows the map's picture `war3mapMap.blp` (256 x 256), not a
picture made from the terrain: on a map whose terrain was replaced, the
minimap still showed the old map's picture. It is drawn over the
minimap frame (SPEC-0028); in a 1024 x 768 window the whole picture took
the square from (17, 584) to (191, 758) in pixels, so the map's full
width (the `war3map.w3e` area, not only the camera bounds) maps onto it.

The fog darkens it per cell state of the local player:

| State | Brightness (screen over picture, mean of all pixels) |
|---|---|
| visible | 0.738 |
| fogged | 0.738, the same as visible |
| masked, map flag `0x0010` set | 0.404 (0.55 of visible) |
| masked, map flag `0x0010` clear | 0.003 (black) |

The three channels were darkened alike (0.738 / 0.738 / 0.735 for red,
green, blue). Flag `0x0010` is a bit of the flags field of
`war3map.w3i` (the editor option that leaves masked areas partly visible);
IceCrown has it set (flags `0xd472`). The visible-to-picture value 0.738
also contains the difference between the picture as decoded for the
test and as shown; take visible as the picture itself and masked as 0.55
of it. Measured with the whole map in one state (`SetFogStateRect` on
the world bounds), tests RWJ and RWK; with a footman and a walking knight
(test RWI) the cells near units and along the knight's way were equally
bright and the rest at the masked value, with soft edges between the
states.

## Edge cases

- Near the map's edge, mask cells outside the map are skipped, and so
  are the cells behind them (their parents chain through the skipped
  cell); read from the code, not measured.
- Fog modifiers created by scripts (`CreateFogModifierRadius`) do not use
  the masks above (open question 1).

## Open questions

1. The cells revealed by `CreateFogModifierRadius` (irregular in the
   tests, with holes for a radius of 256) and by the other fog natives.
2. The phase of the 80-tick update relative to the start of the game;
   allied vision; units inside buildings or transports; dead units.
3. Doodads that block sight, destructables other than trees, and what
   a unit on a destructable bridge sees.
4. How the fog is drawn in the world (a smooth edge, test RVY) and on
   the minimap between cells of different states (soft edges); how often
   the minimap's fog is redrawn.

## Sources

- Test maps RVQ to RWK on 1.27b under Wine, 2026-10-20: units of 20
  types alone in separate parts of a flat map and of IceCrown, by day and
  at night, trees, a block raised by one cliff level; the state of every
  fog cell for player 0 read with `IsVisibleToPlayer`,
  `IsFoggedToPlayer` and `IsMaskedToPlayer`; a walking peasant with the
  cells ahead of it read at every tick.
- The masks, the parents and the cell heights read from the memory of
  the running game.
- `Units\UnitBalance.slk`, `Units\DestructableData.slk`.

## Verification

1. **Mask** (RVV): a peasant by day (sight 800, radius 6) at a cell
   centre plus (40, 20) on flat ground: the 121 cells of the radius 6
   mask above, and no others.
2. **Radii** (RVZ, RWA, RWB): the units of the radius table each reveal
   exactly the mask of that radius (ground) or circle (flying).
3. **Trees** (RWD): 93 cells; the 5 tree cells of the wall visible, the
   cells east of them hidden.
4. **Update** (RWG): a peasant walking east at 190: the first cell 6
   ahead of its cell becomes visible at an update, 80 ticks after the
   previous change or a multiple of it.
