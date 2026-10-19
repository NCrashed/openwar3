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

### Command card content

Each button of the command card sits at the column and row given by the
`Buttonpos` field (`x,y`, column 0 to 3 from the left, row 0 to 2 from
the top) of the entry that the button stands for, in the unit and
ability data (`Units\*UnitFunc.txt`, `Units\*AbilityFunc.txt`,
`Units\CommandFunc.txt`, the later files of the archives over the
earlier ones). Its icon is the entry's `Art`. The entries are:

- the standard commands of `Units\CommandFunc.txt` for a unit that can
  move and attack: `CmdMove` (0,0), `CmdStop` (1,0), `CmdHoldPos` (2,0),
  `CmdAttack` (3,0), `CmdPatrol` (0,1);
- the unit's abilities (`abilList` of `Units\UnitAbilities.slk`) that
  have a button, at their `Buttonpos` (for a toggled ability in its
  other state at `Unbuttonpos`);
- for a building, the unit types of `Trains`, the researches of
  `Researches` and the types of `Upgrade`, each at its own `Buttonpos`,
  and `CmdRally` (3,1) when it trains units;
- for a worker, the build button of its race (`AHbu` for Human, at
  (0,2)), which opens the build menu;
- `CmdCancel` (3,2) while a building trains, is under construction or
  in a submenu.

Seen in the game (test map RTY, Human):

| Selected | Buttons (column, row) |
|---|---|
| peasant | move (0,0), stop (1,0), hold (2,0), attack (3,0), patrol (0,1), repair (1,1), gather (3,1), build (0,2), call to arms (1,2) |
| peasant, build menu | the 11 types of `Builds` at their `Buttonpos`, cancel (3,2) |
| footman | move, stop, hold, attack, patrol, defend (0,2) |
| barracks | footman (0,0), rifleman (1,0), knight (2,0), rally (3,1), its 3 researches in row 2 |
| barracks training | the same and cancel (3,2) |
| town hall | peasant (0,0), its research (3,0), rally (3,1), keep (0,2), and two buttons at (1,2), (2,2) |
| farm (finished) | none |
| farm under construction | cancel (3,2) only |

A button whose requirements are not met (rifleman and knight without
their buildings, defend without its research) is shown with a dark
icon and stays in its place. The button of the unit's current order
has a green border (stop for an idle unit). A button of an ability
that can be switched to cast by itself (repair) has small yellow
corners.

The hotkey of a button is the `Hotkey` of its entry in the strings
files (`Units\CommandStrings.txt`, `Units\*UnitStrings.txt`,
`Units\*AbilityStrings.txt`); pressing it does the same as a click on
the button (same replay actions, below).

### Information panel content

Seen for one selected unit (the labels are texts of the game's
string files, here of a Russian client):

- the unit's name at the top centre of the panel;
- life as `current / maximum` in green below the portrait (and mana
  below it for units with mana);
- for a unit with a weapon: an attack icon with the attack label
  and the damage range `min - max` with `min = dmgplus + dice` and
  `max = dmgplus + dice * sides` (peasant 5 - 6, footman 12 - 13); an
  armour icon with the armour label and the armour value (peasant 0, footman
  2); a status line below. The attack and armour icons of a footman
  carry a small number, the level of the matching upgrade (0);
- for a building: the armour (5 for the farm, the barracks and the
  town hall) and, for a building that gives food, a food icon with the
  food it gives (12 for the town hall, 6 for the farm);
- for a building that trains: the icon of the unit in training, the
  training label over the progress bar of "Grids built by the game"
  and the slots 2 to 7 of the queue below;
- for a building under construction: its icon and the construction label
  over the progress bar, and its growing life (SPEC-0026) below the
  portrait.

### Selection

The player's selection holds at most 12 units. Units are named in
replay actions by their id pairs (entry and creation number, SPEC-0024).
The game writes each change of the selection as replay actions in the
time slot in which the input happened; their effect follows the rules
of SPEC-0024 ("When actions take effect"). The actions are:

| Action | Bytes after the action id | Meaning |
|---|---|---|
| `0x16` | mode (1 add, 2 remove), count (16 bit), count id pairs | change the selection |
| `0x17` | group (0 to 9), count (16 bit), count id pairs | assign a control group |
| `0x18` | group, one byte (3 in all tests) | select a control group |
| `0x19` | unit type rawcode (4 bytes), one id pair | select the subgroup |
| `0x1A` | none | written right before `0x19` |

Inputs and what they write (all measured with mouse clicks and keys):

- **Click on a unit.** The new selection is that unit: `0x16`
  remove with the units selected before (if any), `0x16` add with the
  unit, then `0x1A`, `0x19` with its type and id. A click on the
  ground selects nothing and keeps the selection (nothing written).
- **Shift-click on a unit that is not selected**: `0x16` add with it,
  nothing else. **Shift-click on a selected unit**: `0x16` remove with
  it, then `0x1A` and `0x19` with the first unit left.
- **Rectangle (drag with the left button).** The new selection is the
  units in the rectangle (at most 12). The game writes the difference
  to the old one: `0x16` remove with the units that are no longer
  selected (if any), `0x16` add with the new ones (if any), then
  `0x1A` and `0x19` with the subgroup. A drag that starts on the
  interface (the top bar) selects nothing. **Shift and drag** adds:
  `0x16` add only.
- **Double click on a unit, or Ctrl-click**: the first click selects
  the unit as above; then the units of the same type and player that
  are on the screen are added, at most 12 in all, the nearest to the
  clicked unit first (in a test with 16 footmen the 11 added were the
  11 nearest in world distance): `0x16` remove with the units other
  than the clicked one, `0x16` add with the added ones. Ctrl-click
  wrote the same set as a double click.
- **Tab**: `0x1A` and `0x19` with the next subgroup (below).
- **Ctrl + digit** (1 to 9, 0): `0x17` with the group 0 to 9 (key 1 is
  group 0, key 0 is group 9) and all selected units. **Shift + digit**
  also writes `0x17` for that group (with the selected units added to
  it).
- **Digit**: `0x18` with the group and the byte 3, then `0x1A` and
  `0x19`. Pressing the digit again (which moves the camera to the
  group) writes `0x18` again, without `0x19`.

The ids in `0x16` and `0x17` are in ascending order of the entry
number.

**Subgroups.** The selected units form one subgroup per unit type. The
subgroups are ordered by the type's `prio` field (`Units\UnitData.slk`,
higher first: footman 6, knight 3, rifleman 2, peasant 1); the first
one is active after a new selection and Tab moves to the next one,
after the last back to the first (footman, knight, rifleman, footman).
`0x19` names the active subgroup's type and its first unit.

**More than 12 units in a rectangle.** Which 12 are taken was not
fully understood. With 16 footmen in a 4 x 4 square and nothing
selected before, and with 14 footmen in a row, the 12 created first
were taken in one test each; other drags over the same row gave other
sets (for example the 10 created first and the last two) depending
on the previous selection and the direction of the drag (open
question 4).

### Orders from the interface

Orders are written as the actions of SPEC-0024 (`0x10` without
target, 15 bytes; `0x11` with a point, 23 bytes; `0x12` with a point
and a target object, 31 bytes; each: action id, flags (16 bit), order
id (32 bit), two 32-bit fields `0xFFFFFFFF`, then the point as two
floats and, for `0x12`, the target's id pair or two `0xFFFFFFFF`).
The same orders were written whether a command button was clicked
with the mouse or its hotkey was pressed:

| Input | Action | Order id | Flags |
|---|---|---|---|
| Stop (S) | `0x10` | 851972 (`stop`) | 0 |
| Hold position (H) | `0x10` | 851993 (`holdposition`) | 0 |
| Attack (A), then click on the ground | `0x12`, no target | 851983 (`attack`) | 0 |
| Move (M), then click on the ground | `0x12`, no target | 851986 (`move`) | 0 |
| Patrol (P), then click on the ground | `0x12`, no target | 851990 (`patrol`) | 0 |
| right click on the ground | `0x12`, no target | 851971 (`smart`) | 0 |
| right click on a unit | `0x12`, the unit, its position as the point | 851971 | 0 |
| any of these with Shift held | as above | as above | bit 0x0001 set |
| any of these with several units selected | as above | as above | bit 0x0008 set |
| train a unit (building selected, its hotkey) | `0x10` | the unit type's rawcode | 0x0042 |
| cancel (Escape) | `0x61`, then `0x10` | 851976 (`cancel`) | 0x0040 |
| open the build menu (B) | `0x67` | | |
| build (type key, then click) | `0x11` with the building's rawcode | | 0x0004 |

The point of a click on the ground is where the cursor ray meets the
ground (SPEC-0023); a right click on a unit uses the unit's position.

## Edge cases

- The main frames keep their places whatever is selected; frames of
  the information panel for one unit, a building, cargo, an item, a
  destructable and a group all have the panel's rectangle and are
  shown one at a time.

## Open questions

1. The portrait (model, camera, animation); the exact strings and
   colours of the information panel, upgrades shown as bonuses, groups,
   heroes and items; what happens when two buttons have the same
   `Buttonpos`: next parts of this spec.
2. When the time indicator plays `Stand Alternate`.
3. Full screen with another aspect ratio was not tested (only windows).
4. Which 12 units a rectangle over more than 12 units takes.
5. The meaning of the byte 3 in `0x18` and of flag bits 0x0002, 0x0004
   and 0x0040; clicks on the group buttons of the information panel and
   on the portrait.

## Sources

- Frame rectangles read from the memory of the game (1.27b under Wine)
  in a 1024 x 768 and a 1280 x 720 window, test map RTP with a barracks
  selected and a footman in training, 2026-10-19/20.
- Screenshots of the same games.
- Replays with mouse clicks and keys on test maps RTW (16 footmen, 2
  riflemen, a knight and a peasant) and RTX (14 footmen in a row),
  decoded action by action; `c-RTW-1.w3g` to `c-RTW-4.w3g`,
  `c-RTX-1.w3g`.
- `UI\war3skins.txt`, `Units\MiscData.txt` and the time indicator
  model from the game archives.

## Verification

1. A screenshot of a 1024 x 768 window: the minimap image spans pixels
   12 to 190 horizontally, the portrait 266 to 381, the command
   buttons of the first column 791 to 840 (0.618125 x 1280 = 791.2).
2. In a 1280 x 720 window the same frames are 1.25 times wider in
   pixels and 0.9375 times as high.
