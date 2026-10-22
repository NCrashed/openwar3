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

### Creep groups

The creeps of the neutral hostile player are kept in groups, which
spread a fight through a camp.

- **Joining.** A creep joins a group when it is created. It joins the
  nearest group that has had fewer than 12 joiners and whose first
  living member is within 676 of it (distance between centres; it was
  not settled whether this is 676 in world units or a squared distance
  of 26 cells). If there is no such group, it starts a new one. A creep
  is in one group only and stays in it: walking away from its camp
  does not change its group. A dead member drops out; its place can be
  taken by a later joiner. In `c-RZJ-1` all eight creeps of the camp
  are in one group (the only group of the game).
- **Relay.** On every hit on a member, a fatal one included, every
  other living member of its group tries its own call for help, in the
  order of the group's places (the order of joining), each with its own
  0.5 s call timer (SPEC-0019, "Creeps: 0.5 s"). The relay does not
  depend on the victim's own call: when the victim's timer blocks its
  call, the relay still runs. A relay also runs when a creep acquires a
  target at its own acquisition check.
- **No further chain.** A call made in a relay is an ordinary call
  around the member that makes it (`CallForHelp`); it does not start
  another relay.

Read in the game, `c-RZJ-1`:
- 3722: ogre 14 dies of the Water Elemental's hit. Kobold 8, the Ogre
  Lord, gnolls 12 and 13 and ogre 15 try in that order, and ogre 15
  calls.
- 3731: the Archmage's missile hits the Ogre Lord, whose own call is
  blocked. Kobold 8, gnolls 12 and 13 and ogre 15 try, all blocked by
  their timers.
- Ogre 15's calls between 3700 and 4054: 3722 (relay), 3827, 3941 and
  4049 (its own hits). Its other tries fell within 0.5 s of a call.

### Going back home

A creep's home is the point where it was created; a `move` order from
a script does not change it (a kobold moved 600 away by a script, then
drawn into a chase, went back to the point of its creation).

A creep that has gone too far from its home gives up the chase
`GuardReturnTime` (5 s) after a **guard check** finds it outside its
zone (below): it gets a `move` order (851986) to its home, walks back
and stands there with order 0 (within 8 to 12 of the home point in the
tests). Attacks on it
did not delay this in any test: hits by a rifleman 3 s after it passed
600, hits every 1.5 s from the moment it passed 1000, and a hit by the
very knight it chased (which turned back and attacked it 2.5 s after
it passed 600) all left the time from passing 600 to turning back at
1052 to 1080 ticks. On the way home it does not answer attacks (a
kobold shot by a rifleman every 1.5 s kept its `move` order until it
was home) and fights again once it is home. A call for help from a
creep of its camp (SPEC-0019) does reach it on the way home, though:
it answers and attacks at once. Replay `c-RXD-1` (read in the game):
creep 8 reached home in tick 5917 (its order ended), its acquisition
check of 5919 took worker 2 and called for help, and creeps 6, 7 and 9,
still walking home, answered the call in that same tick (in the order
8, 6, 7, 9) and attacked worker 2. When the move order home ends,
the creep's acquisition check starts again 0.01 s later and its guard
check timer restarts (2.0 s); creep 8: home at queue time 29.5793381
(tick 5917), check at 29.5893364 (tick 5919). While the move order home lasts,
the creep's own acquisition check looks for no target at all: it only
restarts itself, after 0.5 s without a target and 1.0 s with one. An
attack taken up by answering a call does not end that state. It ends
only when the move order home ends. Creep 6 of `c-RXD-1` (read in the
game): the return timer gave the move order home at 4557, and its check
restarted with 0.01 s there (4559). Its checks at 4559 .. 5859 (every
0.5 s) and, after it answered the call at 5954, at 5959, 6159, 6359
(every 1.0 s) searched nothing. So it kept unit 2 although unit 3 was
nearer, until the move order ended at 8492.

**Losing the target.** When the target of a creep that fights from its
guard state dies, the creep gets a movement back to its guard point at
once, in that tick. The guard point is the creep's position where its
last walk home ended. Its acquisition check restarts 0.01 s later.
Read in the game, `c-RXD-1`:
- 5917: creep 8's walk home ends at (392.3828, 400.0463) (its committed
  position then, `43c43100`, `43c805ed`).
- 6665: its target, unit 3, dies (creep 9's hit). The same tick already
  had creep 8's backswing-end check, earlier in the queue, which gave
  it a face movement towards unit 3. With the death, creep 8 gets the
  movement back to that point instead.
- 6667: it turns one step towards the point (5.476150 -> 5.976150) in
  its member step. After the batch, its restarted check takes unit 1.
The case of a creep that never walked home (whether the point is then
its start position) was not read.

**Losing the target in a backswing.** A creep whose target dies by its
own hit stands until its backswing ends (`D` + the backswing time on
the queue). A call for help answered during the backswing does not
survive it. At the backswing's end the attack's check finds no target,
so the creep gets the walk back to its guard point, with its
acquisition check restarted 0.01 s later. If that check finds a target,
the attack order follows after the reaction delay. Read in the game,
`c-RXD-1` creep 9:
- 6665: its hit kills unit 3 (backswing 0.6 s).
- 6667: it answers creep 8's call (log: a 9 1); this leaves no trace in
  its movement.
- 6665 .. 6785: no commit; it stands at (388.1508, 391.1045).
- 6785: backswing end. No target, so it gets the walk to its guard point
  (387.4614, 402.6797) = (`43c1bb10`, `43c95700`), its position where
  its walk home ended at 5761.
- 6787: first member step, one turn step towards that point (5.480618
  -> 5.980618). After the batch, its restarted check takes unit 1 (log:
  a 9 1 at 6787), reaction delay 0.1623 s.
- 6820: the attack order on unit 1. Goal: unit 1's position, (389.7260,
  385.4998).
- 6823: it walks; first path point (389.5, 390.5).

A creep that already attacks the target named by a call does not take
it again: its order and path stay, with no extra commit. Read in the
game, `c-RXD-1`: creep 7, chasing unit 1, at 6787 (creep 9's check
took unit 1 then). Creep 7 has only its ordinary walking commit there,
no acquisition row, and its path point (387.5, 388.5) is kept.

**Calls restart the return timer.** A call for help reaches every
creep of the camp. A receiving creep whose return is still in progress
gets its return timer (`GuardReturnTime`, 5 s) restarted, if it stands
within `MaxGuardDistance` (1000, read in the game) of its home point
(its start position in the camp). The return is in progress from the
start of its return timer until its walk home ends. This also holds
when the creep keeps its order because it already attacks the
caller's target. The creep's zone (`GuardDistance`) plays no part.
The same restart comes from every event that reaches the creep this
way, not only from a creep that acquires a target:
- the call of a camp creep that is hit while it is busy fighting
  (SPEC-0019: a damaged unit calls whether idle or busy, at most once
  in 3 s);
- the area call of a spell and the impact call of a missile
  (SPEC-0019, "Reach", "Spell damage");
- a hit on the creep itself.
`c-RZJ-1` ogre 14 (read in the game): its return timer started at 1436
and was restarted at 1511, 1549, 1577, 1629, 1685, 1781, 1829, 1847, 1955,
2051, 2117, 2141, 2220, 2321, 2341, 2387 and 2423. The causes were hits
on itself, calls of creeps 9 and 10 hit by footmen and the Paladin,
Blizzard waves and an elemental's missile. So it fought footman 2 on
and did not walk home.
When the timer expires, the creep gives up: a stop commit, then the
walk home. Read in the game, `c-RXD-1` (homes in world units: creep 6
(100, 450), 7 (-60, 520), 8 (260, 520), 9 (100, 600)):
- creeps 6 and 7 have their return in progress from 4557 on (their
  walk home never ended before 8492); creeps 8 and 9 had ended theirs
  (5917, 5761);
- calls at 5919, 6667, 6763 and 6787 reached all four creeps, and each
  restarted the timers of creeps 6 and 7 only, both within 1000 of
  home;
- the last restart (6787, q 33.9292488) expired at tick 7788;
- creeps 6 and 7 got stop commits at 7788 and walked home from 7790.
Their guard checks (every 2 s) do not stop or restart this timer.

**What restarts the guard checks.** The guard check is a periodic
2.0 s timer. Its phase is set anew (first check 2.0 s later) when:
- the creep's walk home ends;
- the creep enters a fight from having no target: its own acquisition
  check takes a target, or it answers a call;
- the creep's target dies (also for the killer).
A switch to another target while already attacking does not restart
it. Read in the game, `c-RXD-1`, creep 8:
- restarts at 5917 (home), 5919 (its check takes unit 2), 6665 (unit 3
  dies) and 6667 (its check takes unit 1, q 33.3292503);
- no restart at its switches of 6419, 6619 and 6968;
- the checks of 6667 + 2 s * k found it out of its zone at k = 3: return
  timer started at 7868 (q 39.3292503), give-up at 8869.
Creep 9: restarts at 5919 (answering the call), 6665 (its own kill of
unit 3) and 6787 (its check takes unit 1). The creeps that answer
draw their reaction delays like any (order 8, 6, 7, 9 here), and one
that enters the fight outside its zone starts its return timer at once
(creeps 6 and 7). Measured, by the distance from home logged every 4 ticks
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
same tick. The rule behind these times is the guard check below.

### The guard check

Each creep has its own check, made at fixed times:

- **When it starts.** When the creep enters a fight (it acquires a
  target, or it is called to help, SPEC-0019), it is checked at once and
  then every 2.0 s (400 ticks) from that moment. A creep called to help
  enters the fight in the same tick as the creep that called it, so the
  creeps of one camp are checked in the same ticks.
- **The zone** is the circle of radius `GuardDistance` (600) plus the
  creep's collision size around its home: the creep is outside when
  the distance between its position and the home is larger than that
  (kobold, collision 31: inside at 626.0, outside at 637.1, measured
  over 12 kobolds of speeds 270 to 303; at 621 inside, at 651 outside).
- **At a check that finds the creep outside**, the checks stop and a
  timer of `GuardReturnTime` (5.0 s, 1000 ticks) starts; when it
  expires the creep gets the `move` order home. Attacks on the creep or
  by it in the meantime do not change the timer.
- **The timers.** Both are timers of the engine's queue (SPEC-0002):
  the check timer starts at the queue time of the fight entry and
  repeats with expiries `e(n + 1) = add(e(n), 2.0)` (the truncating add
  of SPEC-0004); the return timer starts at the expiry of the check
  that found the creep outside and expires at `add(e, 5.0)`. Each fires
  in the tick `k` with `Q(k) < e <= Q(k + 1)`, so over a long fight the
  ticks drift from the "400 and 1000 ticks" count. Replay `c-RXD-1`
  (creeps 6 to 9, read in the game): start 7.7798247 (tick 1557),
  checks 9.7798243 ... 17.7798233 (ticks 1957 ... 3557, bit for bit by
  the add), outside at 17.7798233, return at 22.7798233, which falls in
  tick 4557 by this rule (Q(4557) = 22.7771778, Q(4558) = 22.7821770):
  the move order home of all four.

So the time from the passing of 600 to the turn is 1000 ticks plus the
wait for the next check (0 to 400 ticks) plus the time the creep needs to
cover its collision size; in the tests 1040 to 1415 ticks. Creeps of
one camp turn in the same tick when they are outside at the same check;
one that is still inside at that check (a kobold slowed to 150 behind
one at 270) turns 400 ticks later, at the check that finds it outside.
Measured (tick of the passing of 600 by the creep's distance from home
logged every tick, and of the `move` order home; times of the checks
read in the running game):

| Case | Passed 600 | Check that found it outside | Turned |
|---|---|---|---|
| two kobolds 128 apart, chasing one knight | 773, 779 | 818 | 1818, 1818 |
| a kobold and a kobold slowed to 200, 128 apart | 797, 873 | 1211 | 2211, 2211 |
| a kobold alone | 870 | 1265 | 2265 |
| a kobold and a kobold slowed to 150 | 743, 931 | 811, 1211 | 1811, 2211 |

### Sleeping

Creeps whose type has `canSleep` 1 (the kobold) sleep at night: they
fall asleep at `Dusk` (18:00) and wake up at `Dawn` (6:00) of
`Units\MiscData.txt`, all in the same tick (test RVC, time of day
running at 4 times the normal speed from 17:30: asleep from the tick at
which the time of day was 18.011, awake from 6.0106). `UnitIsSleeping`
is true from the start of a test at 0:00, and setting the time of day to
6:30 woke all sleeping creeps in the same tick.

A sleeping creep does not acquire: a footman walking past two sleeping
kobolds at 300 was not attacked. A hit wakes the creep that is hit and,
in the same tick, every sleeping creep within about 832 of it (all
creeps of the tests were of `Player(12)`) (tests RVB, RVG, RVH: kobolds at 828, 829.5, 830.5 and 831.5
from the one hit woke up, at 833, 836, 840, 846 and more they slept on).
The radius is the same for creeps with another night sight or collision
size: with `nsra` (night sight 900, collision 16) as the creep hit, the
other one or both, those at 810 to 831.5 woke up, those at 840 and 920
to 945 did not (tests RVI, RVJ). They woke up in the tick of the damage event
of the footman's first hit.

## Edge cases

- The knight that hit a kobold was not hit back while it fled at 350
  (the kobold's speed is 270), so the time without being attacked is
  counted from the passing of `GuardDistance`.

## Open questions

1. What `MaxGuardDistance` and the attacks mentioned in the data's
   description change (in the tests neither changed the time of the
   return).
2. Whether a creep that changes its target, or is attacked by another
   unit, restarts its guard checks; what the 0.3 in the other branch of
   the zone test is about (a creep with a target of some kind, not met
   in the tests).
3. Whether a creep going back home can be made to fight again on the
   way, its regeneration, and how creeps that went home choose their
   facing.
4. Whether woken creeps wake others in turn (a chain), sleeping creeps
   that are only approached (spells, `BuildingPlacementNotifyRadius`),
   and creeps in a fight at dusk.
5. The home of creeps placed in the map (`war3mapUnits.doo`).

## Sources

- Test maps RVA, RVD, RVE, RVF on 1.27b under Wine, 2026-10-20 (day):
  kobolds alone and in pairs 128, 400 and 800 apart, some slowed with
  `SetUnitMoveSpeed`, attacked by knights that ran away; positions of
  the kobolds every 2 or 4 ticks, orders and damage events at every
  change. In RVE the guard checks and the start of the return timer were
  logged in the running game, with the centre and radius they use.
- Test maps RVB, RVG, RVH, RVI, RVJ (time of day fixed at 0:00): eight
  pairs of sleeping creeps, the second of each pair 200 to 1100 north of
  the first, a footman attacking the first; `UnitIsSleeping` of every
  creep at every tick.
- Test map RVC: six sleeping-capable kobolds, time of day from 17:30 at 4
  times the normal speed, `UnitIsSleeping` and `GetTimeOfDay` at every
  change.
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
3. **Guard check** (RVE): a kobold alone, attacked by a knight that runs
   away west: passes 600 from home in test tick 870, is still inside at
   the check of tick 865 (595.7 from home), outside at the check of tick
   1265, gets order 851986 in tick 2265.
4. **Waking** (RVH): kobolds 828 and 831.5 from the kobold that a footman
   hits wake up in the tick of the damage; one 833 away sleeps on.
5. **Dusk and dawn** (RVC): all six kobolds fall asleep in one tick at
   18:00 and wake up in one tick at 6:00.
