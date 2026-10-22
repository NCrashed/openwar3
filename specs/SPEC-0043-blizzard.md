---
id: SPEC-0043
title: Blizzard (Archmage)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, damage events and orders read every tick)"]
---

## Behaviour

Blizzard (`AHbz`, order `blizzard`) is a channeled point spell of the
Archmage. While the caster channels, waves of damage fall on a circle
around the target point.

### Data

From `AbilityData.slk` (1.27b patch):

| Level | Waves (`DataA`) | Damage per wave (`DataB`) | Shards (`DataC`) | Building factor (`DataD`) | Max damage per wave (`DataF`) | Area | Range | Cooldown | Mana |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6 | 30 | 6 | 0.5 | 150 | 200 | 800 | 6 | 75 |
| 2 | 8 | 40 | 7 | 0.5 | 200 | 200 | 800 | 6 | 75 |
| 3 | 10 | 50 | 10 | 0.5 | 250 | 200 | 800 | 6 | 75 |

The shards are the visible ice; they do not decide who is hit (all
units in the area take the same damage, see below).

### Timing

Times are in game ticks of 0.005 s, counted from the spell effect
(the moment `EVENT_UNIT_SPELL_EFFECT` fires; for an Archmage that does
not need to turn it is 60 ticks after the order).

- Wave `k` (`k = 1 .. DataA`) falls at effect `+ 180 + 200 * (k - 1)`:
  the first 0.9 s after the effect, then one per second. The logs show
  some waves one tick later (1029 instead of 1028, 840 instead of 839);
  they were read with a script timer of 0.005 s, which can lag a tick.
  To be checked against the game clock.
- The channel (current order `blizzard`) ends about 21 ticks after the
  last wave: at effect `+ 1201` or `+ 1202` for 6 waves. Then the
  caster has no order.
- The 75 mana are spent at the effect (c-RZJ-1: 251.87 read at tick
  648, 176.88 at tick 649).

### Who is hit

At each wave, every unit whose distance from the target point is
smaller than `Area + collision` is hit, where `collision` is the
unit's own collision size (`UnitBalance.slk`): footman (31) at 229.9 is
hit and at 231.1 is not; peasant (16) 215.0 / 216.1; ogre (48)
247.0 / 248.1. The test is made again at each wave, so a unit that
walks out of the circle stops taking damage.

All players' units are hit: enemies, neutral passive units and the
caster's own units (in c-RZJ-1 two footmen of the Archmage's player).
Buildings are hit too.

### Damage per unit

Let `n` be the number of units hit by the wave. Each unit takes

    d = min(DataB, DataF / n)

and a building takes `d * DataD`. Examples (`GetEventDamage`, level 1
unless noted):

| Units hit | Damage each |
|---|---|
| 4 footmen | 30.00 |
| 9 footmen | 16.67 (150 / 9) |
| 13 footmen | 11.54 (150 / 13) |
| 9 knights, level 3 | 27.78 (250 / 9) |
| a farm and a footman | farm 15.00, footman 30.00 |

The amounts reported to the damage event are not lowered by the
target's armour (footman, knight, farm as above).

## Edge cases

- **Paused units** (`PauseUnit`) take the damage of the first wave
  only. Later waves skip them. Unpaused units next to them take all
  waves.
- A unit exactly at distance `Area + collision` was not tested.

## Open questions

1. Whether a building counts in `n` and whether the cap applies before
   or after the building factor (only one mixed case, below the cap).
2. What ends the channel early (a new order, stun, silence, death) and
   whether the waves already scheduled still fall.
3. Whether the caster itself is hit when it stands in the circle; air
   units; invulnerable units (Divine Shield).
4. The exact tick of the channel end (1201 or 1202 after the effect).

## Sources

- Test maps c-RZO and c-RZP on 1.27b under Wine, single player,
  2026-10-25, in the analyst workspace: six Archmages casting at
  groups of neutral passive units (paused and unpaused), with damage
  events and orders logged every 0.005 s.
- Replay c-RZJ-1 (game-data/1.27b/replays), two casts in a fight.

## Verification

1. c-RZJ-1.log.tsv: cast 1 effect at tick 648 on (250, 150), waves at
   828, 1029, 1229, 1429, 1629, 1829 (30 to each unit in the circle),
   channel end 1850; cast 2 effect 1961, waves 2141 .. 3141, end 3162.
   The waves of cast 2 hit units 11, 14 (creeps) and 2, 5 (the
   Archmage's own footmen).
2. Nine footmen in a 3 x 3 grid with spacing 64 at the target point:
   16.67 each per wave for 6 waves.
3. Nine knights in a 3 x 3 grid with spacing 96, level 3: 27.78 each
   for 10 waves.
4. Footmen placed at distances 227 .. 235 from the target point: hit
   up to 229.9, not from 231.1.
