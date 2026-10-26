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

A missile (`Missilespeed` 1100) launched at the cast point; an enemy
living unit takes `DataA / 2` (100): a footman at d = 400 took 0.00 and
then 100.00 at `o + 152` (launch at `o + 91`).

The flight is one engine timer, as for the missiles of SPEC-0016 that
do not home (no 0.1 s steps): its expiry is `add(add(Q, L / 1100),
0.0001)`, `Q` the start of the launch tick, and the impact runs in the
missile part at the end of the first tick whose end is at or above the
expiry. `L` is measured in the plane from the caster's launch point
(SPEC-0016, with `launchX` -20, `launchY` 60 of `Udea` and the facing
at the launch) to the target's centre at the launch. `c-SBJ-1`: death
knight at (-800, 0) facing 0 (not turned: the target lay 0.065 rad
off), launch point (-740, 20), ogre at (-544.860, -16.732), `L` 198.52;
launch tick start 2.949975, expiry 0x40485AEE (3.1305499), impact at
the end of the tick ending 3.134970. The order ends at the effect +
`castbsw` (1.008) by the cast's end timer (SPEC-0040).

### Frost nova (`AUfn`, unit; lich, `castpt` 0.4)

At the cast point (`o + 80`) the target takes 0.00, then `DataA` (50,
the area part) and `DataB` (100, the target part), each as its own
damage event in that order; other units within `Area` (200) of the
target take `DataA` (50). Order held to `o + 300` (`castbsw` 1.1).

**The slow and its end** (`c-SBJ-1`). Units hit are slowed after their
hit for `Dur`: attack speed by `FrostAttackSpeedDecrease` (0.25) and
move speed by `FrostMoveSpeedDecrease` (0.5) of `Units\MiscData.txt`.
A swing started under the slow has its damage point, backswing and
cooldown timers lengthened (`dmgpt1 / 0.75` and so on). When the buff
ends (its timer, q 8.9599915 for the two kobolds), each running attack
timer of the unit is started again at that moment with its rest
rescaled: new delay = `(expiry - now) / (1 / 0.75)`, with `1 / 0.75`
rounded to float (1.3333334) and the game's arithmetic (SPEC-0004);
multiplying by 0.75 instead is 1 or 2 ulps off. Kobold 6: damage
point started at 0x410962FC with 0x3F01B4E8, cooldown with 0x3FE66667;
at the buff's end 0x3DCCCFC2 and 0x3F88F5F1. Kobold 5: after-swing
timer from 0x4104B17F with 0x3F4CCCCD, cooldown from 0x40F92C61 with
0x3FE66667; at the end 0x3DCCD022 and 0x3EF0A4BA. Timers of a swing
started under the slow keep the slow's lengths when they are not
rescaled (kobold 6's next swing at 10.03).

### Impale (`AUim`, point; crypt lord, `castpt` 0.4)

At the cast point a wave starts along the line to the point; units in
its way are thrown up when it reaches them (footmen at d = 290 and 510
from `o + 93` and `o + 105`: order 851973 from then on), land `DataD`
(1 s, 199 ticks) later and take `DataC` (50) then, and stay stunned
until 2 s (`Dur`) after the landing (order back to 0 404 ticks after
the landing damage, with damage events of 0.00 just before).

**The wave** (`c-SBJ-1`, crypt lord at (-700, -150), point (-200,
-150), effect at q 6.900000). The wave runs 0.3 s in 5 steps, a 0.06 s
timer started at the effect and again at each step (steps at 6.96 ..
7.20). It has 6 points on the line from the caster: the first 125 from
the caster's centre, made at the effect itself, then 120 further at
each step (600 / 5; read as 124.998 and 119.998 through the direction's
rounding). A unit is reached at the first point within 125 + its
collision size of it (fits both footmen above: 290 at the first step,
510 at the second; the reach test itself was not read).

**Facing of a thrown unit.** When a unit is thrown, the
game sets the unit's facing and its path agent's heading (SPEC-0018) to
the facing of the Impale side + pi (here 0 + pi; the caster's facing
and the wave's direction were both 0, so which one is read is open).
The units then turn from that heading at their next walks.

**Calls.** A unit under Impale's stun makes no relayed call for help
(SPEC-0030) and starts no call cooldown when others of its group are
hit (`c-SBJ-1` at 9.799999: only the unstunned kobold started one).

**Cast end.** The cast's end timer (SPEC-0040) has the delay
`wave time + 0.1` = 0.4 here instead of the caster's `castbsw` (1.1):
the order ends at effect + 0.4.

### Carrion swarm (`AUcs`, point; dreadlord, `castpt` 0.3)

A wave (`Missilespeed` 1100) along the line to the point, `Area` 100
wide at the start: footmen at d = 290 and 590 took `DataA` (75) at the
cast point `o + 60` and at `o + 100`. Units of `Player(15)` (neutral
passive) were not hit (its `targs` have no `neutral`).

Read in `c-SBJ-1` (dreadlord at (-700, 150), point (-200, 150), effect
at 9.799999, cast end `castbsw` 1.53): the wave object holds speed
1100, length 800, width 100 at the start and 300 at the end, damage 75,
and a 0.1 s step timer. At the effect a unit query of radius 700.06
runs around the wave's start (-600.25, 150), 100 from the caster. At
each step a query is centred on the wave's point, 110 further per step
(1100 * 0.1), with radius 100 + 200 * travelled / 800 (100.06, 127.43,
154.93, ...). The two kobolds hit (255 and 302 from the caster, 50 and
40 off the line) took their 75 at the effect itself, outside the first
step's circle: so at the effect the damage takes the units of the
whole wave shape (open question 1).

### Mana burn (`AEmb`, unit; demon hunter, `castpt` 0.3)

At the cast point a damage event of 0.00; `DataB` (0.25 s, 50 ticks)
later the target loses `DataA` (50) mana and takes as much damage (an
archmage: 37.50, the hero factor 0.75; life 450 -> 412.50).

### Fan of knives (`AEfk`, no target; warden)

Knives fly at once from the caster (no cast point: the damage of the
nearest came first, before `castpt` would have passed) as missiles to
the enemies within `Area` (400), at most `DataB / DataA` (300 / 75 = 4)
of them: footmen at d = 190 and 300 took `DataA` (75) at `o + 54` and
`o + 83`. Order held to `o + 167` (`castbsw` 0.83 from the order).

Read in `c-SBK-1` (warden at (-560, 150), facing 0, effect at the
order's tick, start 4.499912):
- the knives are made in the order of centre distance from the warden
  (134.5, 143.2, 155.8, 174.4); the fifth nearest (205.2) got none;
- each knife draws twice at its making: game generator 25 once (a die
  for the missile's look only) and game generator 26 once for its
  speed: `Missilespeed` (700) + floor(out * 100 / 2^32), out the 32-bit
  output (the 100 is a per-missile value of the missile's data; the
  draw is skipped when it is 0). Outputs 0x2B8BA704, 0xE2FAF6BD,
  0xFC77FD13, 0xAF45E228 gave 717, 788, 798, 768;
- a knife leaves from the warden's launch point (SPEC-0016: `launchX`
  25 at facing 0 gives (-560, 125)) and flies one timer of `L / speed`
  as death coil's; `L` 152.398, 140.090, 162.380, 149.934; expiries
  0x4095B076, 0x40963F65, 0x4096830A, 0x4096CD4D;
- each knife takes five objects at its making, freed 5.0 s after its
  impact; each impact takes two (the hit), freed 0.0001 s later. The
  cast's own objects are freed at the cast's end (effect + 0.83).

### Shadow strike (`AEsh`, unit; warden, `castpt` 0.5)

Read in the game, `c-SBO-1` (warden at level 2, order at q 4.0 on an
ogre warrior 702 away):
- the order makes a walk; the warden walks to (-536.28, 337.24) and its
  walk arrives at q 5.129987 (372.4 from the ogre's centre, within the
  cast range plus both collision sizes); the cast point timer (0.5)
  starts there; effect at 5.629987, with the cooldown (8.0 s) and the
  cast's end timer (`castbsw` 0.83);
- at the effect a missile is launched (speed 1200, 37.5 cells per
  second) from the warden's launch point (SPEC-0016: `launchX` 25 at
  her facing 79.3 degrees, floored to quarters: (-511.75, 332.5)) to
  the ogre's position (-450, 700), `L` 11.645 cells; one timer of
  0x3E9EFFF5 (0.310546547), expiry 5.940533, impact at the end of the
  tick ending 5.944808;
- at the impact the target takes a damage event of 0.00, then the
  initial damage 75.00 (the ogre died of it, so the damage over time
  was not seen). The impact starts two 0.334 s timers and three 0.1 s
  timers of the buff's objects, and the missile's objects' 5.0 s
  timer.

### Entangling roots (`AEer`, unit; keeper of the grove, `castpt` 0.7)

The target takes a damage event of 0.00 30 ticks after the cast point
(`o + 170`), then `DataA` (15) two ticks later and every second, 9
times in all (`Dur` 9 s; `o + 172`, `o + 372`, ... `o + 1773`), and a
last 0.00 at `o + 1971`.

**The cast's start** (`c-SBK-1`, order at q 6.5). The order makes a
walk; the cast point timer (0.7) starts in the walk update at which the
walk arrives: in range and turned within 0.2 rad of the target (the
chase's arrival, SPEC-0019). The keeper (heading 0, target 0.440 off)
turned at its first update (6.509974) and arrived at the second
(6.539974): cast point timer from 6.5399742, effect at 7.2399740,
0.04 later than order + 0.7. The 0.00 hit comes at the effect.

A rooted unit neither moves nor attacks for `Dur`; its order is put
aside (its walk is given up) as for a stun. The roots' buff takes two
objects, kept to the buff's end.

## Edge cases

- Several spells (death coil, frost nova, mana burn, entangling roots,
  the end of stuns) produce damage events of 0.00; scripts that count
  damage events see them.

## Open questions

1. Carrion swarm: which units the damage at the effect takes (the
   wave's shape is a guess), and why the footman at d = 590 of RYM was
   hit 40 ticks after the cast point.
2. Why fan of knives does not wait for the cast point.
3. Mana burn on units without mana; which field holds fan of knives'
   speed spread (100).
4. Impale: whether the thrown unit's new facing is the caster's facing
   or the wave's direction.

## Sources

- The files above of 1.27b.
- Replays `c-SBJ-1` (death coil, frost nova, impale, carrion swarm
  against creeps) and `c-SBK-1` (mana burn, fan of knives, entangling
  roots against creeps), read in the running game under Wine,
  2026-11-12.
- Test maps RYL (death coil, frost nova, carrion swarm, impale against
  units of `Player(15)`) and RYM (carrion swarm, mana burn, fan of
  knives against paused units of `Player(1)`; entangling roots against
  `Player(15)`) on 1.27b under Wine, 2026-10-21.

## Verification

1. Frost nova on a footman with another 120 away: target 0, 50, 100;
   the other 50; all 80 ticks after the order.
2. Entangling roots: 15 damage nine times one second apart, the first
   172 ticks after the order.
