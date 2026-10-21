---
id: SPEC-0034
title: Hero attributes and what they give
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, values read by JASS natives, damage events)"]
---

## Behaviour

A hero has three attributes, strength (STR), agility (AGI) and
intelligence (INT), one of which is its primary attribute (`Primary` of
`Units\UnitBalance.slk`). The constants are in `Units\MiscGame.txt`
(`StrHitPointBonus` 25, `StrAttackBonus` 1.0, `IntManaBonus` 15,
`AgiDefenseBonus` 0.30, `AgiDefenseBase` -2, `AgiAttackSpeedBonus`
0.02, `StrRegenBonus` 0.05, `IntRegenBonus` 0.05).

**Which data.** The values come from the game data set of the map
(SPEC-0015, "Which copy"): the test maps, which have no melee flag,
use `Custom_V1\Units\UnitBalance.slk` and `Custom_V1\Units\UnitWeapons.slk`
of `War3Patch.mpq`, not `Units\UnitBalance.slk` of that archive (the
melee data). The two differ for heroes: the blademaster has `AGI` 23 in
`Custom_V1` and 24 in the melee copy, the death knight `def` 2 against 1;
the game used 23 and 2.

### Attributes by level

At level `L` an attribute is `floor(base + plus * (L - 1))`, with `base`
the fields `STR`, `AGI`, `INT` and `plus` the fields `STRplus`,
`AGIplus`, `INTplus`. The hero has `L` unspent skill points if none were
spent (`GetHeroSkillPoints`: 1 at level 1, 10 at level 10). Measured
for the paladin (22 + 2.7, 13 + 1.5, 17 + 1.8), the blademaster (18 + 2,
23 + 1.75, 16 + 2.25), the archmage (14 + 1.8, 17 + 1, 19 + 3.2) and the
death knight (23 + 2.7, 12 + 1.5, 17 + 1.8) at levels 1 to 10 set with
`SetHeroLevel`: all 120 values follow the formula (paladin STR 22, 24,
27, 30, 32, 35, 38, 40, 43, 46).

### Life, mana, armor

- Maximum life = `HP` + `StrHitPointBonus` x STR (paladin: 100 + 25 x 22
  = 650 at level 1, 1250 at level 10 with STR 46).
- Maximum mana = `manaN` + `IntManaBonus` x INT (paladin 255 with INT
  17; archmage 705 at level 10 with INT 47); a new or levelled hero's
  mana was at its maximum.
- Armor = `def` + `AgiDefenseBonus` x AGI + `AgiDefenseBase` (paladin
  2 + 0.3 x 13 - 2 = 3.9). Measured through the damage of
  `UnitDamageTarget(..., 100.0, true, false, ATTACK_TYPE_NORMAL,
  DAMAGE_TYPE_NORMAL, ...)`, which is 100 x 0.75 / (1 + 0.06 x armor)
  for a hero (0.75: the spells attack type against hero armor in the
  custom data set, SPEC-0015): 60.778 for the paladin at level 1 (armor
  3.9), 51.090 at level 10 (7.8), 57.960 for the blademaster (4.9),
  63.238 for the archmage (3.1), 61.678 for the death knight (3.6); all
  40 values agree to the printed four decimals.

### Attack

- Damage of a hit = primary attribute x `StrAttackBonus` + `dmgplus1` +
  the dice (`dice1` of `sides1`, SPEC-0015), before armor. Measured
  against paused peasants (armor 0): paladin level 1 (STR 22, 2 x 6)
  24 to 33, level 10 (STR 46) 50 to 54; blademaster (AGI 23, 2 x 12)
  33 to 43; archmage (INT 19, 2 x 4) 21 to 25; death knight (STR 23,
  2 x 6) 27 to 34; all within the ranges of the formula.
- The time between hits is `cool1 / (1 + AgiAttackSpeedBonus x AGI)`:
  paladin 2.2 s with AGI 13 -> 1.746 s, 349 or 350 ticks between swings
  (measured), 289/290 at level 10 (AGI 26); blademaster 1.77 s ->
  242/243 (AGI 23) and 201/202 (AGI 38); archmage 2.13 s -> 318 (AGI 17)
  and 280/281 (AGI 26); death knight 2.33 s -> 375/376 (AGI 12) and
  310/311 (AGI 25).
- The damage point shrinks the same way, `dmgpt1 / (1 + 0.02 x AGI)`:
  blademaster (0.33 s, AGI 23) 45 ticks from the start of the swing to
  the damage, mountain king (0.35 s, AGI 11) 57 (tests RYB, RYC, every
  swing of four heroes; the ticks the swing start draws generator 3,
  SPEC-0036).

## Edge cases

- `GetHeroStr(h, false)` and `GetHeroStr(h, true)` gave the same values
  in all tests (no items or bonuses).

## Open questions

1. Regeneration of life and mana from STR and INT (`StrRegenBonus`,
   `IntRegenBonus`) and its timing.
2. Attribute bonuses from items and abilities (green numbers), and
   whether they count for armor, attack speed and damage as above.
3. Whether the backswing of a hero's attack also shrinks with agility.
4. Skill points and learning abilities, the ability level rule
   (`HeroAbilityLevelSkip`), and the abilities themselves.

## Sources

- `Units\MiscGame.txt`, `Custom_V1\Units\UnitBalance.slk` and
  `Custom_V1\Units\UnitWeapons.slk` of 1.27b.
- Test maps RXX (attributes, life, mana, damage taken) and RXY (four
  heroes at levels 1 and 10 attacking paused peasants, ticks of
  `EVENT_UNIT_ATTACKED` and damage events) on 1.27b under Wine,
  2026-10-21.

## Verification

1. A paladin created by script and set to levels 1 to 10: STR 22, 24,
   27, 30, 32, 35, 38, 40, 43, 46; maximum life 650 to 1250.
2. A paladin of level 1 attacking a paused peasant: swings 349 or 350
   ticks apart, damage 24 to 34.
3. `UnitDamageTarget` of 100 with `ATTACK_TYPE_NORMAL` and
   `DAMAGE_TYPE_NORMAL` on a new paladin: 60.778 life lost.
