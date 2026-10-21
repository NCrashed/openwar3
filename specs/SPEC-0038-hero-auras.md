---
id: SPEC-0038
title: Hero auras (devotion, endurance, unholy): reach, timing, effects
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, buffs and positions read every tick)"]
---

## Behaviour

An aura ability of a hero (`AHad` devotion, `AOae` endurance, `AUau`
unholy, and the others of the same kind) gives its buff (`BuffID` of
`Units\AbilityData.slk`: `BHad`, `BOae`, `BUau`) to the units allowed
by its `targs` (here: friendly ground units and the hero itself) within
`Area` (900) of the hero. Level 1 values are used below.

### Who gets the buff

- The hero has its own buff from the moment it learns the aura.
- Footmen standing 300 and 880 from the hero's centre got the buff, one
  at 944 never did (the test placed it at 920; it was moved to 944 by
  the placement of units). A footman walking past the hero got it at
  the first check after its centre came within 900 (885 at that check).
- The buff is given at checks every 0.5 s: in all tests every gain came
  in the tick of such a check (script ticks 101, 601, 3401, 3501, ...).

### Losing the buff

Buffs are taken away only at a second kind of check, every 2 s (script
ticks 402, 802, 1202, ... of the tests, i.e. game time 3.0, 5.0, 7.0 s
...; the 0.5 s checks that give the buff fall on ticks 101, 201, ...).
A unit loses the buff at the second of these 2 s checks that come after
it left the area (its centre more than `Area` from the hero's centre).
Test RYR (a paladin standing, nine footmen walking out of its aura at
different times): all nine lost the buff exactly so (they left at
ticks 470 to 1751 and lost it at 1202 to 2402), and so did the units
of RYE, RYF and RYG that walked away from a standing hero or that a
walking hero left behind, 10 of 12 cases. In the other two (RYE and
RYG, a hero walking away from a footman that stands) the buff went one
2 s check later than this rule says.

### Effects

- **Devotion aura:** armor + `DataA` (1.5). Measured with
  `UnitDamageTarget(hero, footman, 100.0, false, false,
  ATTACK_TYPE_CHAOS, DAMAGE_TYPE_NORMAL, ...)` every second: 82.64 with
  the buff (armor 2 + 1.5: 100 / (1 + 0.06 * 3.5)), 89.29 without.
- **Endurance aura:** movement speed + `DataA` (10 percent) and attack
  speed + `DataB` (5 percent, not measured): a walking footman's step
  went from 1.350 to 1.485 per tick (270 -> 297). The new speed starts
  at the unit's next update after the check (buff at the check of
  script tick 601, first faster step in 605).
- **Unholy aura:** movement speed + `DataA` (10 percent): the same
  1.485 per tick; the life regeneration `DataB` (0.5) was not measured.
- Endurance and unholy auras did not change the damage of the test.

## Edge cases

- The buff stays on units that walked out of the area for several
  checks (above), so an aura still counts in a fight for a few seconds
  after the hero has left.

## Open questions

1. The two cases of a walking hero that ended one 2 s check late, and
   whether the reach counts collision sizes.
2. The phase of the 0.5 s checks (global or per aura), and the order of
   the checks against the units' updates in a tick.
3. Attack speed of endurance aura, regeneration of unholy aura, and
   the other auras (brilliance, trueshot, thorns, command, vampiric).

## Sources

- `Units\AbilityData.slk` of 1.27b.
- Test maps RYE (paladin, devotion), RYF (tauren chieftain, endurance)
  and RYR (devotion, nine footmen leaving the aura one after another)
  and RYG (death knight, unholy) on 1.27b under Wine, 2026-10-21: the
  hero at (0, 0), footmen at 300, 880 and 920, one walking past at
  y = 300, the hero walking away after 15 s; buffs (by
  `GetUnitAbilityLevel` of the buff), positions and damage written by
  the map's script every tick.

## Verification

1. A paladin with devotion aura and a footman 300 away: within 0.5 s
   the footman has `BHad`; `UnitDamageTarget` of 100 chaos damage does
   82.64.
2. A footman walking at 270 through a tauren chieftain's endurance aura
   walks 1.485 per tick while it has `BOae`.
