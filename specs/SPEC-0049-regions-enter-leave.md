---
id: SPEC-0049
title: Regions and their enter and leave events
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps c-SAQ, c-SAR and c-SAS, values written by the map's script)"]
---

## Behaviour

### Regions are sets of cells

A region is a set of cells of 32 x 32 world units whose edges lie on
multiples of 32 (cell `(floor(x / 32), floor(y / 32))`).

- `RegionAddCell(r, x, y)` adds the one cell that holds `(x, y)`.
  After `RegionAddCell(r, 10, 10)`: (0, 0), (9, 10), (25.9, 10),
  (31.9, 31.9) are in; (32, 32), (42.1, 10), (-5.9, 10), (-0.01, -0.01)
  are out.
- `RegionAddRect(r, rect)` adds every cell from `floor(minx / 32)` to
  `floor(maxx / 32)` and from `floor(miny / 32)` to `floor(maxy / 32)`,
  both ends included. So the cell that holds the rect's maximum edge is
  in. A rect (0, 0, 256, 256) gives cells 0 .. 8 in x and y, so x from 0
  up to (but not including) 288. With y = 100: x = 0, 255.9, 256, 256.1
  and 287.9 are in; -0.1, -31.9 and -32.1 are out. (-0.01, -0.01) is out
  and (0, 0) is in.
- `IsPointInRegion` and `IsUnitInRegion` test the cell of the point, or
  of the unit's position.
- `TriggerRegisterEnterRectSimple(t, rect)` (Blizzard.j) makes a new
  region from the rect and registers the enter event on it. On the
  playable map rect, every unit created on the map enters it.

### When a unit enters or leaves

A unit enters a region when its position moves from a cell outside the
region to a cell inside it. It leaves the region on the opposite move.
The test is made:
- in `CreateUnit`, at the position where the unit is placed (after the
  placement has moved it off other units: asked (120, 120), placed at
  (80, 48), filter at (80, 48));
- in `SetUnitX`, `SetUnitY` and `SetUnitPosition`, at once, at the new
  position (`SetUnitPosition` may also place it elsewhere: asked
  (100, 100), placed at (144, 48));
- for a walking unit, at the commits of its path agent (SPEC-0018), with
  the committed position. It is not tested at the per-tick positions in
  between. Test c-SAQ: a footman walking east. Its per-tick position
  passed x = 0 in tick 631 (x 0.018), but the enter came in tick 633
  (x 2.683). It passed 288 between 854 (287.989) and 855 (289.329), and
  the leave came in 855. 633 and 855 are 37 update cycles of 6 ticks
  apart.

Other events:
- A hidden unit enters and leaves like any (`ShowUnit(u, false)`, then
  `SetUnitX`/`SetUnitY` into the region and out again: one enter, one
  leave). `ShowUnit` itself gives no event, also when showing moves the
  unit (shown inside at (144, 48) instead of (100, 100): no new enter).
- Death gives no leave event. A killed unit inside stays in the region
  (`IsUnitInRegion` true) as long as it exists.
- `RemoveUnit` gives no leave event. An enter that was queued before
  the removal (unit created inside and removed in the same script
  call) still runs. `GetTriggerUnit` then refers to the removed unit,
  which reads as hidden at its last position.
- Each crossing is its own event. A unit moved in and out three times in
  one script call got three enter and three leave events.

### Filter, conditions and actions

The filter of `TriggerRegisterEnterRegion` (and of
`TriggerRegisterLeaveRegion`) runs at the moment of the crossing,
inside the native that moved or created the unit. It runs once for each
registration whose region was entered:
- `GetFilterUnit` is the unit, with its state at that moment;
  `GetTriggerUnit` is null;
- for a unit created by `CreateUnit`, its user data is still 0 (a
  `SetUnitUserData` after `CreateUnit` has not run yet);
- if the filter returns false, that registration gets no event (a
  filter that accepts only `hfoo` let a footman through and dropped a
  rifleman);
- a null filter accepts every unit.

The conditions and actions run later, from a queue, and they read the
state at that later moment. For example, `GetUnitX` of a unit that was
moved in and out again reads the outside position, and
`IsUnitInRegion` reads false. `GetTriggeringRegion` is the region of the
registration, and `GetTriggerEventId` is the enter event (5) or the
leave event (6).

Order of the queued events, read in c-SAR (regions A and B, B created
first; triggers registered A, B, A with a filter, A+B; one call creating
a footman inside both regions, a rifleman inside both regions, and a
footman inside only B):
1. by region, the newest region first: all events of A, then all of B
   (in c-SAQ, too, the playable map region, made after A, came first);
2. within a region, the triggers in the order of their registration on
   that region;
3. within a trigger, the units in the order in which they entered.

So the result was: trigger 1 (footman, rifleman), trigger 3 (footman),
trigger 4 (footman, rifleman), all on A, then on B trigger 2 (footman,
rifleman, footman) and trigger 4 (footman, rifleman, footman). The leave
events of a call came after its enter events (c-SAQ).

When the queue runs: each event is put on the game's timer queue
(SPEC-0002) like a 0 s timer started right after its filter returned
(or at the crossing, with no filter). It runs in the timer phase of the
next tick, in the queue order of such a timer. Read in c-SAS (a 0.005 s
periodic counter `N`, started before everything else):
- footman created by `CreateUnit` in a timer callback in the tick of
  count 99, then timers of 0, 0.002, 0.005 and 0.0075 s started in that
  callback: next tick `N` (100), the enter action, then the 0, 0.002
  and 0.005 s timers, all in that tick; the 0.0075 s timer in the
  tick after;
- rifleman walking in, its enter found at a commit in the tick of count
  357; its filter started a 0 s and a 0.005 s timer before returning:
  next tick `N` (358), the filter's 0 s timer, the enter action, a
  periodic 0.005 s timer of the map started earlier, then the filter's
  0.005 s timer.

## Edge cases

- The rect's maximum edge belongs to the cell above it, so
  `RegionAddRect` covers up to 32 world units more than the rect in x
  and y.
- An event whose unit has left again before the queue runs still
  fires.
- `CreateUnit` inside a region fires both the region's enter and, for
  the playable map rect, that region's enter.

## Open questions

1. Whether the queue entry of an event is exactly that of a 0 s timer
   in all phases (only a timer callback and a walker's commit were
   read). c-SAK-1 logs the two Mirror Image units (created in the
   ability's timer at tick 7262) with the label 7262.
2. Leave filters were not tested with a filter; the y extent of
   `RegionAddRect` was tested only at (-0.01, -0.01) and (0, 0).
3. `RegionClearRect`, `RegionClearCell`, `RemoveRegion` and moving
   rects (`MoveRectTo` after the registration) were not tested.

## Sources

- Test maps c-SAQ, c-SAR and c-SAS on 1.27b under Wine, 2026-10-24:
  regions made by `CreateRegion`, `RegionAddRect` and `RegionAddCell`;
  footmen and a rifleman of `Player(0)` created, moved by `SetUnitX`, `SetUnitY` and `SetUnitPosition`, hidden,
  shown, killed, removed, and walking by a `move` order. The map's
  script logged `IsPointInRegion` tests, the filter calls, the
  conditions and actions, and the walker's position every tick.

## Verification

1. `RegionAddRect(r, Rect(0, 0, 256, 256))`: `IsPointInRegion(r, 287.9,
   100)` is true, `IsPointInRegion(r, -0.1, 100)` and
   `IsPointInRegion(r, 288, 100)` are false.
2. `CreateUnit` inside a region with an enter trigger: the filter runs
   inside `CreateUnit` (before the call returns), and the action runs
   later, after the script call has ended.
3. A unit moved in, out, in, out by `SetUnitX` in one call: two filter
   calls at the moments of entering, then two enter actions and two
   leave actions later, all reading the final (outside) position.
4. A unit walking into a region: the enter comes at a commit of its
   path agent, not at the first tick when its per-tick position is
   inside.
