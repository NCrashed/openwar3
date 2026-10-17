---
id: SPEC-0012
title: Ground texture layering and variations
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps with screenshots)"]
---

## Behaviour

This covers how the ground of a flat tile is drawn from the corner points
of `war3map.w3e`: which ground textures, in which order, and which cell
of each texture's atlas. Cliffs, ramps, water and blight are not covered.

A tile is the square between four neighbouring corner points: bottom left
(BL), bottom right (BR), top left (TL), top right (TR); "top" is the
larger y (north). Each point carries a ground texture index (an index into
the ground tile list of `war3map.w3e`, 0..15) and a variation value
(5 bits, 0..31).

### Atlas cells

A ground texture image is an atlas of square cells, 4 rows high. Cell
size is `height / 4` (64 pixels for the shipped 256-pixel-high images).
Row 0 of the image is the north edge of a tile.

- Cells 0..15 are the left 4 x 4 block, numbered left to right, top to
  bottom (cell `n` at column `n mod 4`, row `floor(n / 4)`).
- Atlases twice as wide as high ("wide", 512 x 256) have a second 4 x 4
  block on the right: cells 16..31, numbered the same way.
- Atlases as wide as high ("narrow", 256 x 256) have only cells 0..15.

In cells 1..14 the alpha channel covers the corners named by the bits of
the cell number: 1 bottom right, 2 bottom left, 4 top right, 8 top left
(so cell 3 covers the bottom half, cell 12 the top half). Cells 0 and 15
are fully opaque in the shipped atlases and serve as full tiles, like
cells 16..31.

### Layering

For each tile:

1. Let `T` be the set of ground texture indices at its four corners.
2. The **lowest index** in `T` is drawn first, as a full tile (cell chosen
   by the variation, below).
3. Each other index in `T` is then drawn **in increasing order of index**,
   using the cell whose number has the bits of the corners where that
   index is (cell number from the rule above), blended with its alpha.

The order is by index in the ground tile list of `war3map.w3e`, not by
the texture's identity. The rule holds for tiles with two, three and
four textures; with overlapping alpha masks, a higher index always lies
on top of a lower one.

### Variation cell of the full tile

The full tile of step 2 uses the variation value of the tile's **bottom
left point**. This is so also when the bottom left point has another
texture (the variation is a property of the point, whatever its texture).

| Variation `v` | Wide atlas cell | Narrow atlas cell |
|---|---|---|
| 0..15 | `16 + v` | 0 if `v` is even, 15 if odd |
| 16 | 15 | 0 |
| 17 | 0 | 15 |
| 18..31 | `v - 2` (16..29) | 0 if `v` is even, 15 if odd |

So for a wide atlas variations 0..15 select the extra full cells, 16 and
17 select the two full cells of the left block, and the rarely used
18..31 select cells 16..29 again. Cells 30 and 31 are never drawn as
full tiles.

## Edge cases

- The variation of a tile's points other than the bottom left one has no
  effect on that tile.
- The cell used by a higher layer (step 3) does not depend on any
  variation.

## Open questions

1. Whether the full tile of the lowest texture is drawn opaque or with its
   alpha. With the shipped atlases this makes no difference (the full
   cells are opaque). With a test atlas whose cell 0 was transparent, a
   wide texture with index 0 still showed the cell's colour, while a
   narrow texture with index 1 showed black; not investigated further.
2. Cliffs, ramps, water, blight and the boundary.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17, read from screenshots taken
  with a top-down camera. The maps replace the ground textures
  `TerrainArt\Northrend\North_dirt.blp` (index 0, wide),
  `North_dirtdark.blp` (index 1, narrow), `North_rock.blp` (index 2, wide)
  and `North_Grass.blp` (index 3, wide) by test atlases: each texture in
  one solid colour, full cells labelled with their number, and cells
  1..14 with circular alpha masks of radius 0.55 tile around the corners
  named by their bits, so that masks of neighbouring corners overlap.
- The alpha of the shipped atlases (`North_dirtdark.blp`,
  `North_dirt.blp`) for the corner bits and the opaque cells 0 and 15.

## Verification

Flat test map (all points at the same height and layer, no water or
cliffs), camera straight down (angle of attack 270, rotation 90).

1. Isolated tiles whose four corners have texture 0 (wide) and variation
   `k` for `k = 0..31`: the labels read `16..31, 15, 0, 16..29`. The tile
   just right of each test tile (left corners `k`) shows the same label,
   the tile just left of it shows the background's.
2. The same with texture 1 (narrow): labels 0 and 15 alternating with
   the parity of `k`.
3. Points with variations 0, 4, 8, 12 by the parity of x and y: every
   tile shows the label of its bottom left point.
4. 24 tiles with the 24 orders of textures 0..3 at the four corners, and
   32 tiles with three or two textures: in every sample point (centre,
   edge midpoints where two corner masks overlap, corners) the colour is
   the one predicted by drawing the lowest index full and the others in
   increasing order (559 of 559 samples; drawing in decreasing order
   mispredicts 307).
5. Tiles whose bottom left point has another texture than the lowest
   one, with variation 5 or 6 there: the full tile shows cell 21 or 22.
