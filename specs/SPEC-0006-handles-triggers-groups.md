---
id: SPEC-0006
title: Handles, trigger conditions, callbacks, groups and forces
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

### Handle ids

- Objects created by the game or by scripts get ids from 0x100000
  (1048576) upwards. Many ids are taken before `main` runs: `Player(0)`
  is 0x100008, and the first `Location` created in `main` of a 12-player
  melee map got 0x10118C.
- Values made by the `Convert...` natives and the constants built from
  them are not allocated objects: their id is the integer they were made
  from (`GetHandleId(GetGameSpeed())` is 2 at speed index 2,
  `GetHandleId(RACE_HUMAN)` is 1).
- `Player(n)` for `n` outside 0..15 returns `null` (id 0); tested for
  -1, 16 and 100.
- An id is reused only after the object is destroyed and no variable
  refers to it any more. A location removed while a local still pointed
  to it kept its id. With the locals set to `null`, the ids freed by
  three locations and a timer were not handed out at once (the next
  objects got new ids), but were reused after a 1 s sleep, the most
  recently freed first (freed 1053793 and 1053795, then new objects got
  1053795 and 1053793).

### Boolean expressions

- `Condition(f)` and `Filter(f)` for the same function return the same
  handle, also across the two natives.
- `And(a, b)` evaluates `b` only if `a` is true; `Or(a, b)` evaluates `b`
  only if `a` is false.
- `And` with a `null` operand, or with an operand destroyed by
  `DestroyBoolExpr`, evaluates as false.

### Trigger conditions

- Conditions are evaluated in the order they were added.
- All conditions are evaluated, also after one has returned false; the
  result is true only if all returned true.
- When an event fires, the actions run only if the result is true.
  `TriggerEvaluate` returns the result; `TriggerExecute` runs the actions
  without evaluating the conditions.

### Callbacks

This covers trigger conditions and the callbacks and filters run by
`ForGroup`, `ForForce`, `GroupEnum...`, `EnumDestructablesInRect` and
similar natives.

- Each callback runs as its own thread with its own operation budget
  (SPEC-0005): a callback that runs 195000 operations after the caller has
  already used 195000 completes, and so does the caller.
- An error in a callback (division by zero, the operation limit) ends
  only that callback; the enumeration continues with the next element and
  the caller continues after the native.
- A sleep ends the callback (SPEC-0005); a filter that sleeps counts as
  false.

### Groups

- `GroupEnumUnits...` natives and `ForceEnumPlayers` first clear the group
  or force they fill.
- `GroupEnumUnitsInRange(g, x, y, r, filter)` takes a unit when the
  distance from its centre to `(x, y)` is at most `r`. `IsUnitInRangeXY`
  also counts the unit's collision size: it is true when the distance
  minus the collision size is at most `r` (a footman, collision 31, at
  distance 1000 is in range from `r = 969`).
- `ForGroup` does not visit units in the order they were added: five
  units added in order 1..5 were visited as 5 3 4 1 2, and after removing
  the first one (`FirstOfGroup`, unit 5) as 3 4 1 2. Filters of
  `GroupEnumUnitsInRange` were called in another order (1 2 4 3 5 for the
  same units). The rule for these orders is open question 1.
- During `ForGroup` the callback may remove and add units. A unit added
  during the enumeration is not visited (three units, the first visited
  removes itself and adds a new one: visited 3 units, the count after is
  3).
- A unit removed from the game with `RemoveUnit` stays in groups for a
  short time: right after `RemoveUnit` the group still counts it and
  `FirstOfGroup` still returns it; 0.5 s later it is gone.

### Destructables

- `EnumDestructablesInRect(r, filter, action)` runs the action for a
  destructable right after its filter returned true, before the filter of
  the next one: with a filter that always returns true, the calls came as
  filter 1, action 1, filter 2, action 2, and so on.
- The destructables are the `war3map.doo` entries whose type is a
  destructable type of the game data. In `(12)IceCrown`,
  `EnumDestructablesInRect(GetWorldBounds(), ...)` called the filter
  4471 times: exactly the number of `war3map.doo` entries with a type
  listed in `Units\DestructableData.slk`. The other 1486 entries
  (doodads) were not enumerated. Custom destructable types of a map were
  not tested.

### Forces

- `ForForce` visits players in ascending player number, whatever the order
  they were added in.
- `ForceEnumPlayers(f, null)` puts in the players 0..11 whose slot is
  playing, users and computers. Players 12..15 are not included although
  their slot state is playing.

### ExecuteFunc

- `ExecuteFunc(name)` runs the named function in a new thread (SPEC-0005).
  The name is case-sensitive.
- An unknown name does nothing; the caller continues.
- A function that returns a value can be run; the value is discarded.
- A function with parameters: after the call the game produced no
  further script output; behaviour not specified (open question 2).

## Open questions

1. Enumeration order of `ForGroup` and of `GroupEnum...` filters.
2. `ExecuteFunc` of a function with parameters.
3. Which arithmetic the range tests use (engine arithmetic or processor
   floats), relevant for units exactly at the boundary.
4. When exactly a freed handle id becomes reusable, and when a removed
   unit leaves its groups.
5. `Or` with a `null` or destroyed operand.

## Sources

- Test maps on 1.27b under Wine, single player, run by the analyst
  workspace, 2026-10-17.

## Verification

Test maps built from a 12-player melee map without victory conditions;
the expected values are the ones quoted above. Units are footmen
(`'hfoo'`) moved to exact positions with `SetUnitX`/`SetUnitY`. Player 1
is set to computer control in `config`.
