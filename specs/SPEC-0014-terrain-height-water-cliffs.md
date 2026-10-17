---
id: SPEC-0014
title: Terrain height, water and cliff models
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps with screenshots)"]
---

## Behaviour

Notation for a corner point of `war3map.w3e`: `h` the 16-bit ground
height field, `w` the low 14 bits of the water field, `layer` the low 4
bits of the last byte (cliff level), `ctex` its high 4 bits (cliff
texture), `cvar` the high 3 bits of the texture details byte (cliff
variation), and the point flags (high 4 bits of the texture byte).
Only the water flag (4) was tested here; the ramp flag (1) is checked by
the ramp code.

### Ground height

- The height of a point is `(h - 8192) / 4 + (layer - 2) * 128` for
  layers 0..14 (raw 7192 -> -250, 9192 -> +250; layer 0 -> -256,
  14 -> 1536). Layer 15 gives the same height as layer 14.
- Inside a tile, `GetLocationZ` interpolates the four corner heights
  bilinearly (checked on 81 points of two sloped tiles).
- Across a cliff or a ramp `GetLocationZ` still interpolates the corner
  heights: from layer 2 to layer 4 over one tile the value rises
  linearly from 0 to 256. The cliff models do not affect it.

### Water

- **Which tiles.** A tile shows water when at least one of its four
  corners has the water flag (4). The whole tile is drawn (a single
  flagged point gives a 2 x 2 tile square, a 2 x 2 block of points a
  3 x 3 square, with straight edges).
- **Surface height.** `(w - 8192) / 4 + 128 * height` where `height` is
  the column of `TerrainArt\Water.slk` for the map's tileset (-0.7 for
  Northrend: -89.6 for `w = 8192`). Water is visible where it is above
  the ground.
- **Colour.** The water surface is coloured per vertex from the depth
  `d` = surface height - ground height at the vertex. The constants come
  from `[Water]` in `UI\MiscData.txt` (`MinDepth` 10, `DeepLevel` 64,
  `MaxDepth` 72) and the colours from `Water.slk` (`Smin_*`, `Smax_*`,
  `Dmin_*`, `Dmax_*`; A, R, G, B).
  1. A table of 256 colours is built: `k = trunc((DeepLevel - MinDepth) /
     (MaxDepth - MinDepth) * 255)` (222 with the shipped values). Entries
     `0..k` go linearly from `Smin` to `Smax`, entries `k..255` from
     `Dmin` to `Dmax` (entry `k` ends up as `Dmin`). Entry `i` of a run
     from `i0` (colour `c0`) to `i1` (colour `c1`) is, per channel,
     `trunc((c1 - c0) * (i - i0) / (i1 - i0) + c0)`.
  2. A vertex with `d <= MinDepth` uses entry 0, with `d >= MaxDepth`
     entry 255, otherwise entry `trunc((d - MinDepth) * 255 /
     (MaxDepth - MinDepth))`.
- A `TerrainArt\Water.slk` imported into a map is not used (the game's
  copy is read before the map is loaded).
- The water texture (`texFile`, `numTex` frames), shoreline doodads
  (`shore*` columns) and `WavesDepth` were not examined.

### Cliff models

Cliff models are chosen per tile from the cliff levels of its corners
BL (x, y), BR (x+1, y), TL (x, y+1), TR (x+1, y+1). Tiles with ramp
flags use other models (open question 1).

1. If all four levels are equal, the tile has no cliff model.
2. Let `lo` be the lowest level. Levels higher than `lo + 2` are
   lowered to `lo + 2`.
3. Each corner gets the letter `'A' + (level - lo)`. The model name is
   `Doodads\Terrain\<cliffModelDir>\<cliffModelDir><TL><TR><BR><BL><n>.mdx`,
   with `cliffModelDir` from `TerrainArt\CliffTypes.slk` (`Cliffs` or
   `CityCliffs`), for example `CliffsAAAB0.mdx` for a tile whose bottom
   left corner is one level higher.
4. The cliff type is that of the first corner, in the order BL, BR, TL,
   TR, whose `ctex` is not 15; its `cvar` is the variation value. If all
   four have `ctex` 15, the points of the 5 x 5 square around (x, y) are
   searched (x - 2 .. x + 2 outer, y - 2 .. y + 2 inner) for the first
   with `ctex` not 15; if there is none, type 0 and variation 0 are used.
   `ctex` indexes the cliff list of `war3map.w3e`.
5. The number of variations of a name is the number of files that exist
   for `n = 0, 1, 2, 3`, counting from 0 and stopping at the first
   missing one. The model used is `n = cvar mod count`.
6. The ground of a tile with a cliff model is not drawn (the model
   brings its own ground).

## Edge cases

- With small raised areas (3 x 3 points of level 7..15 inside level 2),
  `GetLocationZ` at the centre point gave 512 for levels 6..9 and 768
  for 10..15; with 9 x 9 areas the values follow the formula. Not
  explained (open question 3).

## Open questions

1. Ramps (`CliffTrans` models, letters `H`, `L`, `X`, two-tile models).
2. Placement of the cliff model (origin, height) and how its vertices
   follow the ground heights of the corners.
3. The clamping of small raised areas above.
4. Water texture animation and shorelines.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17: `GetLocationZ` on a flat
  test map with chosen heights and levels; top-down screenshots for
  water (custom solid ground textures) and for cliffs.
- Analysis of the 1.27b game library for the colour table of the water
  and the cliff model selection.
- The comments and values of `UI\MiscData.txt` `[Water]`.

## Verification

1. Points at raw heights 7192..9192 (step 100) and plateaus of levels
   0..15: the heights above.
2. Tiles with corner heights 0, 100, 200, 400 and 400, 0, 0, 0:
   bilinear values at a 9 x 9 grid.
3. Water over a slope: no water where the ground is above -89.6; the
   colour changes with depth between 10 and 72 and is constant beyond.
4. Water flags on single points, a 2 x 2 block, a row of three: water on
   every tile with a flagged corner.
5. One point raised by one level, with `CliffsAAAB*`, `CliffsABAA*`,
   `CliffsBAAA*` and `CliffsAABA*` replaced in the map by a footman,
   knight, peasant and rifleman model: the footman shows in the tile to
   the north-east of the point, the knight south-west, the peasant
   south-east, the rifleman north-west. A point raised by three levels
   uses models with `C`.
