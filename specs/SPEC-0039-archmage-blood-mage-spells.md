---
id: SPEC-0039
title: Blizzard, water elemental, flame strike (area and summon spells)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, damage events and orders read every tick)"]
---

## Behaviour

Level 1 of three human hero spells, cast by script orders on targets
in range (so no approach). Data: `Units\AbilityData.slk`; the casters'
`castpt` 0.3 s and `castbsw` 2.4 s (archmage `Hamg`, blood mage `Hblm`,
`Custom_V1\Units\UnitWeapons.slk`). Ticks below are counted from the
order (tick `o`); the cast point is `o + 60`. All damage amounts are
those of the damage events; footmen (armor 2) took them unreduced.

### Blizzard (`AHbz`, order `blizzard`, point)

- A channelled spell: the order (852089) stays until the last wave.
- `DataA` (6) waves, each doing `DataB` (30) to every unit in the area
  (`Area` 200 around the point): two footmen 70 from the point took 30
  in ticks `o + 240`, `o + 440`, `o + 641`, `o + 841`, `o + 1041` and
  `o + 1241` (one wave per second after the first, which comes 180
  ticks after the cast point); the order ended at `o + 1262`.

### Water elemental (`AHwe`, order `waterelemental`, no target)

- At the cast point the unit `UnitID` (`hwat`) appears 200 in front of
  the caster (archmage at (-90, -400) facing 0: elemental at
  (109.75, -400), facing 0), with `EVENT_PLAYER_UNIT_SUMMON` in that
  tick. It lasts `Dur` (60 s; not reached in the test).
- The caster's order (852097) stays until the cast point + `castbsw`
  (`o + 541`).

### Flame strike (`AHfs`, order `flamestrike`, point)

- The strike lands `Cast` (1.33 s, 266 ticks) after the cast point,
  `o + 326`; the caster's order (852488) ends in that tick.
- Units in `Area` (200) take `DataA` (15) at the landing and every
  `DataB` (0.33 s, 66 or 67 ticks) after it, 9 times in all (`o + 326`
  to `o + 855`: 326, 392, 458, 524, 590, 657, 723, 789, 855), then
  `DataC` (4) once a second, 6 times (`o + 1061` to `o + 2061`), all
  for two footmen 60 from the point.

### Paused units take periodic damage only once

Against units paused with `PauseUnit`, blizzard did its first wave and
flame strike its first hit only; the later waves and hits did nothing
(test RYH; the same spells against units that were not paused, test
RYI, did all of the above).

## Edge cases

- The paused-unit case above.

## Open questions

1. Where the first blizzard wave comes from (180 ticks after the cast
   point) and the shards (`DataC` 6) and the reduction against
   buildings (`DataD`), the damage cap `DataF` (150; 6 waves x 30 = 180
   was not capped against a footman).
2. Flame strike's `DataF` (90; 9 x 15 + 6 x 4 = 159 was not capped) and
   `DataE` (buildings).
3. The exact place of the water elemental (200 in front, 109.75 instead
   of 110) and its duration end.
4. Divine shield was not tested (the paladin was not attacked).

## Sources

- `Units\AbilityData.slk`, `Custom_V1\Units\UnitWeapons.slk` of 1.27b.
- Test maps RYH (targets paused) and RYI (targets of `Player(15)`, not
  paused) on 1.27b under Wine, 2026-10-21: heroes of level 5 with the
  spell learned, orders from a script timer, damage events, orders and
  summons written by the map's script every tick.

## Verification

1. Blizzard on two footmen: 30 damage each in six waves 200 ticks
   apart, the first 240 ticks after the order.
2. Flame strike: 15 damage 326 ticks after the order and 8 more times
   every 66-67 ticks, then 4 damage 6 times one second apart.
