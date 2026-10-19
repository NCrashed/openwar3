---
id: SPEC-0017
title: Day/night lighting of the ground
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps with screenshots, game model files)"]
---

## Behaviour

### Which light models

The light comes from two "day/night" models, one for the terrain and one
for units, set by the script with `SetDayNightModels(terrainModel,
unitModel)`.

- The script names the models with the extension `.mdl`; the file loaded
  is the same path with `.mdx` (the archives ship only `.mdx`).
- **No call:** the terrain light is that of
  `Environment\DNC\DNCLordaeron\DNCLordaeronTerrain\DNCLordaeronTerrain.mdx`,
  whatever the tileset letter (measured equal for `N`, `A` and `I`).
- The World Editor writes one `SetDayNightModels` call at the start of
  `main`, with the models listed for the tileset in the sections
  `[TerrainLights]` and `[UnitLights]` of `UI\WorldEditData.txt` (it
  writes the Lordaeron models also for Northrend and Icecrown maps). The
  game itself does not read that file for lighting: the light is what
  the map script asks for.

### The light in the model

A day/night model is an MDX file with one sequence (`Stand`, 333 to
60333 ms) and one light object (`LITE` chunk, a directional light named
`FDirectSun`). The values used:

- `color` and `intensity` (static light colour and intensity),
  `ambColor` and `ambIntensity` (static ambient colour and intensity);
- the tracks `KLAC` (light colour) and `KLBC` (ambient colour), which
  replace the static colours when present. All shipped terrain models
  have both, with Hermite interpolation. Intensity tracks (`KLAI`,
  `KLBI`) would replace the static intensities; none of the tested
  models has them.
- the rotation of the light's node (`KGRT`, a quaternion `x, y, z, w`;
  the tested models have a single key).

**Colours are stored blue, green, red**: the first component of a colour
is blue. (Lordaeron night: stored `(0.80, 0.53, 0.31)` is the bluish
`r = 0.31, g = 0.53, b = 0.80`.)

Measured static intensities: `intensity` 1.3 in all terrain models;
`ambIntensity` 0.2 (Lordaeron), 0.3 (Ashenvale, Dalaran, Dungeon,
Underground), 0.25 (Felwood).

### Time of day

The time of day `h` (hours, `GAME_STATE_TIME_OF_DAY`, 0 <= h < 24) is
the animation time `t = 333 + 2500 * h` ms of the sequence, so the
24-hour day spans the whole sequence.

A game starts at 8:00 and the clock advances one hour per 20 s of game
time. It changes at every tick and is computed from a base hour and the
game time elapsed since that base was set, in the engine arithmetic of
SPEC-0004:

    h = add(B, mul(sub(G, G0), 0x3D4CCCCD))      (0x3D4CCCCD = 0.05)

- `G` is the game time, the sum of the tick length `0x3BA3D70A` added
  once per tick from the start of the game (`add`, so it truncates;
  it does not wrap at 300 s like the timer queue of SPEC-0002).
- At the start `B` = 8.0 and `G0` = 0.
- `SetFloatGameState(GAME_STATE_TIME_OF_DAY, x)` sets `B` = `x` and
  `G0` = the current `G`.
- When `h` reaches 24 it starts again from 0. Taking `B = sub(h, 24.0)`
  and `G0` = the current `G` at that tick reproduces the samples after
  midnight to within 2e-6 hours, not bit for bit; the exact arithmetic
  of the wrap is open.

Measured: 1200 of 1200 consecutive ticks after 1 s of game time bit for
bit (8.050249 at the first, `0x4100CDD2`); after setting 12.0, the 12
ticks right after the call bit for bit and samples up to 230 s later
bit for bit up to one tick of the test's own counter (its 0.005 s
timer slipped by a tick now and then). There was no jump at 300 s of
game time. The truncation in `G` makes the clock
run a little slow: 2000 ticks after a reset to 12.0 the formula gives
12.499916, not 12.5.

At 8:00 the Lordaeron light is already
the constant day value, almost neutral white `(246, 246, 253)`: a strong
blue tint on a map that uses the Lordaeron model at the start does not
come from the light.

A track value at `t`:

- before the first key: the first key's value; after the last key: the
  last key's value;
- between keys `k0` (time `t0`, value `v0`) and `k1` (`t1`, `v1`), with
  `s = (t - t0) / (t1 - t0)`:
  - interpolation 0 (none): `v0`;
  - 1 (linear): `v0 + (v1 - v0) * s`;
  - 2 (Hermite): `h1 * v0 + h2 * v1 + h3 * out0 + h4 * in1` with
    `h1 = 2s^3 - 3s^2 + 1`, `h2 = -2s^3 + 3s^2`, `h3 = s^3 - 2s^2 + s`,
    `h4 = s^3 - s^2`, `out0` the out-tangent of `k0` and `in1` the
    in-tangent of `k1` (each key stores in-tangent, then out-tangent).

### Direction of the sun

The direction towards the light is the unit vector `(0, 0, 1)` rotated by
the node's quaternion `q`: `D = q (0,0,1) q^-1`. In the Lordaeron model
`q = (0.38153890, -0.21586396, -0.44258690, 0.78227085)`, which gives
`D = (-0.6755, -0.4059, 0.6157)`: the sun stands in the south west at
about 38 degrees, and flat ground gets `N . D = 0.6157`. Ashenvale,
Felwood and Dalaran have the same rotation; Dungeon and Underground give
`N . D = 0.8561` for flat ground.

### Colour of the ground

For a ground point with normal `N` (flat ground: `(0, 0, 1)`), per
channel `c` (red, green, blue):

    light_c = min(1, I * L_c * max(0, N . D) + AI * A_c)

with `L` the light colour and `A` the ambient colour at the current time,
`I` and `AI` the intensities. The pixel is the texture colour times
`light_c`.

Example, Lordaeron, 0:00 on flat ground: the first keys of both tracks
store `(0.799, 0.533, 0.313)`, so `L = A = (0.313, 0.533, 0.799)` and
`light = L * 1.3 * 0.6157 + 0.2 * A = (0.313, 0.533, 0.799)`, which is
`(80, 136, 204)` on a white texture; the game showed `(79, 135, 203)`.

### Models

Units, destructables and doodads (`war3map.doo`) are lit by the same
formula of "Colour of the ground", with three differences:

- the light comes from the **unit** day/night model of
  `SetDayNightModels` (`DNCLordaeronUnit` without a call) instead of the
  terrain model;
- `N` is the normal of the model's vertex, turned with the model;
- the clamp at 1 per channel applies as for the ground.

The shadow map (`war3map.shd`) does not darken units: a unit standing in
full shadow looked the same as without shadow. (Cliff models are lit as
ground, SPEC-0014 rule 9; water by the unit light without the clamp,
SPEC-0014.)

### Sloped ground

The ground is lit per corner point and the light is interpolated over
the tile:

1. **Normal of a point.** With `h(i, j)` the height of point `(i, j)`
   from its raw height alone, `(raw - 8192) / 4` (SPEC-0014, "Ground
   height", without the cliff level term `(layer - 2) * 128`), the
   normal is
   `N = normalize(-(h(i+1, j) - h(i-1, j)) / 256, -(h(i, j+1) - h(i, j-1)) / 256, 1)`:
   the central difference over the two neighbours, 256 world units
   apart. Cliff levels do not tilt the normals: ground beside a cliff
   and on a plateau, with equal raw heights, is lit like flat ground.
2. **Light of a point.** `light_c` above with that `N`.
3. **Inside a tile** the light of the four corners is interpolated
   linearly over two triangles, the tile being split along the diagonal
   from its top left corner `(i, j+1)` to its bottom right corner
   `(i+1, j)`.

On a plane the result is the plane's own normal.

### Shadows (`war3map.shd`)

The map file `war3map.shd` darkens the ground where the World Editor
baked the shadows of doodads and cliffs.

**Format.** One byte per cell of 32 x 32 world units, four cells per
tile in each direction: `4 (W - 1)` columns and `4 (H - 1)` rows for a
`war3map.w3e` of `W x H` corner points (`(12)IceCrown`: 193 x 193
points, 768 x 768 bytes). The first row is the southern one (smallest
y), each row runs from west to east, and cell `(i, j)` covers
x `X0 + 32 i .. X0 + 32 (i + 1)`, y `Y0 + 32 j .. Y0 + 32 (j + 1)` with
`(X0, Y0)` the map's lower left corner from `war3map.w3e`. A byte 0 is
no shadow; every other value is full shadow (1, 16, 32, 63, 127, 128,
200, 254 and 255 gave the same darkness). Shipped maps contain only 0
and 255.

**Smoothing.** The shadow amount `s` (0 to 1) at a point is not taken
from its cell directly:

1. The map is divided into blocks of 16 x 16 cells (4 x 4 tiles,
   512 x 512 world units), counted from the lower left corner `(X0, Y0)`.
2. Each block has 16 x 16 samples, placed at
   `X0 + 512 bx + m * 512 / 15`, `Y0 + 512 by + n * 512 / 15`
   (`m, n` = 0..15) for block `(bx, by)`: the first and the last sample
   of a row lie on the block's borders, so the samples are 34.13 units
   apart.
3. Sample `(m, n)` with `1 <= m, n <= 14` has the value of cell
   `(16 bx + m, 16 by + n)` (1 for shadow, 0 for none). A sample on a
   block border is the mean of the two cells that meet at that border:
   the block's own cell and the next cell of the neighbouring block (for
   `m = 0` the cells `16 bx - 1` and `16 bx`, for `m = 15` the cells
   `16 bx + 15` and `16 bx + 16`). Both blocks so have the same values
   along their common border. A sample on a block corner is taken as the
   mean of the four cells there (not measured separately).
4. `s` is the bilinear interpolation of the four samples around the
   point, within its block.

A single shadow cell inside a block thus shows as a soft spot of 34
units radius, whose centre lies up to 16 units away from the cell's
centre; a single shadow cell at a block border shows at half strength on
both sides of the border.

**Darkening.** With `t` the texture colour of the ground (per channel,
0 to 1), the shadowed colour before lighting is

    t' = t + s * a * (c - t),   a = 0.502,   c = (0.048, 0.048, 0.095)

(red, green, blue), and the pixel is `t'` times the light of "Colour of
the ground". Full shadow keeps about half of the texture colour and
adds a little dark blue; because the light multiplies the result, the
ratio of shadowed to lit colour does not depend on the time of day or
the light model. Cliff models are darkened in the same way (SPEC-0014,
rule 9).

### Terrain fog

Fog is set by the script with `SetTerrainFogEx(style, zstart, zend,
density, red, green, blue)`; the World Editor writes such a call into
`main` when the map has fog (23 of the shipped maps in the test
installation do, all with style 0). `(12)IceCrown`, whose script makes
no call, showed no fog. Whether the game applies the fog fields of `war3map.w3i`
(version 25) by itself was not tested. The colour components are 0 to
1.

The ground, units, destructables and water (the ones tested) are mixed
with the fog colour:

    pixel = (1 - f) * colour + f * fog colour

where `colour` is the pixel as described above (lit, shadowed) and `f`
depends on `z`, the depth of the point along the camera's view
direction (not its distance from the eye: looking straight down at
flat ground, `f` was the same over the whole screen):

- style 0 (linear): `f = (z - zstart) / (zend - zstart)`, limited to
  0 .. 1; `density` is not used;
- style 1: `f = 1 - exp(-density * z)`;
- style 2: `f = 1 - exp(-(density * z)^2)`;
- `zstart` and `zend` are not used by styles 1 and 2.

So the densities used by the shipped maps for style 0 (0 to 84.5) have
no effect, and a style 1 or 2 density of 0.5 hides everything.

## Edge cases

- The clamp at 1 is per channel and applied to the light before the
  texture: at Ashenvale noon all three channels saturate and a white
  texture shows pure white.
- In the Lordaeron model the keys from 6:00 (`t` 15333) to 17:21
  (`t` 43700) are equal and their tangents zero, so the light is constant
  there.

## Open questions

1. Water was not measured. Normals of points on the edge of the map
   (missing neighbours) were not measured.
2. Models: animated normals (bones), team colour, material flags
   (unshaded, two-sided) and other filter modes than opaque were not
   tested; the same model placed as a doodad was drawn about 1.22 times
   larger and about 26 units further east than as a unit or
   destructable (not explained).
3. Fog, and whether the light changes with `SetDayNightModels` called
   during the game (only calls in `main` were tested).
4. The time between the samples of 3 hours was not measured; the samples
   at 3:00 and 21:00 fall inside Hermite segments and match.
5. Shadows: whether destructables, doodads and water are darkened by
   `war3map.shd`; maps whose size in tiles is not a multiple of 4 (the
   last, partial block); sample positions were measured to within about
   5 world units (all measured positions lay 3 to 5 units west of the
   formula, not explained).
6. Fog: cliffs and doodads, the sky and the area outside the map,
   `SetTerrainFog` (the older native), `ResetTerrainFog`, the fog fields
   of `war3map.w3i`, how water and the ground under it are fogged (the
   three channels of a water pixel gave different `f`, 0.42 to 0.49, as
   if each layer were fogged by its own depth), and other styles than
   0, 1, 2.

## Sources

- Test maps of 2026-10-18: planes of 18 slopes (gradients 0.25, 0.5 and
  1 towards east, west, north, south, south west, north east), single
  raised points, a raised block of cliff level; grey ground, Lordaeron
  noon, camera straight down, screenshots compared pixel by pixel with
  the model (corrected for the perspective of raised ground).
- Test maps on 1.27b under Wine, 2026-10-17: flat ground with all ground
  textures replaced by solid white (or grey, red) images, camera straight
  down, time of day set with `SetFloatGameState` to 0, 3, ..., 21, the
  colour of the centre of the screen read from screenshots.
- The day/night models in the game archives, `UI\WorldEditData.txt`.
- Test maps on 1.27b under Wine, 2026-10-18: flat ground with solid grey
  and white textures and hand-made `war3map.shd` files (single cells,
  strips 1 to 8 cells wide, blocks, all values 1..255), camera straight
  down, profiles across the screenshots calibrated with a grid texture;
  a copy with the map corner moved by one tile to check where the blocks
  start.

## Verification

The test installation draws terrain textures with 4 bits per channel:
a texture value of 128 is drawn as 136 (`8 * 17`), 255 stays 255. A grey
texture gave `136 * light` (0:00: `(42, 72, 108)`; noon `(131, 131, 135)`),
consistent with the multiplication. Measured on white textures, game
values are 0 or 1 below the model:

| Hour | Lordaeron model | Game | Ashenvale model | Game |
|---|---|---|---|---|
| 0 | 80 136 204 | 79 135 203 | 66 121 161 | 66 121 161 |
| 3 | 120 161 231 | 119 160 231 | 98 165 223 | 97 164 222 |
| 6, 9, 12, 15 | 246 246 253 | 245 245 252 | 255 255 255 | 255 255 255 |
| 18 | 236 231 250 | 235 230 249 | 251 255 255 | 250 255 255 |
| 21 | 125 177 235 | 125 176 234 | 115 184 234 | 114 184 233 |

1. The Lordaeron rows hold with the call `SetDayNightModels` for
   Lordaeron, without any call, and with tileset letters `N`, `A`, `I`.
2. The Ashenvale rows hold with `SetDayNightModels` naming the Ashenvale
   terrain model.
3. **Shadows, strength.** Full shadow (`war3map.shd` cells 255, also any
   other nonzero value), flat ground, Ashenvale light at noon (light 1):
   grey textures 68, 136, 204, 255 give `(40, 40, 46)`, `(74, 74, 80)`,
   `(108, 108, 114)`, `(133, 133, 139)`. Lordaeron light, white texture:
   noon `(245, 245, 252)` lit, `(128, 128, 138)` shadowed; 0:00
   `(79, 135, 203)` and `(41, 70, 111)`. Ashenvale, white, 0:00:
   `(66, 121, 161)` and `(35, 63, 88)`; grey 136 at 0:00 `(35, 64, 86)`
   and `(19, 35, 51)`, at 21:00 `(61, 98, 124)` and `(33, 54, 73)`.
4. **Shadows, smoothing** (map corner at (-12288, -12288), white
   texture, Lordaeron noon, lit 245, full shadow 128 in the red
   channel). Vertical strips one cell wide, 12 cells long, at every fifth
   cell column: each shows as a profile 2 x 34 units wide, fully dark at
   its sample position `512 * floor(k / 16) - 12288 + (k mod 16) * 512 / 15`
   (`k` the column); strips at columns `k` with `k mod 16` = 0 or 15
   show at half strength (185) centred on the block border. One shadow
   cell on each side of the border at x = 0 (columns 383 and 384): full
   darkness at x = 0, falling to none 34 units to either side; either
   cell alone: 185 at x = 0. With the map corner moved to
   (-12160, -12160) the half-strength strips moved to x = 128 and 640.
5. **Slopes.** Lordaeron noon, grey 136, planes with height rising by
   `g` per world unit (red channel; `E` rises towards the east and so
   faces west): `g` 0.25: `W` 99, `S` 110, `SW` 80, `E`, `N`, `NE` 136
   (saturated); `g` 0.5: `W` 66, `S` 88, `SW` 34; `g` 1: `W` 23,
   `S` 49. The plane's normal in the formula gives 99.2, 110.7, 80.2,
   66.6, 87.9, 33.6, 22.8 and 49.0.
6. **Single raised points** (raw height +128 and +64 world units at one
   point of flat ground): the model of "Sloped ground" reproduces the
   screenshots with a mean error of 0.75 and 0.52 (of 255) in the red
   channel; with bilinear interpolation instead of the two triangles
   2.50 and 1.37, with one-sided differences 4 to 12.
7. **Cliff levels.** A 3 x 3 block of points one level up, raw heights
   all equal: the ground on the plateau and next to it is 131, like flat
   ground.
8. **Models.** A test model of six horizontal squares with set normals
   (up; towards the sun `D`; away from it; east; west; up and tilted 0.3
   to the west), grey texture 136, put in place of the footman model, of
   a destructable (`LTbr`) and of a doodad (`LOca`), facing 0, Lordaeron
   light. Noon: 136, 136, 136, `(34, 34, 40)`, `(34, 34, 40)`, 136 for
   all three; the unit light gives `(34.3, 34.3, 40.0)` for the squares
   without sunlight, the terrain light would give `(22.8, 22.8, 26.7)`.
   0:00: `(46, 79, 119)` (up), `(67, 115, 136)` (towards the sun),
   `(13, 22, 32)`, `(13, 22, 32)`, `(50, 85, 128)` (west), `(56, 95,
   136)` (tilted); the unit light gives `(46.9, 79.7, 119.6)`, `(68.2,
   115.9, 136)`, `(12.8, 21.7, 32.6)`, `(50.2, 85.4, 128.1)`, `(56.2,
   95.6, 136)`. The footman model in a map whose `war3map.shd` is all
   shadow: the same values as without it.
9. **Fog.** Flat white ground (drawn `(245, 245, 252)`), camera straight
   down at heights 500 to 6000, fog colour red:
   - style 0, 2000 .. 4000: no fog up to 2000; `(248, 184, 189)` at
     2500, `(250, 123, 126)` at 3000, `(253, 61, 63)` at 3500,
     `(255, 0, 0)` from 4000; the same at the centre and the corners of
     the screen;
   - style 1, density 0.001: `f` 0.392, 0.633, 0.776, 0.865, 0.951,
     0.984, 0.996 at 500, 1000, 1500, 2000, 3000, 4000, 6000 (formula:
     0.393, 0.632, 0.777, 0.865, 0.950, 0.982, 0.998);
   - style 2, density 0.0005: 0.061, 0.220, 0.429, 0.633, 0.894, 0.984,
     1.000 (formula: 0.061, 0.221, 0.430, 0.632, 0.895, 0.982, 1.000);
   - style 1 and 2 with density 0.5: full fog from 500 on.
   A unit, a destructable and water were fogged as well.

