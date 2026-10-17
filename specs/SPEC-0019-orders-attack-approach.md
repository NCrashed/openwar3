---
id: SPEC-0019
title: Basic orders and the approach to an attack target
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

Movement itself (heading, `turnRate`, `propWin`, speed, arrival) is
SPEC-0018. Terms used here: an "update" is the unit's step every 6
ticks; `H` is the heading of SPEC-0018. All tests on flat open ground
with no other units in the way.

### Issuing orders from scripts

The order is taken at once: `GetUnitCurrentOrder` called right after
the `Issue...Order` native already returns the new order.

| Call | Returns | `GetUnitCurrentOrder` after the call |
|---|---|---|
| `IssuePointOrder(u, "move", x, y)` | true | 851986 |
| `IssuePointOrder(u, "patrol", x, y)` | true | 851991 |
| `IssuePointOrder(u, "attack", x, y)` | true | 851983 |
| `IssueTargetOrder(u, "attack", t)` | true | 851983 |
| `IssuePointOrder(u, "smart", x, y)` | **false** | 851971 (and the unit walks) |
| `IssuePointOrder(u, "bogus", x, y)` | false | 0, nothing happens |
| `IssueImmediateOrder(u, "stop")` | true | 0 |
| `IssueImmediateOrder(u, "holdposition")` | true | 0 |

### Stop, hold position and new orders while walking

- **`stop` and `holdposition`** end the motion at once: the position
  does not change after the call (order given at an arbitrary tick, also
  between two updates). The current order reads 0 afterwards in both
  cases.
- **A new `move` while walking** takes effect at the unit's next update.
  The unit goes on walking until then; at that update it does not move
  for one tick and then follows the start rule of SPEC-0018 from its
  current heading. Re-issuing the same `move` costs just that one tick.
  A footman walking east that got a `move` to a point behind it
  (180 degrees) stood for 25 ticks while turning, one to a point at about
  92 degrees for 7 ticks.
- **A new `move` to a point within the arrival distance** ends the order
  at that next update, without motion.
- A `move` after `holdposition` behaves like a `move` given to an idle
  unit.

### Patrol

`patrol` to a point walks there like `move`, stops at the arrival
distance of SPEC-0018, turns round on the spot by the start rule and
walks back towards the point where the unit was when the order was
given, and so on. The current order stays 851991 throughout. A footman
patrolling to a point 600 east stood 24 ticks at each end (the turn by
180 degrees).

### Attack-move without enemies

`attack` to a point with no enemy in reach behaves exactly like `move`
(the same positions tick by tick) and ends with order 0. `smart` to an
empty point behaves the same.

### Approach to an attack target

`IssueTargetOrder(u, "attack", target)` with the target out of range:

1. The unit walks towards the target by SPEC-0018.
2. It stops at the first update at which the distance between the two
   centres is at most `rangeN1 + c_a + c_t`, with `c_a` and `c_t` the
   `collision` sizes of attacker and target (`Units\UnitBalance.slk`
   or the map's object data, field `ucol`). The ordinary arrival rule of
   SPEC-0018 does not apply here.
3. The attack begins at that same update; the damage of the first hit is
   done `dmgpt1` later (`dmgpt1 / 0.005` ticks), and the next hits follow
   every `cool1` (SPEC-0016).

Distances at which the attacker stood (start 600 west of the target,
targets that cannot move):

| Attacker (`rangeN1`, `collision`) | Target collision 16 | 32 | 64 |
|---|---|---|---|
| footman (90, 31) | 130.47 (limit 137) | 146.64 (153) | 179.02 (185) |
| knight (100, 32) | 138.28 (148) | 159.24 (164) | 190.52 (196) |
| rifleman (400, 32) | 446.11 (448) | 462.31 (464) | 494.71 (496) |
| footman, collision 8 | 105.98 (114) | 122.39 (130) | 154.75 (162) |
| footman, collision 48 | 146.68 (154) | 162.84 (170) | 195.24 (202) |

Each distance is less than the limit by less than one update of motion
(8.1 for the footmen, 10.5 for the knight, 8.1 for the rifleman).

The first damage came 100 ticks (footman, `dmgpt1` 0.5), 132 ticks
(knight, 0.66) and 34 ticks (rifleman, 0.17) after the update at which
the attacker stopped; then every 270 ticks (footman, `cool1` 1.35 in
the `Custom_V1` data of a custom map) and 300 ticks (knight and
rifleman, 1.5).

### Turning before an attack

The attack begins only at an update at which the heading, before that
update's turn, differs from the direction to the target by **less than
0.2 radians** (11.46 degrees). Until then the unit turns by `turnRate`
per update without moving (when the target is already in range).

Knight (`turnRate` 0.5, 28.65 degrees per update) next to a target at
angle `a` from its heading, first damage in ticks after the order:

| `a` | 0 | 30 | 45 | 60 | 90 | 120 | 135 | 150 | 180 |
|---|---|---|---|---|---|---|---|---|---|
| ticks | 138 | 144 | 150 | 150 | 156 | 162 | 168 | 168 | 174 |

Footman (`turnRate` 0.6, 34.38 degrees per update) with `a` = 34.38 +
`d`: `d` from 9 to 11 gave 112 ticks (attack at update 2), `d` 11.5 to 16
gave 118 (update 3).

### Reaction delay

When a unit takes an attack order it first waits a random **reaction
delay** before it acts on it (walks towards the target or starts the
swing):

- `ReactionDelay` from `Units\MiscData.txt`, section `[Misc]` (0.25 s
  in 1.27b; the file describes it as the largest random reaction delay
  in seconds).
- One output `r` of game generator 9 (SPEC-0007) gives
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)`, a value in [0, 1),
  as for `GetRandomReal`, and the delay is `mul(x, ReactionDelay)`.
- The unit acts at its first update at or after the end of the delay:
  a knight next to its target hit at `138 + 6 j` ticks after the order
  (6 ticks to its first update, 132 for `dmgpt1`), where `j` is the
  number of whole updates (0.03 s) the delay spans: delays below 6
  ticks gave `j` = 0, 6.8 to 9.2 ticks `j` = 1, 12.4 to 16.1 ticks
  `j` = 2, ... 43.2 to 46.3 ticks `j` = 7.

Verified on 40 seeds (`SetRandomSeed(s)` right before the order, one
knight per order): the delay computed from the first output of
generator 9 after the seed predicts `j` in all 40 cases (two samples
were one tick off because of the test's own counting timer). With seed
7 the delay is 0.0015 s, so `j` = 0; the turning tests above used that
seed. A unit that walks to its target for longer than its delay shows no
extra wait at the swing.

### Chasing a moving target (observations)

Footman attacking a footman copy that walks (without attacking) at a set
speed, started 300 away; the target's heading pointed along its walk
from the start. The rule behind these numbers is open; what holds in
every test:

- **The attacker starts walking after its reaction delay** (previous
  section): its first motion came 4 to 46 ticks after the order, in
  steps of 6.
- **The attacker does not head for the target's current position at
  first.** In the first part of the approach its steps aim at the
  target's position at the time of the order. Later (once it is within
  about 170 of that point in the tests) it aims at the target's current
  position, updated at every update (seen in the one crossing case
  analysed this way). After a pause between swings it
  aims again at a point the target had passed some time before.
- **A swing starts farther away than the static limit** when the
  target moves. Distance between the centres at the update the
  attacker stopped to swing (the static limit is 152):

  | Target speed | Walking away | Walking across |
  |---|---|---|
  | 150 | 199.5, 209.9 | 158.9, 195.1 |
  | 180 | 214.2 | 165.8, 209.6 |
  | 210 | 238.7 | 177.1 |
  | 240 | 251.4 | 176.6 |
  | 270 | never caught | 213.9 |
  | 300 | never caught | 234.2 |

  At other updates the attacker stopped for one update without
  swinging and walked on (for example at 255.7 behind a target at 240,
  at 267 to 284 behind a target at 270), which looks like a new path
  being made.
- **A started swing always hits:** the damage was done `dmgpt1` after
  the swing started, however far the target had walked meanwhile (up to
  374 between the centres at the damage).
- After the backswing the attacker walks again; it does not wait for the
  cooldown to end before walking.
- A target that is moved by `SetUnitPosition` 20 farther while the
  attacker walks was hit from 166.6 (the attacker stopped near the old
  point and swung); moved 40 or more, the attacker went on to the new
  position without stopping and swung from within the static limit.

### Acquisition by idle units (first results)

An idle unit with a weapon attacks an enemy unit on its own:

- **Range.** It acquires an enemy whose centre is at most
  `acquire + c_a + c_t` away (`acquire` from `Units\UnitWeapons.slk`,
  collisions as above): a footman (`acquire` 500, collision 31) took an
  enemy footman copy (collision 31, `Player(1)`) at 562 and not at 563.
  `Player(15)` (neutral passive) is not an enemy of `Player(0)`
  (`IsUnitEnemy` false); its units were not tested for acquisition.
- **Then** it walks to the target and attacks it by the approach rule
  above. `GetUnitCurrentOrder` stays 0 during such an attack.
- **When.** The first acquisition of a newly created unit came at a
  random update: the first motion was `6 k + 5` ticks after creation,
  with `k` from 1 to 24 (30 footmen each with seeds 7 and 8). The values depend only on the
  random seed set before the units were created (the same seed gave the
  same 30 values in the same order); the draw is not yet identified.

## Edge cases

- The target of these tests could not move (move speed 0) and belonged
  to `Player(15)`.
- `smart` to a point returns false although the unit obeys it.

## Open questions

1. Whether other orders and the acquisition of targets use the reaction
   delay as well.
2. The chase rule: when the attacker takes a new aim point, when it
   swings at a moving target, and the stop-and-go at about 250 to 280.
3. Attack-move with enemies, the timing of acquisition scans after the
   first one, retaliation, `minRange`.
4. Queued orders, group orders, formation movement.
5. Orders on units of other players, `hold position` with enemies in
   range.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17 and 2026-10-18, same set-up
  as SPEC-0018; targets were footman copies with 100000 life, move speed
  0 and collision 16, 32 or 64; attackers footman, knight, rifleman and
  footman copies with collision 8 and 48. Damage times from
  `EVENT_UNIT_DAMAGED`, counted by a 0.005 s timer.

## Verification

1. The order table above.
2. Footman walking east, `stop` at tick 100 after the move order: no
   position change after tick 100, rest 126.89 east of the start.
3. The approach table: 15 of 15 attackers stopped within one update of
   motion below the limit, first damage `dmgpt1` after that update.
4. The turning tables: 12 knight and 10 footman cases.
