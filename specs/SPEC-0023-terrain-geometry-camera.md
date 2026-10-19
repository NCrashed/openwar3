---
id: SPEC-0023
title: Terrain geometry on screen and the camera projection
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps with screenshots)"]
---

## Behaviour

### Heights are drawn at world scale

The ground is drawn at the heights of SPEC-0014 ("Ground height"), in
the same units as x and y: one world unit up is as long on screen as
one world unit across at the same distance from the camera. A raw
height difference and a cliff level count alike: ground raised by
`(raw - 8192) / 4 = 128` and ground one cliff level up are drawn at the
same height.

### Field of view

A 3D view is computed for a frame of the interface (SPEC-0028): the
world view for the world frame between the bars, left 0, bottom 0.13,
right 0.8, top 0.58 in the interface's 0.8 x 0.6 units, so `W / H` =
0.8 / 0.45 = 16:9; a unit portrait for the portrait frame. With `a = W /
H` and `fov` the camera's field of view (`CAMERA_FIELD_FIELD_OF_VIEW`
for the world, 70 degrees by default, at most 120: a script setting of
150 read back as 120), the field of view is an angle spread over the
frame's **diagonal** in proportion to its sides:

- vertical: half-angle `v = (fov / 2) / sqrt(1 + a * a)`; the frame's
  top and bottom edges are at `tan(v)` above and below its centre, at
  distance 1;
- horizontal: its left and right edges at `a * tan(v)`.

Each value is computed in single precision (`fov` in radians times
`1 / sqrt(1 + a * a)` times 0.5, then the tangent). For the world frame
`sqrt(1 + a * a)` = 2.0397: 70 degrees gives `tan(v)` = 0.3088 and
0.5490 across, 120 degrees 0.5640 and 1.0027.

The projection matrix is the usual perspective one, with `1 / (a *
tan(v))` and `1 / tan(v)` on the diagonal for x and y and the camera's
near and far planes `n`, `f`: `(n + f) / (f - n)` and `-2 f n / (f - n)`
in the depth row and column. The camera's target point is drawn at the
**centre of the frame** (for the world 0.355 of the height from the
bottom, above the middle of the window), and the picture goes on with
the same projection beyond the frame, under the interface bars.

The interface's 0.8 x 0.6 is then stretched to fill the game window:
in a window that is not 4:3 the picture is scaled unevenly. In the test
window of 1016 x 734 pixels, flat ground seen straight down at distance
1000 had 116.75 pixels per tile across and 112.5 up the screen (the
rule: 118.4 and 114.1 for an eye 1000 above the ground), a ratio of
0.9636, which is `(734 / 1016) / (3 / 4)`.

Measured in a 1024 x 768 window (test RUW): paused footmen 100 apart on
flat ground, camera straight down at distance 1000 (eye 1022.9 above the
ground): 101 pixels per 100 across at 70 degrees and 55.25 at 120, a
ratio of 1.83 (the rule: 1.826; the field of view across a 4:3 frame
would give 1.89, and a tangent spread over the diagonal, `0.8 tan(fov /
2)` across, 2.47). The row of footmen was drawn about 323 pixels from
the top of the window (the frame's centre: 314; the window's: 384).

### Camera position

With the camera fields of `SetCameraField` (`ANGLE_OF_ATTACK` `a`,
`ROTATION` `r`, `TARGET_DISTANCE` `d`), the eye is at distance `d` from
the target point, looking at it; `a` = 270 looks straight down and 360
horizontally. The height of the target point follows the ground, but not
exactly `GetLocationZ` at the target: with the target on the edge of
ground raised by 128 (and by 256), where `GetLocationZ` is 128 (256), the
eye was 1036 (1087) above the low ground for `d` = 1000, i.e. the target
was at about 36 (87). On flat ground the eye is at height `d` above it.

### Where models are drawn

- **Doodads** of `war3map.doo` are drawn at the scale
  `clamp(scale, minScale, maxScale) * defScale`, with `scale` the value
  stored in `war3map.doo` and the three limits from `Doodads\Doodads.slk`
  of `War3Patch.mpq` (the `War3.mpq` copy differs: `LOca` has `maxScale`
  1 there, 1.4 in the patch). Measured, as model size / unit model size:
  `LPcr` (1, 0.8, 1.2): stored 0.5, 0.9, 1.0, 1.1, 2.0 -> 0.83, 0.92,
  1.00, 1.14, 1.22; `BOth` (0.95, 0.9, 1): 1.0, 2.0 -> 0.96, 0.96;
  `LOca` (1.2, 1, 1.4): 0.5, 1.0, 2.0 -> 1.2, 1.2, 1.65.
- **Doodads with a pathing texture** are moved to the grid like
  buildings (SPEC-0022, "Snapping"): `LOca` (`2x2Default`) stored at
  x or y = 0, 400, -400, 256, -256 was drawn at 32, 416, -416, 288,
  -224, which is `64 * floor(p / 64) + 32` (for -256 this differs from
  the building rule, which gives -288). Doodads without a pathing
  texture (`LPcr`, `BOth`) were drawn at their stored position (within
  about 1 unit).
- **Size and height of units.** A unit's model is drawn at the scale
  `modelScale` of `Units\UnitUI.slk` (gryphon rider 1.2: a test model
  drawn 1.17 to 1.2 times as large as on a footman, 1.0; abomination 0.9:
  0.88). A flying unit is drawn at its flying height above the ground
  under it: the gryphon rider (`moveHeight` 240) about 238 above flat
  ground, seen from the side next to a footman, and 232 to 242 above
  ground raised by raw height 256 (`GetLocationZ` 256), read from the
  scale and position of a flat marker seen straight down with the
  camera target on low ground. The flying height is
  `max(moveHeight, moveFloor)` of `Units\UnitData.slk` as changed by the
  map's `war3map.w3u`: a gryphon rider copy with `moveHeight` 60
  (`moveFloor` 90) had `GetUnitFlyHeight` 90 and was drawn 90 above
  flat ground and 82 to 89 above the raised ground; a copy with
  collision size 31 kept 240. Only one value below `moveFloor` was
  tried.
- **Units** are drawn at the centre of their pathing footprint
  (SPEC-0020, "Size classes and footprints"): units of size class 1 and
  3, whose footprints are 2 x 2 and 4 x 4 cells and reach one cell more
  to the west and south, are drawn 16 west and 16 south of `GetUnitX` /
  `GetUnitY`; units of class 0 and 2 (1 x 1 and 3 x 3 cells) at their
  position. The offset is the same at every position (on and off the 32
  grid) and facing. Measured (standing units, the model replaced by a
  flat marker): collision 0, 8, 15 (class 0): no offset; 16 and 31
  (class 1: peasant, acolyte, footman, ghoul): -16.4 +- 2 in x, -15.5
  +- 1 in y; 32 (class 2: knight, rifleman, grunt, paladin, archer,
  huntress, mortar team, Medivh): none; 48 (class 3: abomination and a
  footman copy): -16.3, -15.7. `GetUnitX` returned the given position in
  all cases. Flying units follow the same rule: the gryphon rider
  (collision 8, class 0) is drawn at its position, its copy with
  collision 31 (class 1) 16 west and 16 south of it, over flat and over
  raised ground. Walking units are drawn the same way, at their current
  position plus the offset of their class: footmen (class 1) and
  knights (class 2) walking east and south, paused with `PauseGame` in
  ticks 157 to 162, were drawn within 0.5 of their position plus the
  offset at the end of the 25 ms chunk in which the pause took effect
  (SPEC-0002: the ticks of that chunk still run); no interpolation
  between ticks was seen. A farm (pathing
  texture 4 x 4) was drawn at its position as `GetUnitX` / `GetUnitY`
  gave it (`CreateUnit` at (-256, -192) had put it at (-256, 0), next to
  a walking footman; not examined further).

## Edge cases

- The stretch of the interface's 4:3 depends on the window: on a 4:3 screen
  there is none.

## Open questions

1. How the height of the camera's target point is taken from the ground
   (smoothing, area, timing), and the role of `CAMERA_FIELD_ZOFFSET`.
2. The near and far planes (`CAMERA_FIELD_FARZ`) and fog.
3. Whether the field of view applies the same way in full-screen modes
   and with the `fixedaspect` video setting.
4. How a flying unit's height changes while it flies over a slope or a
   cliff edge (smoothing, timing), buildings, and the snapping of
   doodads with other pathing texture sizes.

## Sources

- Test maps RUV and RUW on 1.27b under Wine, 2026-10-20 (window 1024 x
  768): paused footmen in a row 100 apart, camera straight down at
  distance 1000, field of view 70 and then 120 degrees; positions of
  the units read from the screenshots by their team colour.
- Test maps on 1.27b under Wine (OpenGL, window 1016 x 734, resolution
  setting 800 x 600), 2026-10-18: flat Northrend ground with a grid
  texture (a line at every tile border), blocks of ground raised by raw
  height 128 and 256 and by one cliff level, camera straight down at
  distances 1000, 2500 and 3000 and from the side; positions of the grid
  lines read from the screenshots.

## Verification

1. **Straight down, distance 1000, target on flat ground**: footmen
   100 apart, 101 pixels apart across a 1024-pixel window at FOV 70 and
   55.25 at FOV 120. From the rule: `512 / (0.5490 * 1023)` = 0.912
   pixels per unit on the ground (the parts of the models read stand
   above it, closer to the eye) and a ratio of 1.826 between the two
   settings.
2. **Raised blocks**, camera straight down at distance 1000 with the
   target on the south edge of each block: tile spacing on the low
   ground and on the block top 112.71 and 128.33 pixels (block raised
   by raw height 128 and block one cliff level up: the same values),
   107.43 and 140.00 (raw height 256). With the measured scale this puts
   the tops 126 and 253 above the low ground (128 and 256 expected,
   within the reading accuracy of about 2).
3. **From the side** (angle of attack 359): the top edges of the block
   raised by raw height 128 and of the block one cliff level up are on
   the same screen row.
4. **Flying units over raised ground**: ground raised by raw height 256
   at x 3200 to 4736, y -4864 to -3328, otherwise flat; camera straight
   down at distance 3000 with the target at (2400, -4000); paused gryphon
   riders (`moveHeight` 240) at (1900, -4000) and (3700, -4000), copies
   with collision 31 at (1900, -3500) and (3700, -3500), copies with
   `moveHeight` 60 at (1900, -4500) and (3700, -4500), each drawn as a
   flat marker. `GetUnitFlyHeight` gives 240, 240, 240, 240, 90, 90. The
   markers are where points at heights 240, 496, 240, 496, 90 and 346
   above zero, shifted by 16 west and south for the collision 31 copies,
   project (within about 8 units in height).
