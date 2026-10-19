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
other and going back home. Measured with kobolds (`nkob`), knights, riflemen
and footmen, by day and at night; creeps with abilities are not
covered.

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

A creep's home is the point where it was created; a `move` order from
a script does not change it (a kobold moved 600 away by a script, then
drawn into a chase, went back to the point of its creation).

A creep that has gone farther than `GuardDistance` (600) from its home
gives up the chase about `GuardReturnTime` (5 s) later: it gets a
`move` order (851986) to its home, walks back and stands there with
order 0 (within 8 to 12 of the home point in the tests). Attacks on it
did not delay this in any test: hits by a rifleman 3 s after it passed
600, hits every 1.5 s from the moment it passed 1000, and a hit by the
very knight it chased (which turned back and attacked it 2.5 s after
it passed 600) all left the time from passing 600 to turning back at
1052 to 1080 ticks. On the way home it does not answer attacks (a
kobold shot by a rifleman every 1.5 s kept its `move` order until it
was home) and fights again once it is home. Measured, by the distance from home logged every 4 ticks
(ticks counted by the test's 0.005 s periodic timer from the start of
the test, game tick = count + 202):

| Creep | Passed 600 | Passed 1000 | Turned back | Farthest |
|---|---|---|---|---|
| kobold chasing a fleeing knight | tick 731 | 1027 | 1811 | 2056 |
| the same, shot by a rifleman from tick 1364 | 737 | | 1815 | 2048 |
| kobold shot every 300 ticks from passing 1000 | | 1058 | 1823 | 1362 |
| kobold hit again by the knight it chased (tick 1708) | 816 | | 1896 | 1651 |
| kobold hit by a knight, chasing | 779 | 1079 | 1831 | 2017 |
| second kobold of that camp | 775 | 1091 | 1831 | 1976 |

So the creeps turned back 1052 to 1080 ticks (5.3 to 5.4 s) after
passing 600 from home, attacked or not, and passing 1000 did not make
them turn back at once. The two kobolds of one fight turned back in the
same tick.

### Sleeping

Creeps whose type has `canSleep` 1 (the kobold) sleep at night
(`UnitIsSleeping` true from the start of a test at 0:00). A sleeping
creep does not acquire: a footman walking past two sleeping kobolds at
300 was not attacked. A hit wakes the creep that is hit and the other
creeps of its camp: a footman's first hit on one kobold woke it and a
kobold 400 away in the same tick, 7 ticks after the hit, and both
fought; a kobold far away slept on. When the time of day was set to
6:30, all sleeping creeps woke in the same tick.

## Edge cases

- The knight that hit a kobold was not hit back while it fled at 350
  (the kobold's speed is 270), so the time without being attacked is
  counted from the passing of `GuardDistance`.

## Open questions

1. What `MaxGuardDistance` and the attacks mentioned in the data's
   description change (in the tests neither changed the time of the
   return).
2. Why two creeps of one fight turn back in the same tick (one timer for
   the group, or a check at fixed times; the turn came 20 ticks apart
   for creeps of two separate fights).
3. Whether a creep going back home can be made to fight again on the
   way, its regeneration, and how creeps that went home choose their
   facing.
4. The radius of waking a camp, sleeping creeps that are only
   approached (spells, `BuildingPlacementNotifyRadius`), and the hour at
   which creeps fall asleep and wake up when the day runs by itself.
5. The home of creeps placed in the map (`war3mapUnits.doo`).

## Sources

- Test map RUK on 1.27b under Wine, 2026-10-20, time of day fixed at
  12:00: (A) a kobold and a knight that attacks it and then runs away
  east, (B) two kobolds 128 apart and a footman walking past them at
  about 460, (C) two kobolds 400 apart and a knight that attacks one
  and runs away north. Positions, orders and life of all units logged
  every 4 ticks, damage events logged.
- Test maps RUL and RUN (day): attacks on a creep after it passed 600
  and 1000, by another unit and by the chased knight, a creep moved by a
  script before a chase, a footman attacking a creep on its way home.
- Test map RUM (time of day fixed at 0:00, then set to 6:30): sleeping
  kobolds, a footman walking past, a footman attacking one of a camp.
- `Units\MiscGame.txt`, `Units\UnitWeapons.slk`.

## Verification

1. (A) The kobold is hit in tick 273, chases the knight, passes 600 from
   home in tick 731 and gets order 851986 in tick 1811, 2056 from home;
   it stands at home again (order 0) from tick 3371.
2. (C) Both kobolds get order 851986 in tick 1831 and are home again by
   tick 3355 and 3343.
