---
id: SPEC-0024
title: Replay checksums (turn checksum)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, memory reads and replays of test maps)"]
---

## Behaviour

### Notation

All values are unsigned 32-bit, arithmetic is modulo 2^32.

- `rol3(x)`: rotate left by 3 bits.
- `fold(h, x) = rol3(h XOR x)`.
- `bh(x)` of a 32-bit value: the sum over its four bytes `b` of
  `(b * 0x7A2D) XOR b`. Of a byte string: the same sum over all its
  bytes. The byte order does not matter (it is a sum).
- The id of an object is a pair `(i, n)`: `i` is the number of the entry
  the object holds in the engine's object table, `n` is its creation
  number (see "Object table"). `idsum(obj) = rol3(i XOR n)`.

### Where the checksum is

In a replay every time slot block (id 0x1F, also 0x1E) is followed by a
block 0x22 with a length byte of 4 and a 32-bit little-endian value. The
value is the turn checksum the recording player computed for that time
slot. The same value goes into the player's network message for the
turn. A game shows no message when a replay is played back and its own
checksums differ from the recorded ones: playback continues.

Turns are numbered `k = 1, 2, ...` in replay order. `t_k` is the sum of
the time increments (ms) of time slots `1..k`, `t_0 = 0`.

**Actions between ticks.** The actions of a time slot are applied
between two simulation ticks: after the script timers of one tick
`k - 1` and before the engine timers, unit updates and script timers
of the next tick `k`, with the movement clock (SPEC-0018) already
reading `clock(k)`. So a script timer firing in tick `k` already sees
their effect, and a time taken from the clock by the action is
`clock(k)`. Measured: a train order given with the mouse
(`c-RTP-1.w3g`, SPEC-0027) started its training between the periodic
timer calls of ticks 1005 and 1006 at the queue time 5.02488, and the
training ended in tick 5007, which is the tick of the expiry
`add(clock(1006), 20.0)` (with `clock(1005)` it would have been tick
5006); in `c-RSR-1.w3g` the gold of the build order was already paid
when the periodic timer of that tick read it.

**When actions take effect.** The checksum of turn `k` (the 0x22 block
after time slot `k`) is computed before the actions of time slot `k`
change the game: an action in slot `k` shows first in the checksum of
turn `k + 1`. Measured in two live games of the same test (selection,
`move` order, attack order given with the mouse), reading the parts of
every turn checksum from memory:

| Action in slot `k` | Turn `k + 1` | Turn `k + 2` | Turn `k + 3` |
|---|---|---|---|
| select a unit | `G` | - | - |
| `move` (right click on the ground) | `C`, `D` | `C` | - |
| attack (right click on an enemy) | `C`, `D`, `R` | `C` | `C`, `D` |
| box selection of four units | `G` | - | - |
| group `move` of four units | `C`, `D` | `C` | - |
| open the build menu (action 0x67) | - | - | - |
| build order (action 0x11 with a type and a point) | `C`, `D` | `C` | - |

A group walking in formation also draws the script generator: `E`
changed in four turns while four footmen walked (replay `c-RSM-1.w3g`),
although the script drew no random numbers. When the peasant of a build
order reaches the place and starts the building, `C`, `D`, `R` and `G`
change in one turn (`G` through the player's type record of the new
building, `c-RSN-1.w3g`).

(`C` and `D` change because the order creates internal objects, some of
which are freed one turn later; the walk's path objects are freed when
the unit arrives. `R` changes on the attack order because a game
generator is drawn, SPEC-0019.) In the replay `c-RSL-1.w3g` the three
actions are in slots 36, 51 and 77, so they show in turns 37, 52 and 78.

Each hit of the attack draws a game generator (`R` changes in the turn
after the hit). About 0.5 s after each hit the attacking unit creates a
path cluster and a path, as when it starts to walk, and frees them
again: `C` and `D` change in one turn and `C` in the next, or only `D`
when the objects were created and freed between two checksums. No
random number is drawn there.

**Gathering.** Workers harvesting gold draw no random numbers and do
not change `G` (the gold of the player is not part of the checksum);
they change only `C` and `D`. A change in tick `e` shows first in the
checksum of the first turn whose game time `B` is at least `5 * e` ms.
With `u` the update (ticks `6 m + 1`, SPEC-0018) at which a worker's
walk ends, the ticks are:

| Moment | Objects | Tick |
|---|---|---|
| a `harvest` order to a mine (script or player) | about 19 created per worker | the tick of the order |
| the worker gives the gold at the town hall | 19 created (the walk back), 1 freed; then 2 freed (the old walk) | `u + 6`; `u + 12` |
| the worker enters a free mine (at `u + 6`) | 2 freed (its walk) | `u + 12` |
| the worker waits at a mine another worker is in | 1 created; 2 freed (its walk) | `u + 6`; `u + 12` |
| a worker leaves the mine with gold, 200 ticks (1 s) after it entered; a waiting worker enters in the same tick | 16 created, 15 or 16 freed | the tick it leaves |

The ticks were read in the running game at each creation and freeing
of an object. The leaving is a timer (SPEC-0018): a worker that entered
at update `u + 6` leaves in tick `u + 206`, and one that entered when
the worker before it left leaves 200 ticks after that. In the replay
`c-RXI-1.w3g` (six workers harvesting a mine 1148 from their town
hall, no other units) these ticks, the two draws of the script
generator in ticks 667 and 715 (workers blocked by each other on their
first way to the mine, SPEC-0021, "Blocked by a unit") and the two
ticks below give exactly the turns in which the remainder of
Verification 4 changes, 86 turns up to 41 s:

- tick 201: the script creates the town hall, the mine and the workers;
- tick 441: five objects of the town hall are freed by a timer of 1.2 s
  from its creation (expiry 2.20009, the creation at queue time 1.0001);
- tick 3871: one object is freed by a timer with expiry 19.34970 (cause
  unknown).

In `c-RXD-1.w3g` the first changes are turn 10 (tick 201), turn 14 (the
six `harvest` orders of tick 301) and turn 20 (tick 441); from turn 64
on the creeps fight.

### Turn checksum

    v = rol3(S XOR 6059)
    v = fold(v, B)
    v = fold(v, C)
    v = fold(v, D)
    checksum = fold(v, E)

- 6059 is the build number of 1.27b.
- `S`: the state sum (below).
- `B`: in single player, the game time in ms that has run when the
  checksum is computed, which is `25 * floor(t_(k-1) / 25)` (the game
  runs in steps of 25 ms, SPEC-0002). In network games a different value
  may take its place (open question 1).
- `C`: the number of the entry the next object will get in the object
  table: the first entry of the free list, or the size of the table when
  the free list is empty.
- `D`: the next creation number.
- `E = bh(a) + bh(b)` of the script generator state `a`, `b` (SPEC-0007).

### State sum S

    h = K1
    h = fold(h, G)     the game sum (below)
    h = fold(h, K2)
    h = fold(h, K3)
    h = fold(h, R)     the game generators
    h = fold(h, N)     the network sum (below)
    S = fold(h, X)     the cheats

- `R`: the sum of `bh(a) + bh(b)` over the 45 game generators of
  SPEC-0007 (their order does not matter).
- `X`: the set of active cheats as bit flags, with bit 12 cleared; 0
  without cheats.
- `K1`, `K2`, `K3`: constants of the game. They depend only on the
  loaded game data and the object data of the map (unit, item,
  destructable, ability, buff and upgrade data, the ability and upgrade
  tables) and do not change during the game. Measured on test maps made
  from `(12)IceCrown` without changed object data: `K1 = 0xEA73D958`,
  `K2 = 0xA996F046`, `K3 = 0x414C0D79`. How they are computed from the
  data is not specified (open question 5).

Every step above is linear over XOR, so the checksum can also be written
as `F XOR K`, where `F` is the checksum computed with `K1`, `K2`, `K3` set
to 0 and `K` depends only on them. An implementation can take `K` from
the first recorded checksum whose other parts it knows, and use it for
the rest of the game.

### Network sum N

`N` starts at 0. For every time slot block of the replay, in order, with
`p` the block's payload (the 2-byte time increment followed by the
action data, that is, all bytes after the block's length field):

    w = 0
    for each whole little-endian 32-bit word x of p: w = fold(w, x)
    for each remaining byte y of p (at most 3):    w = fold(w, y)
    N = N + bh(id + bh(w))

with `id` the block id (0x1F). The checksum of turn `k` includes the
blocks `1..k`. A few other network messages (leaving the game and two
more) also add `bh(id)`; this was not tested.

### Game sum G

    G = 12
    for player = 0 .. 11: G = fold(G, P(player))

Players 12 to 15 (neutral) do not take part. All 12 players counted in
the tests, including empty player slots.

The player sum:

    h = fold(T, h1)
    h = fold(h, h2)
    for each filled entry obj of the player's 7-entry list: h = fold(h, idsum(obj))
    P = fold(Y, h)

- `h1`, `h2`: two 32-bit fields of the player, both the float 1.0
  (`0x3F800000`) for every player in the tests (open question 3).
- The 7-entry list holds the player's dead heroes. A hero joins it at
  the end of its death time (`death` of `Units\UnitData.slk`, 1.5 s for
  the paladin) after it dies, and the entries are folded in the order
  the heroes joined. `c-SBP-1`: paladin 12 (ids 1189, 1192) died at
  6.199680 and joined at 7.699680, so the next turn's G changed from
  `0x2CFB6A98` to `0x24BAF689`; paladin 15 joined at 8.449735. When a
  hero leaves the list (a revival) was not tested. It was empty in the
  other tests, which had no dead heroes.
- `T`: the type record sum. Each player has a list of records, one per
  unit type, kept in the order they were created. With the record's
  values `q0 .. q6`:

      x = rol3(q2 XOR q1)
      y = rol3(q4 XOR q0)
      z = rol3(q5 XOR q3)
      r = rol3(rol3(rol3(x XOR y) XOR z) XOR q6)

  `T` starts at 0 and does `T = fold(T, r)` for each record in list
  order, then `T = fold(T, u)` for each value `u` of a second list that
  was empty in the tests. The type id itself is not part of the sum.
  Measured values: a footman record has `q0` = 2147483647 (no limit),
  `q1` and `q4` = the number of footmen (1, then 2 with two footmen),
  `q2`, `q3`, `q5` = 0, `q6` = 8. Every player had three more records
  made at game start, before any unit: `hrtt` (all 0, `q6` = 8), `otbk`
  (the same) and `uske` (`q0` = 25, the rest 0, `q6` = 8).
  A building's record has `q6` = 4. In `c-RXI-1` at its first turn,
  player 0's records were `hrtt`, `otbk`, `uske` as above, `htow`
  2147483647 1 0 0 1 0 4 and `hpea` 2147483647 6 0 0 6 0 8. With 8 for
  the town hall the turn's G comes out as `0x2CFB6A99` instead of the
  game's `0x2FFB6A99`. Every unit type read so far had 8.
  Deaths change two of the values (read in `c-SAK-1`). A kill of a unit
  of type X by player p adds 1 to `q5` of p's record of X, the number of
  units of that type p has killed. If p has no record of X yet, one is
  added at the end of the list with `q0` = 2147483647, `q5` = 1, `q6` =
  8 and the rest 0. Kills by the neutral players change nothing in G.
  A death of one of p's own units takes 1 from `q4` of its type (the
  number alive); `q1` (the number made) stays. Mirror Image copies did
  not change any record. After 23.8 s of `c-SAK-1`, player 0's records
  were, in list order: `hrtt`, `otbk`, `uske` as above; `Hmkg` and
  `Obla` 2147483647 1 0 0 1 0 8; `hfoo` 2147483647 6 0 0 5 0 8 (footman
  7 dead); `nkob` 2147483647 0 0 0 0 3 8 (three kobolds killed); `ngno`
  2147483647 0 0 0 0 1 8. In that game G changed at every kill and every
  loss of player 0, and at nothing else.
- `Y`: the selection sum of the player, from the selection state that
  is kept in sync between players (SPEC-0006 for `SyncSelections`):

      Y = number of selected units
      for each selected unit u, in selection order: Y = fold(Y, idsum(u))
      Y = fold(Y, j)    j = number of the active subgroup (from 0), or 0xFFFFFFFF if none
      for each subgroup, in order:
          Y = fold(Y, number of units in it)
          for each unit u in it: Y = fold(Y, idsum(u))
      for each of the 10 control groups: for each unit u: Y = fold(Y, idsum(u))
      if there is a lead unit: Y = fold(Y, idsum(lead))
      if there is a second marked unit: Y = fold(Y, idsum(it))

  Subgroups are the groups of same-type units within a selection. The
  lead unit was the first selected unit in the test; the second marked
  unit was never set (open question 4). A player with nothing selected
  has `Y = fold(0, 0xFFFFFFFF) = 0xFFFFFFFF`. `c-RTW-3` turn 39, player
  slot 0 after selecting footman (825, 828) (idsum `0x28`): `Y` = 1,
  then `0x148`, `0xA40` (j = 0), `0x5208` (subgroup count 1), `0x29100`,
  `0x148940` (lead). G changed from `0x2CFB72CF` to `0xD9402D30`, which is
  exactly that slot's `Y` going from `0xFFFFFFFF` to `0x148940`. The
  slot index is the player's slot (here 0), not a count of players with
  a selection.

### Object table

The engine keeps one table of internal objects. Each object holds one
entry; freed entries go to a free list and are reused, the last freed
first. Every new object also gets the next creation number. The table
holds many objects besides units: on an empty test map it had about 870
entries before the script created anything (path finding data, flags
and numbers kept in sync, regions and others). A unit's id pair is its
entry in this table and its creation number, for example `(825, 828)`
and `(857, 863)` for two footmen. `C`, `D` and every `idsum` therefore
depend on the creation and freeing of all these objects (open
question 2).

The same pairs identify units in the actions of a replay. In a replay
recorded with real mouse clicks (test map RSL), selecting the player's
footman wrote the pair (827, 830) in the selection action and in the
subgroup action (with the type `hfoo`), and the attack order on the
enemy footman wrote its pair (845, 848) together with the target point
(0, -700), the enemy's position; the same pairs were read from the
units in memory. `GetHandleId` gave unrelated numbers (1048680,
1048681). The creation number is never reused and grows with the order
of creation, so it identifies a unit for the whole game; the entry
number can be reused after the object is freed. A unit takes many
creation numbers when it is made (its own and those of its internal
objects): two footmen created one after the other by the script got
creation numbers 830 and 848, in another test 828 and 863. Units placed
on the map before the game starts were not examined (open question 6).

**Objects of an attack-move order.** In `c-RXR-1` the script gives ten
units attack-move in tick 301; the object table changed only in that
tick between 1.25 and 1.75 s (the searches of tick 307 and later made
no objects). Each order made 16 objects, unit after unit: the order,
one object made with it, eleven objects of the attack ability as the
order starts, and three of path finding as the unit starts to move, of
which the first was freed at once (so the next unit's order took its
entry). In a 0.0001 s timer of the same tick seven of each unit's
objects were freed, unit after unit, in the order 2, 13, 12, 11, 10,
9, 8 (numbering the 16 from 1). Entries came from the head of the free
list and, once it was empty, from the end of the table (size 1243 ->
1349); creation numbers 1247 to 1406. The full list is in
`game-data/1.27b/replays/c-RXR-1.objects-301.tsv`.

### Objects by mechanic

Read in the game on single-unit test maps (c-SBA, c-SBB, c-SBC) and in
`c-SAK-1` (`game-data/1.27b/replays/c-SAK-1.objects*.tsv`). "Deferred"
means freed in a 0.0001 s timer of the world clock started at that
moment (SPEC-0018), so in the same tick after the callback. "Taken"
objects get the next entry of the free list (the last freed first) and
the next creation number, in the order given.

**Second pool.** Some objects live in a second pool with entries of its
own: their own entry is the index with the top bit set, and their
creation number is that index. They do not take entries or creation
numbers of the main table. They are made at deaths, at stuns, when
Mirror Image hides its caster, and once per creep group (with its first
creep).

**Units.** `CreateUnit` takes, in one go and in this order, the objects
of the unit and its abilities. Counts measured (c-SBB, `c-SAK-1`):

| Type | Objects |
|---|---|
| knight, rifleman (attack, no ability) | 17 |
| footman (with Defend), grunt | 18 |
| peasant | 22 |
| priest, sorceress | 25 |
| hero (paladin, mountain king, blademaster) | 24 |
| creep (kobold, gnoll, ogre) joining a group | 28 |
| creep starting a group | 29, plus 1 of the second pool |
| barracks | 27 |
| town hall | 30, plus 1 of the second pool |
| gold mine | 23 |
| farm | 0 |

Some of them are freed deferred: a footman's last two, a hero's
positions 17 to 20, 22 and 23 (of 0 to 23), and eleven of a
creep's. Two of a footman's and four of a creep's are freed at its
death. `UnitAddAbility` of a hero ability (`AHhb`, `AHbz`) or of `Aslo`
takes 2.

The parts by position `k`, as each is made (`c-RXI-1`):
- every unit: `k` 0 .. 9 the unit's own parts. A unit that moves has
  one more at `k` 10 (none for a building). Then the part all units
  have (footman `k` 11, building `k` 10).
- footman: 12 Move, 13 Attack, 14 and 15 its abilities (Defend), 16
  and 17 the start order objects (deferred).
- peasant (22): 12 Build and 13 its second part, 14 Move, 15 Attack, 16
  Harvest, 17 Call to Arms, 18 Repair, 19 a part of Harvest, 20 and 21
  the start order objects (deferred).
- buildings: after `k` 10, three building parts (11 .. 13), freed at
  the end of the game together, 0.1 s before the unit's other parts.
- gold mine (23): 14 its gold ability and 15 its second part, 16 one
  more ability object, 17 Invulnerable, 18 and 19 the two abilities
  every building has (on fire when damaged, and the alarm), 20 and 21
  the start order objects (deferred), 22 one more object made at the
  end of the making (kept).
- town hall (30 + 1): 14 Train and 15 its second part, 16 Upgrade and
  17 its second part, 18 an ability object with four data parts
  (19 .. 22), all five freed at 2.204993 in `c-RXI-1`, 23 Call to Arms,
  24 Return resources, 25 Rally, then the rally point (second pool),
  26 and 27 the two building abilities, 28 and 29 the start order
  objects (deferred).
- `CreateDestructable` (a tree) takes 7: the destructable, three parts
  of it, and three pathing parts (the kind of a building's 11 .. 13).
  The pathing parts are freed when the tree falls (`c-RYP-1` tree 2 at
  50.031601).

**Event objects.** A unit event with at least one registered trigger
takes one object for the event and one more for each registration that
fires (two triggers, or one trigger registered twice: 1 + 2). All are
freed deferred. An event with no registration takes nothing. So a hit
on a unit with one `EVENT_UNIT_DAMAGED` registration takes 2 objects,
and a hit on a unit without any takes none (c-SBA).

**Orders given by the script.**

| Order | Taken (in order) | Freed deferred | Freed when the order ends |
|---|---|---|---|
| `move` to a point | order, record, 12 of the move ability, then the search's 3 | record and 6 of the 12 (the last made first) | the other 6 and the order |
| `attack` to a point (attack-move) | order, record, 11 of the attack ability, then the search's 3 | record and 6 of the 11 | the other 5 and the order |
| `stop` | order, record, 5 | all 7, with the remaining objects of the order it ends | - |

"Ends" is when another order replaces it (or the unit dies). The frees
then also come deferred from that moment.

**Harvest.** Read in `c-RXI-1` (gold) and `c-RYP-1` (lumber), per
worker:
- a harvest order: the order, its record, then the order's parts
  (gold 14, lumber 12, lumber 13 for the order a delivery gives), then
  the walk. For the gold order, parts 10 .. 13 are freed deferred, 8 and 9
  at the end of the walk to the mine (arrival, whether the worker goes in
  or waits), and 0 .. 7 when the order ends (the worker comes out with
  his load). For the lumber order, part 7 is freed at the arrival at the
  tree, the last 4 (5 for the 13) deferred, and the others when the
  order ends (the turn back with a full load);
- arrival at a tree: 3 objects; two are freed deferred and one at the
  worker's next update; then a short walk;
- the walk back (return order): the order and 12 parts; 7 kept to the
  delivery, 5 deferred;
- a drop of the tree (failed arrival check): a stop's set of 3, freed
  deferred, two kept parts of the harvest order freed, and one wait
  object kept 0.5 s, to the moment another tree is taken;
- at a mine: one object per worker that has to wait, made when it starts
  to wait and freed when it goes in;
- once, at the first worker's entry into a mine (`c-RXI-1` 871) and at
  the first hit on a tree (`c-RYP-1` 1089): an ability object with three
  parts in the second pool. At the mine one more main-table object comes
  with it. All are kept;
- a mine's queue timer: at the exit of a worker with which the next
  worker goes in while at least one more is still waiting, the mine
  makes (the first time) a rate part (a value of -1.0 per second over
  11.0 s) or restarts its 11.0 s timer of the world clock. The part is
  freed deferred when that timer runs out. `c-RXI-1`: made at 6.349778,
  restarted at 7.349706 and 8.349600, freed at 19.349699; `c-RXD-1`:
  made at 21.947317, restarted at 22.947149, freed at 33.947247.

**Missiles.** A missile's objects are freed 5.0 s after its impact by
a queue timer, whose expiry makes a deferred timer that frees them.
`c-RXR-1`: impact at 13.778688, the 5.0 s timer at 18.778688 in the
tick of world clock 18.777849, and the free at 18.777948.

**Walks.** Every path search (at an order or at an update) takes a
helper, a cluster and a path, and frees the helper at once. The walk's
path and cluster are freed (path first) at the unit's first update
tick (its 6 m + 1 cycle, SPEC-0018) after the tick in which the walk
was stopped or replaced. The update in the same tick does not count.
Read in c-SBC for a footman whose updates fall on ticks 6 m + 1: new
order at 701 -> freed 703; stop at 901 -> 907; new order at 1501 ->
1507; acquisition stop at 2003 -> 2005; swing start at 2245 (in reach)
-> 2251; the same at 3307 -> 3313; after-swing chase at 2449 -> 2455.

**Acquisition.** An acquisition by a check takes 7 objects for a unit
on attack-move and 10 for a unit without an order of its own (c-SBD,
two footmen, one standing, one on attack-move, each seeing an enemy
appear 400 away):
- attack-move: the last 2 are freed deferred and the 5th at the end of
  the reaction delay (when the attack is taken up). In c-SBC the 4th
  went at the target's death.
- no order: the last 2 are freed deferred and the 8th at the end of
  the reaction delay. The first 7 stay with its attack order.
On a unit with an `EVENT_UNIT_ACQUIRED_TARGET` registration the
event's objects come in addition (see "Event objects"; in `c-SAK-1`
these are the "acquisition at a check" and "event object" rows).

**Unit death.** At every death (a kill, `KillUnit`, with or without
attackers) the dying unit takes a record and 9 queue objects of its
death order, and a footman 1 more (a kobold did not). Freed deferred:
the record, the last 4 queue objects (from the last) and the extra
one. Of the first 5 queue objects 2 are freed 3.0 s after the death
(600 ticks), 1 about 5.0 s after, and 2 stay (with the corpse; not read
further). Four objects of the second pool also come with a death.
An attacker whose own order was an attack on the dead unit takes 1
object, freed deferred, and its order ends. An attacker on attack-move
takes none: it takes a new goal at once (walk objects) to go on with
its attack-move. Read on c-SBE (four kills at separate times: no
attacker, an attack-move attacker, a direct-order attacker, an idle
kobold) and in `c-SAK-1` (kobold 9: death 2753, frees 3353, 3353,
3753).

**Hits.** The swing and the hit take no objects of their own (c-SBC,
no damage registration). In `c-SAK-1` the two objects of each hit are
the damage event and its registration (see "Event objects").

## Edge cases

- Only the sum `R` over the game generators is used, so two generator
  states with the same byte sums give the same checksum.
- The first turns of a game (before about 1.2 s on the test map) had
  other `C` and `D` values while the script created objects.
- Two games of the same map that draw the same random numbers still
  get different checksums from turn 2 on (seen with `c-RSA-1.w3g` and
  `c-RSA-2.w3g`): the length of each time slot follows the real clock
  (100, 101, 102 ms in single player), so `B` and `N` differ. The random
  generators start from the same state in every game (SPEC-0007, "State
  at game start"); the random seed of the replay header is not used for
  them.
- In the first 100 ms of every game some internal objects are freed,
  so `C` changes between turn 1 and turn 2 (on an empty test map from
  "no free entry" to the free entry 806) while nothing else does; the
  random generators are not used there. The checksum of turn 2 therefore
  always differs from what turn 1 would predict.
- `X` has bit 12 cleared, so the cheat with that flag never changes
  the checksum.

## Open questions

1. Network games: a flag that was 0 in single player makes the game use
   a sum over the objects of the object table in place of `B`. When the
   flag is set and what that sum is was not studied.
2. Which actions create and free entries of the object table, and in
   which order. Without this `C`, `D` and the id pairs cannot be
   reproduced; see Verification 4 for a way to use the checksum anyway.
3. The meaning of `h1`, `h2` (possibly handicaps); when a hero leaves
   the dead-hero list (revival).
4. The second marked unit of the selection; the key order of the 10
   control groups (groups were empty in the tests).
5. How `K1`, `K2`, `K3` are computed from the data, and their values on
   other maps and with changed object data.
6. The creation numbers of units placed on the map, and a rule that
   gives the creation number of each unit without reproducing the object
   table (which objects a unit of each type creates).

## Sources

- Analysis of the 1.27b game library: how the turn checksum and its
  parts are computed.
- Memory reads of the running game (1.27b under Wine): the stored turn
  checksum matched the replay's 0x22 values (68 of 68); dumps taken
  while the game was paused by the script at different times in two
  games gave the same `K1`.
- Replays of test maps recorded with `EndGame(false)` (they are in
  `game-data/1.27b/replays/`, see its `README.txt`).

## Verification

1. **Byte hash:** `bh(0x00000001) = 0x7A2C`, `bh(0x00000100) = 0x7A2C`,
   `bh(0x000000FF) = 0x79B22C`, `bh(0x12345678) = 0x83B8F0`,
   `bh(0xFFFFFFFF) = 0x1E6C8B0`.
2. **Network sum from a replay:** for `c-RSF-1.w3g`, the column `N` of
   `SPEC-0024-vectors.tsv` gives `N` after each turn; it depends only on
   the replay's time slot blocks.
3. **Full checksum:** for each row of `SPEC-0024-vectors.tsv` (turns 12
   to 206 of `c-RSF-1.w3g`), the formula above with the row's `B`, `N`,
   `C`, `D`, `E`, `R`, `G`, the constants `K1`, `K2`, `K3` and `X = 0`
   gives the row's `checksum`, which is the replay's 0x22 value. All 195
   rows match.
4. **Using the checksum without the object table:** between two turns
   in which no object is created or freed, no random number is drawn and
   the selection and type records do not change, only `B` and `N`
   change, and both follow from the replay. Undoing them from two
   consecutive checksums gives the same remainder; a turn where the
   remainder changes is a turn in which the game state changed in one of
   the other parts. In `c-RSF-1.w3g` this happens at turn 41 (4286 ms),
   when `C` went from 851 to 858.
