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
7. **Texture.** The models have one texture, the replaceable cliff
   texture, and their texture coordinates cover the whole atlas (rock in
   the upper half, ground in the lower half). With the cliff type's
   `texDir` and `texFile` (`ReplaceableTextures\Cliff`, `Cliff0` or
   `Cliff1`) and the map's tileset letter `t`, the texture is the first
   that exists of:
   1. `<texDir>\<texFile>.blp` in the map;
   2. `<texDir>\<t>_<texFile>.blp` in the game archives (`War3.mpq` has
      them for A, B, C, D, F, G, N, Q, V, W, X, Y);
   3. `<texDir>\<texFile>.blp` in the game archives.
   A `<t>_<texFile>.blp` imported into the map is not used. So the
   tilesets I, J, K, O, Z, which have no such files, would use the
   plain `Cliff0` (Lordaeron dirt) and `Cliff1` (Lordaeron grass). This
   does not hold for `CIsn` on tileset I, whose cliffs look icy unless
   the map imports `Cliff1` (open question 6); J, K, O and Z were not
   tested.
   Evidence: tileset N and I maps importing plain `Cliff0`/`Cliff1` in
   red and green showed red and green cliffs on both; importing only
   `N_`/`I_` versions changed nothing; without imports, `CNdi` cliffs
   show icy rock (`N_Cliff0`) on N and brown dirt and rock (`Cliff0`) on I.
   The snow along the edges of `CNsn` cliffs is ground, see rule 8.
   On tileset I with cliff type `CIsn` (`texFile` `Cliff1`), replacing
   `Cliff1` by an image whose upper half is blue and lower half red made
   the visible faces of the cliff models almost entirely red: the models
   show mostly the lower (ground, for `Cliff1` grass) half of the atlas.
   Without the replacement the same faces look white and blue under the
   Icecrown lighting; no other texture source was found (open question 6).
8. **Ground next to cliffs.** Every tile that gets a cliff model gives
   its four corner points the ground texture `groundTile` of its cliff
   type (`CliffTypes.slk`: `Ndrt` for `CNdi`, `Nsnw` for `CNsn`, ...),
   whatever texture `war3map.w3e` stores for them. The ground is then
   drawn with these textures by the rules of SPEC-0012, so a band one
   tile wide on both sides of every cliff shows the cliff type's ground.
   Tested with solid test textures (all `war3map.w3e` points `Ndrt`): a
   single raised point and a 3 x 3 raised block with cliff type `CNsn`
   only on the raised points showed snow exactly on the tiles predicted
   by this rule (144 of 144 tiles; the cliff tiles on the lower left
   sides take type `CNdi` from their bottom left corner and so give
   `Ndrt`), and with `CNsn` on all points the band appears on all sides.
   When a point is a corner of cliff tiles of different types, the
   tests did not tell which type wins (open question 5).
9. **Light and shadow.** The horizontal parts of a cliff model (its top
   and its foot) are part of the model, textured from the atlas like
   its faces. Nothing of the ground textures is drawn on a cliff tile
   or blended into it: the border between a cliff tile and the ground
   tile next to it is a hard tile edge. In shipped maps it does not
   show because that ground has the cliff type's `groundTile` (rule 8)
   and the ground half of the atlas is painted to match it (`N_Cliff1`
   and `Nsnw` on Northrend).
   Cliff models are lit with the formula of SPEC-0017 ("Colour of the
   ground") and the light of the terrain model (not that of the unit
   model), using the normals of the model's vertices, turned with the
   model when it is placed. The normals of the horizontal parts are
   not all vertical: they lean towards the cliff face (`CliffsAAAB0`:
   from `(0, 0, 1)` at the low corner opposite the raised one to
   `(-0.18, 0.24, 0.95)` at the raised corner, in model coordinates),
   so a cliff top is lit a little differently from flat ground next to
   it. The shadow map darkens cliff models in the same way as the ground
   (SPEC-0017, "Shadows").

### Ramps

Ramps are made of points with the ramp flag (1). Each corner point of a
tile gets a letter; the tile's pattern is its four letters in the order
TL, TR, BR, BL.

**"Upper" points.** Look at the up to four tiles that have the point as a
corner, and at all ramp points among their corners. If those ramp points
do not all have the same level, the point is upper when its own level
equals their highest level.

**Letters.**

- A point without the ramp flag: `B` if it is upper, else `A`.
- A ramp point that is upper: `H`.
- Other ramp points, with `N` the point and its 8 neighbours (those
  inside the map):
  - invalid (no letter) when all 9 exist, are ramp points and have the
    same level, or when the levels of the ramp points in `N` differ by
    more than 1;
  - "vertical" when the neighbours south and north are both ramp points
    with different levels; "horizontal" likewise for west and east;
  - `spread` = highest minus lowest level over all points of `N`;
  - both vertical and horizontal: `S` if `spread <= 1`, else invalid;
  - exactly one of them: `R` if `spread <= 1`, else invalid;
  - neither: `S` if all 9 exist and are ramp points, else `L`.

**Ramp models.** A tile whose pattern is one of the following gets a ramp
model, which also covers a second tile at the given offset:

| Pattern | Second tile | Pattern | Second tile |
|---|---|---|---|
| `HRAA`, `HRAB` | (x+1, y) | `RHAA`, `RHBA` | (x-1, y) |
| `AARH`, `BARH` | (x+1, y) | `AAHR`, `ABHR` | (x-1, y) |
| `AHRA`, `BHRA` | (x, y-1) | `ARHA`, `ARHB` | (x, y+1) |
| `HAAR`, `HBAR` | (x, y-1) | `RAAH`, `RABH` | (x, y+1) |

The model is
`Doodads\Terrain\<rampModelDir>\<rampModelDir><pattern with R replaced by L>0.mdx`
(`rampModelDir` from `CliffTypes.slk`, `CliffTrans` or `CityCliffTrans`),
variation always 0: `HRAA` -> `CliffTransHLAA0.mdx`, `RABH` ->
`CliffTransLABH0.mdx`. Neither tile draws its ground.

**Cliff type of a ramp model.** Look at the tile itself and its east,
west and north neighbours, in this order, keeping only those that have an
`R` or `S` corner. Among their corners (per tile in the order BL, BR, TL,
TR) take the first one with the highest level; the ramp uses that point's
`ctex` (15 counts as 0). If no tile qualifies, type 0 is used.

**What each tile gets.** For every tile:

1. If it has an `R` or `S` corner, it is a ramp tile: it starts a ramp
   model when its pattern is in the table above; otherwise it gets no
   model. It gets no cliff model either way.
2. Otherwise, if its corner levels differ, it gets a cliff model (rules
   of "Cliff models"), also when some of its corners are ramp points
   (letters `H` or `L`).
3. Its ground is drawn unless it got a cliff model, starts a ramp model,
   or is the second tile of a ramp model (its pattern is the "second"
   pattern of a table entry: `RLAA`, `LRAA`, `AALR`, `AARL`, `ARLA`,
   `ALRA`, `RAAL`, `LAAR`). Ramp tiles without a model, such as the middle
   of a wide ramp (`LRRL`, `RHHR`), are drawn as ordinary ground with the
   corner heights of "Ground height".

In `(4)LostTemple.w3m` these rules give 58 ramp models with 14 different
names, all of which exist in the game archives. Replacing
`CliffTransLABH0.mdx` and `CliffTransALHB0.mdx` in a copy of the map
changed exactly the two predicted places (tile (33, 30) with second tile
(33, 31), and tile (24, 32) with (24, 33)); every other ramp looked as
before.

The same map has 17 tiles with ramp points but no `R` or `S` corner and
different levels (pond edges in the north-east, for example tiles
(127, 110) and (127, 111) with models `CliffsBACB` and `CliffsBBAB`). They
get cliff models by rule 2: replacing `CliffsBACB*` and `CliffsBBAB*` in a
copy of the map turned exactly those spots into the replacement models.
An implementation that gives no cliff model to tiles with ramp points
leaves holes there.

## Edge cases

- With small raised areas (3 x 3 points of level 7..15 inside level 2),
  `GetLocationZ` at the centre point gave 512 for levels 6..9 and 768
  for 10..15; with 9 x 9 areas the values follow the formula. Not
  explained (open question 3).

## Open questions

1. Ramps: what the `CliffTrans` models with `X` or `C` in their names are
   for (no pattern above produces them), and an internal per-tile flag
   that makes the game skip a tile entirely (seen in the code, not
   observed in maps).
2. Placement of the cliff model (origin, height) and how its vertices
   follow the ground heights of the corners.
3. The clamping of small raised areas above.
4. Water texture animation and shorelines.
5. Ground next to cliffs (rule 8): which cliff type wins at a point shared
   by cliff tiles of different types, whether a `groundTile` missing from
   the map's ground list is added, and what `upperTile` (only `COrd` sets
   one) does.
6. Where the white and blue look of `CIsn` cliffs on tileset I comes
   from. It is not lighting: importing into the map the very
   `ReplaceableTextures\Cliff\Cliff1.blp` of `War3.mpq` (grass) made
   the same cliffs green, so without the import the game draws them with
   a texture other than that file. No other file of that name, and no
   `I_Cliff1.blp`, exists in any archive of the installation, and only
   the plain names `ReplaceableTextures\Cliff\Cliff0.blp` and
   `...\Cliff1.blp` were found in the memory of the running game. The
   cliff tops do not take the map's imported `Ice_Snow` ground texture
   either.

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
6. **Cliff tiles of `(12)IceCrown`** (tileset N, cliff types `CNdi` and
   `CNsn`), around the gold mine at (6528, -256), camera straight down,
   noon, Lordaeron light, doodads removed (rule 9):
   - `Cliff1` imported as solid magenta: every cliff tile is a magenta
     square with straight edges, its top included;
   - `North_Snow` imported as solid green: the ground turns green, the
     cliff tiles keep the snow of the atlas, again with straight square
     edges;
   - both imported as solid grey 136 (drawn 136): flat ground
     `(131, 131, 135)`, the tops of the cliff tiles mostly
     `(136, 136, 136)` (their normals lean towards the sun, so the light
     saturates); at 0:00 flat ground `(42, 72, 108)` and cliff tops
     between `(41, 70, 105)` and `(49, 84, 126)`;
   - the same with a `war3map.shd` of all 255: ground and cliff tops
     `(74, 74, 80)`; with all 0: as without shadows.
7. **Terrain light on cliffs.** Flat test map, one point raised by
   two levels, cliff type `CNsn`, `Cliff1` and the ground solid grey 136,
   Lordaeron light at noon: the outer corners of the four cliff tiles,
   where the models have the normal `(0, 0, 1)`, show 131 like the flat
   ground; the tiles facing away from the sun go down to 121..128. With
   the light of `DNCLordaeronUnit` (ambient intensity 0.3) every upward
   normal would saturate at 136.
8. **Tileset I** (test map with `CIsn` and `CIrb` cliffs): without
   imports the `CIsn` cliffs look icy white and blue; with the `Cliff1`
   of `War3.mpq` imported unchanged into the map they show green grass
   and brown rock (open question 6).
