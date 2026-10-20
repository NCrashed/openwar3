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
(lower row). The cells overlap by 0.00026 (the step is smaller than the
width); the visible gaps come from the parts inside a cell, which are
inset. The selected units take the cells in order, the upper row from
the left first, then the lower row: the subgroups in the order of the
subgroups (below, "Subgroups"), the units of a subgroup one after the
other (in the tests in the order of creation). Cells without a unit
show nothing. The cells do not move with the number of units: three
footmen, two riflemen and two peasants took the six upper cells and the
first lower one.

Inside a cell with left edge `l` and bottom edge `b` (read from memory,
test RVO and RVP), the parts depend on whether the unit belongs to the
**active subgroup**:

| Part | Active subgroup: left, bottom, width x height | Other units |
|---|---|---|
| icon (the unit's `Art`) | `l + 0.00162`, `b + 0.01513`, 0.028 x 0.028 | `l + 0.00468`, `b + 0.01819`, 0.02188 x 0.02188 |
| life bar (fill area) | `l + 0.00162`, `b + 0.00888`, 0.028 x 0.00375 | `l + 0.00468`, `b + 0.01319`, 0.02188 x 0.0025 |
| mana bar (fill area) | `l + 0.00468`, `b + 0.00388`, 0.02188 x 0.0025 | `l + 0.00468`, `b + 0.00819`, 0.02188 x 0.0025 |
| background | `l - 0.00313`, `b + 0.00219`, 0.0375 x 0.04663 | `l - 0.00313`, `b - 0.00312`, 0.0375 x 0.05225 |

- The background of the active subgroup's cells is a yellow glow
  around the icon and the bars; around the other cells nothing was
  seen.
- The bars of the active subgroup have a dark border: for the life bar
  `l + 0.00062`, `b + 0.00788`, 0.03 x 0.00575 (0.001 around the fill),
  for the mana bar `l + 0.00368`, `b + 0.00288`, 0.02388 x 0.0045. The
  other cells' bars showed no border.
- The life bar is green (footmen, riflemen, knights, peasants at full
  life); the mana bar is not shown for units without mana.

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

- the unit's name at the top centre of the panel; for a hero its proper
  name. The name is a line of text from 0.10306 to 0.11406 in height,
  centred on x = 0.403435 (its width is the width of the text);
- below the name, for some units, a **level line**, centred on the same
  x, from 0.08906 to 0.09756 in height:
  - a hero: the string `INFOPANEL_LEVEL_CLASS` of the game's strings
    (a format with the level and the hero type's name, `%u` and `%s`);
    for the player's own hero the line lies on the **hero level bar**,
    a bar from (0.31344, 0.08594) to (0.49344, 0.10156) with a border
    texture 0.001 larger on each side, which shows the experience
    towards the next level; for another player's hero (an enemy) the
    line is shown without the bar;
  - a unit whose type has the race `creeps` (`Units\UnitData.slk`):
    `INFOPANEL_LEVEL` (a format with the level, `%u`), the `level`
    field of `Units\UnitBalance.slk` (5 for `nkol`, 10 for `nmgd`),
    without a bar, whoever owns it (a kobold of an enemy player showed
    it);
  - no line for units of the playable races (footman, peasant, also a
    footman owned by the creep player) and for buildings;
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

### Portrait

The portrait shows a model chosen from the selected unit's model file
`M` (the `file` field of its type, without the extension):

1. `M_portrait` if that file exists (for example
   `Units\Human\Footman\Footman_portrait.mdx`);
2. otherwise `M` itself (buildings: the farm's own model);
3. if the unit has no model, the orc peon's portrait
   (`units\orc\peon\peon_portrait`).

The model is drawn into the portrait frame (above, 0.09 x 0.08560)
through the camera stored in the model; the portrait models and the
building models looked at have one camera each, with a field of view
of 0.7854 rad (45 degrees), near and far distances 8 and 1000 (the
footman's portrait: position (40.18, 194.43, 88.61), target (8.25,
214.87, 90.07)). The projection is that of SPEC-0023 ("Field of
view") for the portrait frame: `a` = 0.09 / 0.08560 = 1.0514, so with
45 degrees the half-angle up and down is `v` = 0.39270 / 1.45097 =
0.27065 rad, `tan(v)` = 0.2774, and across `a * tan(v)` = 0.2917; the
camera's target is drawn at the centre of the frame. It plays the model's sequences whose names contain
`Portrait` and not `Talk` (`Portrait`, `Portrait 2`, `Portrait 3` of the
footman; `Portrait -1` of the farm) while the unit is selected, and a
`Portrait Talk` sequence while the unit's acknowledgement sound plays.
How one of several `Portrait` sequences is picked, and the timing of
the talk sequences (`UI\SoundInfo\PortraitAnims.slk`), were not
studied.

### Minimap

**Area.** The minimap frame (above) shows the **playable area** of the
map: the camera bounds of `war3map.w3i` extended by the camera margins
(the rectangle of `SetCameraBounds` without `GetCameraMargin`), on
IceCrown x from -11520 to 11520 and y from -11776 to 11264. The picture
`war3mapMap.blp` (SPEC-0032) is stretched over it, and units, icons and
clicks use the same linear mapping. In a 1024 x 768 window: a left click
at pixel `(px, py)` put the camera target at world
`x = 130.16 (px - 104.41)`, `y = -136.35 (py - 669.2)` (four clicks over
the whole minimap, within 0.5); the picture lay over about x 16 to 193,
y 585.5 to 756.5.

**What it shows** (fog of SPEC-0032 over the picture; the local
player's colours in the default mode):

| Object | Drawn as |
|---|---|
| own unit | white square of 3 x 3 pixels (1024 x 768), the same for all units and heroes (footman, knight, peasant, Archmage, gyrocopter, mortar team, rifleman, sorceress) |
| own building | white square of about 5.5 x 6 pixels (5 or 6 wide by the pixel grid), the same for all buildings whatever their footprint (4 x 4 to 16 x 16: farm, scout tower, town hall, keep, barracks, altar, lumber mill, blacksmith, workshop, aviary, sanctum, vault); about 8/256 of the picture's width, a unit about 4/256 |
| unit or building of another player | square of the same size in that player's colour (an enemy of player 1 blue, an ally of player 2 teal) |
| gold mine | yellow round icon, about 9 pixels across |
| neutral building (tavern) | yellow outline of a house |
| creeps that the player sees | squares in their player's colour (dark) |
| creeps in the fog | a circle per creep camp (below) |
| the camera | white rectangle outline, 1 pixel wide (below) |

**Creep camps.** The neutral hostile units that exist when the map
starts (created by the map's units file or by `main`) are grouped into
camps; a camp is shown as one circle at the position of its first creep
while its creeps are not visible. Grouping: a creep joins the camp of an
earlier creep when it is within about 840 of that creep (pairs 740 to
820 apart were one camp, 860 and 900 apart two; three creeps 600 apart
in a row made one camp of the first two and a camp of the third). The
colour and size come from the sum of the camp's levels and `[Minimap]`
of `UI\MiscData.txt`:

| Sum of levels | Colour | Size |
|---|---|---|
| below `MinimapMiddleCampThreshold` (10) | `MinimapWeakCampColor` (0, 200, 0) | 1 |
| 10 up to below `MinimapToughCampThreshold` (20) | `MinimapMiddleCampColor` (255, 128, 0) | `MinimapMiddleCampScale` 1.3 |
| 20 and more | `MinimapToughCampColor` (220, 0, 0) | 1.3 |

(Measured: one creep of level 10 or 11 and two of level 5 orange; four
of level 5 and two of level 10 red; two of level 5 farther than 860
apart two green circles.) Creeps created later by a script are not put
into camps: each showed a green circle of size 1 whatever its level.
The icons are textures named in `UI\war3skins.txt` (`MinimapResourceTexture`
for gold mines, `MinimapNeutralTexture` for neutral buildings); gold
mines and neutral buildings are shown also under fog and where the map
is masked. `MinimapCampPulseScale` (0.5) was not seen in use.

**Camera rectangle.** The rectangle is the axis-aligned bounding box
of the ground area seen in the world frame: the rays through the four
corners of the world frame (SPEC-0023, "Field of view") meet the plane
of the camera target's height, and the smallest rectangle around the
four points is drawn. Measured in a 1024 x 768 window with the target
at (0, 0), against this rule (in pixels of the minimap mapping above):

| Camera | Rule (x, y) | Seen (x, y) |
|---|---|---|
| angle 304, distance 1650 | 95.6 to 113.2, 663.5 to 672.9 | 96 to 112, 664 to 673 |
| angle 304, distance 3000 | 88.4 to 120.4, 658.8 to 676.0 | 89 to 119, 661 to 677 |
| angle 270, distance 1650 | 97.5 to 111.4, 665.5 to 672.9 | 98 to 110, 665 to 672 |
| angle 304, rotation 45 | 97.6 to 114.8, 659.2 to 675.7 | 96 to 112, 661 to 676 |

(The last one is 2 pixels off; that screenshot may have caught the
camera while it changed.) So at the default camera the rectangle is
wider than the area near the bottom of the view: it is as wide as the
far edge of the seen area.

**Creep camp circles over time.** A camp keeps the colour and size it
got when the map started: a camp of two level 5 creeps stayed orange
after one of them was killed. A camp whose creeps all died while the
player did not see it kept its circle; the circle went away when the
player saw the camp's place (a fog modifier over it) and did not come
back when the place was fogged again (test RXB).

Units and buildings under fog are not shown (an enemy under fog
disappeared); the icons of `war3map.mmp` (start locations, gold mines,
neutral buildings of the map's own layout) are not drawn in the game,
only the objects that exist.

**Buttons** (the five minimap buttons, top to bottom, clicked once
each, test RWO):

1. signal: the cursor waits for a point (Escape cancels and writes
   `0x61`);
2. terrain: the picture is replaced by black, only the units and icons
   stay; a second click brings it back;
3. ally colours: enemies red, allies teal, own units white on the
   minimap; a second click changed nothing on the minimap (the third
   mode was not studied);
4. creeps: no change seen with the creeps visible;
5. formation movement: no change on the minimap.

None of them wrote a replay action.

**Clicks.** A left click on the minimap moves the camera target to the
point (above). A right click with a unit selected gives it `smart`
(851971) to the point: replay action `0x12` with flags 0, the point
mapped as above (in the test (5934.3, 6712.1)), no target.

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
- **Clicks on the group cells of the information panel** (test RVO, a
  click at the middle of a cell):
  - a click on a unit of the active subgroup selects that unit alone:
    `0x16` remove with the others, then `0x1A`, `0x19` with its type
    and id;
  - a click on a unit of another subgroup makes that subgroup active
    and changes nothing else: `0x1A`, `0x19` with its type and the
    subgroup's first unit (as Tab);
  - **Shift-click** on a cell removes that unit: `0x16` remove with it,
    nothing else;
  - **Ctrl-click** on a cell keeps only the selected units of that
    unit's type: `0x16` remove with all the others (in the test their
    ids came in neither cell nor id order), then `0x1A`, `0x19` with the
    type and the clicked unit.
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

1. The choice among several portrait sequences; the exact strings and
   colours of the information panel, upgrades shown as bonuses, groups,
   heroes and items; what happens when two buttons have the same
   `Buttonpos`: next parts of this spec.
2. When the time indicator plays `Stand Alternate`.
3. Full screen with another aspect ratio was not tested (only windows).
4. Which 12 units a rectangle over more than 12 units takes.
5. The meaning of the byte 3 in `0x18` and of flag bits 0x0002, 0x0004
   and 0x0040; clicks on the portrait; right clicks and double clicks on
   the group cells; the area of a cell that takes a click.

## Sources

- Test maps RWX, RWY, RWZ, RXA (the level line: a footman, a peasant, an
  Archmage of player 0, a Blood Mage of player 1, kobolds and an ice
  giant of the creep player and of player 1, a footman of the creep
  player; screenshots and the text frames read from memory).
- Test maps RWL to RWW and RXB on 1.27b under Wine, 2026-10-20 (IceCrown,
  1024 x 768 window): own, enemy and allied units and buildings, a gold
  mine, a tavern, creep camps of four levels, seen or under fog;
  screenshots of the minimap, clicks on its buttons and on the minimap,
  the camera target and the orders logged.
- Test maps RVO (six footmen, three riflemen, two knights, a peasant)
  and RVP (three footmen, two riflemen, two peasants) on 1.27b under
  Wine, 2026-10-20, 1024 x 768 window: the units selected by the script,
  the group cells and their parts read from the memory of the game,
  screenshots, then clicks, Shift-, Ctrl-clicks on the cells and the
  replay actions they wrote.
- Test map RUT on 1.27b under Wine, 2026-10-20: a selected footman;
  the portrait camera (field of view, near, far) and the frame it is
  drawn for read from the memory of the running game.
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
