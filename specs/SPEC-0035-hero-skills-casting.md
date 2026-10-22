---
id: SPEC-0035
title: Hero skills, learning and casting a targeted ability
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, values read by JASS natives every tick)"]
---

## Behaviour

Heroes learn abilities with skill points and cast them with orders.
This spec covers the learning rules and the timing of a unit-targeted
ability, measured on the paladin's Holy Light (`AHhb`, order
`holybolt`, order id 852092). Ability data are in
`Units\AbilityData.slk` (`levels`, `reqLevel`, `levelSkip`, and per level
`Cool`, `Cost`, `Rng`, `DataA`, ...); the cast timing of the caster is
`castpt` and `castbsw` of `Units\UnitWeapons.slk` (paladin 0.5 and 1.67
s, the same in `Custom_V1`).

### Skill points and learning

- A hero has one skill point per level (1 at level 1, SPEC-0034), and
  `SelectHeroSkill` spends one to learn an ability or raise its level.
- Level `k` of an ability needs hero level `reqLevel + s * (k - 1)`,
  with `s` the ability's `levelSkip` if it is not 0, else
  `HeroAbilityLevelSkip` of `Units\MiscGame.txt` (2). Holy Light
  (`reqLevel` 1, `levelSkip` 0): levels 1, 2, 3 at hero levels 1, 3, 5.
  Resurrection (`AHre`, `reqLevel` 6, one level): at hero level 6.
- A call that the rules do not allow spends nothing.

Measured (test RYA): a paladin raised level by level with `SetHeroLevel`
and, at each level, four attempts each of `AHhb` and `AHre`: the ability
levels and skill points read after every call followed these rules at
every step (Holy Light 1 at level 1, 2 at 3, 3 at 5; Resurrection 1 at
6; skill points left 6 at level 10).

### Casting on a unit

With the order given in tick `o` (script order, SPEC-0019):

1. **In range** (the target within `Rng` plus both collision sizes,
   the range test of SPEC-0019 with `Rng` in place of `rangeN1`) and
   facing it: the cast starts at once; the effect comes `castpt` later
   (100 ticks for 0.5 s): Holy Light healed 200 (`DataA1`) and the 65
   mana (`Cost1`) were taken in tick `o + 100`. The mana is paid at the
   effect, not at the order.
2. **Out of range:** the hero walks towards the target as for an
   attack (SPEC-0019) and stops at the range test (with `Rng` 800 and
   collisions 32 and 31: stopped 860.7 from the footman); the effect
   came 99 ticks after its last position change.
3. After the effect the order stays for `castbsw` (334 ticks for
   1.67 s; order 852092 until `o + 100 + 334`), then it is 0.
4. The cooldown (`Cool1`, 5 s) starts at the effect; an order given
   while it runs does nothing (no effect, no mana; the order given 400
   ticks after the effect).
5. **In range but not facing:** the order gives a movement towards the
   target as for an attack (SPEC-0019): a stop commit at the order and a
   cluster whose place is the target's per-tick position then. At its
   member steps the hero stands and turns (at most `turnRate` per update)
   until it is within 0.2 rad of the place's direction. At that update
   the movement ends and the cast starts; the effect follows `castpt`
   later. Read in the game, `c-RZH-1`, Mountain King's Storm Bolt on
   creep 8:
   - order in tick 801 (queue time 4.0), place (388.4560, 384.3517) in
     cells;
   - 805: one turn from 6.168117 to 0.090873, the place's direction;
   - 811: within 0.2, so the cast starts (`castpt` 0.4 s);
   - 891: the effect; the cooldown (9 s) starts.
6. **After the backswing** the order ends (order 0), and an order the
   hero had before the cast (an attack-move) is not resumed. Its
   acquisition check runs 0.01 s later, as for an idle unit (SPEC-0019).
   Read in `c-RZH-1`: Storm Bolt's backswing (0.5 s) ended at 991;
   check at 993 took creep 7.

## Edge cases

- **The target dies before the effect.** The cast ends without an
  effect at the moment the effect was due. No mana is spent and the
  cooldown does not start, so a new cast can follow at once. `c-RZJ-1`:
  Holy Light ordered on footman 5 at 3101 (order from 3102). The footman
  dies at 3142, and the order ends at 3228 with the mana unchanged
  (245.45 at 3227, regeneration only). A new Holy Light, ordered at
  3301, has its effect at 3425, and the 65 mana are spent there.
- An order given exactly when the cooldown should have run out (1000
  ticks after the effect, in a script timer of that tick) also did
  nothing; whether the cooldown ends a tick later or the order is
  refused for another reason is open.

## Open questions

1. Turning before a cast (`castpt` from the moment the hero faces the
   target, as for attacks?), and the reaction delay.
2. Other target kinds (point, no target, area) and channelled
   abilities, and what interrupts a cast.
3. The effects of the other hero abilities.

## Sources

- `Units\AbilityData.slk`, `Units\UnitWeapons.slk`, `Units\MiscGame.txt`
  of 1.27b.
- Test map RYA on 1.27b under Wine, 2026-10-21: learning attempts with
  `SelectHeroSkill`; two paladins ordered by script to cast Holy Light
  on paused, damaged footmen 300 and 1200 away; mana, the target's life,
  the order and the position read every tick.

## Verification

1. A paladin of level 1 learns Holy Light; `SelectHeroSkill` for level 2
   fails until hero level 3.
2. Paladin 300 from a footman with 100 life, `holybolt` in tick `o`:
   life +200 and mana -65 in tick `o + 100`; current order 852092 until
   `o + 434`.
