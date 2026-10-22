---
id: SPEC-0046
title: Brilliance Aura (Archmage)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, buff level, mana and positions read every tick)"]
---

## Behaviour

Brilliance Aura (`AHab`, passive) raises the mana regeneration of
friendly units around the Archmage, the Archmage included. The buff is
`BHab`.

### Data

From `AbilityData.slk` (1.27b patch): `Area` 900 at all levels; bonus
`DataA` 0.75 / 1.5 / 2.25 mana per second at levels 1 / 2 / 3; targets
air, ground, friend, self, vulnerable, invulnerable.

### Effect on regeneration

While a unit has the buff, its mana regeneration rate per second is its
own rate plus `DataA`. The rate is applied tick by tick with the
arithmetic of SPEC-0013 ("Regeneration"). Measured over 10 s, from
0 mana:

| Unit | No aura | Level 1 | Level 2 | Level 3 |
|---|---|---|---|---|
| Priest (`regenMana` 0.6667) | 0.66657 | 1.41646 | 2.16634 | 2.91623 |
| Archmage (0.05 per point of intelligence) | 0.94986 (int 19) | 1.69975 (int 19) | 2.74959 (int 25) | 3.79943 (int 31) |

The bonus starts in the tick the buff appears and ends in the tick it
is removed.

### Pulses

The aura works in pulses every 100 ticks (0.5 s), counted from the
moment the hero learned the ability. The first pulse comes 1 to 2 ticks
after learning (learned at script tick 136: pulses at 138, 238, ...).
Number the pulses 0, 1, 2, ... At each pulse:

1. On pulses 0, 4, 8, ... (every 2 s): every unit that has this aura's
   buff and was **not** marked since the previous such pulse loses the
   buff (it is gone in the next tick). All other buffed units lose
   their mark.
2. Every friendly unit whose distance from the Archmage is smaller than
   `Area + collision` (its own collision size) gets the buff, if it
   does not have it, and is marked.

So a unit that walks in gets the buff at the next pulse. A unit that
walks out keeps it for 2 to 4 s: until the second "every 2 s" pulse
after the last pulse that found it in range.

### Range

A priest (collision 16) at 910.16 from the Archmage had the buff, one
at 916.25 did not: the bound is `900 + 16`, as for Blizzard (SPEC-0043).

## Edge cases

- Several Archmages with the aura at different levels were not tested
  (which buff level wins, and whether the bonuses add).
- Units that are not the Archmage's player's (allies) were not tested.

## Open questions

1. The 1 or 2 tick offset of the first pulse after learning (the two
   tests differ by one tick, which may be the script timer).
2. The pulse origin for an Archmage that already has the aura when the
   map starts or that is revived.

## Sources

- Test maps c-RZU, c-RZV and c-RZW on 1.27b under Wine, single player,
  2026-10-25, in the analyst workspace: Archmages with the aura at
  levels 0 to 3, priests (abilities removed, 0 mana) at fixed distances
  and walking in and out; buff level, distance and mana logged every
  0.005 s.

## Verification

1. Priests at 100 .. 910 from a level 1 Archmage gain 14.16 mana in 10 s
   (from 1000 to 3000 ticks); at 916 and beyond 6.67.
2. Aura learned at script tick 0, eight priests walking in from 1100 at
   speed 270: those inside 916 at tick 601 get the buff at 601, the one
   that crossed at 609 gets it at 701. Five of them walk out, crossing 916
   between 2217 and 2372. All five lose the buff at 2802.
3. Same with the aura learned at tick 136: buff at 638, lost at 2939.
