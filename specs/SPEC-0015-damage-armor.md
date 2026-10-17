---
id: SPEC-0015
title: Damage, armor and attack types (UnitDamageTarget)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

All arithmetic is the engine arithmetic of SPEC-0004 (`add`, `sub`,
`mul`, `div`, `i2r`, `f2i`); "<" compares float values.

### Game data

The constants come from `Units\MiscGame.txt`, section `[Misc]`:
`DefenseArmor`, `DamageBonusNormal`, `DamageBonusPierce`,
`DamageBonusSiege`, `DamageBonusMagic`, `DamageBonusChaos`,
`DamageBonusSpells`, `DamageBonusHero` (lists in the armor type order
small, medium, large, fort, normal, hero, divine, none), and
`AgiDefenseBase`, `AgiDefenseBonus`.

- **Which copy.** With the default game data set (the `war3map.w3i` field
  "game data set" is -1 in both maps tested), a map with the melee flag
  (0x04) uses `Units\MiscGame.txt`; a map without it uses
  `Custom_V1\Units\MiscGame.txt`. The two differ, for example
  `DamageBonusSpells` against hero armor: 0.70 in the melee map, 0.75 in
  the custom map (both measured). Other data set values were not tested.
- **Numbers** in these files are read with `s2r` of SPEC-0004:
  `"0.06"` is `div(6, 100)` = `0x3D75C290` (not the IEEE nearest
  `0x3D75C28F`), `"0.70"` is `0x3F333334`. `"1.50"`, `"0.75"`, `"1.25"`,
  `"2.00"`, `"0.50"`, `"0.05"` are exact.

### Attack types of scripts

| JASS constant | Data list |
|---|---|
| `ATTACK_TYPE_NORMAL` (0) | `DamageBonusSpells` |
| `ATTACK_TYPE_MELEE` (1) | `DamageBonusNormal` |
| `ATTACK_TYPE_PIERCE` (2) | `DamageBonusPierce` |
| `ATTACK_TYPE_SIEGE` (3) | `DamageBonusSiege` |
| `ATTACK_TYPE_MAGIC` (4) | `DamageBonusMagic` |
| `ATTACK_TYPE_CHAOS` (5) | `DamageBonusChaos` |
| `ATTACK_TYPE_HERO` (6) | `DamageBonusHero` |

The armor type of a unit is its `defType` (`udty`).

### Armor value

- Non-heroes: `def` (`udef`).
- Heroes: `def + AgiDefenseBase + AgiDefenseBonus * agility`
  (paladin: 2 - 2 + 0.3 * 13 = 3.9, which reproduces all 36 measured
  damage values; the order of the additions makes no difference there).

### Armor reduction

For an armor value `a` (a real):

- `a >= 0`: `x = mul(a, DefenseArmor)`, reduction `r = div(x, add(1.0, x))`.
- `a < 0`: `n = -f2i(a)` (so -3.5 counts as -3), at most 20, and
  `r = R[n]` with `R[n] = div(i2r(-floor(100000 * (1 - 0.94^n))),
  i2r(100000))`, the power and the floor computed exactly
  (`R[0] = 0`, `R[1]` from -6000, `R[2]` from -11640, `R[3]` from -16941,
  ...). The table does not depend on `DefenseArmor`.

### Damage of UnitDamageTarget

`UnitDamageTarget(source, target, amount, attack, ranged, attackType,
damageType, weaponType)`:

1. `M` = the data list of `attackType` at the target's armor type.
2. If `damageType` is `DAMAGE_TYPE_NORMAL`: `f = sub(1.0, r)` with the
   reduction above; for every other damage type (`MAGIC`, `UNIVERSAL`,
   `UNKNOWN`, `FIRE`, `ENHANCED`, `SPIRIT_LINK`, `DEFENSIVE`,
   `DEMOLITION`, `POISON`, `SLOW_POISON` tested) armor does not count:
   `f = 1.0`.
3. `d = mul(mul(amount, f), M)`.
4. **Floor:** if `d < min(amount, 1.0)`, then `d = min(amount, 1.0)`.
   So damage that armor or the type would bring below 1 is 1 (or the
   amount itself when that is below 1): 1.5 against armor 10 gives 1.0,
   0.7 against any armor gives 0.7.
5. `GetEventDamage()` in `EVENT_UNIT_DAMAGED` triggers is `d`. These
   triggers run before the life is lowered: inside them the target still
   has its old life.
6. The target's life is then lowered by `d` (SPEC-0013 for death).

- The multiplier `M` applies to every damage type; the `attack` and
  `ranged` flags change nothing.
- Damage from an allied unit or from the target itself is applied the same
  way. With `source = null` no damage is done.

Example (custom map): 100 pierce damage, `DAMAGE_TYPE_NORMAL`, against
medium armor 10: `x = mul(10, 0x3D75C290)`, `f = 0.625`, result 46.875
(`R2SW` 46.88); with `DAMAGE_TYPE_MAGIC` 75.0.

## Edge cases

- Armor below -20 counts as -20.
- `DamageBonusChaos` is 1.0 everywhere, so chaos damage of type
  `DAMAGE_TYPE_NORMAL` is affected only by armor.

## Open questions

1. Ethereal units (`EtherealDamageBonus`), magic immunity
   (`MagicImmunesResistDamage`), `MissDamageReduction`: not tested.
2. Game data sets other than the default.
3. Damage from real attacks (as opposed to `UnitDamageTarget`): base
   damage, dice and upgrades.

## Sources

- Test maps on 1.27b under Wine, 2026-10-17: custom unit types (based on
  the footman, `uhpm` 100000, armor types and values set in
  `war3map.w3u`), damaged with `UnitDamageTarget` from a footman of
  another player; `GetEventDamage` read as bit patterns.
- Analysis of the 1.27b game library for the reduction formula and the
  negative armor table.
- `Units\MiscGame.txt` and its `Custom_V1` copy.

## Verification

1. 7 attack types x 8 armor types, armor 0, 100 damage: the data
   multipliers (custom map: Spells vs hero 75; melee map: 70).
2. 11 armor values (1, 2, 3, 5, 7, 10, 13, -1, -3, -7, 0) x 12 amounts
   (100, 37.3, 1234.567, 9999, 0.7, 1, 1.5, 2, 3, 0.3, 0.01, 5) x 3
   multipliers (chaos 1.0, pierce 0.75, melee 1.5 against medium armor):
   all 396 `GetEventDamage` values bit for bit as above.
3. The paladin (hero armor 3.9) with the same amounts: 36 of 36.
4. Damage types and flags as listed, against medium armor 10.
