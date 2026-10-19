---
id: SPEC-0030
title: Creeps guarding their place (Neutral Hostile)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps logged every tick)"]
---

## Behaviour

Units of the neutral hostile player (`Player(12)`, "creeps") stand at
the place where they were created, their **home**, and fight units of
other players that come near, as other units do (acquisition,
SPEC-0019). This spec gives what is particular to them: helping each
other and going back home. It is a first version from one test map
(kobolds `nkob`, a knight and a footman, by day); sleeping at night and
creeps with abilities are not covered.

### Data

From `Units\MiscGame.txt`:

| Field | Value | Use |
|---|---|---|
| `GuardDistance` | 600 | distance from home after which a chase can end |
| `MaxGuardDistance` | 1000 | (see open question 1) |
| `GuardReturnTime` | 5.0 | seconds |
| `CreepCallForHelp` | 600 | radius of the call for help between creeps |

### Joining a fight

A creep acquires a target like any unit (its weapon's `acquire`, 500
for a kobold). Other creeps near it come as well: in test RUK two
kobolds stood 128 apart; a footman walking past came within 497.8 of
the first one (inside its 500) when it was 618.5 from the second, and
both left their places (in ticks 24 apart, the reaction delays of
SPEC-0019). A kobold 400 from one that a knight hit joined the chase
in the same way. The call reaches `CreepCallForHelp` (600) from the
creep that acquired or was hit (assumed from the data; the tests only
had creeps 128 and 400 apart).

While a creep chases by acquisition its current order is 0
(`GetUnitCurrentOrder`).

### Going back home

A creep that has gone farther than `GuardDistance` (600) from its home
gives up the chase `GuardReturnTime` (5 s) later if nobody attacks it
in that time: it gets a `move` order (851986) to its home, walks back
and stands there with order 0 (within 8 to 11 of the home point in the
tests). Measured, by the distance from home logged every 4 ticks
(ticks counted by the test's 0.005 s periodic timer from the start of
the test, game tick = count + 202):

| Creep | Passed 600 | Passed 1000 | Turned back | Farthest |
|---|---|---|---|---|
| kobold chasing a fleeing knight | tick 731 | 1027 | 1811 | 2056 |
| kobold hit by a knight, chasing | 779 | 1079 | 1831 | 2017 |
| second kobold of that camp | 775 | 1091 | 1831 | 1976 |

So the creeps turned back 1052 to 1080 ticks (5.3 to 5.4 s) after
passing 600 from home, and passing 1000 did not make them turn back at
once. The two kobolds of one fight turned back in the same tick.

## Edge cases

- The knight that hit a kobold was not hit back while it fled at 350
  (the kobold's speed is 270), so the time without being attacked is
  counted from the passing of `GuardDistance`.

## Open questions

1. What `MaxGuardDistance` changes (by the data's description, a creep
   beyond it goes back even while it is attacked; not tested), and
   whether the 5 s are counted again after each attack on the creep.
2. Why two creeps of one fight turn back in the same tick (one timer for
   the group, or a check at fixed times; the turn came 20 ticks apart
   for creeps of two separate fights).
3. Whether a creep going back home can be made to fight again on the
   way, its regeneration, and how creeps that went home choose their
   facing.
4. Sleeping creeps (`canSleep`) at night, waking up, and the
   notification of creeps when a building is placed near them
   (`BuildingPlacementNotifyRadius`).
5. How "home" is defined for creeps placed in the map (`war3mapUnits.doo`)
   and for creeps that are given orders by a script.

## Sources

- Test map RUK on 1.27b under Wine, 2026-10-20, time of day fixed at
  12:00: (A) a kobold and a knight that attacks it and then runs away
  east, (B) two kobolds 128 apart and a footman walking past them at
  about 460, (C) two kobolds 400 apart and a knight that attacks one
  and runs away north. Positions, orders and life of all units logged
  every 4 ticks, damage events logged.
- `Units\MiscGame.txt`, `Units\UnitWeapons.slk`.

## Verification

1. (A) The kobold is hit in tick 273, chases the knight, passes 600 from
   home in tick 731 and gets order 851986 in tick 1811, 2056 from home;
   it stands at home again (order 0) from tick 3371.
2. (C) Both kobolds get order 851986 in tick 1831 and are home again by
   tick 3355 and 3343.
