---
id: SPEC-0040
title: Chain lightning, war stomp, mirror image, healing wave
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, generator states and positions read in the running game)"]
---

## Behaviour

Level 1 to 3 of four orc hero spells, cast by script orders on targets
in range. Data: `Units\AbilityData.slk`; `castpt` of the casters
(`Custom_V1\Units\UnitWeapons.slk`): far seer and shadow hunter 0.3 s,
tauren chieftain 0.47 s, blademaster 0.3 s. Ticks are counted from the
order (tick `o`). Targets were footmen of `Player(15)` (not paused).

### Chain lightning (`AOcl`, order `chainlightning`, unit)

- The first target takes `DataA` (85) 0.25 s (50 ticks) after the cast
  point (`o + 110`), then the lightning jumps to another unit every
  0.25 s, each jump doing `1 - DataC` (0.85) of the previous one:
  85.00, 72.25, 61.41 (to the units 144 and 172 apart), up to `DataB`
  (4) targets, within `Area` (500) of the last one.
- It jumps to the nearest unit not hit yet (centre to centre from the
  unit just hit). Test RYO (level 3: 180, `DataB` 8, six footmen): the
  order was the nearest unhit unit at every jump (158, 158, 234, 156
  and 389 away), with 180.00, 153.00, 130.05, 110.54, 93.96, 79.87.

### War stomp (`AOws`, order `stomp`, no target)

- At the cast point (`o + 94`) every enemy ground unit within `Area`
  (250) takes `DataA` (25) and is stunned for `Dur` (3 s; heroes
  `HeroDur` 2 s): order 851973 from that tick, back to 0 603 ticks
  later (as the stuns of SPEC-0037). When the stun ended, each unit had
  a damage event of 0.00 from the caster.

### Mirror image (`AOmi`, order `mirrorimage`, no target)

- At the cast point (`o + 60`) the hero is hidden. 0.5 s later
  (`DataD`, `o + 160`) the hero and `DataA` images (1, 2, 3 for levels
  1 to 3) appear on `n = DataA + 1` places around the hero's position
  `(x, y)` and facing `f`: place `k` (`k = 0 .. n - 1`) is at angle
  `t_k = f + 90 - k * 360 / n` degrees (counter-clockwise from the
  x axis; so place 0 is 90 degrees to the left of the facing and the
  places follow clockwise), at distance `Rng` (128):
  `X = x + 128 cos t_k`, `Y = y + 128 sin t_k`, and the unit stands at
  `(32 floor((X - 32) / 32) + 16, 32 floor((Y - 64) / 32) + 16)`.
  All 13 places of the tests (blademasters of levels 1, 2, 3 facing 0
  and one of level 3 facing 90) follow this exactly.
- The hero takes place `k = floor(r * n / 2^32)`, `r` one output of
  game generator 12 (SPEC-0007) drawn at that moment; the images take
  the others. Four casts, four draws, all four places as this rule.
  Places are counted in the order in which they are made. `c-SAK-1`
  (read in the game): the first draw of generator 12 in that game, from
  its seed state `a = 0xC5F37156`, `b = 0x901C14D4`, gave `r =
  0xEA8CF3FD`, so with `n = 2` the hero took place 1, (1200, -176), and
  the image place 0.
  After that draw come one draw of generator 2 and one of the script
  generator for each of the `n` units placed (test RYJ, one image: two
  such pairs in the same tick).
- The images last `Dur` (60 s) and are illusions (`EVENT_PLAYER_UNIT_
  SUMMON` for each).

### Healing wave (`AOhw`, order `healingwave`, unit)

- The first target is healed `DataA` (130) 50 ticks after the cast
  point, then the wave jumps to another wounded friendly unit every
  0.25 s, each time healing `1 - DataC` (0.75) of the previous amount:
  130, 97.5, 73.12, up to `DataB` (3) targets, never above the
  target's maximum life.
- It jumps to the most wounded friendly unit within `Area` (500) of the
  unit just healed, not the nearest; units with full life are left out.
  Test RYO (level 3: 300, `DataB` 5, six footmen of 420 life with 100,
  400, 50, 300, 420 and 200): healed 100 -> 400, then the one with 50
  (not the nearer one with 400), then 200, then 300, then 400, with
  300, 225, 168.75 and the capped 119.55 and 19.49. Lowest life and
  most missing life are the same here (one unit type).

## Edge cases

- The war stomp stun ends with a damage event of 0.00.

## Open questions

1. Ties in the jump choices (equal distance, equal wounds).
2. Shockwave, hex, serpent wards, feral spirit, wind walk,
   reincarnation, endurance aura's attack speed.
3. The placement of mirror images when a place is blocked.

## Sources

- `Units\AbilityData.slk`, `Custom_V1\Units\UnitWeapons.slk` of 1.27b.
- Test maps RYJ (all four spells) and RYK (four blademasters casting
  mirror image of levels 1, 2, 3 and 3, facing 0, 0, 0 and 90) and RYO
  (chain lightning and healing wave of level 3 among six footmen) on 1.27b
  under Wine, 2026-10-21: damage events, life, orders, units entering
  the map, and the generator states at each draw.

## Verification

1. Blademaster of level 3 at (-90, 400) facing 0 casting mirror image:
   the four units at (-112, 464), (16, 336), (-112, 208) and
   (-240, 336), the hero at place `floor(r * 4 / 2^32)` of generator 12.
2. Chain lightning on three footmen: 85, 72.25, 61.41 damage, 0.25 s
   apart, the first 110 ticks after the order.
