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

## Edge cases

- The clamp at 1 is per channel and applied to the light before the
  texture: at Ashenvale noon all three channels saturate and a white
  texture shows pure white.
- In the Lordaeron model the keys from 6:00 (`t` 15333) to 17:21
  (`t` 43700) are equal and their tangents zero, so the light is constant
  there.

## Open questions

1. Sloped ground (normals other than straight up), cliffs and water were
   not measured; the formula above is expected to use the vertex normal.
2. The unit model (`...Unit.mdx`) and how units, doodads and destructables
   are lit.
3. Fog, and whether the light changes with `SetDayNightModels` called
   during the game (only calls in `main` were tested).
4. The time between the samples of 3 hours was not measured; the samples
   at 3:00 and 21:00 fall inside Hermite segments and match.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17: flat ground with all ground
  textures replaced by solid white (or grey, red) images, camera straight
  down, time of day set with `SetFloatGameState` to 0, 3, ..., 21, the
  colour of the centre of the screen read from screenshots.
- The day/night models in the game archives, `UI\WorldEditData.txt`.

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
