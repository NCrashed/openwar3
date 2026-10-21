---
id: SPEC-0041
title: Death coil, frost nova, impale, carrion swarm, mana burn, fan of knives, entangling roots
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, damage events, life and orders read every tick)"]
---

## Behaviour

Level 1 of seven undead and night elf hero spells, cast by script
orders on targets in range. Data: `Units\AbilityData.slk`,
`Units\UndeadAbilityFunc.txt`, `Units\NightElfAbilityFunc.txt`; `castpt`
and `castbsw` of the casters from `Custom_V1\Units\UnitWeapons.slk`.
Ticks are counted from the order (tick `o`); "d" is the distance from
the caster's centre. Damage amounts are those of the damage events;
footmen (armor 2) took them unreduced; a hero target takes 0.75 of
them (spells against hero armor, SPEC-0015).

### Death coil (`AUdc`, unit; death knight, `castpt` 0.452)

A homing missile (`Missilespeed` 1100, SPEC-0016 "Missiles") launched
at the cast point; an enemy living unit takes `DataA / 2` (100): a
footman at d = 400 took 0.00 and then 100.00 at `o + 152` (launch at
`o + 91`, hit after three 0.1 s steps).

### Frost nova (`AUfn`, unit; lich, `castpt` 0.4)

At the cast point (`o + 80`) the target takes 0.00, then `DataA` (50,
the area part) and `DataB` (100, the target part), each as its own
damage event in that order; other units within `Area` (200) of the
target take `DataA` (50). Order held to `o + 300` (`castbsw` 1.1).
The slow (`Dur` 4 s) was not measured.

### Impale (`AUim`, point; crypt lord, `castpt` 0.4)

At the cast point a wave starts along the line to the point; units in
its way are thrown up when it reaches them (footmen at d = 290 and 510
from `o + 93` and `o + 105`: order 851973 from then on), land `DataD`
(1 s, 199 ticks) later and take `DataC` (50) then, and stay stunned
until 2 s (`Dur`) after the landing (order back to 0 404 ticks after
the landing damage, with damage events of 0.00 just before).

### Carrion swarm (`AUcs`, point; dreadlord, `castpt` 0.3)

A wave (`Missilespeed` 1100) along the line to the point, `Area` 100
wide at the start: footmen at d = 290 and 590 took `DataA` (75) at the
cast point `o + 60` and at `o + 100`. Units of `Player(15)` (neutral
passive) were not hit (its `targs` have no `neutral`).

### Mana burn (`AEmb`, unit; demon hunter, `castpt` 0.3)

At the cast point a damage event of 0.00; `DataB` (0.25 s, 50 ticks)
later the target loses `DataA` (50) mana and takes as much damage (an
archmage: 37.50, the hero factor 0.75; life 450 -> 412.50).

### Fan of knives (`AEfk`, no target; warden)

Knives fly at once from the caster (no cast point: the damage of the
nearest came first, before `castpt` would have passed) as missiles of
speed 700 to the enemies within `Area` (400): footmen at d = 190 and
300 took `DataA` (75) at `o + 54` and `o + 83` (flight `d / 700`
within 3 ticks). Order held to `o + 167` (`castbsw` 0.83 from the
order).

### Entangling roots (`AEer`, unit; keeper of the grove, `castpt` 0.7)

The target takes a damage event of 0.00 30 ticks after the cast point
(`o + 170`), then `DataA` (15) two ticks later and every second, 9
times in all (`Dur` 9 s; `o + 172`, `o + 372`, ... `o + 1773`), and a
last 0.00 at `o + 1971`.

## Edge cases

- Several spells (death coil, frost nova, mana burn, entangling roots,
  the end of stuns) produce damage events of 0.00; scripts that count
  damage events see them.

## Open questions

1. The wave speeds and widths of impale and carrion swarm (the hit
   ticks above fit neither `Missilespeed` exactly).
2. Why fan of knives does not wait for the cast point.
3. Slows (frost nova), the root's hold, mana burn on units without
   mana.

## Sources

- The files above of 1.27b.
- Test maps RYL (death coil, frost nova, carrion swarm, impale against
  units of `Player(15)`) and RYM (carrion swarm, mana burn, fan of
  knives against paused units of `Player(1)`; entangling roots against
  `Player(15)`) on 1.27b under Wine, 2026-10-21.

## Verification

1. Frost nova on a footman with another 120 away: target 0, 50, 100;
   the other 50; all 80 ticks after the order.
2. Entangling roots: 15 damage nine times one second apart, the first
   172 ticks after the order.
