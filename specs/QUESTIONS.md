# Open questions

Behaviour the implementation needs but no spec covers yet. Analysts answer
a question with a spec (see [TEMPLATE.md](TEMPLATE.md)) and remove it here.
Each entry says where the question came from and what the code does until
it is answered.

## Trigger strings (`war3map.wts`)

Answered by SPEC-0003; its own open questions are listed there. Choices
the implementation made where the spec is silent:

- **Malformed first definition.** If a number first appears as a header
  without a body and later with a braced body, which text is shown? Until
  answered: the empty text of the first, malformed definition.
- **Indented opening brace.** A `{` after spaces or tabs opens the body.
- **Missing closing brace.** A body that reaches the end of the file keeps
  the text read so far.
- **Comments before the brace.** Comment lines between the header and the
  `{` are skipped, as editor-saved maps put one there; a non-comment line
  instead of the `{` makes the entry empty.

## Map info (`war3map.w3i`)

The layout is confirmed by parsing every shipped map to its last byte; the
meaning of these fields comes from community notes only:

- order of the four camera bound corners;
- channel order of the fog and water colours;
- meaning of the flag bits and of the player ally priority masks.

## Terrain (`war3map.w3e`)

Layout and grid size are confirmed against every shipped map (the grid is
the playable area plus the camera bound margins, and the tileset matches
`war3map.w3i`). The meaning of these fields comes from community notes only:

- how ground height and water level map to world height;
- the point flag bits (ramp, blight, water, boundary) and bit 14 of the
  water field;
- what `texture_details` selects, and how layer height and cliff texture
  produce cliffs.

## Doodads (`war3map.doo`)

Every doodad of every shipped map lies on its terrain, which confirms the
position fields. Not yet confirmed:

- the meaning of the flags byte and of `life_percent` for doodads that are
  not destructible;
- what the `z` field of special doodads means;
- how `item_table` and `dropped_item_sets` interact when both are set.

## Object modifications (`war3map.w3u` and related files)

All object files of the shipped maps parse to their last byte, every
modified field exists in the game's metadata tables, and each metadata type
is always stored as the same value type. Not yet confirmed:

- **End marker.** Current editors write 0 or the object's code after each
  value; files of format version 1 also hold codes of unrelated objects
  there. Does the game read this field at all?
- **Versions 1 and 2** have the same layout in the shipped maps. Does the
  game treat them differently?
- **Data column** of ability, doodad and upgrade fields: confirm that 1
  means `DataA` and how 0 is used.
- **Precedence** when the original table changes a field that the map's
  SLK or INI files also set, and how levels beyond an object's level count
  are handled.

## Placed units (`war3mapUnits.doo`)

All unit files of the shipped maps parse to their last byte, and every
player of `war3map.w3i` has its start location placed at the coordinates
given there. Not yet confirmed:

- **Random kind.** After the abilities comes a kind: 0, 1 or 2 with data,
  or another value with no data. Reign of Chaos maps use -1 for units that
  are not random; `FrozenThrone/(4)Roundabout.w3x` has 0x281 on a gold mine.
  How does the game treat values other than 0, 1 and 2?
- the meaning of the flags byte and of the two bytes after the owner;
- the 24-bit level and the item class byte of the level form, and the
  default gold of a mine when the field is 0;
- whether dropped item sets in Reign of Chaos maps behave like those of
  Frozen Throne maps, which also have an item table index.

## Regions, cameras and sounds (`war3map.w3r`, `w3c`, `w3s`)

All files of the shipped maps parse to their last byte, every region's
rectangle is ordered, every region ends with byte 0xFF, and every ambient
sound of a region names a sound of the same map. Not yet confirmed:

- the float after a camera's far clipping distance;
- the sound fields without a documented meaning (`unknown_a` to
  `unknown_h`) and the meaning of the sound flag bits.

## JASS syntax (`ow3-jass`)

Precedence, literals, escapes, names, line endings and reserved words are
answered by SPEC-0005. Still open:

- **Unary `+`.** SPEC-0005 lists `not` and unary `-`; the parser also
  accepts unary `+` (test map `c-UP-unary-plus` loads and prints
  `UP +5 = 5`). Does the game?
- **Raw codes of five or more characters.** SPEC-0005 rejects 2 and 3
  characters; the parser rejects every length other than 1 and 4.
- **`false` as a name** is rejected like `true`, which SPEC-0005 assumes.

## JASS names and types (`ow3-jass::check`)

`common.j` with `Blizzard.j`, all shipped map scripts and all but seven AI
scripts pass the checker with the rules of SPEC-0005. Still open:

- **Outdated AI scripts.** `Scripts\demo.ai`, `h04_orange.ai` and the five
  `orc03*.ai` call `common.ai` functions with older signatures (for example
  `CampaignAI` with one argument, while `common.ai` takes `integer farms,
  code heroes`) or use undeclared names. Does the game ever compile them,
  and against which `common.ai`?
- **`null` as an initial value.** SPEC-0005 accepts `null` for an integer
  or real in arguments and in a `return` that is not the last statement,
  and rejects it in `set`. The checker also rejects it as the initial value
  of a local or global integer or real. Is that right?
- **Last statement.** A `return null` counts as the last statement only at
  the top level of the function body, not inside an `if` that ends the
  function.

## JASS operation costs (`ow3-jass::vm`)

The costs of SPEC-0005 reproduce its four published loop measurements and
the costs it adds for conversions, `if` with an empty branch and a bare
`return`. Choices where the spec is silent:

- **Global initialisers** run before any thread and are not counted.
- **Nested threads** may nest 100 deep; one more fails in the caller. The
  game has no limit before its stack overflows (about 1200, SPEC-0005),
  and SPEC-0005 asks to stop earlier.

## Game time (`ow3-sim`)

The timer queue follows SPEC-0002 and reproduces its tests T1c and T3c
within the resolution of the 1000 s reference clock (2^-14 s). The test
maps T3c, SP, PQ (SQ), PZ, LT and RD run as scripts in `ow3-sim` and match
the spec, as does a script test of periodic restarts, `TimerStart` and
`DestroyTimer` in the callback, and pause and resume. Choices where the
spec is silent:

- **Order inside a chunk.** The chunk's ticks run first, then the sleep
  check that ends it. At full speed this puts checks at game times 0.1, 0.2,
  ... as the spec says.
- **Threads due at the same check** resume in the order they went to sleep.
- **`TimerGetRemaining` of a long timer** is the length of the run minus
  the parts done and the time spent in the current part, all in engine
  arithmetic. This keeps `TimerGetElapsed` of the 1000 s clock in test PZ
  at exactly 1.0000, which adding up the remaining parts did not (it read
  1.0001 with the exact `fmod`). Which operations does the game use?
- **`TimerGetTimeout` after `ResumeTimer`** still reads the timeout given
  to `TimerStart`, while `TimerGetElapsed` counts from the resume, and a
  periodic timer goes on with its original period. Is that so?
- **`PauseTimer` inside the timer's own callback** does nothing, since the
  timer is not in the queue then; the periodic timer restarts after the
  callback. Is that so?
- **Script execution time.** Our scripts take no real time, so test CU
  (stall) cannot be reproduced from the script; catch-up is tested by
  adding real time directly.

## Handles and natives (`ow3-sim::game`)

SPEC-0006 is followed for `Condition` and `Filter`, `And`, trigger
conditions, callbacks and `ExecuteFunc`. Choices where the specs are
silent:

- **Handle ids.** Eight ids are taken before the players, so `Player(0)` is
  0x100008, and the 16 players follow in order. Other ids the game takes
  before `main` are not reproduced (the first `Location` of a 12-player
  melee map is 0x10118C in the game). Ids of destroyed objects are never
  reused: SPEC-0006 describes reuse that depends on variables no longer
  referring to the object, which needs reference tracking we do not have.
- **`Or` and `Not` with a `null` or destroyed operand** count it as false,
  like `And` (SPEC-0006 open question 5).
- **`Condition` after `DestroyBoolExpr`** of the shared boolexpr makes a new
  one.
- **`ExecuteFunc` of a function with parameters** ends the calling thread
  (SPEC-0006 open question 2).
- **Strings are UTF-8 here**, byte strings in the game (SPEC-0005). A
  `SubString` that cuts a multi-byte character gives U+FFFD instead of half
  of it, and a negative `start` is taken as 0, as SPEC-0005 allows.
- **Random numbers** follow SPEC-0007. The state at the start, before any
  `SetRandomSeed`, is that of `SetRandomSeed(0)` (SPEC-0007 open
  question 1).

## Forces, groups and enumerations (`ow3-sim::game::enumerate`)

Tests B1, B6 and L5 match SPEC-0005 and SPEC-0006. Units are placeholders
with owner, type, position, facing and a dead flag until units are
specified. Choices where the specs are silent:

- **`ForGroup` order** is the order units were added, and `GroupEnum*`
  filters run in the order units were created. SPEC-0006 shows other
  orders (open question 1).
- **Changes during `ForGroup`.** It works on a copy of the group taken at
  the start, so units added are not visited (as in SPEC-0006), and skips
  handles that no longer refer to units. A removed unit leaves its groups
  at once; SPEC-0006 says it stays briefly.
- **Range and rect tests** use engine arithmetic:
  `add(mul(dx, dx), mul(dy, dy)) <= mul(r, r)` with `dx = sub(x_u, x)`, and
  `min <= x_u <= max` on both axes (SPEC-0006 open question 3).
- **Dead units** are enumerated by `GroupEnum*` until they are removed.
- **Counted variants** stop after `countLimit` units or players have been
  added; a limit of 0 or less adds none.
- **`EnumDestructablesInRect`** goes in the order of `war3map.doo` and then
  the order destructables were created; SPEC-0009 says this is not the
  game's order (open question 4).
- **`ForceEnumAllies` and `ForceEnumEnemies`** take the same players as
  `ForceEnumPlayers`, split by `IsPlayerAlly`.
- **`EnumItemsInRect`** works like `EnumDestructablesInRect`: each filter,
  then its action, in the order the items were created.
- **`GroupEnumUnitsOfType`** takes the units whose type has the given
  string id (see "Units, destructables, items and death events").
- `IsUnitInRangeXY` takes the distance with an IEEE square root, with
  which the spec's boundary (969 at 1000) holds exactly; the engine's
  `SquareRoot` arithmetic gives a bit more than 1000.
- **Not yet:** `GroupEnumUnitsSelected`.

## Map loading and player slots (`ow3-sim::map`, `game::players`)

`Map::load` and `Game::start` follow SPEC-0008: archive priorities, slots
before `config`, filling the slots before `main`, alliances and map flags.
All 191 maps of the installation (outside the test folders) start and run
30 s without a thread error. Choices where the spec is silent:

- **The local player's name** is "Player" (the game uses the profile name).
- **Melee races and the computer.** A race preference that is not one of
  the four races (random, for example) gives human, so in a melee map
  both the local player and the computer play human. The computer stays
  in slot 1 with the start location and team that `config` gave it, and
  plays only when the map has two or more players. SPEC-0008 open
  questions 1 and 2.
- **A map without the fixed-settings flag** is set up as melee (flag 0x20
  of `war3map.w3i` decides).

## Units, destructables, items and death events (`ow3-sim::data`, `game::widgets`)

Units, destructables and items are placeholders until they are
specified: type, owner (units), position, facing (units) and hit points.
String ids, hit points of new units and placed destructables, unknown
types and the order and filters of death events follow SPEC-0009. Choices
where the spec is silent:

- **`GetHeroLevel`** is 1 for every hero: heroes do not gain experience
  yet.
- **`KillUnit`** has no killing unit, so the filter of a player unit death
  event is never called yet; deaths by damage wait for the unit spec.
- **Hit points and death.** Setting those of a destructable to 0 or less
  kills it (SPEC-0009 for 0; units follow SPEC-0013). `IsUnitType` knows
  only `UNIT_TYPE_HERO` and `UNIT_TYPE_DEAD`.
- **Conditional life regeneration.** `night` follows the spec's hours,
  judged by the hour of the tick and of the tick before, also after a
  jump of a suspended clock (open question 4). At a change between day
  and night the regeneration goes on from the current value at the new
  rate, an arithmetic not measured. `day` and `blight` never hold yet;
  only a hero's strength part applies then. A map's own regeneration
  type (`uhrt`) replaces that of the base type.
- **Regeneration arithmetic.** The elapsed time is a sum of `dt` per
  tick, and a hero's first step after creation is `sub(dt, 0.0001)`. This
  matches the footman and paladin life columns of
  `SPEC-0013-regen-samples.tsv` bit for bit; the paladin mana column
  differs by one unit of the last place at 3 of 60 ticks (with the rate
  `mul(i2r(17), 0.05)`). `mul(i2r(n), dt)` minus 0.0001 instead misses one
  life value. How the game computes the elapsed time and the hero mana
  rate is open.
- **Hero attributes** follow the spec for max life, max mana, life and
  mana. `SetHeroStr` restarts life regeneration 0.0001 s later at the
  new rate, `SetHeroInt` mana regeneration the same way (inferred).
  Agility is only kept for `GetHeroAgi`: armor and the primary
  attribute's damage still come from the type. There are no items, so
  no bonuses.
- **Turning** (SPEC-0013 open question 1): `orientInterp` 3, 7 and 8 brake by the
  closed form, the others by their table; 2 has no largest step (at
  least 0.285 in the spec). After `SetUnitFacing` the facing changes at
  the updates of the shared cycle, the first one after the call's tick
  (1 to 6 ticks later; the spec saw 3 to 6). A unit walking an order is
  not affected; one that has arrived and still turns takes the angle as
  its heading (not measured).
- **A thread resumed after `TriggerSleepAction`** (SPEC-0013 open question
  2) sees regeneration like any other thread.
- **Blocked ground** (SPEC-0013 open question 3): `CreateUnit` places the
  unit at the snapped point whatever is there. `SetUnitY` below
  `minY - 32` and `CreateUnit` far outside the world, which hang the
  game, are accepted.
- **A destructable placed with 0 percent life** is alive and enumerated
  (SPEC-0009 open question 2).

## Damage and armor (`ow3-sim::combat`, `game::widgets`)

`UnitDamageTarget`, `EVENT_UNIT_DAMAGED`, `GetEventDamage` and
`GetEventDamageSource` follow SPEC-0015, including the map's own
`Units\MiscGame.txt`, armor types outside the lists, and damage to
destructables and items. Choices where the spec is silent:

- **Without `MiscGame.txt`** every multiplier is 1, `DefenseArmor` 0.06
  and the agility constants 0.
- **Items at 0 life** (open question 4) stay where they are and can be
  enumerated like other items.
- **Killing unit.** A unit that dies of the damage has the source as its
  killing unit.
- **Hero armor** uses the agility of the type (`AGI`), without levels or
  attribute changes, as `add(add(def, AgiDefenseBase),
  mul(AgiDefenseBonus, agility))`.
- Ethereal units, magic immunity and missed attacks (open question 1) are
  not handled.

## Attacks of units (`ow3-sim::combat`, `game::attack`, `ow3-sim::random`)

The damage of one hit, the dice from game generator 2, the reseeding of
the 45 game generators and the `Custom_V1` data set follow SPEC-0016 and
SPEC-0007. Choices where the specs are silent:

- **Numbers of the `Units\` tables** (SPEC-0016 open question 4) are
  read as the nearest real; `s2r` would give `1.5000001` for `cool1`
  "1.5".
- **Which copy of a table.** A `Units\*.slk` imported into the map comes
  first, then, for maps without the melee flag, the game's `Custom_V1`
  copy, then the game's `Units\` copy.
- **Attack timing.** An attack starts as SPEC-0019 says (in range,
  facing the target); its first hit comes `dmgpt1` after that update and
  later hits `cool1` apart, rounded to whole ticks; hits come after the
  timers of their tick. Units do not acquire targets on their own.
- **Custom unit types** start from their base type as the map has
  changed it (`war3map.w3u` changes to standard types are applied
  first); whether the game starts them from the unchanged base is not
  known. The viewer draws them with `umdl` and `usca` or the base
  type's look.
- **State at game start.** The game generators start as after
  `SetRandomSeed(0)`, like the script generator.

## Natives that keep state for scripts (`game::players`, `game::widgets`)

The ignored test `maps` counts the stubbed natives that the 191 installed
maps call in 30 s (after `config` and `main`); the most common ones that
scripts read back are now implemented. Choices where no spec says
anything yet:

- **Tech, player and game states** are kept as scripts set them: nothing
  changes them on its own (no research, income, upkeep or day and night).
  A tech without a limit reads -1, everything else 0 until set. What does
  `GetPlayerTechMaxAllowed` read without a limit, and what are the game's
  starting values of player states (gold, lumber, food) and of
  `GAME_STATE_TIME_OF_DAY`?
- **`TriggerRegisterTimerExpireEvent`** triggers run after the timer's
  callback, in the order they were registered, with `GetExpiredTimer`.
- **`GetRectCenterX`** is `mul(add(min, max), 0.5)`.
- **Camera bounds** are the smallest and largest coordinates of the four
  points given to `SetCameraBounds` (before that, of the eight values in
  `war3map.w3i`), and `GetCameraMargin` is 0 for every margin, so the
  playable area of Blizzard.j is exactly what `main` passes. What are the
  game's margins and bounds?
- **Objects not modelled yet** (sounds, dialogs, quests, boards, effects,
  regions, camera setups and the like) are real handles, so that tests
  against `null` behave; every other native on them does nothing.
- **Units.** `SetUnitX` and the like move a unit anywhere, without the
  limits of the map; `SetUnitOwner` ignores `changeColor`;
  `GetPlayerUnitCount` counts living units and `GetPlayerStructureCount`
  living units whose type has `isbldg`.
- **`VersionGet`** is `VERSION_FROZEN_THRONE`.
- **Not yet:** events of player and game states, players leaving,
  regions; unit orders and abilities (they wait for the unit spec).

## Leaderboards, multiboards and dialogs (`game::boards`)

Texts are stored with trigger strings resolved (SPEC-0003). Leaderboard
rows, `GetLocalizedString` and dialog clicks follow SPEC-0010, and a
dialog shown to the local player stops game time and sleeps. Choices
where the specs are silent:

- **Sorting** keeps the order of equal rows (SPEC-0010 open question 2).
- **String lists.** `GetLocalizedString` reads the four string files of
  `UI\FrameDef` from the game archives only (SPEC-0010); when a key
  appears twice, the later entry is kept (open question 1).
- **Multiboards** have no limit on rows and columns; each
  `MultiboardGetItem` makes a new handle.

## Terrain rendering (`ow3-render::terrain`)

Ground textures follow SPEC-0012 (layering, corner bits, variation
cells, opacity of the full tile). Still open:

- **Not drawn yet:** blight, the boundary as the game shows it.
  Points with flag 0x8 are darkened. The top-down image of
  `ow3-render::terrain` still tints water and draws cliffs as slopes.

Heights, water and cliff models follow SPEC-0014 in `ow3-render::cliffs`,
`ow3-render::water` and the viewer. Choices where the spec is silent:

- **Green cliffs on Icecrown.** With rule 7 the cliffs of
  `(4)LostTemple.w3x` (tileset I, types `CIsn` and `CIrb`) show the
  grass and dirt of the plain `Cliff1` and `Cliff0` on the models, which
  does not look like the game. Something that gives cliff models of
  these tilesets their look is missing from the spec (asked).
- **Ground next to cliffs** (SPEC-0014 open question 5). Where cliff
  tiles of different types share a point, the later tile (by rows from
  the south) gives the ground; a `groundTile` missing from the map's
  ground list is not added. Cliff textures and water frames come from
  the tileset archive mounted below the map and above all game archives
  (its place against `War3Patch.mpq` is open). Tilesets differ in
  lighting, which the viewer does not model.
- **Cliff placement** (SPEC-0014 open question 2). The shipped models
  span x -128..0, y 0..128 and raise the corner of the name's BL letter
  at (0, 0), BR at (0, 128), TR at (-128, 128), TL at (-128, 0); the
  viewer places model point (x, y) at (y, -x) from the tile's bottom left
  corner, at the height of the lowest level, and raises each vertex by
  the corners' ground heights (layers excluded), interpolated
  bilinearly. A tile whose model cannot be read keeps its ground.
- **Ramp model origin.** A ramp model's origin is the bottom left corner
  of the lower left of its two tiles (from the geometry of the
  `CliffTrans` models).
- **`GetLocationZ`** interpolates as
  `lerp(lerp(bl, br, u), lerp(tl, tr, u), v)` with
  `lerp(a, b, t) = add(a, mul(sub(b, a), t))`; the order is not
  specified. Outside the grid it takes the nearest edge. Small raised
  areas (open question 3) follow the formula.
- **Water surface** follows SPEC-0014: the frame of the game time (from
  4.98 s, inside the range of verification 11; open question 7), squares
  of `cells` tiles from the map's corner, depth colour and alpha, the
  unit light unclamped, blended over the drawn ground. All colours of
  the viewer are computed on the texture values as shown, without an
  sRGB conversion, as the formulas of the specs are. Without the simulation the viewer's
  own clock drives the frames. No waves (the spec finds none); drawn after the
  models without writing depth.
- **Shoreline models** are placed by SPEC-0014 at the water surface, at
  their own scale, playing their first sequence (Stand 2) from a phase
  of their own (open question 8); several variations go by the tile.
- **Particles** (`PRE2`) follow the plain reading of the public format
  documentation (`ow3-render::particles`): emission, cone, speed and
  variation, gravity, the three segments, head cells; quads face the
  camera or lie in the model's xy plane (`XYQuad`), turned so that the
  top of the texture points the way the particle moves (as the shore
  foam shows in the game). Not done: tails,
  squirts, model space, sorting, light and fog; only placed models
  (doodads, shorelines) emit, units do not. Their random numbers are the
  viewer's own. A placed model starts with 10 s of particles already
  simulated.
- **Ground of ramp middles** (open question 2). The `CliffTrans` models
  are half a level up at the corner of the letter `R` (heights 128, 64, 0
  from `H` over `R` to the far end). The viewer draws ground points with
  the letter `R` or `S` 64 higher than "Ground height" says, so that the
  ground beside a ramp meets its model; with the plain heights a step of
  64 showed under the model's edge. `GetLocationZ` keeps the plain
  heights.

## Movement (`game::movement`)

The move order follows SPEC-0018, part 1: heading and facing, the
propulsion window, speed, per-tick positions and arrival. Choices where
the spec is silent:

- **Paths** (SPEC-0020). Point orders, patrols and attackers on their
  way to their goal points follow the paths of the pathing map; units
  on maps without a `war3map.wpm` go straight. The high-level search stops after 400 iterations (open
  question 1); a unit whose block is not free walks straight to the
  order point; a local path that cannot leave its start cell counts as
  walked; repeated blocks (open question 3) are left out. Against the
  PF maps the ten units of the spec's tables come to rest within 0.025
  of the game.
- **Buildings and destructables** (SPEC-0022). The grid of 64 is that
  of world coordinates (open question 1); a building's footprint goes
  when it dies or is removed (open question 2), a destructable's
  texture changes at once when it dies (open question 4). Textures are
  read for the building types and destructable types of the map when
  it loads. Against c-BG2 the moved positions of the spec's table come
  out exactly; the walkers of c-BW1, c-BC1 and c-BC2 rest within 0.055
  of the game and end their orders at the spec's ticks, as far as the
  maps' prints every 6 ticks show.
- **Units as obstacles** (SPEC-0021). The budget window is 2 ticks
  (open question 1), and a path that had to queue searches at its
  unit's next update; a moving unit that follows no path (an attacker,
  a unit on a map without a pathing map) is never made to wait, the
  unit that meets it waits instead; air units and units of other
  players are not told apart. On c-OM461 the stands of the first and
  fourth pairs come within 0.06 of the game; the second and third pairs
  turn the other way, on one unit of the last place of the squared
  speeds, as in the spec. The westward footman's stands of the spec's
  table come out (394..437 there, about 393..435 here), the knight's
  only in part (322..341 and 376..389 there; 321..339 and 375..387
  here, but not 352..359). On c-UO2 the four walkers created first
  start one update after the other eight, as the spec says.
- **Movement arithmetic** (SPEC-0018). The movement clock starts at 0
  with the first tick of the game (open question 2); the extra commits
  between updates are left out, so the footman of verification 1 rests
  within 0.1 of the game while its first 116 ticks are exact. The
  update cycle is shared by all units at the ticks 5, 11, 17, ... of
  the simulation, one later than the game's timer ticks 4, 10, 16, ...
  of SPEC-0018, as the 0.5 s timers of the tests fire at tick 101.
- **Facing brake.** The tables of `SPEC-0013-facing-brake.tsv`, one per
  `orientInterp`, are interpolated linearly in the engine arithmetic
  (log-log in the spec, which needs logarithms the simulation does not
  have): up to 0.002 degrees off. Above the last point of a table
  nothing brakes.
- **Arrival** compares the squared distance in cells of 32 with 0.49
  squared, in the engine arithmetic (open question 1: the peasant's
  stops are not explained by any form of the test).
- **Orders** follow SPEC-0019: `move`, `patrol`, `attack` and `smart` to
  a point, `attack` on a target, `stop` and `holdposition`. The reaction
  delay applies to `attack` on a target only (open question 1).
- **Chasing** (SPEC-0019 open questions 2 and 6). A moved target does
  not renew the goal at once. A unit that reaches its goal before its
  weapon is ready stands until `cool1` after the last hit. A start
  block that is not free gives way to the free neighbouring block whose
  centre is nearest, for every high-level search. The update cycle has
  the game's phase (SPEC-0018, updates in ticks 7, 13, ...). Against
  c-CH1, c-CW1 and c-CS214 (verifications 5 and 7, in the maps' own
  ticks) the swings, resumptions, standing updates and damage come at
  the spec's ticks, apart from two first hits one tick late; against
  c-CH5 (verification 6) all 13 attackers start and swing at the spec's
  ticks. The tests of the SPEC-0019 tables give their orders in an
  update tick (0.51 s), so that they are taken 6 ticks later as in the
  game's tests. An attack order by a script makes the unit turn and walk
  from its first update at or after the order plus 1 plus the delay,
  and swing no earlier than the first update at or after the order plus
  7 plus the delay ("When it starts"). The knights of SPEC-0019 and
  SPEC-0016 match; of the turning table every turning knight matches,
  the facing one is 6 ticks later than printed. In c-CH5 attackers 5, 6
  and 10 (delays ending 0.1 to 0.6 ticks before an update) start one
  update late: the table needs the first update 5 ticks after the
  orders, the simulation has it 4 after them. The orders come from a
  0 s timer started by the action of `TriggerRegisterTimerEvent` of
  1.00 s; in which tick the game runs them is a question for the
  analyst.
- **Weapons** (SPEC-0016): a type has the weapons `weapsOn` turns on; a
  target is hit by the first one whose `targs` name one of the target's
  `targType` classes, else by weapon 1. Destructables count as `debris`
  and items as `item`; relation words of `targs` (`enemy`, `friend`,
  ...) are not checked. Map object data changes weapon 1 only.
- **Area damage**: units within `Farea`, `Harea`, `Qarea` of the target's
  centre (the fields themselves, open question 6) whose classes are in
  `splashTargs` take the full hit, `Hfact` or `Qfact` of it before armor,
  in the order of their creation after the target; with `enemy` in the
  list only enemies. Destructables and items are not damaged by areas.
  `MiscData.txt` is looked up like `MiscGame.txt`: the map's copy, then
  `Custom_V1` for maps that are not melee, then the game's.
  Destructables and items have collision 0. No queued orders.
- **Spill and bounce** (SPEC-0016 open question 7) are not done: there
  are no upgrades, so no spill, and the choice and reach of a bounce are
  open.
- **Acquisition** (SPEC-0019): the checks, the choice of target and the
  reaction delay follow the spec; the acquisition ticks of verification
  8 match for both seeds, and so do the turns at 10, 10, 34 and 64
  (units created in tick 201 as in the game's test). A unit's checks
  fall in the tick `floor(100 * x)` after its creation, and the
  reaction delay of an acquisition is added to `0.5 * x` and counted
  from the tick of the creation, which equals counting from the exact
  time of the check, but its rounding is not measured. Units created
  in `main` (tick 0) are assumed to count the same way. Units that are not idle and
  paused units do not acquire, buildings with weapons acquire like
  units (open in the spec); `PauseUnit` only stops the acquisition so
  far. The distance check uses plain reals, which can differ from the
  engine arithmetic in the last place. `EVENT_UNIT_ACQUIRED_TARGET`
  runs after all the checks of the tick, unit by unit in the order of
  acquisition; only `GetTriggerUnit` is set (the spec names no response
  for the target).
- **Attack-move** (SPEC-0019): checks, choice, stop and reaction, and the
  walk on after the kill follow the spec; verification 9 matches for
  all ten acquisitions and nine resumptions when the orders come in a
  tick 0 modulo 6 as in the game's test. A unit in its reaction delay
  keeps its facing too (it wobbled by 0.001 rad otherwise). If the
  target dies by another's hit, the unit walks on at once (not
  measured). Any other order taken ends the attack-move.
- **Retaliation and call for help** (SPEC-0019): a weapon hit on an idle
  unit makes it and the idle units round it acquire the attacker, all
  drawing their reaction delays from the tick of the hit, the victim
  first and the helpers in the order of their creation (open). The call
  reaches `CallForHelp + 180` round the victim's centre, a circle (the
  spec's area is open; 180 fits all its distances). Helpers are units
  of the victim's player, or of a player passive and with help response
  toward it that it asks for help (help request); only that both
  directions with all flags answer and help response alone does not is
  measured. Units without a weapon and workers (`peon` in `type` of
  `UnitBalance.slk`) neither retaliate nor help; damage by scripts calls
  no help. In verification 10 the victim and the helper draw the third
  and fourth delays as in the game, and turn 46 and 16 ticks after the
  damage instead of 45 and 15: the hit lands in the tick of the swing's
  damage point here, the glaive's flight is not modelled.
- **Order ids** (SPEC-0019): `attackground` is known to `OrderId` but
  not taken as an order (no spec of its behaviour). A target `move`
  walks to where the target is when ordered, and an immediate `move`
  stops the unit like `stop`; the game only showed that both are taken
  and the current orders that follow.
- **The clock** (SPEC-0017) past midnight starts again from the hour
  less 24 at the game time then, which matches the game to 2e-6 hours
  but not bit for bit.

## Camera (the viewer, SPEC-0023)

The projection has the field of view (70 degrees) across the diagonal
of a 4:3 frame, stretched over the window. Choices where the spec is
silent:

- **Target height** is `GetLocationZ`-like ground height at the target
  point, not the game's smoothed value (open question 1).
- **Near and far planes** are 16 and four times the distance plus
  20000 (open question 2); no fog.
- **Camera fields** are the viewer's own (`--camera` distance, yaw,
  pitch); `SetCameraField` from scripts is not applied.

## Day/night lighting (`ow3-render::daylight`, the viewer)

The ground is lit as SPEC-0017 says, from the terrain model that
`SetDayNightModels` names (Lordaeron without a call). Choices where the
spec is silent:

- **Models** are lit as the spec says, by the same formula from the unit
  model of the call (`DNCLordaeronUnit` without one) with their turned
  normals and the clamp at 1, without the shadow map; water by the same
  light without the clamp. Doodads are placed at the scale and position
  of `war3map.doo` (the game drew a doodad 1.22 times larger and 26 units
  further east than the same model as a unit, open question 2).
- **Cliffs** take the ground light with their own normals (SPEC-0014
  rule 9).
- **Sloped ground** is lit at the corner points and interpolated over
  the two triangles of a tile, as the spec says. Cliff models are bent
  onto the raw heights of the ground under them; their vertices on the
  border of the model's tiles take the ground's normal there (the corner
  normals interpolated bilinearly), the inner ones keep the model's. The
  light is then computed per pixel from the interpolated normal, where
  the game interpolates the light (the difference was not measured).
- **Shadows** of `war3map.shd` darken the ground and the cliff models
  by the formula of the spec; units, doodads and water are not darkened
  (open question 5). The renderer interpolates the block samples by
  linear filtering of an 8-bit texture, so border values of 0.5 are
  stored as 128/255. A block cut by the map edge counts its missing
  cells as without shadow, and the 3 to 5 units the game's samples lie
  west of the formula are not applied.
- **Time of day** is `GAME_STATE_TIME_OF_DAY` as the script sets it; the
  day does not run yet, and without a script the viewer shows noon.
- **Fog** is not drawn (open question 3).

## Math natives (`ow3-math::natives`)

The eleven natives of SPEC-0011 reproduce all 12474 lines of
`SPEC-0011-vectors.tsv`, including the 174 measured in the game.

- **A typo in SPEC-0011.** Under "Asin and Acos", step 1 reads
  `n = f2i(x * 2^30)` "(the pattern of `x` plus `15 * 2^23`)". Adding
  `15 * 2^23` multiplies by 2^15; the vectors need 2^30, that is
  `30 * 2^23` (`15 * 2^24`), which is what the code does. Please correct
  the spec.
- **`Pow` with an integer exponent of 2^31 or more** returns 0 instead of
  hanging; also `expp` with such an integer part.
