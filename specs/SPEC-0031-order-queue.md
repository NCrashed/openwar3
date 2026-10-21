---
id: SPEC-0031
title: Queued orders (Shift)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (replays built with chosen actions, logged every tick)"]
---

## Behaviour

An order given with Shift held is written with bit `0x0001` of the
order's flags set (SPEC-0028, "Orders from the interface"); with
several units selected bit `0x0008` is set as well (flags 9). Such an
order is **queued**: the unit carries it out after the orders it
already has. Orders without bit `0x0001` are not queued. Ticks, updates
and the movement rules are those of SPEC-0018 and SPEC-0019. Measured
with footmen on open ground; scripts cannot queue orders, so all tests
are replays with the order actions of SPEC-0024 written into chosen time
slots.

### The queue

- Each unit has its own queue of orders behind its current order. A
  queued order given to a unit **without a current order** (idle,
  current order 0) is carried out at once, like an order without Shift.
- A queued order given to a unit with a current order is put at the
  **end** of its queue. Nothing changes in the unit's motion when it is
  added.
- An order **without** bit `0x0001` replaces the current order as in
  SPEC-0019 and **empties the queue**: a footman walking to a point with
  one more point queued, given a plain `move` elsewhere, went to the new
  point and never to the queued one.
- There was no limit up to 21 orders: a `move` and 20 queued `move`s
  given in 21 consecutive time slots were all carried out, in the order
  given.

### Passing from one order to the next

When the current order ends (for a `move`, at the update at which the
unit arrives, SPEC-0018), the first queued order becomes the current
order and starts as a new order given at that update: the unit stands
while it turns towards the new goal (SPEC-0019, "Stop, hold position and
new orders while walking") and then walks. The current order reads the
id of the new order from then on; between two `move`s it stays 851986
throughout. Measured stands at the passing points:

| Turn at the point | Stand |
|---|---|
| 90 degrees (square: four `move`s, single footman) | 12 ticks at each corner |
| about 118 degrees (zigzag, points 60 apart along x, 100 across) | 18 ticks at each point |
| about 150 degrees (the first point of the zigzag) | 24 ticks |
| about 35 degrees (group, see below) | 6 ticks |

The unit passes to the next order where it arrived, within its arrival
distance of the point (in the zigzag 15.0 from each point); it does not
walk on to the point itself.

### Orders that do not end

An order that has no end of its own keeps the rest of the queue
waiting:

- **`patrol`** queued after a `move`: at the end of the `move` the
  current order became `patrol` (851991) and the footman patrolled
  between the end of the `move` and the patrol point; the `move` queued
  after the `patrol` was never carried out (until a plain `stop` 5 s
  later emptied the queue).

### Other kinds of queued orders

- **`attack` to a point** (attack-move) queued after a `move`, with no
  enemy near: carried out like a `move` (current order 851983), and at
  its end the next queued `move` started (current order 851986 directly,
  without 0 in between).
- **`stop`** queued between two `move`s does not empty the queue: at the
  end of the first `move` the footman stood for 24 ticks (a 90 degree
  turn otherwise gives 12) and then carried out the second `move`. The
  current order read 851986 through all of it.
- **`holdposition`** queued after an attack-move: at the end of the
  attack-move the current order became 0, which is also what hold
  position reads (SPEC-0019); the footman stood.
- A plain `move` to a point within the arrival distance, given to an
  idle unit, with an attack-move queued right after it in the next time
  slot: the `move` ended at the unit's next update (SPEC-0019) and the
  current order was 0 for the next 14 ticks; then the attack-move
  started.

### Groups

A queued order given to several units (flags 9) is queued by every
member. A group `move` (flags 8) and a queued group `move` (flags 9) to
a point 400 further: each member, when it arrived at its place of the
first order (SPEC-0025), stood for one update and walked on to its place
of the second order, each at its own time (the four footmen passed at
ticks 13676, 13736, 13742 and 13790).

Every member's queued order has the point of the click as its target.
The members do not start it together: each starts it when its own
previous order ends, and each time one more member starts it the game
lays the group out again (SPEC-0025) from the members that have started
it so far, with their positions at that moment, and makes a new
movement cluster for them with the click point as its goal; the
members already under way change to their new places. Read in the game
(replay `c-RTW-3`, a queued group `smart` to a point for seven footmen,
cluster goals and member offsets at every update): the first footman to
finish the first order walked to the click point itself (a group of
one, offset 0), the second made a group of two (offsets +-(2.3155,
-1.8076) cells), and so on until the seventh made the final layout of
all seven, 174 ticks after the first. In the test of the four footmen
above the final layouts came close to the one obtained "as if the
members stood at their places of the previous order" (within 8 to 15),
probably because the late members were still near their places when the last one
started.

## Edge cases

- Shift with nothing queued and no current order is the same as no
  Shift.
- The current order does not show the queue: between orders of the same
  kind (`move`, `move`) `GetUnitCurrentOrder` does not change at the
  passing.

## Open questions

1. Queued orders on units (`smart` on an ally = following, attack on a
   unit, `harvest`), building and training orders with Shift, and items.
2. The exact stand at a passing point (a function of the turn only, or
   also of the update phase), and the 14 ticks with order 0 after a
   `move` that ended at once.
3. Whether a queued `holdposition` or `stop` behaves like the plain one
   in all respects (acquisition), and a queue given to a unit while it
   holds position.
4. The largest number of queued orders, and queued orders given to a
   group of several clusters.

## Sources

- Replays `c-RVL-1.w3g` and `c-RVM-1.w3g` on 1.27b under Wine,
  2026-10-20, on the test map `c-RTW.w3x` (16 footmen, two riflemen, a
  knight, a peasant): selection and order actions written into chosen
  time slots of an empty replay of the map; played back with a copy of
  the map whose script logs the position, facing and current order of
  every unit at every tick (the logs are `.positions.tsv` next to the
  replays).
- Replay `c-RTW-3.w3g` (orders with Shift given with the mouse, turns
  168 and 255), played back the same way (`c-RTW-3.positions.tsv`).

## Verification

Footman 825 (creation 828) at (-400, -150) in `c-RVL-1`:

1. `move` to (-400, -700) takes effect in tick 806; queued `move`s to
   (0, -700), (0, -300), (-400, -300). The footman stands 12 ticks at
   about (-400, -685), (-12, -700) and (0, -312) (ticks 1204 to 1215,
   1504 to 1515, 1804 to 1815) and reaches the last point with order 0
   in tick 2104; its current order is 851986 from 806 to 2103.
2. Queued `move` to (-800, -300) while idle takes effect at once (order
   851986 in tick 2616); a queued `move` to (-800, 100) and then a plain
   `move` to (-400, 100) in tick 2816: the footman turns towards
   (-400, 100) and never goes to (-800, 100).
3. In `c-RVM-1` the footman stands 18 ticks at 20 points of the zigzag,
   each 15.0 from its point, and ends with order 0 at (790.8, -988.2).
