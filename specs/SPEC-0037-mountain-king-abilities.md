---
id: SPEC-0037
title: Storm bolt, thunder clap and stuns (mountain king abilities)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test map, values read by JASS natives every tick)"]
---

## Behaviour

Three abilities of the mountain king (`Hmkg`) at level 1, with the data
of `Units\AbilityData.slk` and the hero's `castpt` 0.4 s and `castbsw`
0.5 s (`Custom_V1\Units\UnitWeapons.slk`). The cast timing is that of
SPEC-0035: the effect comes `castpt` after the cast starts, the order
stays for `castbsw` after the effect. All damage below is the amount of
the damage event (`GetEventDamage`).

### Storm bolt (`AHtb`, order `thunderbolt`)

- At the effect (80 ticks after the order, both casters facing their
  targets) a missile is launched (`Missilespeed` 1000 and
  `MissileHoming` 1 of `Units\HumanAbilityFunc.txt`).
- When it arrives the target takes `DataA` (100) damage. Against a
  footman (armor 2) the event was exactly 100.00, against a paladin of
  level 1 (armor 3.9) exactly 75.00: the armor value does not reduce
  it, the hero factor 0.75 of the spells attack type does (SPEC-0015).
  A damage event of 0.00 comes first, in the same tick.
- The target is stunned for `Dur` (5 s) or, if it is a hero, `HeroDur`
  (3 s): its current order reads 851973 while stunned and it does not
  move or attack. The order went back to 0 in the tick 1003 ticks
  (footman) and 603 ticks (paladin) after the damage tick.
- Flight: a homing missile (SPEC-0016, "Missiles": steps of 0.1 s,
  a hit when the target is closer than 100). Launched in tick `e`, the
  damage came in `e + 41` (footman 302 away at the launch, walking
  towards the caster at 270: two steps) and `e + 20` (paladin 186 away,
  walking towards it: one step).
- The order stays `thunderbolt` (852095) until `e + 100` (`castbsw`).

### Thunder clap (`AHtc`, order `thunderclap`, no target)

- At the effect (80 ticks after the order) every enemy ground unit
  within `Area` (250) of the caster takes `DataA` (60) damage, exactly
  60.00 for footmen 150 and 152 away (armor not counted), both in that
  tick.
- The units hit are slowed for `Dur` (5 s; heroes `HeroDur` 3 s): their
  attack interval doubled (`DataD` 0.5 attack speed reduction: the
  footmen's swings came 530 and 541 ticks apart instead of 270 while
  slowed, and 270 again after it ended), and their movement speed is
  cut by `DataC` (0.5, not measured).
- The order stays `thunderclap` (852096) until `e + 100`.

### Bash stun (`AHbh`)

With a bash proc (SPEC-0036) the target takes the extra `DataC` (25)
damage, exactly 25.00 against a footman with armor 2 (armor not
counted), and is stunned for `Dur` (2 s; heroes `HeroDur` 1 s): the
footman's order was 851973 from the hit on and went back to 0 in the
tick 402 ticks after the damage tick.

### Stun timing

In all three stuns the order came back 2 or 3 ticks later than `Dur`
(1000, 600 and 400 ticks) counted from the damage tick: 1003, 603 and
402 ticks (the script timer of the test reads a change made during
the updates in the next tick; these numbers are corrected for that).
Whether the end waits for the unit's next update is open.

## Edge cases

- Storm bolt and bash damage ignore the armor value; storm bolt against
  a hero is reduced by the hero factor of spells.

## Open questions

1. The launch point of storm bolt (the caster's centre in both tests).
2. Movement slow of thunder clap and the exact attack interval while a
   slow starts or ends during a cooldown.
3. The tick in which a stun ends (the 2 or 3 ticks above).
4. Avatar (`AHav`) and the abilities of the other heroes.

## Sources

- `Units\AbilityData.slk`, `Units\HumanAbilityFunc.txt`,
  `Custom_V1\Units\UnitWeapons.slk` of 1.27b.
- Test map RYD on 1.27b under Wine, 2026-10-21: four mountain kings of
  `Player(0)` (storm bolt on a footman and on a paladin of `Player(1)`,
  thunder clap next to two footmen, bash against a footman), positions,
  orders and damage events written by the map's script every tick.

## Verification

1. A mountain king with storm bolt casting on a footman in range: the
   footman takes 100 damage after the flight and has order 851973 for
   5 s; a paladin takes 75 and is stunned 3 s.
2. Thunder clap next to two footmen: 60 damage each in the tick 80
   ticks after the order; their next swings come twice as far apart
   for 5 s.
