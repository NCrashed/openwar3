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
  did not respond, nor did units that already had a target;
- all of them then wait their reaction delay (generator 9, counted from
  the tick of the damage; the damaged unit draws first) and attack the
  attacker by the approach rule.

The reach of the call depends on `CallForHelp` of `Units\MiscGame.txt`
(600 in 1.27b; the map's copy is used, SPEC-0015): footmen up to about
`CallForHelp + 175` from the damaged footman responded (with 600: 750 in
all 16 directions, 776 in all directions around a victim at (16, -16),
783 only on one side of a victim at (0, 0), 800 nowhere; with 300: 480
yes, 485 no). The exact shape of the area is open (open question 3).

**Order of the responders.** A call for help (a damaged unit, or an
idle creep that acquires a target, SPEC-0030) visits the units around
the caller through a grid of cells of 256 x 256 world units from the
map's lower left corner (-12288, -12288 on the test maps): the rows of
cells from the bottom up, in each row the cells from left to right, and
in each cell the unit created later first. A unit lies in the cell of
the point (x - c, y - c), its position less its collision size `c`
(the tests used collision 31; 31 and 32 cannot be told apart). The
caller itself is skipped, and each responder that acquires the target
fires `EVENT_UNIT_ACQUIRED_TARGET` and draws its reaction delay in this
order, after the caller's own draw. Each responder then calls for help
itself in the same way (a chain), so units out of the caller's reach
but within a responder's are reached in the same tick, after all the
caller's responders.

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
- **Waypoints of the chase path** are reached by the range test: at an
  update at which the attacker is within `rangeN1 + c_a + c_t` (the
  range test above) of the current waypoint, the next waypoint is taken
  by the look-ahead, the attacker stands still for that update, and it
  gets a new path from its position along the rest of the chase path.
  (Footman behind a wall: the waypoint was taken over at 134.8 from it,
  not at 142.9 one update before.)
- **A new goal and the existing path.** When a new goal is taken and it
  is the same point as before (a target that does not move), nothing
  changes. Otherwise the chase path is searched again; if its current
  waypoint is the same point as the attacker's current waypoint, the
  attacker keeps its own path, else it gets a new one at once.
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
  after the damage) the attacker takes a goal as above, with the
  target's position at that tick for both searches. It moves from its
  next update on, starting from its current facing as its heading
  (SPEC-0018, as for a new order). At its 12th update counted from that
  first one (66 ticks later) it takes a new goal and stands still for
  that update; then the 17-update schedule goes on. This was seen after
  every swing in every test. Behind targets it could not reach (speed
  270) it stood for one update at intervals of 96 to 102 ticks.
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
- **A standing follower** does not start again as soon as the gap grows
  past 300. In `c-RTW-3` footman 825 followed footman 915 and stood at
  a gap of 295.6 (from tick 4279); 915 then walked past it with a group (gap
  down to 20.5) and away; the gap passed 300 at about tick 5397 and 825
  started walking in tick 5474, at a gap of 374 (positions in
  `c-RTW-3.positions.tsv`). It then stopped at gaps of 341.5, 290.7 and
  293.5 within the next 200 ticks, near the other footmen.

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
  `Player(15)` (neutral passive) is not an enemy of `Player(0)`
  (`IsUnitEnemy` false).
- **Checks.** Each unit with a weapon checks for targets on its own
  schedule: when it is created it draws one output `r` of game generator
  2 (SPEC-0007, the generator of the damage dice), with
  `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)` as for the reaction
  delay. Its first check is `100 * x` ticks (`0.5 * x` seconds) after
  its creation, and then every 100 ticks (0.5 s). Units without a weapon
  (a farm) draw nothing. The draws are made in the order of creation.
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
- **Reaction delay.** After acquiring, the unit waits the reaction delay
  of attack orders (generator 9, `ReactionDelay`, see above), counted
  from the expiry of the check (the queue's time during the check, also
  for the units that answer its call for help), with the draws of
  generator 9 in the order of the acquisitions. The end of the delay is
  again a timer: in the tick whose queue interval holds it (as for the
  checks) the unit takes the target as its goal, with the target's
  position read in that tick, and gets a new cluster (SPEC-0021,
  "Searches of many units in the same tick"). At its first update after
  that it searches its path to that position and starts to act (turn,
  walk): 60 of 60 footmen. In `c-RXD-1` the delays of creeps 7, 8, 6
  and 9 were 0.1221, 0.0626, 0.2499 and 0.0406 s from 7.779825, so they
  ended at 7.901925, 7.842425, 8.029725 and 7.820425, in ticks 1581,
  1569, 1607 and 1565; the creeps searched at their updates of ticks
  1585, 1573, 1609 and 1567, each towards the position of its target
  (peasant 5) in the tick its delay ended (x = -376.586, -387.984,
  -351.890 and -391.784), read in the game.
- **Then** it walks to the target and attacks it by the approach rule
  above. `GetUnitCurrentOrder` stays 0 during such an attack.
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
3. The exact area of the call for help (the reach measured from the
   damaged unit depended on the direction and on the victim's position
   by up to about 15 units); which alliance flag is needed; the order of
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

## Sources

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
