---
id: SPEC-0013
title: Unit life and mana, regeneration, facing and position
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

### Starting values

A new unit has full life. Max life follows SPEC-0009.

- Max mana: `manaN` of `Units\UnitBalance.slk` (or the map's value); for
  heroes plus 15 per point of intelligence (paladin, intelligence 17:
  255; demon hunter, 16: 240).
- Starting mana: `mana0` for non-heroes (sorceress: 75 of 200); heroes
  start with full mana.
- Units without mana read 0 for both.

### SetUnitState and SetWidgetLife

- `UNIT_STATE_LIFE`: values above max life are clamped to max life.
  A value of **0.405 or less kills the unit** (0.406 does not), also
  negative values. The death happens inside the call: death event
  triggers run before `SetUnitState` returns. A dead unit reads life 0
  and `IsUnitType(u, UNIT_TYPE_DEAD)` is true.
- `SetWidgetLife(u, x)` behaves the same for units.
- `UNIT_STATE_MANA`: clamped to 0..max mana. Units without mana stay at 0.
- `UNIT_STATE_MAX_LIFE` and `UNIT_STATE_MAX_MANA` cannot be set: the call
  does nothing (max life 420 stayed 420 after setting 500, and life 480
  was then clamped to 420).

### Regeneration

Life and mana regenerate continuously, every game tick (0.005 s of game
time, SPEC-0002), at a constant rate per second:

- Life of non-heroes: `regenHP`, when `regenType` allows it: `always`;
  `night` only at night (a demon hunter regenerated only its strength
  part at 12:00); `blight` only on blight (a ghoul off blight: 0).
- Life of heroes: `regenHP` (under the same `regenType` condition) plus
  0.05 per point of strength (paladin: 0.25 + 22 * 0.05 = 1.35 per s).
- Mana of non-heroes: `regenMana` (sorceress: 0.6667 per s).
- Mana of heroes: 0.05 per point of intelligence only; the `regenMana`
  value of the hero's data is not added (paladin: 0.85 per s, demon
  hunter 0.80, keeper of the grove 0.90).

The per-strength and per-intelligence constants are the game's gameplay
constants (0.05); the 25 life and 15 mana per point are likewise.

**Arithmetic.** For a footman the life read after `n` ticks since
`SetUnitState(LIFE, v0)` is `add(v0, mul(rate, e))` with
`e = mul(i2r(n), dt)` (or `dt` added `n` times; both give the same
values here), `rate = 0.25`, `dt = 0x3BA3D70A`: the value is computed
from the starting value and the elapsed time, not by adding a portion
All 60 measured ticks match; `SPEC-0013-regen-samples.tsv` lists the
measured values.

**Heroes: restart after an attribute change.** When a hero is created, and
whenever its strength changes (`SetHeroStr`), its life regeneration is
restarted 0.0001 s later (the shortest timer delay of SPEC-0002) at the
new rate; in between it does not regenerate. At creation the same holds
for mana. So the first step after creation, or after a strength change,
covers 0.0049 s instead of 0.005 s: paladin life +867 units of the last
place instead of 884 or 885, footman-based hero `'Hf00'` 160 instead of
163 or 164, paladin mana 4367 instead of 4456; after `SetHeroStr(30)` the
paladin's life step was 1123 instead of 1147, while its mana step was
normal. Non-heroes (footman, sorceress) and heroes whose state is set
0.5 s after creation show full first steps. That intelligence changes
restart mana regeneration in the same way is an inference, not tested.

**Night.** For `regenType` `night` it counts as night while the
clock runs from 18.01 to 6.01 hours (not from 18 to 6). With the clock
running (`GAME_STATE_TIME_OF_DAY` advancing, at time-of-day scale 0.5, 1
and 2), an archer (`earc`, `regenHP` 0.5) started to regenerate at the
first tick whose hour was above 18.01 (18.0101 to 18.0105, the tick
before at 18.00995 or 18.00999 had no gain) and stopped one tick after
the hour passed 6.01 (last tick with a gain at 6.0102 to 6.0105).
When the script jumps the hour with `SetFloatGameState` while the clock
is suspended (`SuspendTimeOfDay(true)`), the result is not a function
of the new hour alone: hours up to 6.001 and from 18.001 on always gave
night after a night hour, and 6.01 to 17.9 day after a day hour, but
for example 18.0 gave day after 12 and night after 0, 17.999 night after
18.001 and day after 0, and 18.001 day after 17.999 (open question 4).
In every such case the unit kept the same state for the whole second
measured.

**Hero attributes.** `SetHeroStr`, `SetHeroAgi` and `SetHeroInt` take
effect at once, with or without `permanent` (no difference was seen):

- Max life is `100 + 25 * strength` and max mana `15 * intelligence` for
  the paladin (base 22 strength: 650; 17 intelligence: 255), with the
  strength and intelligence including item bonuses. Agility changes
  neither.
- Life changes by the same amount as max life: from 325 of 650, strength
  22 -> 30 gave 525 of 850; a full hero stays full. If that would leave
  less than 1, the life becomes 1 (300 of 650 and strength 10: life 1 of
  350; also from 299, 301, 10 and 1 life). Mana changes in the same way
  but stops at 0 (100 of 255 and intelligence 2: 0 of 30; then
  intelligence 40: 570 of 600).
- Strength may go to 0 (max life 100) and below: at -5 the max life and
  the life become -25 and the hero dies inside the call (its death event
  runs before `SetHeroStr` returns; later reads give life 0).
- Setting the same value again changes nothing.
- Items with attribute bonuses (`rst1` +3 strength, `rin1` +3
  intelligence, `rag1` +3 agility) count at once when added with
  `UnitAddItemById`: `GetHeroStr(h, true)` 25 and `GetHeroStr(h, false)`
  22, max life 725, max mana 300. A dead hero takes no items.

### Facing

- `CreateUnit` takes the facing in degrees modulo 360 into 0..360:
  400 -> 40, -90 -> 270, 720.5 -> 0.5.
- The facing is stored in radians. Setting a facing `a` (degrees) stores
  `r = mul(a, 0x3C8EFA35)` (pi/180); if `|r| >= 2 pi` (`0x40C90FDB`),
  `r = mul(frac(mul(r, recip(2 pi))), 2 pi)` (`frac` keeps the sign,
  SPEC-0004); then, if `r < 0`, `r = add(r, 2 pi)`. `GetUnitFacing`
  returns `mul(r, 0x42652EE1)` (180/pi). This reproduces all 30 tested
  values bit for bit: 33.3 -> 33.299995 (`0x42053332`), 40 -> 39.999996,
  360 -> 359.99997 (not reduced: `mul(360, pi/180)` is just below 2 pi),
  400 -> 40.000019, -90 -> 270.0, -0.001 -> 359.99899, -1000.25 ->
  79.749939, 10000.7 -> 280.70065.
- `SetUnitFacing` sets the unit's heading (SPEC-0018) at once; the
  facing that `GetUnitFacing` returns then turns towards it over time.
  `GetUnitFacing` changes only at the unit's updates, every 6 ticks
  (0.03 s); the first change came 3 to 6 ticks after the call, depending
  on the phase of the update cycle. The turn speed is set by the
  `orientInterp` column of `Units\UnitData.slk` (field `uori`), not by
  `turnRate` (custom types with `turnRate` 0.1 to 3.0 turned exactly
  like the footman; `uloc` with `turnRate` 0.2 turns like the footman,
  which has 0.6, because both have `orientInterp` 0).
- `SetUnitFacing` on a unit walking a `move` order has no effect at all:
  footmen given `SetUnitFacing` 90, 200 and 0 (and 30 for another one)
  at ticks 100, 400 and 700 of their walk had bit for bit the same
  positions and facings at every tick as without the calls.

### Facing turn rule

At every update, let `r` be the angle from the facing to the heading
(the shorter way, after the heading's own turn of that update), and `w`
the facing step of the previous update (0 when the facing was at rest).

1. If `r` is below about 0.001 radians (snaps were seen up to 0.0573
   degrees, none at 0.0612), the facing is set to the heading and `w`
   becomes 0.
2. Otherwise the facing turns towards the heading by

       step = min(w + s, m, B(r))

   and `w` becomes `step`.

`s` (acceleration) and `m` (maximum step) per update, in radians, by
`orientInterp` (all nine values measured with footman copies that
differ only in `uori`; 0 to 5 are used by the game's units):

| `orientInterp` | `s` | `m` | Example units |
|---|---|---|---|
| 0 | 0.07 | 0.2 | `hfoo`, `hpea`, `hmil` |
| 1 | 0.03 | 0.1 | `hmtt`, `hgry`, `hmtm` |
| 2 | 0.015 | at least 0.285 (not reached within 180 degrees) | `hgyr`, `hsor` |
| 3 | 0.005 | 0.1 | ships (`hbot`, `hdes`), `nadk` |
| 4 | 0.04 | 0.15 | `hkni`, `Hamg` |
| 5 | 0.05 | 0.18 | `ogru`, `Hpal` |
| 6 | 0.1 | 0.3 | - |
| 7 | 0.003 | 0.08 | - |
| 8 | 0.001 | 0.05 | - |

`B` is a braking limit that depends only on `r` and the `orientInterp`
value. It is given as a measured table, `SPEC-0013-facing-brake.tsv`
(columns `orient_interp`, `remaining_deg`, `step_deg`, in degrees):
for each value, points from 0.057 degrees up to the largest remaining
angle at which the brake was ever the smallest of the three terms
(11.4 degrees for 0, 11.1 for 1, 18.1 for 2, 68 for 3, 68 for 4, 66 for
5, 45 for 6, 66 for 7, 80 for 8). Above that range `B` never limited the
step in any test, including full-speed turns of up to 180 degrees, so
it can be taken as unlimited there. Interpolate linearly in
`log r` / `log step`.

For `orientInterp` 3, 7 and 8 the table follows a closed form, with `r`
and the step in radians:

    B(r) = sqrt(C * r)            if r >= m
    B(r) = sqrt(C * r) * r / m    if r < m

with `C` = 0.0083333 (3), 0.0053333 (7) and 0.0017857 (8), that is
`m^2 / 1.2` for 3 and 7 and `m^2 / 1.4` for 8: a constant
deceleration of `C / 2` per update while far from the end, then a
faster fade. (`step^2 / r` is constant to six digits over the whole
first range.) For the other values the brake has a different shape and
only the table is available.

The rule with these tables reproduces, at every update, the facing of
495 measured turns (55 target angles from 0.3 to 180 degrees for each
`orientInterp` value) within 0.002 degrees, apart from 11 single updates
(a snap decided differently right at the threshold, and gaps in the
table for `orientInterp` 6). It also reproduces the facing that chases
a slowly turning heading during movement (SPEC-0018).

Footman, 0 -> 180 degrees: steps of 0.07, 0.14, then 0.2 radians, the
brake from about 7.5 degrees before the end, last values 179.954, then
180.0 after 0.705 s.

### Position

- `CreateUnit` places the unit at `floor(4x) / 4`, `floor(4y) / 4` on open
  ground (-1000.3 -> -1000.5, 100.37 -> 100.25, -2999.9 -> -3000).
- `SetUnitX`, `SetUnitY` and `SetUnitPosition` store coordinates relative
  to the world's lower left corner (`GetWorldBounds`, SPEC-0008) in the
  engine arithmetic: `x' = sub(add(x, -minX), -minX)` with truncation, so
  -1000.3 reads back as -1000.30078 and -1200.7 as -1200.70020 on a map
  with `minX = minY = -12288`.
- `SetUnitPosition` keeps the unit inside the world shrunk by 32 on each
  side: (50000, 50000) gave (12256, 12256).
- `SetUnitX` and `SetUnitY` do not clamp. `X` values from -30000 to
  50000 and `Y` values up to 50000 kept the unit alive and readable.
  **A `Y` of `minY - 32` or less (-12320 here) hangs the game** (-12319
  works). `CreateUnit` at (50000, 50000) also hung the game.
  An implementation should not hang; maps cannot rely on these cases.

## Edge cases

- Death through `SetUnitState` runs the unit's death triggers
  synchronously (see SPEC-0009 for the order).

## Open questions

1. A closed form of the brake for `orientInterp` 0, 1, 2, 4, 5 and 6,
   and `m` for value 2.
2. A thread resumed after `TriggerSleepAction` that sets life and starts
   a 0.005 s periodic timer reads no regeneration at the first expiry
   (all unit types), unlike a thread run from a timer trigger.
3. Placement of new units on blocked ground.
4. How jumps of the hour with a suspended clock decide between night
   and day (above), and the regeneration types `day` and `blight`.
5. What `permanent` of `SetHeroStr` and its kind changes.

## Sources

- Test maps on 1.27b under Wine, single player, 2026-10-17, in the
  analyst workspace. Values printed with `R2SW` and as bit patterns.

## Verification

1. Starting values and the `SetUnitState` cases above, with footman,
   sorceress, paladin, demon hunter and peasant.
2. Regeneration sampled every 0.005 s for 60 ticks:
   `SPEC-0013-regen-samples.tsv`; every 0.05 s for 2.5 s for six types
   (rates as listed).
3. Positions and facings as listed; `SetUnitY(u, -12319)` works,
   `SetUnitY(u, -12320)` hangs.
4. **Night.** Archer at 50 life, the hour set every second (clock
   suspended) to 12, 0, 5.99, 6.0, 6.01, 17.99, 18.0, 18.01, 23.99, 3,
   9, 21: gain 0 at 12, 6.01, 17.99, 18.0, 9; 0.4975 in the other
   seconds. Clock running from 17.95 and from 5.95: as above.
5. **Hero attributes.** Fresh paladins at the life given, then
   `SetHeroStr` to the value given: 300 / 10 -> 1 of 350; 650 / 10 ->
   350 of 350; 650 / 0 -> 100 of 100; 650 / -5 and 100 / -5 -> dies;
   10 / 21 -> 1 of 625; 0.5 / 22 -> unchanged; 650 / 23 -> 675 of 675;
   325 / 30 -> 525 of 850.

