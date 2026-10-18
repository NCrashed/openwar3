---
id: SPEC-0022
title: Buildings, destructables and doodads as obstacles
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (process memory, test maps)"]
---

## Behaviour

Path finding (SPEC-0020, SPEC-0021) sees, besides the cells of the
pathing map and the units, the footprints of buildings and of
destructables (trees, gates, rocks, ...). Doodads need nothing at run
time: their footprints are already part of the map's `.wpm`.

### Doodads

The game does not stamp doodads (objects of `Doodads\Doodads.slk` in
`war3map.doo`) into the pathing grid. The map's `war3map.wpm` already
contains them: in `(12)IceCrown`, of the 1484 doodads whose type has a
pathing texture, 1482 lie on `.wpm` cells that are marked not walkable
under every red pixel of their texture. A doodad placed in a test map's
`war3map.doo` without a matching `.wpm` changed no cell.

Destructables in the same file are not part of the `.wpm` (4431 of the
4471 trees and other destructables of `(12)IceCrown` stand on free
`.wpm` cells; the others overlap doodads or cliffs) and are added at
run time as described below.

### Pathing textures

Buildings take their texture from the field `pathTex` of their unit
type (`Units\UnitData.slk`), destructables from `pathTex` and, after
death, `pathTexDeath` of `Units\DestructableData.slk` (`_` or `none`
for no texture). A texture is an uncompressed TGA image in the game
archives (`PathTextures\...`), one pixel per cell of 32 x 32; the
bottom row of the image is the row of smallest y. A pixel blocks:

- **red** (red channel 255): ground units, in every check of SPEC-0020
  and SPEC-0021 (high-level search, local search, seeing a point,
  look-ahead check);
- **green**: flying, and for ground units only the high-level search
  (the same as `.wpm` flag 0x04, SPEC-0020);
- **blue**: building (not studied further).

Only the values 0 and 255 occur in the textures of buildings and
destructables. All their widths and heights are even.

The cells are those of the texture, possibly rotated, placed at the
object's position:

1. **Rotation.** The angle used is the object's facing in degrees; for
   a destructable whose type has `fixedRot` 0 or more, `fixedRot`
   instead (`-1`: the facing given at creation). Only the exact values
   0, 90 and 180 rotate the texture (the facing as stored, so 89.99999
   from a facing of 90 counts as 90): by 90 degrees counter-clockwise,
   by 180 degrees, and by 90 degrees clockwise. Any other angle
   (270, 360, 0.5, 45, 89.5, ...) leaves the texture as it is. For a
   cell `(i, j)` relative to the texture centre (`i`, `j` from `-w/2`
   to `w/2 - 1`), 90 degrees counter-clockwise gives `(-j - 1, i)`,
   180 degrees `(-i - 1, -j - 1)`, 90 degrees clockwise
   `(j, -i - 1)`. The scale of a destructable does not matter.
2. **Snapping.** The object itself is moved to the grid, each world
   coordinate `p` separately, with `w` the size of the (rotated)
   texture along that axis in cells: if `w / 2` is even,
   `p' = 64 * trunc(p / 64)`; if `w / 2` is odd,
   `p' = 64 * trunc(p / 64) + 32` for `p > 0` and `- 32` for `p < 0`
   (`trunc` towards zero, world coordinates). `GetUnitX` /
   `GetDestructableX` return the moved position. This applies to
   `CreateUnit` with a building type, to `CreateDestructable` and to
   destructables of `war3map.doo`.
3. **Placement.** With `C` the moved position in cells (relative to
   the map's lower left corner), the texture covers the cells
   `C - w/2 .. C + w/2 - 1` along each axis.

A building also occupies the cells of its own `collision` as a unit
(SPEC-0021); in the tests these lay inside its texture.

### Death of a destructable

When a destructable dies its texture stops blocking at once, and its
`pathTexDeath`, if any, is placed in the same way instead (a gate
leaves only its posts). A killed tree blocks nothing.

### What the walker sees

All these cells are static obstacles: the high-level search, the local
search, seeing a point and the look-ahead check treat them like
unwalkable `.wpm` cells (blocks containing them are not free in the
quadtree). Footmen walking past buildings followed the same paths as a
model in which the red cells of the buildings were written into the
`.wpm`.

## Edge cases

- `fixedRot` 225 (some ramps) is not one of the rotating angles: such
  textures are used as they are.
- A destructable created with a facing near but not equal to 0, 90 or
  180 is not rotated (`XTmx`, an 8 x 2 texture: horizontal at 30, 45,
  60, 135 and 270 degrees, vertical at 0 and 180).

## Open questions

1. Whether the snapping is relative to world coordinates or to the map
   corner (the test maps had their corner at -12288, a multiple of 64).
2. Preplaced buildings from `war3mapUnits.doo`, buildings built by
   workers, and the removal of a building's footprint when it dies.
3. Red values other than 0 and 255 (only found in doodad textures,
   which the game does not use at run time).
4. When exactly the texture of a dying destructable changes (it had
   changed at the first reading, 30 ticks after a `KillDestructable` at
   map initialisation).

## Sources

- Reading the pathing grid and its obstacle records from the memory of
  the running game, test maps on 1.27b under Wine, 2026-10-18.
- `PathTextures\*.tga`, `Units\UnitData.slk`,
  `Units\DestructableData.slk`, `Doodads\Doodads.slk` of the game
  archives; `(12)IceCrown.w3m`.

## Verification

Flat test map, `.wpm` without obstacles, map corner at (-12288, -12288).

1. **Snapping and placement of buildings** (`CreateUnit`, `Player(0)`),
   given position -> `GetUnitX/Y` -> cells of the red area:

   | Type (texture) | Given | Moved to | Red cells x, y |
   |---|---|---|---|
   | `hhou` (4x4SimpleSolid) | (-8000, -8000), facing 0 | (-8000, -8000) | 132..135, 132..135 |
   | `hhou` | (-7999.5, -5000.5) | (-7936, -4992) | 134..137, 226..229 |
   | `hhou` | (465, -5065) | (448, -5056) | 396..399, 224..227 |
   | `htow` (16x16Simple) | (-5552, -4352) | (-5504, -4352) | 206..217, 242..253 |
   | `unpl` (UndeadNecropolis), facing 270 | (-4400, -8000) | (-4352, -8000) | 244..251, 130..139 |
   | `unpl`, facing 0 | (-8000, -8000) | same | 128..137, 130..137 |
   | `unpl`, facing 90 | (-6800, -8000) | (-6784, -8000) | 168..175, 128..137 |
   | `unpl`, facing 180 | (-5600, -8000) | (-5568, -8000) | 206..215, 130..137 |
   | `unpl`, facing 45, 135, 225, 315, 300, 360, 0.5 | | | as facing 270 |

2. **Destructables** (`CreateDestructable`):

   | Type (texture, `fixedRot`) | Given, facing | Moved to | Red cells x, y |
   |---|---|---|---|
   | `LTlt` (4x4Default, 270) | (-4975, -7990), 270 | (-4928, -7936) | 228..231, 134..137 |
   | `XTmx` (8x2Default, -1) | (-8000, -6000), 0 | (-8032, -5952) | 132..133, 194..201 |
   | `XTmx` | (-6800, -6000), 30 | (-6784, -5984) | 168..175, 196..197 |
   | `XTmx` | (400, -6000), 0, scale 0.5 | (416, -5952) | 396..397, 194..201 |
   | `LTg1` (Gate1Path, 270) | (-6400, -4000), 0 | (-6400, -3968) | 174..193, 258..261 |
   | `LTg3` (Gate1Path, 0) | (-1600, -4000), 270 | (-1600, -3968) | 332..335, 250..269 |

   Destructables of the same types placed through `war3map.doo` gave
   the same results (17 of 17).
3. **Death.** `LTg1` at (2000, 2000) killed at creation: from then on
   only the 16 red cells of `Gate1PathDeath` block (x 436..455,
   y 444..447 as the texture shows them); a killed `LTlt` blocks
   nothing.
4. **Walkers past buildings.** Footmen from (-2700, y) to (-1300, y),
   a building at (-2000, y + d): town hall `htow` (d = 0 and -150),
   barracks `hbar` (d = 40), necropolis `unpl` facing 0 (d = -60) and
   270 (d = 100), farm `hhou` (d = 30); y = -8000, -6000, ..., 2000. A
   model following SPEC-0018, SPEC-0020 and this spec reproduced all six
   trajectories within 0.07 world units at every tick.
5. **Green only.** Destructables `YTac` (4x4Cyan, blue and green) at
   (-2000, -6000) and `YTab` (2x2Cyan) at (-2000, -3000), footmen from
   (-2500, y) to (-1500, y): both walkers took a high-level detour but
   crossed the cells in their local paths; a model with the rule of
   SPEC-0020 (0x04 only for the high-level search) reproduced them
   within 0.01, one blocking the cells in every check missed by up to
   34 world units. The same with `.wpm` cells of byte 0x44 in place of
   the destructables (test BC2): the same trajectories.
