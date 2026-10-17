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
- `SetUnitFacing` turns the unit over time. `GetUnitFacing` changes only
  every 6 ticks (0.03 s), the first time 3 ticks after the call. The
  turn speed is set by the `orientInterp` column of `Units\UnitData.slk`,
  not by `turnRate` (custom types with `turnRate` 0.1 to 3.0 turned
  exactly like the footman; `uloc` with `turnRate` 0.2 turns like the
  footman, which has 0.6, because both have `orientInterp` 0). Measured
  turn per update, in radians, while far from the target: it starts at
  `s`, grows by `s` per update and then stays at the maximum `m`:

  | `orientInterp` | Units tested | `s` | `m` |
  |---|---|---|---|
  | 0 | `hfoo`, `uloc` | 0.07 | 0.2 |
  | 1 | `hmtt`, `hgry` | 0.03 | 0.1 |
  | 3 | `nadk` | 0.005 | not reached in 2 s |
  | 4 | `hkni`, `Hamg` | 0.04 | 0.15 |

  Near the target the steps shrink and the facing snaps to the target
  once less than about 0.001 radians remain (footman, 0 -> 180: last
  values 179.954, then 180.0 after 0.705 s). The exact rule of this final
  phase is open.

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

1. The turning rule near the target, and the parameters of the other
   `orientInterp` values (2, 5..8).
2. A thread resumed after `TriggerSleepAction` that sets life and starts
   a 0.005 s periodic timer reads no regeneration at the first expiry
   (all unit types), unlike a thread run from a timer trigger.
3. Placement of new units on blocked ground.

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
