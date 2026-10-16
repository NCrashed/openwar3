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
- **Nested threads.** Threads started by natives inside other threads
  (`ExecuteFunc`, `TriggerExecute`, `TriggerEvaluate`, `ForGroup`, ...)
  may nest 100 deep; one more fails in the caller. What does the game do
  with deep nesting, for example a trigger that runs itself with
  `TriggerExecute` without sleeping?

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
- **`SubString`** uses byte positions clamped to the string, and an end
  before the start gives "". **`StringCase`** changes ASCII letters only.
  What does the game do with positions out of range and with non-ASCII
  text?
- **Random numbers.** `GetRandomInt` and `GetRandomReal` use a stand-in
  generator so that scripts see varied values: maps that wait
  `PolledWait(GetRandomReal(20, 45))` and then run their own trigger again
  recursed without end while the stub returned 0. We need the game's
  generator: algorithm, seed at game start, `SetRandomSeed`, and how
  bounds (inclusive, swapped) are handled. This matters for replays and
  multiplayer.

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
- **`EnumDestructablesInRect`** first tests every destructable in the rect
  with the filter, then runs the action for those that passed, in the
  order of `war3map.doo`. Or does it run the action right after each
  filter?
- **`ForceEnumAllies` and `ForceEnumEnemies`** take the same players as
  `ForceEnumPlayers`, split by `IsPlayerAlly`.
- **Not yet:** `EnumItemsInRect`, `GroupEnumUnitsOfType` (needs unit
  names), `GroupEnumUnitsSelected`, `IsUnitInRangeXY` (needs collision
  sizes) and death events.

## Map loading and player slots (`ow3-sim::map`, `game::players`)

`Map::load` reads a map with the game archives, and `Game::load_map`
starts it as single player. All 191 maps of the installation (outside the
test folders) load, run `config` and `main` and 30 s of game time without
a thread error. Test W1 matches SPEC-0003. Choices where no spec says
anything yet:

- **Archive order.** Files come from the map, then `War3Patch.mpq`,
  `War3xlocal.mpq`, `War3x.mpq`, `War3local.mpq`, `War3.mpq`. A map may
  replace `Scripts\common.j` and `Scripts\Blizzard.j` with its own copies.
  Is that the game's order, and does the game accept such copies?
- **Who plays in single player.** The local player is the first user slot
  of `war3map.w3i`. Computer slots of the map play, and so does a slot
  that `config` gives to a computer with `SetPlayerController`; other user
  slots are empty. The neutral slots 12..15 always play (SPEC-0006). Is
  that what the game does for melee maps and for maps with fixed player
  settings?
- **Slots before `config`.** Controller from the `war3map.w3i` slot type
  (user, computer, neutral, rescuable), race from the slot (human when the
  race is selectable), name from the slot with trigger strings resolved,
  start location `n` for player `n`, team by force. Slots the map does not
  list are users (0..11) or neutral (12..15), named "Player <n+1>". Which
  of these does the game set, and to what?
- **`GetPlayerRace` with a selectable race** gives human. What does the
  game pick in single player?
- **Alliances.** A player is an ally of itself and of every player it
  gives `ALLIANCE_PASSIVE`; `IsPlayerEnemy` is the opposite. Is that the
  rule for `IsPlayerAlly` and `IsPlayerEnemy`, and which alliances does
  the game set before `config` and `main`?
- **World bounds** span the whole terrain grid: from the `war3map.w3e`
  origin to `origin + (size - 1) * 128`. Each `GetWorldBounds` call makes a
  new rect. What are the game's bounds, and does it return a new rect?
- **Destructables** are the `war3map.doo` entries whose type is in
  `Units\DestructableData.slk` or is a custom destructable of the map.
  Are other placed doodads ever enumerated?
