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
   per hit of a two-dice weapon). Creating a unit with a weapon also
   takes one output of this generator (SPEC-0019, acquisition): between
   `SetRandomSeed` and the hits of the tests below no such unit was
   created.
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
- When the first hit comes is SPEC-0019: after the random reaction
  delay (one output of game generator 9) the attack begins at an update
  once the attacker is in range and turned towards the target, and the
  damage follows `dmgpt1` later. Between the seed and the first hit
  exactly one output of game generator 9 (the delay) and two of
  generator 2 (the dice) were drawn, and nothing else (generator states
  read from process memory for 7 seeds).

### Second weapon

`weapsOn` tells which weapons a unit has (1: weapon 1, 2: weapon 2, 3:
both). A unit with both uses, for a given target, the weapon whose
target list (`targs1`, `targs2`) allows the target; the other fields
of that weapon (`cool2`, `dmgplus2`, `dice2`, `sides2`, `atkType2`,
`rangeN2`, the area fields) then apply, with the steps above read for
weapon 2:

- mortar team (`hmtm`, weapon 1 `ground,...`, weapon 2 `structure`)
  against an enemy farm: hits every 3.5 s, 63.5 to 66.9 damage, no area
  damage (a unit 60 from the farm was not hit), i.e. weapon 2;
- gargoyle (`ugar`, weapon 1 `air`, weapon 2 `ground,...`) against a
  footman: hits every 2.2 s (`cool2`), 16.1 to 18.8 damage; against a
  gryphon rider: every 1.4 s (`cool1`), 62 to 69 damage.

Which weapon is taken when both allow the target was not tested.

### Area damage

A weapon with area fields (`Farea`, `Harea`, `Qarea`, `Hfact`, `Qfact`,
`splashTargs`; tested with the mortar team's weapon 1: 25, 150, 250,
0.4, 0.1) damages, besides its target, every unit around the target
whose type is in `splashTargs`, at the same time as the target:

- by the distance `d` from the target's position to the unit's
  position (the unit's collision does not count: units with collision
  16 at 36 got the middle share): `d` up to about `Farea`: the full
  hit; up to about `Harea`: `Hfact` of it; up to about `Qarea`: `Qfact`
  of it; beyond: nothing. The measured limits were about one unit
  larger than the fields (full at 25.53, not at 26.19; middle share at
  150.95, not at 150.99; small share at 250.84, not at 251.09) and seemed
  centred about one unit beyond the target in the direction of the
  shot (open question 6);
- the share appears to be taken of the hit's amount before armor:
  units of the same type as the target took 22.5 and 5.625 where the
  target took 56.249996 (0.4 and 0.1 of it, but rounded as a fresh
  computation, not as a product); each victim's own armor was not
  varied;
- allied and neutral units are damaged as well (`splashTargs` of the
  mortar team has no `enemy`): a unit of the mortar team's own player
  50 from the target and one of `Player(15)` 70 from it took the middle
  share. A unit is damaged once per hit, even when it is the target.

Each damaged unit gets its own `EVENT_UNIT_DAMAGED` with the attacker as
the source, all in the tick of the hit.

## Edge cases

- A melee weapon (`weapTp1` `normal`) applies its damage at the damage
  event time; no projectile was involved in the tests. The rifleman
  (`instant`) behaved the same.

## Open questions

1. The role of `backSw1` and acquisition.
2. Upgrades (`dmgUp1`, `DmgUpg`), projectiles and
   missing (`MissDamageReduction`).
3. Which systems other than attacks draw from generators 0..44.
4. How real numbers in `.slk` files are read. For `MiscGame.txt` it is
   `s2r` of SPEC-0004; for `.slk` values no measurement has told `s2r`
   from the IEEE nearest value yet (`"1.5"` would give 1.5000001 with
   `s2r`).
6. The exact limits and centre of area damage (measured to about 0.5
   unit, one direction of shot), area damage of `aline` and `mline`
   weapons (`spillDist`, `spillRadius`, `damageLoss`), `targCount`, and
   area damage against structures and trees.

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
4. **Area damage.** Mortar team of `Player(1)` 1000 west of a paused
   target (a footman copy with collision 0 and 5000 life), more copies
   around the target at measured distances (from the units' positions
   after creation): shares 1 at 23.8, 24.0, 25.3, 25.5, 25.8; 0.4 at
   26.2, 26.4, 27.0, 30.2, 60, 100, 148.1, 149.2, 150.3, 150.45, 150.95;
   0.1 at 150.99, 151.5, 152.0, 160.8, 200, 248.2, 249.1, 250.3, 250.6,
   250.84; 0 at 251.09, 251.5, 251.8, 256.5, 300. The same in four
   consecutive hits. One unit at 251.87, north-north-east of the target,
   took 0.1 (the direction dependence of open question 6).
5. **Second weapon.** As listed under "Second weapon", hits counted for
   10 s.

