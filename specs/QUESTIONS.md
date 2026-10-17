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
- **Not yet:** `GroupEnumUnitsSelected` and `IsUnitInRangeXY` (needs
  collision sizes).

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
- **Conditional life regeneration.** There is no time of day or blight
  yet, so `regenHP` of a type whose `regenType` is `night`, `day` or
  `blight` never applies; only a hero's strength part does. The map's
  `uhrt` field is not read.
- **Regeneration arithmetic.** The elapsed time is a sum of `dt` per
  tick, and a hero's first step after creation is `sub(dt, 0.0001)`. This
  matches the footman and paladin life columns of
  `SPEC-0013-regen-samples.tsv` bit for bit; the paladin mana column
  differs by one unit of the last place at 3 of 60 ticks (with the rate
  `mul(i2r(17), 0.05)`). `mul(i2r(n), dt)` minus 0.0001 instead misses one
  life value. How the game computes the elapsed time and the hero mana
  rate is open.
- **`SetHeroStr`** and the other attribute setters are not implemented
  yet, so a strength change does not restart life regeneration.
- **Turning near the target** (SPEC-0013 open question 1): the unit
  reaches the target once it is no more than a step away, while the game
  shortens the last steps. `orientInterp` 2 and 5..8 turn like 0, and 3
  has no maximum step.
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
- **Attack orders.** Only `IssueTargetOrder(u, "attack", target)` and
  `IssueImmediateOrder(u, "stop")` are known. An attack order draws one
  output of game generator 9 and does not use it; the first hit comes `dmgpt1`
  after the order (open question 1; the game takes 140 to 191 ticks on the
  update steps of the unit), later hits `cool1` apart, rounded to
  whole ticks; hits come after the timers of their tick. Units do not
  move, turn, acquire targets or check their range.
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
  ground list is not added. A lettered cliff texture imported into the
  map would be read in place of the game's copy. Tilesets differ in
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
- **Water surface** (open question 4): the first frame
  `<texFile>00.blp`, repeating every `cells` tiles, multiplied by the
  depth colour and blended with its alpha, drawn after the models without
  writing depth. No animation, no shorelines.
- **Ground of ramp middles** (open question 2). The `CliffTrans` models
  are half a level up at the corner of the letter `R` (heights 128, 64, 0
  from `H` over `R` to the far end). The viewer draws ground points with
  the letter `R` or `S` 64 higher than "Ground height" says, so that the
  ground beside a ramp meets its model; with the plain heights a step of
  64 showed under the model's edge. `GetLocationZ` keeps the plain
  heights.

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
