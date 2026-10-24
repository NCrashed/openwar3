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
- Each swing starts a cooldown timer of `cool1` and a hit timer of
  `dmgpt1`, both from the queue time at the swing start (SPEC-0002).
  When the attacker can swing again (target in range and faced), the
  next swing starts inside the cooldown timer's callback, at the
  cooldown's expiry, not at an update. Rifleman 7 in `c-RXR-1`: first
  swing at 3.2699974 (an update), cooldown expiry 4.7699971, second
  swing at that time with its hit timer at 4.9399971 (the damage in the
  tick whose queue interval holds it). The cooldown is kept when the
  target changes (SPEC-0019, "A new target and the old cooldown").
- A swing arms its cooldown only if `cool1` is longer than what is left
  of the running cooldown.
- When the attack ends (the target dies, a new order, or any other
  end), the swing's hit timer decides what happens to the cooldown:
  - hit timer still running (its expiry later than the queue time of
    the end): the cooldown is cancelled with the hit timer, the swing
    deals nothing and the unit may swing again at once;
  - hit timer expired, or expiring at exactly that queue time, or no
    swing running: the cooldown runs on to its expiry, and a hit timer
    that has not fired yet is dropped (no damage).
  `c-SAK-1`: footman 4 swung at kobold 8 at 15.479987 (hit timer
  15.979987); the kobold died at 15.919988, so his cooldown was
  cancelled and his next swing came well before 16.829987 (damage tick
  3353). Footmen 4 and 6 had their hit timers on ogre 13 at the same
  time 30.259863; footman 4's hit killed it first, footman 6 dealt
  nothing and his cooldown ran on to 31.109863, where his next swing
  started (damage tick 6323).
- When the first hit comes is SPEC-0019: after the random reaction
  delay (one output of game generator 9) the attack begins at an update
  once the attacker is in range and turned towards the target, and the
  damage follows `dmgpt1` later. Between the seed and the first hit
  exactly one output of game generator 9 (the delay) and two of
  generator 2 (the dice) were drawn, and nothing else (generator states
  read from process memory for 7 seeds).

### Missiles

A weapon of type `missile` (`weapTp1`, the archer `earc`) does not
damage at its damage point: at `dmgpt1` after the start of the swing it
launches a missile whose speed is `Missilespeed` of the unit's entry in
`Units\*UnitFunc.txt` (archer 900, troll headhunter 1200, blood mage
900; `Missilearc` 0.15 for these), and the damage comes when it arrives.

- The flight time is `L / Missilespeed` seconds, `L` the distance from
  the launch point to the target's centre. With `(x, y)` the attacker's
  position and `F` its facing at the launch (the facing it shows, which
  may still be turning towards the target), the launch point is
  `(x + launchY cos F + launchX sin F, y + launchY sin F - launchX cos F)`
  (`Units\UnitWeapons.slk`; archer 62 and 11, troll headhunter -34 and
  -5), each coordinate then floored to a multiple of 0.25; positive
  `launchX` is to the right of the facing. Five troll missiles of
  `c-RXR-1` (launch point, facing and position read in the game) fit
  this exactly; the direction to the target or the heading of the last
  move in place of `F` were off by 0.6 to 1.1. So
  for a target straight ahead at centre distance `d` the damage tick is
  the launch tick (the swing start + `dmgpt1`, 144 or 145 ticks for the archer's 0.72 s) + about
  `200 * (d - 62) / 900` ticks.
- When `L` is below 90 there is no flight: the damage comes in the tick
  of the launch.
- The damage dice (`dice1` outputs of game generator 2, above) are drawn
  at the launch, not at the impact, and the launch draws one more output
  of generator 2 right after them (for the missile itself; the hit
  amount does not use it): an archer (`dice1` 1) and a troll headhunter
  (`dice1` 1) each made two draws at each launch, a rifleman (`instant`,
  `dice1` 2) its two dice only (replay `c-RXR-1`, ticks 713 to 787, and
  test RXM). In test RXM the draws of the five
  archers' first shots came in ticks 457, 463, 475, 493 and 499, the
  ticks of their damage points (swing starts 312, 318, 330, 348, 354
  plus 145), while the damage came in ticks 543, 504, 475, 590 and 563
  (the archer at 150, with no flight, in 475 itself).

Measured (tests RXM, RXN: archers attacking a paused peasant straight
ahead, ticks of `EVENT_UNIT_ATTACKED`, which fires at the start of the
swing, and of the damage event; six hits each): damage minus swing start
145 or 144 ticks at centre distances 100, 140, 150 and 151; 165/164 at
152 to 155, then 166, 167, 168, 169, 175, 181, 186, 198, 209, 231 and 242
at 160, 163, 166, 170, 200, 225, 250, 300, 350, 450 and 500, all within
one tick of the formula (the first hits of each run one tick later than
the last, by the drift of the clock). An `instant` weapon (rifleman)
damages at its damage point like a melee one (above).
- The flight is an engine timer (SPEC-0002) started at the launch.
  `L` is measured in the plane, from the launch point to the target's
  centre at the moment of the launch. What happens next depends on
  `MissileHoming` of the unit's (or ability's) entry in
  `Units\*Func.txt`:
  - **not homing** (units without the field, such as the archer and
    the troll headhunter): one timer of `L / Missilespeed`; when it expires the
    missile hits its target wherever the target is by then. Test RXO
    (archers 300 from peasants walking away, across and towards them):
    for all ten arrows the flight time set at the launch was the plane
    distance from the launch point to the target's launch-time position
    divided by the speed, and the damage came when that timer expired,
    with no later correction. The timer's start is the time of the
    launch tick's start, `Q(k)` (the movement clock, SPEC-0002), not
    the queue time of the launch callback, and the impact comes 0.0001
    s (the queue's minimum delay) after the flight time: its expiry is
    `add(add(Q(k), div(L, Missilespeed)), 0.0001)`, and the missile
    hits in the missile part at the end of the first tick whose end
    time `Q(k + 1)` is at least that. In `c-RXR-1` troll 18 launched in
    tick 2631 (Q(2631) = `41526176`, `L / speed` = `3ed6f926`): the
    impact ran at the expiry `415919a7` (13.5687628), above Q(2715) =
    `4159197e`, so at the end of tick 2715 (without the 0.0001 it would
    have been 2714); troll 16's launch in tick 4131 (q 20.6499920,
    Q(4131) 20.6475353, 0.348333031) hit at the expiry `41a7f7bd`, at
    the end of tick 4200 (from q it would have been 4201);
  - **homing** (`MissileHoming=1`: heroes, casters, towers, the storm
    bolt of SPEC-0037, ...): the missile moves in steps of 0.1 s. At
    the launch and at the end of every step it measures `L` again, to
    the target's current centre; if `L` is below `Missilespeed * 0.1` it
    hits (a timer of 0.0001 s, so in that tick or the next), otherwise
    it moves `Missilespeed * 0.1` straight towards that point and the
    next step ends 0.1 s later. Test RYD (storm bolts, speed 1000, the
    targets walking towards the caster): launched at 1.9 s, steps ended
    at 2.0 and 2.1 s, each 100 (3.125 cells) further; one bolt was
    186 from its target at the launch, 58 after the first step and hit
    at 2.0 s, the other 302 at the launch, 172 after the first step, 47
    after the second, and hit right after 2.1 s.
  The rule "below 90 no flight" above is the same test (`L` below
  `Missilespeed * 0.1`) made at the launch.
- Artillery (`weapTp1` artillery, mortar team `hmtm`: `dmgpt1` 1.0 s,
  `Missilespeed` 900, `Missilearc` 0.35, `launchY` 25, `launchX` 30)
  follows the same flight time, measured along the ground: against
  paused peasants at centre distances 300, 500, 700, 900 and 1100 the
  damage came 262, 305, 350, 394 and 439 ticks after the swing start
  (test RXP; 200 + `200 * L / 900` gives 261.5, 305.9, 350.4, 394.8,
  439.3); the arc does not lengthen it.
- Artillery does not follow its target: the shell flies to the
  target's position at the launch and does its area damage (below,
  "Area damage") there. Test RXS: mortar teams 1000 from peasants
  walking across and away at 190; the peasants were 211.8 and 249.8
  from their positions at the launch when the shells came down, and got
  2.75 and 2.70 (`Qfact1` 0.1 of about 27, the outer zone up to
  `Qarea1` 250).
- A missile whose target dies or is removed while it flies does no
  damage, to the target or to anyone else (test RXS: archers 500 from
  paused peasants, `KillUnit` and `RemoveUnit` 0.8 s after the start
  of the swing, between the launch and the arrival; no damage event,
  also not for a second peasant 60 behind the target).

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

The glaive thrower (`ebal`, `weapTp1` `aline`, areas 25, 50, 150, shares
0.4 and 0.25) behaves the same way against a target that does not move:
units up to 40 from the target took 0.4, from 60 to 140 took 0.25, from
160 on nothing, in every direction; units on the line of the shot before
or behind the target took nothing more.

### Spill and bounce

- **Spill** (`spillDist`, `spillRadius`, `damageLoss`): `spillDist` is 0
  in all shipped weapon data and is raised by the upgrades `Rhhb` and
  `Repb` (effect `rasd`, +200). A gryphon rider (`hgry`, `mline`,
  `spillRadius` 50, `damageLoss` 0.2) of a player with `Rhhb` that hit a
  target 400 east of it also damaged the units behind the target on the
  line of the shot: those 80 to 230 beyond the target and at most 50 to
  the side (a unit 40 behind the target, one 55 to the side and one 260
  behind were not hit). The damage came in steps as the hammer flew on,
  41 and 81 ticks after the hit on the target, and each damaged unit
  took 0.8 of what the previous one took (0.8, 0.64, 0.512, 0.41,
  0.328, ...; the order was not always that of the distance). Without
  the upgrade there was no spill. A glaive thrower with `Repb` showed no
  spill (open question 7).
- **Bounce** (`mbounce`: huntress `esen`, `targCount1` 2, `Farea1` 400,
  `damageLoss1` 0.5): 33 ticks after the hit on the target the glaive
  hit the nearest other unit (150 away; units at 300, 380, 450 and 600
  were not hit) for 0.5 of the target's damage.

## Edge cases

- A melee weapon (`weapTp1` `normal`) applies its damage at the damage
  event time. The rifleman (`instant`) behaved the same.

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
7. Spill and bounce: the exact start and end of the spill along the
   line, its order and timing, why the glaive thrower with `Repb` did not
   spill, bounces with more targets (`targCount`, upgrade `Remg`) and the
   choice of the bounce target.
9. The launch point of a homing missile and the height (`launchZ`,
   `impactZ`, `Missilearc`), which do not change `L` in the tests.

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

