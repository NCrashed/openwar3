---
id: SPEC-0016
title: Damage of unit attacks
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

### Data set

Weapon and balance values come from `Units\UnitWeapons.slk` and
`Units\UnitBalance.slk`. As for `MiscGame.txt` (SPEC-0015), a map without
the melee flag uses the `Custom_V1\Units\...` copies of the game archives
where they exist, and a melee map uses `Units\...` of `War3Patch.mpq`.
The copies differ: the knight has `dmgplus1` 25 and `cool1` 1.5 in
`Custom_V1`, 28 and 1.4 in `War3Patch.mpq` `Units\`; the custom map
measured 25 + 2d5 and hits every 1.5 s.

### Damage of one hit

For weapon 1 of the attacker:

1. `base = dmgplus1`; for a hero, plus its primary attribute (`Primary`
   in `UnitBalance.slk`; the paladin, `STR` 22, `dmgplus1` 0, has base 22).
2. `dice1` dice with `sides1` sides are rolled from random generator
   number 2 of SPEC-0007 ("Game generators"): for each die one output
   `r` of that generator, the die showing `1 + floor(r * sides1 / 2^32)`
   (the `GetRandomInt(1, sides1)` mapping). The dice of one hit are
   consecutive outputs, and hits use the outputs in order (two outputs
   per hit of a two-dice weapon, nothing else drew from generator 2 in
   the tests).
3. `amount = i2r(base + sum of dice)`.
4. The damage is then that of SPEC-0015 for `amount`, the weapon's
   attack type (`atkType1`: `normal` is the list `DamageBonusNormal`,
   `pierce` `DamageBonusPierce`, `hero` `DamageBonusHero`, ...) and
   `DAMAGE_TYPE_NORMAL`: armor and the type multiplier apply, the floor
   applies, `EVENT_UNIT_DAMAGED` triggers see the result.

Example (custom map, `SetRandomSeed(777)`, target with medium armor 10,
`f = 0x3F1FFFFF` as in SPEC-0015), bit patterns of `GetEventDamage()`:

| Attacker | Raw sums | Results |
|---|---|---|
| knight (x 1.5) | 31 32 31 31 32 29 | `41E87FFE` `41EFFFFE` `41E87FFE` `41E87FFE` `41EFFFFE` `41D97FFE` (29.062496, 29.999996, ..., 27.187496) |
| paladin (x 1.0) | 28 30 30 29 31 26 | `418BFFFF` `4195FFFF` `4195FFFF` `4190FFFF` `419AFFFF` `4181FFFF` (17.499998, ...) |
| rifleman (x 0.75) | 21 22 21 21 22 19 | `411D7FFE` `4124FFFE` `411D7FFE` `411D7FFE` `4124FFFE` `410E7FFE` (9.843748, ...) |

### Timing

- Consecutive hits on a target that does not move are exactly `cool1`
  apart (1.5 s for the custom knight, measured from game time at each
  damage event).
- When the first hit comes is SPEC-0019: the attack begins at an update
  once the attacker is in range and turned towards the target, the
  damage follows `dmgpt1` later, and a unit that is already in range at
  the order may wait a random 0 to 8 extra updates first. Between the
  seed and the first hit exactly one output of game generator 9 and two
  of generator 2 (the dice) were drawn, and nothing else (generator
  states read from process memory for 7 seeds).

## Edge cases

- A melee weapon (`weapTp1` `normal`) applies its damage at the damage
  event time; no projectile was involved in the tests. The rifleman
  (`instant`) behaved the same.

## Open questions

1. The random wait before the first hit (SPEC-0019) and the role of
   `backSw1` and acquisition.
2. Weapon 2, splash, upgrades (`dmgUp1`, `DmgUpg`), projectiles and
   missing (`MissDamageReduction`).
3. Which systems other than attacks draw from generators 0..44.
4. How real numbers in `.slk` files are read. For `MiscGame.txt` it is
   `s2r` of SPEC-0004; for `.slk` values no measurement has told `s2r`
   from the IEEE nearest value yet (`"1.5"` would give 1.5000001 with
   `s2r`).

## Sources

- Analysis of the 1.27b game library: `SetRandomSeed` reseeds the game
  generators (SPEC-0007).
- Test maps on 1.27b, 2026-10-17: knight, paladin and rifleman attacking a
  passive target of `Player(15)` with 10000 or 90000 life; damage from
  `GetEventDamage` (bit patterns) and game time from a running timer;
  all other units removed.

## Verification

1. Knight against armor 0 (`normal` type) after `SetRandomSeed(123)` and
   `SetRandomSeed(456)`: 40 hits each, all as predicted.
2. 40 knights, each created next to the target, `SetRandomSeed(5000 + k)`,
   first hit only: 40 of 40, and a `GetRandomInt` right after the hit
   returned the first script output after the seed (the attack did not
   draw from the script generator).
3. Knight, paladin and rifleman against medium armor 10, 6 hits each
   after `SetRandomSeed(777)`: 18 of 18 bit patterns.
