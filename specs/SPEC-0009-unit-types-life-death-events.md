---
id: SPEC-0009
title: Unit type ids, hit points of new objects, death events
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

### String ids of unit types

- `UnitId(s)` ignores ASCII case: `"footman"`, `"Footman"` and
  `"FOOTMAN"` all give `'hfoo'`. An unknown string and `""` give 0.
- `UnitId2String` of a standard type gives the `name` column of
  `Units\unitUI.slk` (`'hfoo'` -> `"footman"`, `'Hpal'` -> `"paladin"`).
- A custom unit type of the map (from `war3map.w3u`) that is not a hero
  has the string id `"custom_"` followed by its raw code:
  `UnitId2String('h000')` is `"custom_h000"`, and
  `UnitId("custom_h000")` and `UnitId("CUSTOM_H000")` give `'h000'`.
- A custom hero type (`'H000'` based on `'Hpal'`) has the empty string id:
  `UnitId2String('H000')` is `""`.
- A custom type does not take over the string id of its base type:
  with custom types based on `'hfoo'` and `'ogru'` in the map,
  `UnitId("footman")` is still `'hfoo'` and `UnitId("grunt")` is `'ogru'`.
- `UnitId2String` of an unknown raw code gives `""`.

### Heroes

A unit type is a hero exactly when the first character of its raw code
is an uppercase ASCII letter `A`..`Z` (the top byte of the integer is
0x41..0x5A). The type's data does not matter:

| Type | Based on | `IsHeroUnitId` | `IsUnitType(u, UNIT_TYPE_HERO)` | `GetHeroLevel` | Max life |
|---|---|---|---|---|---|
| `'Hpal'` | | true | true | 1 | 650 |
| `'H000'` | `'Hpal'` | true | true | 1 | 650 |
| `'O000'` | `'Hpal'` | true | true | 1 | 650 |
| `'h003'` | `'Hpal'` | false | false | 0 | 100 |
| `'Hf00'` | `'hfoo'` | true | true | 1 | 420 |
| `'h004'` | `'hfoo'` | false | false | 0 | 420 |

- `IsHeroUnitId` looks only at the code: `IsHeroUnitId('Zzzz')` is true
  although no such type exists (and `CreateUnit` of it gives `null`).
  Codes starting with `@` (0x40), `[` (0x5B), a digit, a lowercase letter,
  a space, 0x00 or 0xC0 are not heroes.
- A hero made from a non-hero type (`'Hf00'`) has strength 0, so its life
  is just the `HP` value; it has 0 skill points and the proper name
  `"Default string"` (no proper names in its data). Heroes from hero data
  start with 1 skill point and a proper name from their list.
- A non-hero code based on a hero type (`'h003'`) gets no strength bonus:
  its life is the base `HP` (100 for the paladin).
- A new hero's max life is its `HP` (or the map's `uhpm`) plus 25 per
  point of starting strength (`STR`, or the map's `ustr`): a paladin with
  `uhpm` 200 has 750, a paladin with `ustr` 30 has 850, a footman-based
  hero `'Hf01'` with `ustr` 10 has 420 + 250 = 670.

### Hit points of new units

`GetUnitState(u, UNIT_STATE_MAX_LIFE)` of a new unit, and its life:

- Units that are not heroes: the `HP` column of `Units\UnitBalance.slk`
  (footman 420), or the value set by the map with `uhpm` (a custom footman
  with `uhpm` 333 had 333).
- Heroes (see above): `HP` plus 25 per point of strength at level 1, which
  is the `realHP` column for the shipped heroes. The paladin (`HP` 100, strength 22) has 650, the
  crypt lord (`HP` 25, strength 26) has 675. A custom hero based on the
  paladin without changes also has 650. For all non-hero types of the
  1.27b data the two columns are equal.

### Hit points of placed destructables

A destructable placed in `war3map.doo` with life percentage `p` (the byte
after the flags) starts with `HP * p / 100` hit points, where `HP` is the
`HP` column of `Units\DestructableData.slk` (or the map's value);
`GetDestructableMaxLife` is `HP`. For a tree with `HP` 50:

| Percentage | `GetDestructableLife` | `GetDestructableMaxLife` |
|---|---|---|
| 100 | 50.0 | 50.0 |
| 50 | 25.0 | 50.0 |
| 1 | 0.5 | 50.0 |
| 0 | 0.0 | 50.0 |

The value is not rounded (0.5 for 1 percent). `GetWidgetLife` gives the
same values.

`SetDestructableLife(d, 0)` on a living destructable kills it: its
`TriggerRegisterDeathEvent` trigger fired.

### Unknown types

`CreateUnit`, `CreateItem` and `CreateDestructable` with a raw code that
is not in the game data or the map (`'zzzz'`) return `null` (handle id 0).

### Order of death events

When a unit dies, the triggers registered for its death run grouped by the
kind of registration, in this order:

1. `TriggerRegisterDeathEvent` (widget death) triggers;
2. `TriggerRegisterPlayerUnitEvent` triggers with
   `EVENT_PLAYER_UNIT_DEATH`;
3. `TriggerRegisterUnitEvent` triggers with `EVENT_UNIT_DEATH`.

Within one kind, the triggers run in the order they were registered. Each
trigger is handled completely (filter, conditions, actions) before the
next one starts. Six triggers registered as unit, player (with filter),
widget, unit, player (without filter), widget ran as: widget 3, widget 6,
player 2 (filter, condition, action), player 5, unit 1, unit 4.

All of them ran inside the native that killed the unit
(`UnitDamageTarget` here): the order was complete when it returned.

### Filter of player unit death events

For `TriggerRegisterPlayerUnitEvent(t, p, EVENT_PLAYER_UNIT_DEATH, filter)`:

- The trigger fires only for units owned by `p`; for the death of a unit
  of another player the filter is not called either.
- **`GetFilterUnit()` in the filter is the killing unit**, not the dying
  unit. This held for a killer of another player and for a killer of
  the same player.
- When the unit dies without a killing unit (`KillUnit`), **the filter is
  not called** and the trigger fires as if the filter had returned true.
  With a filter that always returns false, `KillUnit` still ran the
  action; a death by damage from a unit did not.
- Other event responses inside the filter:
  - `GetTriggerPlayer()` is `p`;
  - `GetTriggerUnit()`, `GetDyingUnit()` and `GetKillingUnit()` are
    `null`;
  - `GetTriggerEventId()` and `GetTriggeringTrigger()` are the values of
    the thread in which the unit died: for a death caused by a script
    running from a timer trigger, the timer event and that trigger; for a
    death in combat, `null` (id 0) for both.
- In the conditions and actions of the trigger the responses are those of
  the death event: `GetTriggerUnit()` and `GetDyingUnit()` are the dying
  unit, `GetKillingUnit()` the killer or `null`, `GetTriggerPlayer()` the
  owner `p`, `GetTriggerEventId()` is `EVENT_PLAYER_UNIT_DEATH` and
  `GetTriggeringTrigger()` is `t`. For a `TriggerRegisterUnitEvent`
  trigger, `GetTriggerEventId()` is `EVENT_UNIT_DEATH`.

## Edge cases

- `GetFilterUnit()` outside a filter is `null`; inside a death filter
  `GetFilterPlayer()` and `GetEnumUnit()` are also `null`.

## Open questions

1. Whether other player unit events also give their filter the second
   unit of the event (attacker, killer), and whether events without a
   second unit skip the filter in the same way.
2. A destructable placed with 0 percent life was enumerated by
   `EnumDestructablesInRect` over a large rect, but not over a 30 x 30
   rect around its position; whether such a destructable counts as dead.
3. Names of custom units: a custom footman with two modifications
   (`uhpm`, then `unam`) kept the new hit points but `GetUnitName` did not
   show the new name; with only `unam` the name was used. Not
   understood yet; may be a fault of the test file.
4. The order of `EnumDestructablesInRect` (it is not the order of
   `war3map.doo`).

## Sources

- Test maps on 1.27b under Wine, single player, run by the analyst
  workspace, 2026-10-17. Custom types were added with a `war3map.w3u` of
  format version 2 written for the test; destructable life percentages
  were changed in `war3map.doo`.

## Verification

Test maps built from a 12-player melee map without victory conditions.
The script runs from a 1 s timer trigger; units are footmen.

1. `UnitId` with the three spellings, `UnitId2String` of `'hfoo'`,
   `'h000'` (custom footman), `'H000'` (custom paladin), `'zzzz'`: as above.
2. `CreateUnit` of `'hfoo'`, `'h000'` (`uhpm` 333), `'Hpal'`, `'H000'`,
   `'Ucrl'`: max life 420, 333, 650, 650, 675.
3. Four trees (`'WTst'`) in `war3map.doo` set to 50, 1, 0 and 100 percent:
   life 25, 0.5, 0, 50.
4. `CreateUnit`, `CreateItem`, `CreateDestructable` of `'zzzz'`: id 0.
5. Six triggers on one footman's death as above, killed with
   `UnitDamageTarget` from a footman of player 1: order w3 w6 p2 p5 u1 u4.
6. One player death trigger with a filter: `KillUnit` -> action only;
   `UnitDamageTarget` from a unit of player 1 -> filter with that unit as
   `GetFilterUnit()`; a combat kill (attack order) -> same, with event id
   and triggering trigger 0 in the filter.
7. Heroes: `CreateUnit` of the types of the table above, and
   `IsHeroUnitId` of codes starting with 0x40, 0x41, 0x5A, 0x5B, 0x30,
   0x61, 0x7A, 0xC0, 0x20, 0x00: true only for 0x41 and 0x5A.
