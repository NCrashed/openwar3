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

## Edge cases

- The stretch of the 4:3 frame depends on the window: on a 4:3 screen
  there is none.

## Open questions

1. How the height of the camera's target point is taken from the ground
   (smoothing, area, timing), and the role of `CAMERA_FIELD_ZOFFSET`.
2. The near and far planes (`CAMERA_FIELD_FARZ`) and fog.
3. Whether the field of view applies the same way in full-screen modes
   and with the `fixedaspect` video setting.

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
