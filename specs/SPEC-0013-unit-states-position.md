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
every tick. All 60 measured ticks match. For heroes the first tick after
the set gives a slightly smaller step (see open questions);
`SPEC-0013-regen-samples.tsv` lists the measured values.

### Facing

- `CreateUnit` takes the facing in degrees modulo 360 into 0..360:
  400 -> 40, -90 -> 270, 720.5 -> 0.5.
- The stored facing goes through a conversion: 33.3 reads back as
  33.299995.
- `SetUnitFacing` turns the unit over time at its turn rate; right after
  the call `GetUnitFacing` still gives the old value, one second later
  the new one.

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

1. The first regeneration step of heroes after `SetUnitState` is smaller
   than the following ones (paladin life +867 units of the last place,
   then 884 or 885 per tick).
2. Turn rate and the exact facing conversion.
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
