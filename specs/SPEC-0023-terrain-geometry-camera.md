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

`CAMERA_FIELD_FIELD_OF_VIEW` (70 degrees by default) is the field of view
across the **diagonal of a 4:3 frame**. With `t = tan(fov / 2)`:

- horizontal: `tan(h / 2) = 0.8 t` (70 degrees: `h` = 58.5 degrees);
- vertical: `tan(v / 2) = 0.6 t` (70 degrees: `v` = 45.6 degrees).

The frame is always computed for an aspect of 4:3 and then stretched to
fill the game window: in a window that is not 4:3 the picture is scaled
unevenly. In the test window of 1016 x 734 pixels, flat ground seen
straight down had 116.75 pixels per tile across and 112.5 pixels per tile
up the screen, a ratio of 0.9636, which is `(734 / 1016) / (3 / 4)`.
(The interface covers the top and bottom of the window; the 3D picture
is computed for the whole window.)

A projection with 70 degrees taken as the vertical field of view shows
a much larger area at the same camera distance (vertical tangent 0.70
instead of 0.42) and so a stronger perspective.

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
  all cases. A walking footman (class 1, `move` east at 270) was drawn
  -7.4, -16.2 from its position (open question 4). A farm (pathing
  texture 4 x 4) was drawn at its position as `GetUnitX` / `GetUnitY`
  gave it (`CreateUnit` at (-256, -192) had put it at (-256, 0), next to
  a walking footman; not examined further).

## Edge cases

- The stretch of the 4:3 frame depends on the window: on a 4:3 screen
  there is none.

## Open questions

1. How the height of the camera's target point is taken from the ground
   (smoothing, area, timing), and the role of `CAMERA_FIELD_ZOFFSET`.
2. The near and far planes (`CAMERA_FIELD_FARZ`) and fog.
3. Whether the field of view applies the same way in full-screen modes
   and with the `fixedaspect` video setting.
4. Where moving units are drawn (one walking footman was drawn about 9
   units ahead of the class 1 offset in its direction of motion), flying
   units and buildings; the snapping of doodads with other pathing
   texture sizes.

## Sources

- Test maps on 1.27b under Wine (OpenGL, window 1016 x 734, resolution
  setting 800 x 600), 2026-10-18: flat Northrend ground with a grid
  texture (a line at every tile border), blocks of ground raised by raw
  height 128 and 256 and by one cliff level, camera straight down at
  distances 1000, 2500 and 3000 and from the side; positions of the grid
  lines read from the screenshots.

## Verification

1. **Straight down, distance 1000, target on flat ground** (FOV 70):
   116.75 pixels per tile across, 112.5 up the screen. From the formula:
   `508 / (0.8 tan 35)` = 907 pixels for one unit at distance 1 across,
   so 116.1 pixels per tile; measured 116.75 (0.6 percent).
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
