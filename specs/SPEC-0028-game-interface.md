---
id: SPEC-0028
title: Game interface outside the frame definition files
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (frame rectangles read from the memory of the running game, screenshots, data files)"]
---

## Behaviour

This spec gives the parts of the in-game interface that the frame
definition files (`UI\FrameDef\UI\*.fdf`) do not fix: where the game
places its main frames, the grids it builds itself and how the whole
interface is mapped to the window. It is the first part of the
interface spec; the content of the command card, the information panel,
the portrait, selection and the replay actions of each input follow in
later parts (open questions).

### Interface coordinates and the window

All frames are placed in interface coordinates: x from 0 (left) to 0.8
(right), y from 0 (bottom) to 0.6 (top). The whole interface, and the
3D view of the world as well, is drawn for this 4:3 area and stretched
to the window, whatever the window's aspect ratio: with a window of
1280 x 720 (16:9) every frame keeps the interface coordinates of the
4:3 case below and is 1.333 times wider on the screen than high; the
selection circle of a building was 1.01 times as wide as high in a
1024 x 768 window and 1.38 times in the 1280 x 720 one. The registry
option `fixedaspect` changed nothing in a window. So a point `(x, y)`
of the interface is at the window pixel
`(x / 0.8 * W, (1 - y / 0.6) * H)` for a window of `W` x `H` pixels.

### Main frames

Rectangles as left, bottom, right, top in interface coordinates, read
from the frames of a running game (1024 x 768 and 1280 x 720 gave the
same values):

| Frame | Left | Bottom | Right | Top |
|---|---|---|---|---|
| world view (the 3D scene, clicks on the world) | 0 | 0.13 | 0.8 | 0.58 |
| minimap | 0.009375 | 0.006875 | 0.14875 | 0.145 |
| minimap buttons, 1 (top) | 0.154375 | 0.123125 | 0.1775 | 0.14375 |
| minimap buttons, 2 | 0.154375 | 0.10125 | 0.1775 | 0.121875 |
| minimap buttons, 3 | 0.154375 | 0.079375 | 0.1775 | 0.1 |
| minimap buttons, 4 | 0.154375 | 0.056875 | 0.1775 | 0.0775 |
| minimap buttons, 5 (bottom) | 0.155 | 0.029375 | 0.178125 | 0.05 |
| portrait | 0.2075 | 0.030625 | 0.2975 | 0.11623 |
| information panel | 0.309375 | 0 | 0.4975 | 0.114063 |
| command card | 0.618125 | 0.002 | 0.792625 | 0.131 |
| resource bar | 0.461719 | 0.5782 | 0.8 | 0.6 |
| upper button bar (Quests, Menu, Allies, Log) | 0 | 0.578 | 0.34 | 0.6 |
| hero bar | 0.00375 | 0.2155 | 0.04175 | 0.57375 |
| idle worker button | 0.00375 | 0.17 | 0.04175 | 0.208 |
| chat input line | 0.2 | 0.13 | 0.6 | 0.16 |
| message frame 1 | 0.21 | 0.18 | 0.59 | 0.23 |
| message frame 2 | 0.051875 | 0.2305 | 0.551875 | 0.4805 |
| message frame 3 | 0.00625 | 0.2 | 0.78 | 0.6 |
| message frame 4 | 0.21 | 0.524 | 0.59 | 0.574 |
| cinematic panel, console art | 0 | 0 | 0.8 | 0.6 |

The minimap's rectangle is that of the area the map image is drawn in
(its lower left corner is 0.009375, 0.006875 from the lower left corner
of the interface). The five minimap buttons are, from the top, as
their icons show: signal (ping), terrain, ally colours, creep camps,
formation. Which of the four message frames shows which kind of
message (chat, unit messages, errors) was not checked.

### Grids built by the game

**Command card.** 12 buttons in 4 columns and 3 rows, each 0.038 x
0.038:

- column `c` (0 to 3, from the left): left edge
  `0.618125 + 0.043625 c` (0.618125, 0.66175, 0.705375, 0.749);
- row `r` (0 to 2, from the top): bottom edge `0.093 - 0.043 r`
  (0.093, 0.05, 0.007), top edge 0.038 higher.

So the card's own rectangle (above) is the union of the buttons plus
0.005 at the right and the bottom. A button's place in the card is
the column and row of its `Buttonpos` (next part).

**Inventory.** 6 buttons in 2 columns and 3 rows, each 0.03125 x
0.03125: left edges 0.51625 and 0.55625, bottom edges 0.080625,
0.0425, 0.004375 from the top row down.

**Hero bar.** 7 buttons, each 0.038 x 0.038, left edge 0.00375; the
top one from 0.53575 to 0.57375, each further one 0.051181 lower (tops
0.57375, 0.52257, 0.47139, 0.42021, 0.36904, 0.31786, 0.26668).

**Idle worker button.** One button of 0.038 x 0.038 at
(0.00375, 0.17).

**Group in the information panel.** When several units are selected,
12 buttons in 6 columns and 2 rows, each 0.03125 wide and 0.046 high:
left edges `0.308906 + 0.030990 c` (0.30891, 0.33990, 0.37089, 0.40187,
0.43286, 0.46385), bottom edges 0.055556 (upper row) and 0.004781
(lower row).

**Training and construction progress bar** (information panel of a
building that trains or is built): (0.370625, 0.065938, 0.475625,
0.075938).

### Time of day indicator

The indicator is the model named by `TimeOfDayIndicator` in
`UI\war3skins.txt` for the player's race (for Human
`UI\Console\Human\HumanUI-TimeIndicator.mdl`, stored as `.mdx`), drawn
in a frame that covers the whole interface with its lower left corner
at (0, 0): the model's own coordinates place it (at the top centre of
the screen). Its animation is not played by itself. The animation time
is set from the time of day `h` (SPEC-0017, 0 to 24):

    t = h / DayHours * ScaledAnimTime

with `DayHours` = 24 and `ScaledAnimTime` = 60 from the game's misc
data (`Units\MiscData.txt`), so `t` is 2.5 s per game hour, in the
model's sequence `Stand` (0 to 60000 ms in the Human model): midnight
is the first frame, noon 30000 ms. The model also has a sequence
`Stand Alternate` (67500 to 127500 ms), whose use was not found.
A frame of 0.04 x 0.04 with its top centre at the top centre of the
interface carries the indicator's tooltip.

## Edge cases

- The main frames keep their places whatever is selected; frames of
  the information panel for one unit, a building, cargo, an item, a
  destructable and a group all have the panel's rectangle and are
  shown one at a time.

## Open questions

1. The content of the command card (buttons from the unit and ability
   data, `Buttonpos`, hotkeys, submenus, cancel), of the information
   panel (texts, numbers, icons), the portrait (model, camera,
   animation), selection rules and the replay actions written for each
   click and key: next parts of this spec.
2. When the time indicator plays `Stand Alternate`.
3. Full screen with another aspect ratio was not tested (only windows).

## Sources

- Frame rectangles read from the memory of the game (1.27b under Wine)
  in a 1024 x 768 and a 1280 x 720 window, test map RTP with a barracks
  selected and a footman in training, 2026-10-19/20.
- Screenshots of the same games.
- `UI\war3skins.txt`, `Units\MiscData.txt` and the time indicator
  model from the game archives.

## Verification

1. A screenshot of a 1024 x 768 window: the minimap image spans pixels
   12 to 190 horizontally, the portrait 266 to 381, the command
   buttons of the first column 791 to 840 (0.618125 x 1280 = 791.2).
2. In a 1280 x 720 window the same frames are 1.25 times wider in
   pixels and 0.9375 times as high.
