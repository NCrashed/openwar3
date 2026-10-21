---
id: SPEC-0042
title: Random item types (ChooseRandomItem, ChooseRandomItemEx)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, generator draws and the item type list read in the running game)"]
---

## Behaviour

The natives `ChooseRandomItem(level)` and `ChooseRandomItemEx(type,
level)` return the id of a random item type (0 if there is none). Melee
maps made by the editor call them in the drop functions of their creeps
(for example the creeps of `(12)IceCrown.w3m`: a `RandomDistAddItem` of
`ChooseRandomItemEx(ITEM_TYPE_CHARGED, 3)` with weight 100); what is
then created is up to the script (`RandomDist*` of `Blizzard.j`, script
generator, `CreateItem`).

### The list of item types

The game keeps the item types in the order of the rows of the item data
table it loaded (`ItemData.slk`, one row per item type, the order of the
file). Which file is loaded depends on the map, as for all object data:

| map | file (from `War3Patch.mpq`) | measured |
|---|---|---|
| `(12)IceCrown.w3m` with another script (test RZB) | `Units\ItemData.slk` | order and pools exact |
| a TFT custom map `.w3x` (test RZA) | `Custom_V1\Units\ItemData.slk` | order and pools exact |

The two files differ in order and in some levels (for example `sres`
has `Level` 5 in both, but stands at another place; `ankh` is level 4
in the first and level 5 in the second).

### The choice

`ChooseRandomItemEx(type, level)`:

1. Go through the item types in list order and keep those with
   `pickRandom` = 1, `Level` = `level`, and `class` equal to `type`
   (`ITEM_TYPE_PERMANENT` = `Permanent`, `ITEM_TYPE_CHARGED` =
   `Charged`, `ITEM_TYPE_POWERUP` = `PowerUp`, `ITEM_TYPE_ARTIFACT` =
   `Artifact`, `ITEM_TYPE_PURCHASABLE` = `Purchasable`,
   `ITEM_TYPE_CAMPAIGN` = `Campaign`, `ITEM_TYPE_MISCELLANEOUS` =
   `Miscellaneous`; `ITEM_TYPE_ANY` keeps every class). This gives a
   list of `n` item types.
2. If `n` = 0 the result is 0, and no random number is drawn.
3. Otherwise one output `r` (32 bits) of game generator 35 (SPEC-0007,
   "Game generators") is drawn, and the result is the item type at
   index `floor(r * n / 2^32)` of the list (the high 32 bits of the
   64-bit product `r * n`).

`ChooseRandomItem(level)` is the same with every class kept and the
column `oldLevel` in place of `Level`.

Measured (tests RZA and RZB, 600 calls each: the five calls above for
levels 0 to 9, twelve times): the 396 calls with a non-empty list each
drew exactly one output of generator 35 and returned the item at that
index of the list built as above (396 of 396 in both data sets); the
204 calls with an empty list drew nothing and returned 0. No other
generator and not the script generator was used by these natives.

## Edge cases

- An empty list (for example permanent items of level 0, or any class
  of level 9 in these data) returns 0 and leaves generator 35 as it
  was, so the draws of later calls do not shift.
- Items without `pickRandom` = 1 are never returned by these natives.

## Open questions

1. Which item data file a TFT melee map (`.w3x` of the melee folder)
   and a RoC custom map load.
2. `ChooseRandomCreep` and `ChooseRandomNPBuilding` (presumably the
   same scheme over unit types).
3. The other uses of generator 35.

## Sources

- Test maps RZA (TFT, a copy of a custom `.w3x`) and RZB (a copy of
  `(12)IceCrown.w3m` with a test script) on 1.27b under Wine,
  2026-10-21: each call's arguments, the generator drawn, the drawn
  output and the list size, read in the running game; the returned ids
  logged by the map's script; the game's list of item types read in
  memory and compared row by row with the files above (273 of 273).
- `Units\ItemData.slk` and `Custom_V1\Units\ItemData.slk` of
  `War3Patch.mpq` 1.27b.

## Verification

1. With the item data of `(12)IceCrown.w3m` (first file), charged level
   0 is the list `pomn`, `wshs` (in that order); `ChooseRandomItemEx(
   ITEM_TYPE_CHARGED, 0)` returns `pomn` when `r < 2^31` and `wshs`
   otherwise.
2. A sequence of calls with empty lists between non-empty ones draws
   exactly one output of generator 35 per non-empty list.
