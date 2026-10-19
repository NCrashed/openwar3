---
id: SPEC-0027
title: Training units (Human barracks and town hall)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps, replays with mouse clicks, memory reads)"]
---

## Behaviour

This spec covers a finished building of the player that trains units:
the Human barracks (`hbar`) training footmen (`hfoo`) and the town
hall (`htow`) training peasants (`hpea`). It describes the order, the
costs and the food, the queue, the timing, where the new unit appears
and the rally point. Ticks, the movement clock `clock(k)` and the
update cycle are those of SPEC-0018; arithmetic is that of SPEC-0004.

The values below were measured in the game: a footman costs 135 gold
and 0 lumber, uses 2 food and takes 20 s; a peasant uses 1 food and
takes 15 s. A farm gives 6 food, a town hall 12 (the food cap of
8 farms and 2 town halls was 72).

### The order

- From a script: `IssueImmediateOrderById(building, 'hfoo')`, with the
  rawcode of the unit type as the order id. It returns true when the
  order is accepted.
- From the user interface (building selected, hotkey of the unit): a
  replay action `0x10` (order without target) with flags `0x0042`, the
  rawcode as order id (`0x68666F6F` for `hfoo`) and two 32-bit fields
  `0xFFFFFFFF` (no object). It takes effect in the turn after its slot
  (SPEC-0024).

When an order is accepted:

1. a trigger on `EVENT_PLAYER_UNIT_ISSUED_ORDER` runs with the rawcode
   as `GetIssuedOrderId`;
2. the gold and lumber cost is paid at once;
3. the unit is put at the end of the building's queue; if the queue was
   empty, its training may start at once (below);
4. `GetUnitCurrentOrder(building)` returns the rawcode until the end of
   the tick and 0 from the next tick.

An order is refused (returns false, nothing is paid, no queue change)
when the queue already holds 7 units (the 8th of eight orders in one
tick was refused) or the player has less gold than the cost (100 gold
for a footman).

### Start of training

The first unit of the queue starts training as soon as the player's
food allows it: `food used + food of the unit <= food cap`. At the
start a trigger on `EVENT_PLAYER_UNIT_TRAIN_START` runs
(`GetTrainedUnitType` is the type) and the player's food used grows by
the unit's food. If the food does not allow it, the unit waits at the
head of the queue without any event; it starts at the moment food is
freed: when a unit of the player was killed by `KillUnit` from a
script, the waiting footman started inside that call (its start event
came before the next script line), with food 6/6 before and after.

The time of the start, `Ts`, is the movement clock at that moment:

- for a training started by an order or by freed food, `clock(k)` of
  the tick;
- for a training that starts because the one before it finished, the
  expiry of that training plus 0.0001: `Ts = add(e_prev, 0.0001)`
  (0.0001 is `0x38D1B717`).

The training ends at the expiry `e = add(Ts, bldtm)` of the type's
build time.

### Timers of the engine

The end of a training is a timer of the engine on the movement clock,
like the end of a construction (SPEC-0026). Such a timer fires in the
first tick `k` with `clock(k) >= e`, before the script timers of that
tick, and while its actions run the movement clock reads `e` instead of
`clock(k)`. So a training ends between two ticks of the clock, and its
end can come 4001 to 4007 ticks after its start for 20 s, depending on
the drift of the clock (measured in one barracks training seven
footmen one after the other from 1.5 s: the ends had the clock values
21.4999981, 41.5000954, 61.5001945, 81.5002899, 101.5003891,
121.5004883 and 141.5005798, in ticks 4302, 8303, 12306, 16308, 20310,
24312 and 28319).

### End of training

When the timer fires:

1. the unit is created, of the player of the building, facing 0
   (`GetUnitFacing` 0), at the place found as below; as any unit with a
   weapon it draws game generator 2 once (SPEC-0007); the training
   draws nothing else;
2. a trigger on `EVENT_PLAYER_UNIT_TRAIN_FINISH` runs
   (`GetTrainedUnit` is the new unit); the food used does not change;
3. if the building has a rally point, the unit gets an order to walk
   there (below); otherwise its order is 0 and it stands;
4. the next unit of the queue starts training at once (its start event
   comes right after the finish event), if the food allows it.

### Cancelling

The order 851976 (`cancel`; from the interface key Escape: replay
action `0x61` followed by `0x10` with flags `0x0040` and order 851976)
removes the last unit of the queue. Its cost is given back in full
(the gold shows from the next tick) and a trigger on
`EVENT_PLAYER_UNIT_TRAIN_CANCEL` runs. If that unit was in training,
its food is freed as well (food used 4 -> 2 in the test).

### Where the new unit appears

The unit is put on the first free place of a search in rings around
the building:

1. Let the building's pathing texture (SPEC-0022) cover the cells `X0`
   to `X1` and `Y0` to `Y1` (all its cells, also the unblocked border:
   the barracks texture is 12 x 12 cells with a blocked 8 x 8 centre).
2. Ring `n` (`n = 1, 2, ...`) is the border of the cell rectangle with
   `L = X0 - n`, `R = X1 + 1 + n`, `B = Y0 - n`, `T = Y1 + 1 + n`: the
   cells `(x, y)` with `x` = `L` or `R`, or `y` = `B` or `T`. For a unit
   of size class 1 (2 x 2 cells, SPEC-0020) these are the positions at
   which its footprint lies just outside the texture rectangle,
   touching it.
3. The cells of a ring are tried counter-clockwise, side by side in the
   cyclic order top, left, bottom, right, where
   - top is `(R - 1, T)`, `(R - 2, T)`, ... `(L, T)`;
   - left is `(L, T - 1)` down to `(L, B)`;
   - bottom is `(L + 1, B)` up to `(R, B)`;
   - right is `(R, B + 1)` up to `(R, T)`.
   The side the search starts with depends on the rally point
   `(rx, ry)` relative to the building's position `(bx, by)`, with
   `dx = rx - bx`, `dy = ry - by`:

   | Rally point | First side |
   |---|---|
   | `dx >= 0`, `dy >= 0` (also the building's own position) | top |
   | `dx < 0`, `dy >= 0` | left |
   | `dx < 0`, `dy < 0`, or no rally point | bottom |
   | `dx >= 0`, `dy < 0` | right |

4. A cell is free when the unit's footprint at that cell is passable
   (SPEC-0020, SPEC-0022) and not occupied by a standing unit
   (SPEC-0021). The first free cell is taken and the unit is placed at
   the centre of that cell. If no cell of the ring is free, the next
   ring is tried.

So without a rally point the footmen of a barracks at `(bx, by)`
appear at `(bx - 176, by - 208)`, then, while the earlier ones still
stand there, at `bx - 112`, `bx - 48`, `bx + 16`, ... (one footprint
further each time) on the same row. With a rally point to the north
or east the first place is `(bx + 208, by + 240)`, to the south-east
`(bx + 240, by - 176)`, to the north-west `(bx - 208, by + 208)`. For
the town hall (16 x 16 cells) the first places were
`(bx - 240, by - 272)` without and `(bx + 272, by + 304)` with a rally
point to the north.

### Rally point

The rally point of a building is set with
`IssuePointOrder(building, "setrally", x, y)` or, from the interface,
by a right click on the ground with the building selected (replay
action `0x12`, order 851971 `smart`, with the point and no object).
A unit trained by a building with a rally point gets the order 851970
(`OrderId2String` gives an empty string for it) right after it is
created, walks to the rally point by the rules of a `move` order
(SPEC-0018, SPEC-0020) and has order 0 once it has arrived (it stopped
within the arrival radius of the point: (587.86, -3000.57) for the
point (600, -3008)).

## Edge cases

- Units created by `CreateUnit` from a script take their food at
  once, also above the cap (test RTU: a footman, a peasant and three
  more footmen with one farm gave food 2/6, 3/6, 9/6), and a farm
  created by the script raises the cap at once (9/6 -> 9/12 in the
  same call). Only training waits for food.
- The first unit of a queue that starts at once is paid before its
  start event: the event already sees the lower gold and the higher
  food used.
- A unit that waits for food does not delay the units behind it in
  the queue from being paid (all were paid when ordered), but they
  wait behind it.
- Cancelling with only queued units removes the last one; the unit in
  training goes on.
- A trained unit that is placed where earlier units stand moves the
  place along the ring; units that walked away to a rally point free
  their place, so with a rally point every unit appeared at the same
  place.

## Open questions

1. Other buildings and units of other size classes (the ring rule was
   only seen for class 1 units), and units that cannot be placed at
   all.
2. Cancelling an item in the middle of the queue from the interface,
   and the replay action for it.
3. A rally point on a unit or a destructable.
4. What else a training changes in the checksum parts (SPEC-0024)
   besides the new unit; the parts per turn of `c-RTP-1` are given.
5. Training in a building under construction or being upgraded, and
   the food of units of other players.

## Sources

- Test maps on 1.27b under Wine, 2026-10-19, logged every tick (gold,
  lumber, food, orders, events with the game time of a reference
  timer): RTN (two barracks, one food-limited queue, one cancel), RTO
  (queue of 8 orders, cancel of the unit in training, positions on a
  flat map), RTQ and RTR (rally points in eight directions, town
  halls), RTT (waiting for food, not enough gold), RTS (random
  generators).
- A replay with mouse clicks, `c-RTP-1.w3g`.
- Debugger on the running game: the movement clock at the start and
  end events, the random generator calls.

## Verification

1. **Timing** (RTO): one barracks at (0, -3008), seven footmen ordered
   at 1.5 s (tick 301): start events at once and at each end; ends in
   ticks 4302, 8303, 12306, 16308, 20310, 24312, 28319, each with the
   movement clock values listed under "Timers of the engine". A second
   barracks given one footman ended in tick 4302 as well.
2. **Queue and costs** (RTO): gold 5000 -> 4865 at the first order and
   135 less for each of the next six; the eighth order returns false
   and costs nothing.
3. **Cancel** (RTO): the one footman of the second barracks cancelled
   at 6.0 s: gold back +135 and food 4/60 -> 2/60 in the next tick.
   (RTN): of two queued footmen the second is cancelled, food stays.
4. **Food** (RTN, food cap 12): the sixth footman of a queue does not
   start when food is 12/12; (RTT, cap 6): the fourth starts inside the
   `KillUnit` of one of the three trained footmen.
5. **Places** (RTO, barracks at (0, -3008), no rally point): the seven
   footmen at x = -176, -112, -48, 16, 80, 144, 208, y = -3216. (RTQ,
   RTR): first places for the rally directions as in the table above.
6. **Replay** `c-RTP-1.w3g`: the rally point at (13.787, -464.71) is to
   the south-east of the barracks at (0, 0), so the footman appears at
   (240, -176), takes order 851970 and walks to the point; random
   generator 2 is drawn once at its creation (RTS: one call, movement
   clock 23.999949 for a footman ordered at 4.0 s).
