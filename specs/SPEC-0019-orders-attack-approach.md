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

**Order ids.** `OrderId(name)` gives the numeric id of an order name,
ignoring case (`"MOVE"` is `move`); an unknown name and `""` give 0:

| Name | Id |
|---|---|
| `smart` | 851971 |
| `stop` | 851972 |
| `attack` | 851983 |
| `attackground` | 851984 |
| `move` | 851986 |
| `patrol` | 851990 |
| `holdposition` | 851993 |

`OrderId2String(id)` gives the name back (`move`, `attack`, `stop`,
`smart`) and `null` for 0 and for an id without an order (12345).
(`GetUnitCurrentOrder` after a `patrol` reads 851991, one more than
`OrderId("patrol")`.)

The `...ById` natives behave like the named ones with that id:

| Call (one footman, in this order; `t` an enemy footman) | Returns | `GetUnitCurrentOrder` after the call |
|---|---|---|
| `IssuePointOrderById(u, 851986, x, y)` | true | 851986 |
| `IssuePointOrderById(u, 12345, x, y)` | false | 851986 (unchanged) |
| `IssuePointOrderById(u, 0, x, y)` | false | 851986 (unchanged) |
| `IssueImmediateOrderById(u, 851972)` | true | 0 |
| `IssueImmediateOrderById(u, 851986)` | true | 0 |
| `IssueTargetOrderById(u, 851983, t)` | true | 851983 |
| `IssueTargetOrderById(u, 851986, t)` | true | 851986 |
| `IssueImmediateOrderById(u, 851993)` | true | 0 |
| `IssuePointOrderById(u, 851983, x, y)` | true | 851983 |
| `IssuePointOrder(u, "attackground", x, y)` | true | 851984 |
| `IssuePointOrderById(u, 851984, x, y)` | true | 851984 |
| `IssueImmediateOrder(u, "move")` | true | 0 |

### Stop, hold position and new orders while walking

- **`stop` and `holdposition`** end the motion at once: the position
  does not change after the call (order given at an arbitrary tick, also
  between two updates). The current order reads 0 afterwards in both
  cases.
- **A new point order while walking** (`move`, `smart`, `patrol` or
  attack to a point), whatever order the unit was carrying out (`move`,
  `patrol`, attack-move, `smart` on a point, following a unit), stops
  the unit at once: its position does not change after the tick of the
  order. At its next update it starts on the new order as from
  standing: the start rule of SPEC-0018 from its current heading, so it
  stands while it turns and then walks. Measured (test RXL, nine
  footmen walking east, script orders in tick 603, between two
  updates; updates in ticks `6 m + 1`): all nine stopped in 603; with a
  `move` back (180 degrees) all moved again from their update of 631
  (positions from 632, 29 ticks after the order), with a `move` 4 to 25
  degrees off their way from their update of 607; orders in tick 601,
  an update tick (given after the updates of that tick), gave the same.
  The same in the replay `c-RTW-3`: a footman on `patrol` given `smart`
  on a point behind it in tick 3111 stopped in 3111 and walked from
  3140.
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

### Attack-move with enemies

A unit walking an `attack` order to a point looks for enemies on its
way:

- **Checks.** Every 100 ticks, counted from the tick after the order
  (the tick after the `IssuePointOrder` call): the first check 102 ticks
  later, then every 100 ticks. When the unit has killed a target the
  count starts again from the tick of the killing hit (first check 102
  ticks later). The checks do not depend on the unit's creation draw of
  generator 2 and are not aligned with the update cycle (orders given at
  6 different ticks, 10 attackers with seed 7: all acquisitions at
  order + 102 + 100 k).
- **Range and choice** as for idle units (an enemy footman at most 562
  away for a footman; nearest unit).
- **Reaction.** At the check that finds a target the unit stops at once
  and waits the reaction delay of attack orders (generator 9, counted
  from the check's tick), then attacks the target by the approach rule
  below. The point order itself draws no reaction delay. The current
  order stays `attack` (851983) all the time.
- **After the kill** the unit stands until the end of its backswing and
  goes on towards its point at the next update (a footman: about 105
  ticks after the killing hit, in two tests). When it arrives the order
  becomes 0.

### Retaliation and call for help

When an enemy damages a unit that has no order (idle):

- the damaged unit acquires the attacker in the tick of the damage,
  whatever the distance (a glaive thrower 1000 away, beyond the
  footman's acquisition range of 562);
- other idle units of the same player near the damaged unit acquire the
  attacker in the same tick; the order in which they respond (and so the order
  of their draws of generator 9, below) is given under "Order of the
  responders"; units of another player do so too when the
  alliances allow it (player 2 passive toward player 0 with help
  request and help response set in both directions; with help response
  alone they did not). Walking units (with a `move` order) and peasants
  did not respond (units walking an attack-move without a target do
  respond, see below); units that already had a target in range did not
  respond either, but one whose target was out of its range did and
  switched at once to the attacker (no reaction delay; replay
  `c-RXR-1` tick 827: troll 16, chasing rifleman 7 far away, answered
  grunt 14's call against footman 4 and swung at its next update),
  presumably by the rating of "Checks while attacking";
- units walking an attack-move that have no target respond like idle
  units, with the same reach: footmen walking past a footman hit by
  an enemy footman answered at 538 and 626 and not at 672 and 750 (test
  SAH), and in `c-RZJ-1` at tick 788 footmen 2, 5, 6 and 7 (246 to 519
  from the Paladin, hit by a Shockwave) acquired the Ogre Lord. They
  stand from the next tick, as if they had stopped, and their reaction
  delay counts from that next tick (footman 6 drew 0.0226 at 788 and
  walked at 799, footman 2 drew 0.0115 and searched at 793);
- all of them then wait their reaction delay (generator 9, counted from
  the tick of the damage; the damaged unit draws first) and attack the
  attacker by the approach rule.

**Call cooldown.** A damaged unit calls for help whether it is idle or
busy (attacking or chasing), but at most once in 3.0 s: when it calls,
a 3.0 s timer of that unit starts, and while it runs, damage to the
unit makes no call and starts no new timer. In `c-RXR-1` grunt 13
called at 24.4399948 and 27.4399948 (the 3.0 s timer of the first call
expired in the same moment as the second hit, before it); its seven
hits in between made no call, among them rifleman 8's at 24.1098976
(tick 4823), which fell into the cooldown of an earlier call. Over all
43 attempts between 23.0 and 27.46 s, the 26 that followed a call of
the same unit in that window matched this rule.

**Creeps: 0.5 s, and every call counts.** For the creeps of a camp the
timer is 0.5 s. Which unit property selects 0.5 s instead of 3.0 s is not
identified; all eight creeps of `c-RZJ-1` have it, the player units of
the tests above do not. The timer belongs to the calling unit and is
started by every call it makes: for a hit on itself, for an acquisition
(SPEC-0030), and when another creep's call reaches it. A creep reached
by a call calls in turn (the chain under "Order of the responders"),
also while it is fighting and keeps its target. Read in the game,
`c-RZJ-1` 6.9 .. 12.2 s: all eight creeps called in the same ticks 1415,
1549, 1685, 1829, 1955, 2117, 2220, 2321 and 2423, and every other attempt
fell inside 0.5 s after one of these. Creep 9's own hits at 1613, 1882,
1883 and 2153 made no call; its hit at 2423 came 102 ticks after the
call of 2321 and called.

**Reach.** There are two kinds of call, with `CallForHelp` of
`Units\MiscGame.txt` (600 in 1.27b; the map's copy is used, SPEC-0015)
as their base:

- the **damaged unit's call** (the one with the cooldown above): a
  circle of radius `CallForHelp` around the damaged unit; a unit is
  reached when its centre is within `CallForHelp` plus its own
  collision size. Footman pairs (victim, one responder, script
  damage, tests RZC to RZE): 615 to 631 answered, 634 to 665 did not
  (collision 31); a ring of footmen at 750 and one at 640 did not
  answer (test RZF). In `c-RXR-1` (tick 6479, rifleman 6 hitting grunt
  10) the query, read in the game, had radius 600 around grunt 10 and
  returned troll 18 at 611.1 but not troll 19 at 635.2 (collision 32);
- the **impact call** of a missile: when the missile hits, a circle of
  `CallForHelp + 150` around the impact point, with the units of all
  players in it handed to the same response; again a unit counts when
  its centre is within that radius plus its collision. In test RZG (a
  glaive thrower attacking an idle footman, the layout of the
  2026-10-18 retaliation test) the glaive's hit made this call with
  radius 750 around the impact point and no damaged-unit call, and the
  footmen at 750 and at (530, 530) answered. This explains the reach
  measured in that earlier test with real attacks: 750 + 31 = 781
  (750 in all 16 directions, 776 in all directions around a victim at
  (16, -16), 783 only on one side of a victim at (0, 0), 800 nowhere;
  with `CallForHelp` 300: 480 yes, 485 no, 300 + 150 + 31 = 481). The
  150 is inferred from these values. Several missile and spell impact
  routines make this call.

A walking unit that answers an impact call moves on through the tick
of the impact and stands from the next tick. `c-RZJ-1` tick 1019: the
elemental's missile hits creep 10 first in that tick, and gnoll 13's
commit after it still has its walking velocity. Its commit in 1020 is
at the same position with velocity 0.

Instant and melee hits make only the damaged unit's call.

**Spell damage.** Damage of an area spell makes a call around the
spell's own centre, with the units of all players handed to the same
response, like the impact call above. Its radius is `CallForHelp`
plus the spell's area, and a unit counts when its centre is within
that radius plus its collision size (footmen, collision 31, tests SAH
to SAJ):

| Spell | Centre | Radius + 31 | Answered / not |
|---|---|---|---|
| Blizzard (area 200) | the target point | 831 | 825 / 835 |
| Thunder Clap (area 250) | the caster | 881 | 878 / 885 |
| Shockwave (`AOsh`, area 125) | the wave's centre at the update that hit | 756 | 748 / 757.9 |

The creep Shockwave (`ACsh`, area 150, SPEC-0047) gives 781. A Thunder
Clap 146 ticks after the victim's own call (melee hit, its 3.0 s timer
running) still made this call, so the call does not depend on the
victim's timer. Whether spell damage also makes the damaged unit's own
call and starts its timer was not separated (open question).

**Order of the responders.** A call for help (a damaged unit, or an
idle creep that acquires a target, SPEC-0030) visits the units around
the caller through a grid of cells of 256 x 256 world units from the
map's lower left corner (-12288, -12288 on the test maps): the rows of
cells from the bottom up, in each row the cells from left to right, and
in each cell the unit that entered the cell last first. A unit is
entered (at the front of each cell's list) in every cell its box
`x - c .. x + c`, `y - c .. y + c` covers, `c` its collision size (the
tests used 31; 31 and 32 cannot be told apart), at its creation and
again whenever its box moves into a new cell; it is visited once, at
the first of its cells in the scan, which is the cell of the point
(x - c, y - c). So two units first met in the same cell come in the
order in which they last entered it (for units that never moved, the
later created first; for units that walked, the one that came in
last first). The
caller itself is skipped, and each responder that acquires the target
fires `EVENT_UNIT_ACQUIRED_TARGET` and draws its reaction delay in this
order, after the caller's own draw. A responding creep calls for help
itself in the same way, since an idle creep that acquires a target
calls (SPEC-0030); this makes a chain, so creeps out of the caller's
reach but within a responder's are reached in the same tick, after all
the caller's responders (test RXG, kobolds: each responder's call read
in the game, radius 600 around itself). Player units that answer a
call make no call of their own (`c-RXR-1` tick 6479: none of the six
responders entered the call, so troll 19 was not reached through them).

Measured with script damage on a kobold among eight kobolds 300 to 400
away (test RXG, five layouts: a circle created clockwise and
anticlockwise, a line along x and one along y created in random order,
a 3 x 3 block in random order): the order was the same for both
circles (not the order of creation) and matched the rule in four of
the five layouts. In the line along x the kobold 400 west of the caller
responded last, after the one 300 east, although by the rule it comes
first: it was not visited by the caller's own query (seen in the
game's enumeration, which returned it but did not pass it to the
response) and was reached through the chain (open question 8).

In `c-RXD-1` the `nkob` at (-60, 520) acquired peasant 5 in tick 1557;
the creeps responded in the order `nkob` (260, 520), `nkob` (100, 450),
`nkot` (100, 600): cells (row, column) (49, 47) for the caller, (49, 48)
for the two `nkob` (the later created first), (50, 48) for the `nkot`.

Test SAA (five kobolds created A to E, A, B, C, D at (-60, -60),
(+60, -60), (-60, +60), (+60, +60) around a point on a 256 boundary and
E placed near that point): answers A, E, B, C, D; read in the game's
enumeration: A first met in cell (59, 59), E and B in (60, 59) with E
(entered last) first, C in (59, 60), D in (60, 60).

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

**Which direction it turns to.** A unit standing in range turns to the
place of its current movement order, which is fixed when the order is
given; the target is not read again at the turning updates. Each update
turns it by at most `turnRate` towards that place (the angle stepped from
the current heading), until it is within 0.2 rad. Two things give a
standing attacker such a place:
- a new target (acquisition or a switch to a better target, also during
  a backswing): the place is the target's position at that moment,
  the per-tick position of SPEC-0018 (between the target's updates);
- the end of the backswing (`D` + the backswing time on the queue, `D`
  the damage; kobolds: 0.6 s = 120 ticks): the attack checks the range
  to its target (the range test above). In range, it gives the unit a
  new "face the target" movement whose place is the target's per-tick
  position at that moment; out of range, it takes up the chase ("After
  the swing" below). This is the only such check in a swing cycle; there
  is none right after a new target.
Read in the game, `c-RXD-1` (damage ticks 1913, 1955, 1961, 1967 for
creeps 6, 7, 8, 9):
- creep 7: new target at 1857, place (375.2547, 391.2776) in cells;
  turns 4.860676 -> 4.360676 -> 3.860676 -> 3.377398 at 1861, 1867,
  1873 (the last one the direction to the place), swing at 1879; damage
  1955, backswing end 2075: in range, new place (380.3779, 389.6185),
  turns 3.377398 -> 3.877398 -> 4.377398 -> 4.877398 at 2077, 2083,
  2089;
- creep 6: damage 1913, a switch to unit 3 at 2031 (place of 2031),
  backswing end 2033: in range, new place (379.1333, 389.6884), unit 3's
  per-tick position of 2033 (4/6 of the way between its updates of 2029
  and 2035); turns 4.385801 -> 4.103981 at 2035 (less than one step, so
  straight to that place's direction);
- creep 8: a switch to unit 2 at 2007 (its place within 0.2 rad: no
  turn); damage 1961, backswing end 2081: place (383.3033, 387.5001),
  unit 2's position at 2081; turns one full step 4.363140 -> 4.863140 at
  2083, then within 0.2 rad (about 0.02 off): no further turn;
- creep 9: damage 1967, backswing end 2087: out of range, the chase
  goes on (walking from 2089).

**Before a swing.** One routine decides whether the swing starts. It
runs when the cooldown expires and when a movement of the attack ends
(a face movement or a chase arriving in range: the member's arrival,
turned within 0.2 rad of its place). In order:
1. the cooldown is still running: nothing happens;
2. the target is not visible: chase;
3. the range test above fails: chase (a new goal);
4. the angle test: if the target's current position lies more than
   `AttackHalfAngle` off the unit's heading, the unit gets a face
   movement towards that position; `AttackHalfAngle` is a game constant,
   0.5 rad (read in the game);
5. otherwise the swing starts.
The 0.2 rad of the member's arrival only ends the movement. The swing
itself needs 0.5. Read in the game, `c-RXD-1`, creep 8 attacking unit
5, which walks:
- 7010: the cooldown renewal finds unit 5 in range, so a face movement.
- 7016 .. 7040: it turns to that place, reaching 3.795862.
- 7046: the face movement ends. Unit 5 now lies about 0.506 off, more
  than 0.5, so another face movement (place = unit 5 at 7046) and no
  swing.
- 7052: one turn step to 3.295862.
The exact measure of the angle test (the stored heading as here, or
the model facing) was not separated.

### Reaction delay

When a unit takes an attack order it first waits a random **reaction
delay** before it acts on it (walks towards the target or starts the
swing):

- `ReactionDelay` from `Units\MiscData.txt`, section `[Misc]` (0.25 s
  in 1.27b; the file describes it as the upper limit of a random delay
  in seconds).
- One output `r` of game generator 9 (SPEC-0007) gives
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)`, a value in [0, 1),
  as for `GetRandomReal`, and the delay is `mul(x, ReactionDelay)`.
- **When it starts.** For an order given by a script in tick `o`
  (ticks numbered as in SPEC-0018 "Phase from the start of the game";
  an order given in `main` counts as given in tick 1), the unit acts at
  its first update that is at or after `o + delay` (delay in ticks) and
  not earlier than tick `o + 2` (the update in the tick right after the
  order never acts on it): it turns towards the target, and walks if the
  target is out of range. A unit that is already in range and facing
  the target starts its swing one update later and hits `dmgpt1` after
  that.
  - Out of range (footman 600 from its target, facing it), first facing
    change and first motion: (order tick, delay in ticks -> update):
    (50, 5.41 -> 61), (53, 0.31 -> 55), (300, 0.31 -> 307: not the
    update of tick 301).
  - Attackers of verification 6 (orders in tick 201, from a 0 s timer
    started in the action of a 1 s timer event; units created in the
    same tick), first turn: delays 0.31, 0.77, 42.84, 10.51, 24.76,
    15.09, 3.43, 1.37, 17.42 -> 205, 205, 247, 217, 229, 217, 205, 205,
    223 (`o + 1 + delay` would give 223 and 211 for 15.09 and 3.43).
  - In range (footman next to its target, turning by a few degrees),
    first facing change: (50, 0.31 -> 55), (50, 5.41 -> 61), (50, 6.58
    -> 61), (50, 11.66 -> 67), (53, 0.31 -> 55), (53, 5.41 -> 61).
  - In range and facing (knight next to its target), start of the swing
    (hit 132 ticks later): (0, 0.31 -> 13), (0, 6.58 -> 19), (50, 0.31
    -> 61), (50, 5.41 -> 67), (50, 6.58 -> 67), (50, 11.66 -> 73), (53,
    5.41 -> 67), (53, 6.58 -> 67), (56, 0.31 -> 67), (56, 6.58 -> 73),
    (103, 0.31 -> 115), (103, 6.58 -> 121), (300, 0.31 -> 313), (301,
    0.31 -> 313), (500, 0.31 -> 511), (501, 0.31 -> 511).
- Acquisitions (idle units and attack-move, below) count the delay from
  the time of the check instead.

An earlier series on 40 seeds (`SetRandomSeed(s)` right before the
order, one knight per order) showed the same steps of one update per
6 ticks of delay; its order times were not counted in ticks of the game
(the test map used `TriggerSleepAction`) and do not fix the start of the
delay. With seed 7 the delay is 0.0015 s (0.31 ticks); the turning tests
above used that seed. A unit that walks to its target for longer than its delay shows no
extra wait at the swing.

During the delay the unit does nothing: a footman walking east that got
an attack order on a target to the north stopped at once (the position
did not change from the order on) and stood 10 to 53 ticks, depending
on the seed, before it turned towards the target. It does not keep its
old course.

### Range test

Whether a unit is in range of its target is decided in the movement
system's units of 32 (SPEC-0018), in the engine arithmetic of
SPEC-0004: with `R = range / 32 + c_a / 32 + c_t / 32` (at least
0.49) and `d2` the squared distance between the two centres divided by
32, the target is in range if `d2 < R * R` or `|d2 - R * R| < 0.001`.

### Chasing a moving target

The attacker does not steer at the target's current position. It walks
a path (SPEC-0020, SPEC-0021) to a **goal point**, the target's position
at the time the goal was last taken, and takes a new goal only from
time to time:

- **Taking a goal.** The goal is the target's position at that moment.
  The chase keeps two paths: a high-level path of the chase from the
  attacker's position to the goal, and the attacker's own path to the
  current waypoint of that high-level path (chosen by the look-ahead of
  SPEC-0020, step 1), followed as in SPEC-0020 and SPEC-0021. When the
  current waypoint is the goal itself, the attacker's local search goes
  to the goal point (the target's position when the goal was taken; a
  search again after a block keeps it) and ends early when it **touches
  the target**: when a cell is expanded, the steps to its eight
  neighbours test the cells of SPEC-0020 for the attacker's size class,
  and if one of the tested cells is occupied by the target (its
  footprint at its position at its own last update, SPEC-0021), the
  search stops at once and the expanded cell is the end of the path
  (its centre; the neighbours of that cell are not added). For two
  units of size class 1 this happens at the first expanded cell
  `(x, y)` whose square `x - 2 .. x + 1`, `y - 2 .. y + 1` shares a
  cell with the target's 2 x 2 footprint. A cell that is not passable
  for a static reason is not looked at for units, so the target is not
  touched through it. The attack range plays no part in the search.
  Measured in `c-RXD-1` (the 59 local searches of the creeps between
  7.8 s and 16 s, read in the running game): 58 ended by touching their
  target, each at the expanded cell this rule gives with the target's
  footprint at its position 6 ticks earlier (its last update); a stop
  at the first removed cell within `rangeN1 / 32` would have given
  another end in 21 of the 59. The other search (tick 3049, a search
  again after a block, towards a goal taken 66 ticks earlier) ended
  after 20 expansions without touching and short of its goal cell
  (open question 9). The expansions of creep 8's search
  in tick 1573 are in `c-RXD-1.search-1573.tsv`. Whether the attacker starts with the
  first point of the path or walks straight to its end is decided as
  usual (SPEC-0020, step 2: straight only if the search met no
  unpassable cell). The target is moving, so it is not an obstacle for
  the search (SPEC-0021).
- **The goal follows the target at renewals, by 2 x 2 squares.** The
  chase keeps a tracked point of its target, refreshed to the target's
  current position (SPEC-0018, after the target's own update of that
  tick if it has one) only at the cluster's first update after the
  chase order and at each renewal of the schedule below. At every
  cluster update the tracked point is compared with the goal by 2 x 2
  squares, `floor(x) >> 1` and `floor(y) >> 1` in path cells: if both are
  equal the goal stays; if either differs, the goal becomes the
  tracked point. Between renewals the tracked point does not change, so
  the goal changes only at those updates. (The change is also limited
  to once per 10 update cycles by two stamps of the tracking; with the
  17-update schedule this never bound in the cases read.) Read in the
  game: `c-RXD-1` creep 6's backswing-end check at 6293 took unit 2 at
  (386.0105, 387.49948) (`43c10158`, `43c1bfef`); at the cluster's first
  update at 6295 the tracked point was unit 2 after its own update of
  6295, (385.95114, 387.49915), square x 192 against 193, so the goal
  became that point (`43c0f9bf`, `43c1bfe4`) and creep 6 turned to it
  (3.744548 at 6307). Creep 9's check at 3659 took unit 3 at (425.5160,
  389.5788); at 3661 the tracked point (425.5730, 389.5657) was in the
  same square, so the goal stayed (and with it creep 9's turns up to
  3703). In `c-RXR-1` (2.9 s to 4.05 s, 829 comparisons of 34 chases)
  the goal followed only at renewals, 102 ticks (17 updates) apart.
- **Waypoints of the chase path** are reached by the range test: at an
  update at which the attacker is within `rangeN1 + c_a + c_t` (the
  range test above) of the current waypoint, the next waypoint is taken
  by the look-ahead, the attacker stands still for that update, and it
  gets a new path from its position along the rest of the chase path.
  (Footman behind a wall: the waypoint was taken over at 134.8 from it,
  not at 142.9 one update before.) This holds for the waypoints before
  the goal only; the last point of the chase path is walked to its end.
- **Arrival at the member's place.** At each member step (also for a
  single unit, which is a cluster of one) the unit counts as arrived
  when both hold: the distance from the position it reaches with this
  update's step to its place (the current segment goal along the
  cluster's path plus its offset, SPEC-0025) is at most its reach, and
  the direction from it to the place differs by at most 0.2 rad from
  the direction of the step it is about to make (its facing if it does
  not move; the difference wrapped to `-pi .. pi`). The reach is the
  order reach (the order's range in cells, range / 32, but at least
  0.49) only while the cluster's target count below is 0; otherwise it
  is 0.49.
  The target count: at each cluster update whose order has a unit as
  its target, a test of the target against the walking unit's player
  is made (read in part; it behaves as "the target is not visible to
  that player", which is presumably what it is). While the test passes
  the count goes up by one per update; when it fails (or the target is
  not a unit: a tree, a point) the count is 0. The count changes only
  the reach: the places follow the waypoints of the cluster's path in
  both cases, the target's point being the last one. A unit with a flag not seen set counts as
  within reach always. An arrived unit gets speed 0 at that update,
  keeps its heading, and its path is
  dropped; at its next update it searches again, high level and local,
  from its position to its target. A unit within reach but not turned
  (angle above 0.2) makes no path step at that update: it stands (speed
  0) and turns to the direction of its place, and at its next update it
  is normally turned and arrives. Replay `c-RYP-1`, peasant 5, place
  (353.5, 393.5) cells, reach 4.125: at 1003 4.1293 from the place,
  walked on; at 1009 3.9610, angle -0.3400: stood, heading 0.721511 ->
  0.381491 (the exact direction to the place; whether the turn rate can
  limit this turn was not seen); at 1015 angle 0: arrived; at 1021 its
  search to the tree's point (360, 390). Replay `c-RYP-1`, peasant 4 on its
  way to a tree, read in the game: order reach 4.125 cells (132, which
  is the gather range `Rng1` 116 of `Ahar` plus 16; where the 16 comes
  from, presumably the peasant's collision size, was not read), place
  (353.5, 375.5) cells (the segment goal of the cluster path). At 841
  it was 4.1681 from the place (angle 0.1712): walked on; at 847 3.9928
  (angle 0.1787): stood, and at 853 it searched to the tree's point
  (360, 380); its target is a tree, so the count was 0 all along. In
  replay `c-RXD-1` worker 0's harvest order sets an order reach of
  2.0625 cells (66). At its first update its cluster path (goal first,
  in units of 2 cells) was (224, 192) = the mine, (216.75, 192.75),
  (208.75, 189.75), (200.75, 189.75), (192.75, 189.75), (184.75,
  189.75), (176.75, 189.75), (168.75, 189.75), start (165.438, 189.0),
  and its place was (353.5, 379.5) cells, the waypoint (176.75,
  189.75). The count was 1, 2, 3, ... from the first update to the
  update at clock 16.7982 (count 510), and the test used 0.49 at every
  update read; at clock 16.8282 the count was 0 (the current waypoint
  was the mine's point (448, 384) cells already before), the worker
  then 27.4 cells (876) from it. On its walk back to the town hall the
  count was 0 and the
  order reach 6.0 cells (192). Worker 5 on a later walk to the mine
  (mine visible, count 0, reach 2.0625), place the cluster waypoint
  (433.5, 387.5) cells: at 3577 2.2083 from it, walked on; at 3583
  2.0302 (angle 0): arrived; at 3589 the waypoint was the mine's point
  (448, 384) and it searched. Worker 3 at 3361, before the mine was
  visible (reach 0.49): it started the update at (416.8857, 391.4958),
  its step took it to (417.0638, 391.4971), 0.4362 from its place
  (417.5, 391.5) (angle 0.0010): arrived there with speed 0; at 3367
  the mine was visible for all six workers, its next place was the
  mine's point (448, 384) (the legs 8.0 and 8.16 from (208.75, 195.75)
  first reach 10 there) and it searched and walked on. In the first walks of the six workers
  (clock 4.56 to 4.74, around tick 937) the reach used was 0.49
  throughout. The earlier reading of a reach of about 66 to the
  current path point (`c-RXD-1` worker 5 at 3583) was a misreading of
  this test (it is the arrival of worker 5 at 3583 above).
- **A new goal and the existing path.** At a renewal the target's
  position becomes the new goal only if it lies in another cell of the
  64 grid of the high-level search (cells of 64 x 64 counted from the
  map's lower left corner) than the old goal; in the same cell the old
  goal stays and nothing changes (no search). Test RYV: chased paused
  footmen moved across the chase by 20, 40, 50, 56, 60, 64, 70 and 90
  before the first renewal: all but the one moved by 20 (which stayed
  in its cell) were taken as new goals with a search; replay `c-RXR-1`
  tick 799: grunt 11's old goal (74.816, 43.008) and its target at
  (123.412, 44.088), 48.6 apart but in the same cell: no new goal. Otherwise the chase path is searched again; if its current
  waypoint is the same point as the attacker's current waypoint, the
  attacker keeps its own path, else it gets a new one at once.
- **A held search.** The new path needs a search of the attacker's own
  path, and that search may be held by the search throttle (SPEC-0021,
  60 ticks after the path's last search). What the attacker does until
  the held search runs depends on how the goal changed:
  - At a renewal of the same group (the countdown below ran out), the
    attacker keeps walking its old local path, with its usual turns and
    waypoint advances. `c-RZJ-1` creep 15: renewal at 769, last search
    727. It reaches a waypoint at 763, turns in place at 763 and 769, and
    walks from 775. The held search runs at 787 and its path replaces
    the old one.
  - When the group is replaced by a new one (a new chase, or the forced
    new group after a swing), the old local path is dropped with the old
    group. The attacker stands with no path and no look-ahead, and only
    turns toward the new goal. `c-RXD-1` creep 9: last search 8006, new
    group at 8018 (its old group's countdown still at 5), the new group's
    first update at 8030. It stands and turns until the held search at
    8066; its heading ends at 3.2749, the direction to the new goal
    (360.218, 387.5).
- **Start in a block that is not free.** When the attacker stands next
  to a wall so that its level-0 block is not free, the high-level search
  started from the free neighbouring block nearest to the attacker (one
  case, open question 6).
- **Schedule.** Right after a goal is taken a countdown is set to
  `round(0.33 * d)` clamped to 16 .. 132 (with 0.33 the real
  `s2r("0.33")`; `d` the distance from the attacker to the new goal in
  units of 32), and it goes down by one at each of the attacker's
  updates. The next goal is taken at the update at which it has run
  out, so for distances below 48 units of 32 (1536) a new goal comes
  every 17 updates, 102 ticks. (Static analysis: 165 is added to the
  countdown in one mode of the mover that was not seen in the tests.)
  A goal taken outside the attacker's updates (at the end of a reaction
  delay, below) starts the countdown at the attacker's first update
  after it, the update of its search, which does not count: in
  `c-RXD-1` the creeps searched at 1567, 1573, 1585 and 1609 and took
  their next goals 102 ticks later (1669, 1675, 1687, 1711) and again
  102 ticks after that.
- **A new goal ends a wait.** A wait of SPEC-0021 belongs to the path
  that a new goal replaces, so the attacker walks on at the update of
  its new goal: in `c-RXD-1` creep 9 began a 5-update wait for kobold 6
  in tick 1759 and moved again at its new goal in tick 1771, the third
  update of the wait.
- **Start of the attack.** At each update, before moving, the attacker
  stops and starts its swing if the target or the goal point is within
  the range test above (`rangeN1 + c_a + c_t`). In 15 swings against a
  goal point the distances at the swing were 129.4 to 134.5 and at the
  update before 137.5 to 142.6 (footman against a peasant, limit 137);
  one swing came an update later than this rule gives (at 136.68 the
  attacker walked on; open question 2). So a swing at a target that walks away starts from
  farther than the static limit: from 178 to 218 behind a target
  walking away at 150 (start distances 200 to 650, 9 tests), 214 at
  180, 239 at 210, 251 at 240 (static limit 152).
- **A started swing always hits:** the damage comes `dmgpt1` after the
  swing started, however far the target walked meanwhile (up to 374
  between the centres at the damage).
- **After the swing** the chase goes on at once (it does not wait for
  the cooldown). One tick after the end of the backswing (`backSw1`
  after the damage) the attacker takes a goal as above for both
  searches. The goal is the target's position at the start of the tick
  in which the backswing ends (in `c-RXR-1` rifleman 9's backswing
  ended in 5053 and its goal was grunt 15's position at the start of
  5053, not at its end; all agent commits match with this). It moves
  from its next update on, starting from its current facing as its heading
  (SPEC-0018, as for a new order). At its 12th update counted from that
  first one (66 ticks later) it takes a new goal and stands still for
  that update; then the 17-update schedule goes on. This was seen after
  every swing in every test. Behind targets it could not reach (speed
  270) it stood for one update at intervals of 96 to 102 ticks.
  More precisely, the new goal with the stand comes at the first update
  of the attacker at or after the tick in which its cooldown allows the
  next swing, `D + cool1 - dmgpt1` in ticks (`D` the tick of the
  damage; SPEC-0016, "Timing"), which is also the earliest update at
  which it can start a swing again. In the chase tests above (footman:
  `cool1` 1.35 s, `dmgpt1` 0.5 s, so `D + 170`) the stands were at 598,
  958, 592, 904, 634 and 946 (damage 427, 788, 421, 733, 463, 776,
  updates in ticks `6 m + 4`), and in `c-RXD-1` (kobolds: 1.35 s and
  0.38 s, `D + 194`; damage 1913, 1955, 1961 and 1967 for creeps 6, 7,
  8 and 9, updates in ticks `6 m + 1`) at 2107, 2149, 2155 and 2161,
  read in the game: all ten by this rule ("66 ticks after the first
  motion" fits the footmen only through their numbers). In terms of
  the timer queue: the swing starts in an update batch at queue time
  `U_s` (SPEC-0018, "Order within an update tick"), and the cooldown is
  an engine timer started there with expiry `add(U_s, cool1)`. It fires
  in the tick whose interval holds it, ordered with that tick's update
  batch by time (SPEC-0002). With `cool1` a multiple of 0.03 the batch
  that comes `cool1 / 0.03` updates later is a little earlier than the
  expiry (45 steps of `P` are 1.3499880 against 1.35 for the kobolds),
  so the timer fires just after that batch, in the same tick: creep 6
  of `c-RXD-1`, swing in the batch of tick 1837, cooldown at q =
  10.529950142 after the batch of tick 2107 at q = 10.529938698. That
  tick is `R`. At the timer the attacker takes the target's position
  (its agent position after the target's own update of that tick, if
  any) as its new goal; the attacker's update of `R` has already run:
  an attacker that was walking had its look-ahead check there as at any
  walking update (with its waits and blocks, SPEC-0021); one that was
  standing did nothing. The search for the new goal comes at its next
  update, `R + 6`, and it walks (or turns towards its path) from that
  update. When the expiry falls before the batch of its tick (possible
  for an interval that is not a multiple of 0.03, such as a hero's
  attack interval of SPEC-0034), the goal comes before the update of
  that tick and the search at that update; this case was not
  measured. Read in the game: c-CH1 row 1, look-ahead at
  598 with the old velocity, search at 604 (test ticks); creep 9 of
  `c-RXD-1`, look-ahead at 2161 (it made creep 7 wait 20 updates and was
  blocked), search at 2167; creep 7, standing, nothing at 2149, search
  at 2155 towards peasant 3's position at 2149. The count runs
  whether the attacker walks or not: creep 7 ended its backswing in
  tick 2075 with its target still within the range test of the new
  goal point, stood (its cooldown had not run out), and walked only
  after its next goal in tick 2149, from its update of 2155, although
  the target had left its range at about tick 2125. Such an attacker
  has its goal and path like a walking one and turns while it stands:
  from its first update after the end of the backswing its heading
  turns towards its path by the turn rule of SPEC-0018 (0.5 rad per
  update for the kobold: 3.3774, 3.8774, 4.3774, 4.8774, 5.3774 at its
  updates 2077 to 2095, then 5.3774) and its facing follows by the
  facing rule of SPEC-0013 (steps 0.07, 0.14, then 0.2 per update),
  with velocity 0 at every update, because the start-of-attack test
  above holds (the goal point is within range) and the swing waits for
  the cooldown. It is facing its heading by tick 2143; after the new
  goal of 2149 its heading was 5.2104 and it walked from 2155 (read in
  the game). A switch of target during the backswing (creep 8 in tick
  2007, creep 6 in tick 2031) is taken at once (next paragraph); it did
  not change these ticks.
- A target moved with `SetUnitPosition` by 40 or more while the attacker
  walked made the attacker go straight to the new place (it took a new
  goal at once); moved by 20 it did not, the attacker stopped near the
  old place and swung from 166.6.
- Distances at which the first swing started, for comparison:

  | Target speed | Walking away | Walking across |
  |---|---|---|
  | 150 | 199.5, 209.9 | 158.9, 195.1 |
  | 180 | 214.2 | 165.8, 209.6 |
  | 210 | 238.7 | 177.1 |
  | 240 | 251.4 | 176.6 |
  | 270 | never caught | 213.9 |
  | 300 | never caught | 234.2 |

### Following an allied unit

`smart` on a unit of the same player (a right click on it) and `move`
with that unit as the target behave the same: the unit follows the
other one (tests RUO, RUP, RUQ, RUS; footmen following a footman or a
peasant). Let `gap = d - c_f - c_l`, the distance between the centres
less the collision sizes of the follower and the leader (31 for the
footman, 16 for the peasant).

- **Follow range** 300. The follower walks towards the leader and
  stops at the first of its updates at which `gap <= 300` (the range
  test above with range 300). With the leader standing still and start
  gaps 303 to 1238, the follower stopped at gaps from 292.3 to 299.9;
  with a start gap of 303 it stopped after one update.
- **Start within the range.** When the start gap is 300 or less, the
  follower still walks towards the leader: it stops at the first update
  at which the gap is no larger than the distance it has walked since
  the order, so it covers about half of the start gap (start gaps 238
  to 298: stops at gaps 119 to 144 after 15 to 19 updates; a start gap
  of 303 already gives the rule above).
- **A moving leader.** The follower takes the leader's position as its
  goal point and takes a new one from time to time as in a chase (about
  every 102 ticks, see "Chasing a moving target"); when the leader
  stopped, the follower stopped at a gap of 295.5, by the follow range.
- **The follower's search** ends as a chase search does: at the first
  expanded cell from which a step test touches the leader's footprint
  (above, "Chasing a moving target"). In `c-RTW-3` follower 828 searched
  in tick 5413 from cell (382.815, 374.304) towards 918 at (391.964,
  366.712): 14 removals, no unpassable cell, end at cell (389, 367),
  whose centre (389.5, 367.5) is the end of its path; it walked straight
  to that point.
- **A standing follower** does not start again as soon as the gap grows
  past 300. In `c-RTW-3` footman 825 followed footman 915 and stood at
  a gap of 295.6 (from tick 4279); 915 then walked past it with a group (gap
  down to 20.5) and away; the gap passed 300 at about tick 5397 and 825
  started walking in tick 5474, at a gap of 374 (positions in
  `c-RTW-3.positions.tsv`). It then stopped at gaps of 341.5, 290.7 and
  293.5 within the next 200 ticks, near the other footmen.
  The rule behind this, read in the game for follower 828 of `c-RTW-3`
  (`game-data/1.27b/replays/c-RTW-3.follower828.tsv`): the follower's
  cluster renews its goal every 17 updates (countdown 16 .. 1, then the
  renewal, as in a chase; a stop does not restart the count, only the
  start of a new order with its new cluster does); at a renewal the goal
  becomes the leader's position when it lies in another 64 square, and
  only then is the gap tested: more than 300 to the goal, and the
  follower searches and walks; otherwise it goes on standing. 828 stood
  from tick 4279; at the renewals of 4903, 5005, 5107, 5209 and 5311 the
  gaps to the new goals were 198, 97, 27, 63 and 185, at 5413 318.4 (it
  searched and walked), and after its stop at the end of that path at
  5491, at 5515 323.0 (searched again).

Measured stops (footman following a footman, start gaps 300 and less;
the same numbers with a peasant as the leader when the start gap is the
same):

| Start gap | Updates walked | Gap at the stop |
|---|---|---|
| 238.0 | 15 | 116.5 |
| 245.0 | 16 | 115.4 |
| 259.0 | 16 | 129.4 |
| 262.5 | 17 | 124.8 |
| 273.0 | 17 | 135.3 |
| 276.5 | 18 | 130.7 |
| 283.0 | 18 | 137.2 |
| 298.0 | 19 | 144.1 |
| 303.0 | 1 | 294.9 |
| 338.0 | 5 | 297.5 |

### Acquisition by idle units

An idle unit with a weapon attacks an enemy unit on its own:

- **Range.** It acquires an enemy whose centre is at most
  `acquire + c_a + c_t` away (`acquire` from `Units\UnitWeapons.slk`,
  collisions as above): a footman (`acquire` 500, collision 31) took an
  enemy footman copy (collision 31, `Player(1)`) at 562 and not at 563.
  Creeps use the same range (probe `c-SAN`: kobolds of `Player(12)`
  took footmen on hold position up to 561 away, not at 565). The
  distance is taken to the target's path position as of its last
  update (its last commit, SPEC-0018), not to the position a script
  reads every tick: in `c-SAK-1` kobold 10's check in tick 678 (queue
  time 3.386470) found nothing, since footman 7's commit of tick 673 was
  563.86 away; its next commit (tick 679, 555.86) came after the check.
  `Player(15)` (neutral passive) is not an enemy of `Player(0)`
  (`IsUnitEnemy` false).
- **Checks.** Each unit with a weapon checks for targets on its own
  schedule: when it is created it draws one output `r` of game generator
  2 (SPEC-0007, the generator of the damage dice), with
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)` as for the reaction
  delay. Its first check is `100 * x` ticks (`0.5 * x` seconds) after
  its creation, and then every 100 ticks (0.5 s). Units without a weapon
  (a farm) draw nothing. The draws are made in the order of creation.
  A summoned unit does the same when it is created at the spell effect
  (`c-RZJ-1`, Water Elemental 16: tick 460, queue time 2.299999952,
  delay 0.119724 s, checks at 2.4197235 + 0.5 k; the one at 4.4197235
  ran in tick 884).
  The checks are timers of the game's timer queue (SPEC-0018, "Order
  within an update tick"; SPEC-0002). The first expires at the queue's
  time at the creation plus `0.5 * x` s, each next one 0.5 s later, and
  a check runs in the tick `k` whose queue interval holds its expiry
  `e`: `clock(k) < e <= clock(k + 1)` (in tick `k` the queue moves from
  `clock(k)` to `clock(k + 1)` and fires every timer it passes, at its
  expiry). The queue's time at the creation is the expiry of the timer
  whose callback creates the unit, not the movement clock: units
  created by the actions of the test maps (a 0 s timer started in the
  callback of a 1.0 s timer) have 1.0001, while the movement clock of
  their tick 201 reads 0.999999. In `c-RXD-1` the `nkob` that found the
  first target (`x` = 0.55945) had its 14th check expire at 7.779825
  (`0x40F8F453`, `1.0001 + 0.5 * x + 13 * 0.5`, read in the game), which
  lies between `clock(1557)` = 7.7796745 (`0x40F8F318`) and
  `clock(1558)` = 7.7846742, so it checked in tick 1557 (with the
  movement clock compared with the expiry it would have been 1558).
  `EVENT_UNIT_ACQUIRED_TARGET` fires at the check that finds a target
  (60 of 60 footmen with seeds 7, 8 and 9 earlier; their ticks were
  counted by a script timer and agree with this rule); an enemy put
  into range at tick 300 or 333 was found at the next check of each
  unit.
- **Choice of target.** Of the enemies in range it takes the nearest
  unit, and a unit before a structure even when the structure is
  nearer (a farm 150 away lost against a footman 300 away, in both
  orders of creation; a structure alone in range was not tested). The field `prio` of
  `Units\UnitData.slk` does not matter (a footman copy with `prio` 10 at
  305 lost against a footman, `prio` 6, at 300; one with `prio` 0 at
  295 won). Between enemies at exactly the same distance it takes the
  one with the smaller y, and with equal y the one with the smaller x
  (tested with targets 300 away in the eight directions, independent of
  the order of creation).
  Before the distance comes what the enemy is doing: an enemy that is
  attacking the unit itself is preferred to one attacking another unit
  of the unit's side (its player or an ally), and that one to an enemy
  that attacks nobody; only between enemies of the same rank does the
  nearer one win. "Attacking" means the enemy has a unit as the target
  of its current attack (an attack order or an acquired target); a
  unit on attack-move that has not acquired anything attacks nobody.
  Measured in `c-RXR-1` (the rating of every candidate read in the
  game, nine acquisitions in ticks 503..592, all as this rule): in
  556 grunt 11 had footmen 2 (513.4 away, attack-moving), 0 (547.5)
  and 3 (549.0, attacking grunt 13) in range and took 3; in 544
  grunt 14 preferred footmen 4 and 5 (both attacking 14) to 3
  (attacking 13) and 2; in 592 grunt 13 took footman 3, which was
  attacking it, over 0, 2, 4 and 5.
- **Remembered enemy.** Each unit remembers the attacker of the last call
  for help that reached it (SPEC-0019 "Retaliation and call for help"),
  also when it did not answer it because it was busy. When a check
  finds no enemy within its radius, the unit takes that remembered
  enemy if it is still a valid target, within 2000 of the unit, and
  passes one more test that was not identified (probably that it can
  be seen); the attack then goes as any acquisition (event, generator 9
  draw, reaction delay). The memory is cleared when that unit becomes
  an invalid target (for example dies). In `c-RXR-1` troll 16's check
  of 32.0905838 (tick 6420), radius 500, found no enemy; it took
  rifleman 9, 718 away, remembered from a call for help of grunt 15 at
  30.5558720.
  Details read in the game (test maps c-SAU .. c-SAX, footmen, collision
  31, and `c-SAK-1`):
  - The remembered enemy is stored only if it is within 2000 plus both
    collision sizes of the unit at that moment (center distance 2060
    stored, 2090 not), and the same reach holds at the check (taken at
    1960 .. 2060, not at 2090 or 2120; an enemy not stored at 2100 was
    not taken later at 1900).
  - Besides calls of other units, a hit on the unit itself stores its
    attacker the same way, except when the attacker is the unit's own
    current target. `c-SAK-1` footman 4, attacking kobold 10 from 8046
    on and hit by it six times, kept kobold 19 (stored from footman 2's
    calls at q 41.958847 and 46.008842). After kobold 10's death his
    check of q 50.008579 found no enemy within 500 and took kobold 19,
    1236 away (event in 10005, reaction delay 0.012422).
  - A unit with a direct `attack` order on a target stored nothing from
    such calls (c-SAV: it answered and left before the reach test). The
    same units on an attack-move did.
  - The fallback runs at the check of a unit without a target (the
    0.01 s check after its target died, also on an attack-move).
  - The enemy must be visible to the unit's player: with fog of war on
    (c-SAY, the enemies 1500 .. 2120 away, out of sight, `IsUnitVisible`
    false) none was taken; the same map with fog off (c-SAX) took every
    one within the reach. Whether this is tested at the store, at the
    check or at both was not separated.
- **Reaction delay.** After acquiring, the unit waits the reaction delay
  of attack orders (generator 9, `ReactionDelay`, see above), counted
  from the expiry of the check (the queue's time during the check, also
  for the units that answer its call for help), with the draws of
  generator 9 in the order of the acquisitions. The end of the delay is
  again a timer: in the tick whose queue interval holds it (as for the
  checks) the unit takes the target as its goal and gets a new cluster
  (SPEC-0021, "Searches of many units in the same tick"). At its first
  update after that it searches its path and starts to act (turn,
  walk): 60 of 60 footmen. The goal taken at the end of the delay is
  the target's position at the start of that tick: its last committed
  position (SPEC-0018, the commit at its own last update) moved on by
  its velocity for the time since, `x + vx * (Q(k) - Q(c))` with `Q(c)`
  the start of the tick of that commit; for a standing target this is
  its position. At the unit's first update the cluster's countdown is
  at its renewal, and the rule of "A new goal and the existing path"
  applies with the target's position at the start of that tick (the
  goal is replaced only if it lies in another 64 square). In `c-RXR-1`
  grunt 15's delay ended in tick 622 (q 3.1087813): its goal was
  footman 4's commit of tick 619 moved on to Q(622), cells (383.55365,
  387.46287); at its first update (625) footman 4 was still in the
  same 64 square and the search went to that goal. Grunt 11's delay
  ended in tick 591; at its first update (595) footman 3 had moved
  into the next 64 square (x 381.868 at the start of 591, 382.037 at
  the start of 595, in cells), so it searched towards (-62.83, 40.04),
  footman 3's position at the start of 595. In `c-RXD-1` the delays of
  creeps 7, 8, 6 and 9 were 0.1221, 0.0626, 0.2499 and 0.0406 s from
  7.779825, so they ended at 7.901925, 7.842425, 8.029725 and
  7.820425, in ticks 1581, 1569, 1607 and 1565; the creeps searched at
  their updates of ticks 1585, 1573, 1609 and 1567 towards x =
  -376.586, -387.984, -351.890 and -391.784, read in the game.
- **Then** it walks to the target and attacks it by the approach rule
  above. `GetUnitCurrentOrder` stays 0 during such an attack.
- **Target rating.** One rating orders the candidates everywhere a unit
  chooses an enemy:
  - its acquisition checks when idle or on an attack-move;
  - its checks while attacking (below);
  - the comparison when it answers a call for help (the caller's
    attacker against its current target; "Retaliation and call for
    help").
  The full rating, with all its parts in order and who uses them, is
  SPEC-0048, read from the game; where this summary and SPEC-0048
  differ, SPEC-0048 holds. Every unit uses a common set of parts; creeps
  (units of the neutral hostile player) add more, and creeps of level 7
  or more add more still, such as life bands (100 / 200 / 300) that rank
  above "in range" of this summary. Summary of the common parts, from
  the most to the least important (SPEC-0048 orders the threat parts
  below in range, slower and current target):
  1. **In range**: the enemy is within the range test of the attack;
     the current target in range ranks higher still.
  2. **Threat**: the enemy is attacking the unit itself (higher) or one
     of its allies (lower). It counts only while that enemy's attack is
     in progress (attacking or moving to attack) and not disabled: a
     stunned enemy carries no threat.
  3. **Slower** (creep raters only): an enemy whose movement speed is
     below the creep's own ranks higher than one that is not (equal
     speed counts as not slower), in and out of range (`c-RZJ-1` 1019).
     Player units do not use it: the Archmage at 1851 took the Ogre
     Lord (320, 454 away) over ogre 14 (270, 530 away), both in range.
     Read in the game, `c-RZJ-1` tick 1019: the creeps (speed
     270) rated the Water Elemental (220) above the Paladin (270), both
     out of range and both attacking a creep. So gnoll 13 left the
     Paladin, 167 away, for the elemental, 443 away.
  4. **Distance**: between equal ratings the strictly nearer one wins.
     A tie keeps the current target, or the one found first.
  Read in the game, `c-RZH-1`:
  - footmen 1 and 3 at 1103: creeps 11, 7, 10, 13 and 9, each attacking
    an ally, rate equal; creep 8, stunned by the Storm Bolt, rates
    lower; both take creep 7, the nearest of the best;
  - at 989 a call made footmen 1 and 3 switch from creep 8 (out of
    range, no threat) to creep 9 (attacking footman 5). Footmen 2 and 4,
    whose targets were in range, kept them;
  - creep 13 at 1192 took the nearest of three equal footmen;
  - `c-RXR-1` 779..787: every responder with its current target in
    range kept it.
- **Checks while attacking.** The checks go on while the unit attacks
  a target it acquired (or was called to), but less often: at each
  check the next one is set 1.0 s after this one's expiry if the unit
  has a target at that moment, and 0.5 s after it if not (so the first
  check after an acquisition is still 0.5 s later). At a check with a
  target the unit compares its target with the enemies whose centres
  are within half its `acquire` plus both collision sizes (250 + 31 +
  16 = 297 for a kobold and a peasant; in `c-RXD-1` enemies at 295.7
  were looked at, none at 300.8 or more) and takes the best of them:
  - an enemy within the range test of the attack (`rangeN1 + c_a +
    c_t`, above) is better than one out of it;
  - the current target within that range is better than any other
    enemy within it, so the unit keeps a target it can hit;
  - between enemies equal by these rules the strictly nearer one is
    better, measured from the unit's centre (a tie keeps the one found
    first, the current target before the others).
  If the best is another enemy, the unit attacks it at once:
  `EVENT_UNIT_ACQUIRED_TARGET` fires, there is no reaction delay and no
  call for help, and the unit takes the new target as its goal in that
  tick and gets a new cluster (SPEC-0021); it searches at its next
  update. Measured in `c-RXD-1` over all 61 checks of the four creeps
  up to 22 s (enemies looked at and the order of their rating read in
  the game): the switches in ticks 1836, 1857, 2007, 2031, 2207, 4236,
  4257, 4407, 4431, 4436 and 4457 and no switch at the other checks,
  for example creep 6 in tick 1831 kept peasant 5 at 139 (in range)
  against peasant 3 at 172, and creep 9 in tick 1836 left peasant 5 at
  239 for peasant 3 at 214 (both out of range). Only peasants were
  rated in these tests; the rating has further parts (kinds of units,
  units attacking, life) that were not measured (open question 10).
- **When the target dies.** At the moment a unit dies (in the hit of
  an instant or melee weapon, or for a missile in the missile part at
  the end of the tick, SPEC-0016), the attack of every unit attacking
  it ends. Each such unit then goes back to the orders it had before
  the attack (the order it was given, and for units that acquired the
  target themselves the state of looking for targets), either at once
  or, if it is in the backswing of a swing (`backSw1` after the hit),
  when that backswing ends. Going back to looking for targets restarts
  the acquisition check timer with 0.01 s. No generator draw is made at
  the death. The check that follows is an ordinary one: it restarts the
  check timer with its period, rates the enemies (above) and the new
  attack has its generator 9 draw and its reaction delay. A unit on
  attack-move also takes up its move to the point again at that moment
  (a new search towards the point at its next update, which may come
  before the check). Taking up the move stops the unit where it is at
  that moment (the death's time, or the backswing's end): a walker
  that was still chasing makes no further motion in the ticks up to
  its next update, and the units map used by other units' searches in
  between holds it at that place. `c-RZJ-1`: the Ogre Lord died at
  queue time 37.137806, before the updates of tick 7430; footman 4,
  the Paladin, footman 7 and another footman took up their attack-move
  to (300, 0) at that time, each searched at its update of 7430, and
  the search of footman 4 (56 iterations) went round footman 7 standing
  at its stop position.
  An idle unit that acquired the target takes up the walk back to its
  guard point (the stored entry: a point and the value 50, see below)
  in the same way: grunt 14 in
  `c-RXR-1` lost its target in its backswing, the backswing ended at
  24.0499172 (tick 4811), at its update of tick 4813 it searched
  towards its start point (400, 150), and the 0.01 s check later in
  that tick gave it footman 2 (reaction delay 0.156090).
  **Already there.** Taking up the walk back first tests whether the
  unit is at its point: it is if the squared distance from the unit's
  path position to the point, in path cells, is less than
  `(value / 32 + c)^2`, `c` its collision size in cells (footman
  31 / 32, a hero, grunt or Water Elemental 1.0, Ogre Lord 1.5), or
  equal to it within a tiny tolerance. So the reach is value +
  collision size (82 for a unit of collision 32). A unit that is there
  does not move and is plainly idle again. Grunt 14 above was 170
  from its point and walked. Water Elemental 16 of `c-RZJ-1` lost its
  target in its backswing; at the backswing end 13.109992 its point
  (-395.0, 55.25), the summon point, was 79.9 away: squared distance
  6.2413 < 2.5625^2 = 6.566406, so it stood.
  **Writing the entry.** An acquisition by the unit's own periodic
  check, when the unit is idle with no walk back pending, writes the
  entry anew with the unit's position at that moment and the value 50.
  An acquisition while a walk back is the unit's order does not: grunt
  14's check above left its point at (400, 150). Water Elemental 16 of
  `c-RZJ-1` had its entry written at 3.919724 (its first acquisition,
  at the summon point), 13.119991 (the check right after the walk back
  above ended at once) and 19.119989, both at (-315.168, 51.0146), the
  place where it had stood since the end of its first approach (tick
  883). At the Ogre Lord's death (37.137806) it took up the walk back
  to that point, was there, and stood.
  Measured in `c-RXR-1`:
  - footman 4 died in the missile part of tick 2360 (now 11.7990208);
    grunts 14, 15 and trolls 19, 18 had their checks at 11.8090200
    (tick 2363), in that order, took footmen 5, 5, 5 and 3, with
    reaction delays 0.0129, 0.1334, 0.1020 and 0.2035 s from that time;
    the killer, troll 16, was in its backswing and had no such check;
  - grunt 12 died in rifleman 6's hit at 12.8899956 (tick 2579), with
    footmen 1, 2 and riflemen 6, 7, 8, 9 attacking it. Only footman 2
    (last hit in 2459, backswing over) went back at once: in tick 2581
    its update searched towards its attack-move point (600, 0), then
    its check took grunt 13. Footman 1 (hit in 2519) went back at the
    end of its backswing, 13.0899944 (tick 2619), and its check at
    13.0999937 took grunt 11; riflemen 7, 9 and 8 acquired 2 ticks
    after their backswings ended (2631, 2655, 2703).
  During such a backswing the unit's periodic check and calls for help
  still choose targets (`EVENT_UNIT_ACQUIRED_TARGET` fires) without a
  reaction delay, but its attack starts only from the check after the
  backswing: rifleman 6, the killer of grunt 12, took grunt 10 at its
  periodic check at 13.5099993 (tick 2703) and grunt 13 by a call for
  help at 13.5299950 (2707), and after its backswing ended (13.5899954)
  its 0.01 s check took 10 with a reaction delay of 0.146777.
  When the current target is still alive and such a check in the
  backswing finds a better target (the rating above), the switch is
  made at once: the unit takes the new target and from its next
  standing updates turns towards it by its turn rate, still in its
  backswing; the swing at the new target waits for the cooldown as
  usual. `c-RXD-1` creep 7: damage point on worker 5 and, in the same
  tick 4457, a check that took a new target; at its updates of 4459 and
  4465 (speed 0) its heading went 6.204931 -> 5.704931 -> 5.210267 (the
  direction to the new target), then stayed until 4561. (Implemented
  this way, the replay's path events match to 4747.)
  The end of that backswing then runs the attack once more against the
  new target, as any backswing end: out of reach it starts the chase
  anew, even though the unit already chases that target (a commit with
  velocity 0, no aim, heading kept; at its next update a new search).
  `c-SAK-1` (read in the game), both on kobold 10 while kobold 19 (hit,
  alive) walked away:
  - footman 6: hit 9325, check takes kobold 10 at 9347 (commit), search
    and path at 9351, walks; backswing end at 9425 (commit, no aim),
    new search at 9429;
  - footman 2: hit 9319, check takes kobold 10 at 9371 (commit), search
    at 9375, turns where he stands (wait4 for footman 6 at 9387, 9417);
    backswing end at 9419 (commit, no aim), new search at 9423 (same
    path).
- **Cooldown expiry.** When the cooldown of the last swing expires, the
  swing starts in that timer's callback if the unit's last update found
  it ready (target in range and heading within 0.2 rad of it, the test
  of "Turning before an attack"); grunt 10 in `c-RXR-1` switched to
  footman 1 at 24.6099854, was ready at its update of 24.7195358 and
  swung at the expiry 24.7299862. Otherwise (out of range, or still
  turning) the approach is restarted: the unit stops at once (no motion until its
  next update) and at its next update it searches again towards the
  target and walks on at once. Footman 1 in `c-RXR-1` chased grunt 11
  from its search in tick 2647; its update of 2689 moved it, its old
  cooldown expired at 13.4399939 in tick 2689 after the update batch,
  it did not move in ticks 2690 to 2695, searched again (same goal) at
  its update of 2695 and moved from 2696; it swung at 13.5899115 (hit
  in tick 2819). One case read.
  Creeps 6 and 8 of `c-RXD-1`, switched to new targets during their
  backswings (ticks 2031 and 2007), were still turning at their
  expiries (10.5299501, 10.7699480): no swing there; creep 6 swung at
  its update of 10.6199379, the first that found it ready.
  In range but not facing at the expiry, the unit gets a face movement
  (the unlimited-reach approach of "Target in reach at acquisition")
  instead of a new search: a commit with velocity 0, no aim, heading
  kept. It turns towards the target's per-tick position at the expiry
  (fixed then, not re-aimed while the target walks on) and swings when
  that movement ends (arrived when facing, its end runs the attack
  start). `c-SAK-1` (read in the game), the Blademaster on ogre 18
  (walking away): expiry at 48.293095 (tick 9662), the turn point the
  ogre's per-tick position of 9662 (direction 5.319594 from him), heading
  4.363876 -> 4.963876 (9663) -> 5.319591 (9669), the movement's end
  and swing at 48.358574 (9675), hit 9718.
- **A new target and the old cooldown.** The cooldown of the last
  swing goes on when the target changes. In `c-RXR-1` grunt 15 (target
  in range, it did not move) ended its reaction delay at 11.9424612,
  but its swing at footman 5 started in the callback of its old
  cooldown at 12.2299929 (the last swing at footman 4 at 10.6299929
  plus `cool1` 1.6), the hit at 12.5599929.

## Edge cases

- The target of these tests could not move (move speed 0) and belonged
  to `Player(15)`.
- `smart` to a point returns false although the unit obeys it.

## Open questions

1. Whether other orders use the reaction delay as well (acquisition
   does).
2. One chase in which the attacker did not start its swing at a goal
   point 136.68 away (limit 137);
   what makes the attacker take a new goal 66 ticks after a chase
   resumes; when a moved target makes the attacker take a new goal at
   once; earlier observations of an attacker following the target's
   current position (not reproduced in the chases of the verification
   below).
3. The 150 of the impact call (inferred, not read from data), whether
   area spells make the impact call and whether it has a cooldown, and
   whether the collision added is the responder's or the caller's (the
   same in all tests); which alliance flag is needed; the order of
   the helpers' draws of generator 9; retaliation by units that are not
   idle; `minRange`; acquisition of air units and by buildings with
   weapons; whether the 0.5 s period depends on any data field;
   `GuardDistance` and returning home.
4. Queued orders, group orders, formation movement.
5. Orders on units of other players, `hold position` with enemies in
   range.
6. How the start block is chosen when the attacker's own block is not
   free (one case seen), and what the attacker does when no free block
   is near.
7. Following: whether the rule for a start within the range compares
   the gap with the distance walked or with half the start gap (the
   same thing on a straight way), where the boundary between the two
   rules lies for start gaps between 298 and 303, the current order of
   a follower that stands, following units of other players; when a
   standing follower checks the gap again (77 ticks after it passed
   300 in the one case seen).
8. Why one unit inside the caller's reach is left out of its call
   (test RXG, the line along x), and the order of the chained calls when
   several responders reach new units.
9. How the chase search of `c-RXD-1` in tick 3049 (creep 9) ended:
   without touching its target and before its goal cell.
10. The other parts of the rating of targets at a check while attacking
   (heroes, structures, units that attack the unit or its allies, the
   life of the candidates) and their order.
11. Whether spell damage also makes the damaged unit's own call and
   starts its 3.0 s timer ("Spell damage").

## Sources

- Test maps RZC, RZD, RZE, RZF and RZG on 1.27b under Wine,
  2026-10-21: pairs and rings of footmen around a footman damaged by a
  script or by a glaive thrower; acquisitions logged, the calls'
  queries (centre, radius, units returned) read in the running game.
- Test map RXG on 1.27b under Wine, 2026-10-20: five layouts of nine
  kobolds, the middle one damaged by a script; `EVENT_UNIT_ACQUIRED_TARGET`
  logged, the call for help and its unit query read in the running game.
  Test maps RXE, RXF (copies of `c-RXD`): acquisitions logged, the tick
  of the first one read from the movement clock.
- Test maps RUO, RUP, RUQ and RUS on 1.27b under Wine, 2026-10-20:
  pairs of a footman and a footman or peasant standing still on one
  line, start distances 300 to 1500, `smart` and `move` on the leader,
  the follower's position logged every tick until it stopped.
- Test maps of 2026-10-18 for attack-move (paused enemy footmen beside
  the way) and retaliation (a glaive thrower attacking an idle footman,
  idle footmen, riflemen, archers and peasants around it).
- Test map of 2026-10-18 for `OrderId`, `OrderId2String` and the
  `...ById` natives (one footman, results read right after each call).
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
5. **Chases.** Flat open map, `SetRandomSeed(7)` (reaction delay below
   one update), footman attackers (`rangeN1` 90, `collision` 31) ordered
   at tick 0 to attack a peasant-based target of `Player(15)` (100000
   life, `collision` 16) that is ordered at the same tick to walk 2000
   north (or east):

   | Attacker from | Target from, speed, direction | First motion | Swings start (update) | Damage | Resumes (first motion) | Stand |
   |---|---|---|---|---|---|---|
   | (-2500, -3000) | (-2000, -3400), 190, north | 5 | 322, 688 | 427, 788 | 533, 893 | 598, 958 |
   | (-2500, -6000) | (-2000, -6400), 100, north | 5 | 322, 634, 1072 | 421, 733, 1178 | 527, 839 | 592, 904 |
   | (-2500, 0) | (-2000, -400), 150, north | 47 | 358, 676 | 463, 776 | 569, 881 | 634, 946 |
   | (-2500, -6000) | (-2000, -6000), 190, east | 5 | 706 | | | |

   New goals in the first row were taken at updates 4, 106, 208, 310
   (target at (-2000, -3400), (-1998.1, -3309.2), (-1998.4, -3212.3),
   (-1998.6, -3115.4)), after the first swing at tick 528 (target at
   (-1999.1, -2908.4)) and at update 598. The local path read from memory
   at tick 20 ran from (305.875, 290.25) in 14 cell steps to the cell
   centre (319.5, 278.5) (units of 32), 2.14 from the target's position
   (321.5, 277.75). A model following this spec, SPEC-0018 and SPEC-0020
   reproduced the attackers' positions within 0.31 world units at every
   tick for the first, second and fourth rows over the whole test
   (up to 1140 ticks) and for the third row up to tick 1108, where the
   game did not yet start the third swing (open question 2).
6. **Swing start against the goal point.** 13 footmen at (-2500 -
   0.625 k, -10000 + 1600 k), k = 0 .. 12, each attacking a peasant of
   `Player(15)` at (-2000, y) that walks east to (2000, y), all ordered
   at tick 0 (`SetRandomSeed(7)`; the reaction delays differ per
   attacker): first motion and the update of the first swing:

   | k | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
   |---|---|---|---|---|---|---|---|---|---|---|---|---|---|
   | First motion | 5 | 5 | 47 | 17 | 29 | 17 | 5 | 5 | 23 | 17 | 29 | 29 | 47 |
   | Swing (update) | 706 | 706 | after 840 | 802 | 820 | 802 | 712 | 712 | 814 | 802 | 826 | 826 | after 840 |

   A model with the range test as the limit gave all eleven swings at
   the same updates and the positions within 0.1 world units.
7. **Chase round a wall.** Walls of unwalkable cells at
   -2000 <= x < -1968, y0 - 400 <= y < y0 + 400; footman at (-2500, y0)
   attacking a peasant-based target at (-1850, y0): standing (move speed
   0; y0 = -6000), walking east to (150, y0) (y0 = -3000), walking north
   to (-1850, y0 + 2000) (y0 = 0). The standing target was hit at
   tick 908 (swing from update 802), the northbound one at 721 (from
   616); the eastbound one was not caught in 1000 ticks. In the first
   row the attacker stood still at update 388 when it took over the
   waypoint (-2000, -6416) of the chase path. A model following this spec
   reproduced all three attackers within 0.015 world units at every
   tick.
8. **Acquisition.** A footman of `Player(1)` paused with `PauseUnit` at
   (0, 0), created first, and 20 footmen of `Player(0)` on a circle of
   radius 300 round it, created after it, all in the same tick (tick 201
   of the game, from a 1 s timer) after `SetRandomSeed(7)`. Ticks below
   are counted from the tick of the creation (= 0). Acquisition ticks of
   the circle footmen, sorted: 3 8 18 22 26 31 36 44 45 46 48
   68 70 72 81 84 88 88 90 94; with seed 8: 1 5 13 16 19 32 33 33 40 46 67
   69 70 73 79 81 85 89 91 96. Their update ticks are 4, 10, 16, ...
   (ticks 205, 211, ... of the game). The first change of facing (start
   of the turn towards the target) of the footman acquiring at tick 3
   (seed 7) came at tick 10, of the one at 8 also at tick 10: the first
   checks at `100 x` = 3.766 and 8.056, plus the delays 0.305 and 0.766,
   end at 4.071 and 8.822, so the first update at or after them is 10
   for both (tick 4 is too early for the first one by 0.071). Further
   ones: 22.559 + 10.514 -> 34 (first facing change at 34), 18.155 +
   42.838 -> 64.
9. **Attack-move.** Seed 7, ten footmen of `Player(0)` created at
   (-3000, y) with `y = -9000 + 1500 k`, `attack` to (2000, y) given at
   the same tick; paused enemy footmen (5000 life) at
   (-1500 + 100 k, y + 250). With the tick of the order as 0, the
   acquisitions came at 802, 902, 902, 1002, 1102, 1202, 1202, 1302,
   1402, 1502 (k = 0..9), each at the first check (order + 102 + 100 j)
   where the distance was at most 562 (largest distance not taken
   569.3, smallest taken 544.7 at the check before, 448.1 to 544.7 at
   the checks taken). The units stood from the tick after the
   acquisition and turned or walked again in ticks 805, 907, 949, 1015,
   1129, 1219, 1207, 1309, 1423 (k = 0..8): the first update at or after
   the check plus the reaction delays of the first nine outputs of
   generator 9 (0.31, 0.77, 42.84, 10.51, 24.76, 15.09, 3.43, 1.37, 17.42
   ticks), in the order of the acquisitions. Orders given at 0, 17, 33,
   50, 71 and 99: acquisitions at 802, 919, 1035, 1152, 1273, 1401
   (always order + 102 + 100 j). A footman that killed a 15-life target
   with its hit at 916 acquired the next target at 1518, and resumed
   walking at 1021.
10. **Retaliation.** Seed 7, a footman at (0, 0), a glaive thrower of
    `Player(1)` at (0, 1000) ordered to attack it, a second footman at
    (300, 0), more at (1200, 0) and (-1500, 0): at the first damage both
    near footmen acquired the thrower, the far ones did nothing; the
    damaged one started to turn in tick damage + 45, the other at
    damage + 15, matching the third and fourth outputs of generator 9
    (the first two were used by the thrower: created together with the
    footmen 0.5 s before its order, with an acquisition range of 1150 it
    acquired the footman at (0, 0) at its own first check, one draw, and
    the attack order drew the second).
10. **Following** (RUS). A footman at (-7000, y) given `smart` on a
   standing footman at (-7000 + D, y): for D = 345 (start gap 283) it
   stops after 18 updates, 145.8 from its start; for D = 365 (gap 303)
   after one update; for D = 400 (gap 338) after 5 updates, 40.5 from
   its start.
