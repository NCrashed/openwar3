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

Precedence, literals and escapes are answered by SPEC-0005. Still open:

- **Reserved words.** The parser reserves the six built-in type names
  (`integer`, `real`, `boolean`, `string`, `handle`, `code`) as well as the
  keywords; SPEC-0001 confirms this only for `code` as a parameter name.
- **Unary `+`.** SPEC-0005 lists `not` and unary `-`; the parser also
  accepts unary `+`. Does the game?
- **Raw codes of five or more characters.** SPEC-0005 rejects 2 and 3
  characters; the parser rejects every length other than 1 and 4.
- **Lines.** Are string literals across line breaks accepted, and is a
  carriage return alone a line break?
- **Names.** May names start or end with an underscore?

## JASS names and types (`ow3-jass::check`)

`common.j` with `Blizzard.j`, all shipped map scripts and all but seven AI
scripts pass the checker. Two map test scripts for SPEC-0001 fail with the
native name clash that spec describes. Not yet confirmed:

- **Outdated AI scripts.** `Scripts\demo.ai`, `h04_orange.ai` and the five
  `orc03*.ai` call `common.ai` functions with older signatures (for example
  `CampaignAI` with one argument, while `common.ai` takes `integer farms,
  code heroes`) or use undeclared names. Does the game ever compile them,
  and against which `common.ai`?
- **`null` as an integer.** `(6)BomberCommand.w3x` returns `null` from an
  integer function and the map is shipped, so the checker accepts `null`
  where an integer is expected. Is that true everywhere (arguments,
  assignments, `real`, `boolean`), and what value results?
- **Repeated locals.** The same map declares `local real rHeight` twice in
  one function. The checker accepts a repeated local of the same type. Is
  a different type accepted, and can a local repeat a parameter's name?
- **Order of declarations.** The checker requires a function to be
  declared above the code that calls it or refers to it with `function`,
  and a global above the global initialiser that uses it.
- **Shared names.** May a global share its name with a function, or a local
  with a global (the checker allows both)?
- **Comparisons.** The checker allows `==` and `!=` between any two handle
  types and between `null` and handle, string or code values.
- **Code values.** Can `function F` refer to a native, or to a function
  that takes parameters?

## JASS operation costs (`ow3-jass::vm`)

The costs of SPEC-0005 reproduce its four published loop measurements and
the costs it adds for conversions, `if` with an empty branch and a bare
`return`. Choices where the spec is silent:

- **Global initialisers** run before any thread and are not counted.
- **Callbacks** that natives run through `code` values: see "Forces, groups
  and enumerations" below.

## Game time (`ow3-sim`)

The timer queue follows SPEC-0002 and reproduces its tests T1c and T3c
within the resolution of the 1000 s reference clock (2^-14 s). The test
maps T3c, SP, PQ (SQ), PZ, LT and RD run as scripts in `ow3-sim` and match
the spec. Choices where the spec is silent:

- **Order inside a chunk.** The chunk's ticks run first, then the sleep
  check that ends it. At full speed this puts checks at game times 0.1, 0.2,
  ... as the spec says.
- **Threads due at the same check** resume in the order they went to sleep.
- **`fmod` for long timers.** Answered: SPEC-0004 now defines it in
  "Fractional part and remainder" (not in this repository yet). Until it
  arrives we use `mul(sub(t, floor(t)), 120.0)` with `t = div(timeout,
  120.0)`, which lacks the final correction.
- **`TimerGetRemaining` of a long timer.** We add the parts still to run to
  `sub(e, q)` with `add` and `mul(i2r(parts), 120.0)`. How does the game
  compute it?
- **Pausing a long timer.** `ResumeTimer` splits the saved remaining time
  again by the same rule. Is that right?
- **Never-started timer.** `sub(0, 0)` gives -0.0 under SPEC-0004, so we
  return 0 directly for `TimerGetElapsed` of a timer never started. What
  does `R2S` print for -0.0?
- **Periodic restart.** A periodic timer is queued again before its
  callback runs, with the queue time at its expiry. Is it before or after
  the callback, and does `TimerStart` in the callback replace it?
- **`DestroyTimer`** of a running timer removes it from the queue. Does a
  destroyed timer whose callback is running still fire later if periodic?
- **Script execution time.** Our scripts take no real time, so test CU
  (stall) cannot be reproduced from the script; catch-up is tested by
  adding real time directly.

## Triggers and natives (`ow3-sim::game`)

Provisional choices in the scheduler and natives, used by the test maps:

- **Handles** of objects are numbered from `0x100000` upwards in creation
  order. A `Convert*` value is not an object: its handle id is the integer
  itself (answered by test B1, `GetHandleId(GetGameSpeed())` gives 2).
  Players from `Player` are objects created on first use. Is the first
  object handle always `0x100000`, and are ids of destroyed objects reused?
- **Conditions** run in their own threads, in the order added, and stop at
  the first false; a condition that sleeps or ends by an error counts as
  false. Is that the game's order and short-circuit?
- **`Condition` and `Filter`** return a new boolexpr on every call. Does the
  game return the same handle for the same function?
- **`ExecuteFunc`** of an unknown name or of a function with parameters
  does nothing. What does the game do?
- **`R2S` and `R2SW`** print with Rust's rounding (`{:.3}` and width and
  precision). What are the exact rules, especially rounding of halves,
  large values and -0.0?

## Forces, groups and enumerations (`ow3-sim::game::enumerate`)

Tests B1, B6 and L5 match SPEC-0005 (callbacks and filters that sleep end
at the sleep, the enumeration goes on). Units are placeholders with owner,
type, position, facing and a dead flag until units are specified. Choices
where the specs are silent:

- **Budget and errors.** Each callback and each filter runs in a thread of
  its own, with its own operation budget, and an error ends only that
  callback. Is that so, or do they share the caller's budget?
- **A filter that sleeps** counts as false, like a condition. Is the unit
  or player then left out?
- **Order.** `ForForce` goes by increasing player id; `ForGroup` in the
  order units were added; `GroupEnum*` in the order units were created.
- **Clearing.** `GroupEnum*` and `ForceEnumPlayers*` clear the group or
  force first. Does `ForceEnumPlayers` clear?
- **`ForceEnumPlayers` with a `null` filter** adds every player whose slot
  is playing; see "Map loading and player slots" for which slots those
  are.
- **Changes during `ForGroup`.** It works on a copy of the group taken at
  the start and skips handles that no longer refer to units. What happens
  when the callback adds or removes units, or a unit is removed from the
  game while still in a group?
- **Range and rect tests.** A unit is in range when
  `add(mul(dx, dx), mul(dy, dy)) <= mul(r, r)` with `dx = sub(x_u, x)`, and
  in a rect when `min <= x_u <= max` on both axes. Does the range include
  the unit's collision size, and which operations does the game use?
- **Dead units** are enumerated by `GroupEnum*` until they are removed.
- **Counted variants** stop after `countLimit` units or players have been
  added; a limit of 0 or less adds none.
- **`And`, `Or`, `Not`** test left to right and stop as soon as the result
  is known; a `null` operand or a destroyed boolexpr counts as true. Each
  call makes a new boolexpr.
- **`Player(n)`** gives `null` outside 0..15.
- **`EnumDestructablesInRect`** first tests every destructable in the rect
  with the filter, then runs the action for those that passed, in the
  order of `war3map.doo`. Or does it run the action right after each
  filter? A destructable is in the rect when `min <= x <= max`.
- **Not yet:** `EnumItemsInRect`, `GroupEnumUnitsOfType` (needs unit
  names), `GroupEnumUnitsSelected` and death events.

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
  of `war3map.w3i`; the slots in the game are the local player's and every
  computer slot. Other user slots are empty. Is that what the game does
  for a map started from the single player menu, for melee maps and for
  maps with fixed player settings?
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
- **Real literals in `config`.** By SPEC-0004 and SPEC-0005 a literal reads
  like `S2R`, so `DefineStartLocation(0, -8384.0, 1792.0)` gives
  `GetStartLocationY(0) = 1792.0001` (`0x44E00001`). Can this be confirmed
  in the game?
- **Random numbers.** `GetRandomInt` and `GetRandomReal` use a stand-in
  generator so that scripts see varied values: maps that wait
  `PolledWait(GetRandomReal(20, 45))` and then run their own trigger again
  recursed without end while the stub returned 0. We need the game's
  generator: algorithm, seed at game start, `SetRandomSeed`, and how
  bounds (inclusive, swapped) are handled. This matters for replays and
  multiplayer.
- **Nested threads.** Threads started by natives inside other threads
  (`ExecuteFunc`, `TriggerExecute`, `TriggerEvaluate`, `ForGroup`, ...)
  may nest 100 deep; one more fails in the caller. What does the game do
  with deep nesting, for example a trigger that runs itself with
  `TriggerExecute` without sleeping?
- **`SubString`** uses byte positions clamped to the string, and an end
  before the start gives "". **`StringCase`** changes ASCII letters only.
  What does the game do with positions out of range and with non-ASCII
  text?
