---
id: SPEC-0036
title: Chance-on-attack abilities (critical strike, bash, drunken brawler)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, generator states and rolls read in the running game)"]
---

## Behaviour

Critical Strike (`AOcr`), Bash (`AHbh`) and Drunken Brawler (`ANdb`)
give an attack a chance to do more. The three share one roll. The
chance is not a fixed probability per attack: it grows with every
attack that did not proc and falls back after one that did (a
"pseudo-random distribution").

### The roll

Each of these abilities keeps a counter `N`, 1 when the ability is
learned. At the **start of each swing** of its owner (the tick of
`EVENT_UNIT_ATTACKED`, SPEC-0016), if the roll is allowed (see below):

1. `p = DataA` of the ability's level (the chance in percent: 15 for
   critical strike, 20 / 30 / 40 for bash, 10 for drunken brawler);
2. `i = trunc(add(mul(p, 0.2), 0.5))` (SPEC-0004 operations; `p` as a
   float), clamped to 0..20: the chance rounded to a step of 5 percent;
3. `c = C[i] * N` (the product in single precision, `N` converted to
   float), with the table `C` below;
4. one value `x` is drawn from game generator 3 (SPEC-0007, "Game
   generators") and turned into a real number in [0, 1) as
   `GetRandomReal` does: `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)`;
5. if `c >= x` the swing procs and `N` becomes 1; otherwise it does not
   and `N` becomes `N + 1`.

The proc is decided at the swing start and applies to the hit of that
swing (the damage at the damage point, or the missile launched then).
At every swing start the previous proc is cleared, also when no roll is
allowed for that swing.

| `i` | percent | `C[i]` | | `i` | percent | `C[i]` |
|---|---|---|---|---|---|---|
| 0 | 0 | 0 | | 11 | 55 | 0.2953 |
| 1 | 5 | 0.0038 | | 12 | 60 | 0.3333 |
| 2 | 10 | 0.0147 | | 13 | 65 | 0.3781 |
| 3 | 15 | 0.0320 | | 14 | 70 | 0.4203 |
| 4 | 20 | 0.0547 | | 15 | 75 | 0.4607 |
| 5 | 25 | 0.0819 | | 16 | 80 | 0.5 |
| 6 | 30 | 0.1126 | | 17 | 85 | 0.5799 |
| 7 | 35 | 0.1461 | | 18 | 90 | 0.6666 |
| 8 | 40 | 0.1815 | | 19 | 95 | 0.7706 |
| 9 | 45 | 0.2184 | | 20 | 100 | 1 |
| 10 | 50 | 0.2562 | | | | |

The values are single-precision floats of the decimal values shown
(for example `0x3D03126F` for 0.0320, `0x3D600D1C` for 0.0547).

Measured (tests RYB and RYC, two blademasters and two mountain kings of
level 1 attacking paused peasants of `Player(1)` and of `Player(15)`,
about 40 s): 215 rolls; for every roll the value `x` computed from the
generator 3 state before the draw equalled the game's value bit for
bit, and the chance was `C[i] * N` with the counter rule above in all
215 (crit `c` = 0.032, 0.064, ... 0.352 after ten misses; bash 0.0547,
0.1094, ... 0.4923 after eight).

The draw is the only use of generator 3 in these tests (no other draw
at the swing start or the hit), so the order of draws is the order of
the swing starts within a tick.

### When the roll is made

Every swing of the four heroes against their peasants rolled, the
first swing included. Targets of `Player(15)` (neutral passive) were
allowed for both abilities: rolls and procs against them as against
`Player(1)`. Swings without a roll (`N` and the generator untouched)
are expected when the ability is not learned or disabled, or the
target is not allowed by the `targs` of the level; not measured.

### Effects

- **Critical strike:** the damage of the hit is multiplied by `DataB`
  of the level (2 at level 1). All 17 proc hits of the tests did an
  even amount from 56 to 88, twice the normal 28 to 44 of a level 1
  blademaster (SPEC-0034) against armor 0.
- **Bash:** in the tick of the hit an extra damage event of `DataC`
  (25) to the target comes before the hit's own damage event (all 13
  proc hits); the target is stunned for `Dur` (2 s), `HeroDur` (1 s)
  for heroes (the stun itself not measured yet).
- **Drunken brawler** uses the same roll with `DataA` (critical part);
  its effects are not measured.

### Evasion (`AEev`)

Evasion is not part of this distribution. When an attack of an enemy
hits the unit with evasion, after the attacker's damage dice have been
drawn (SPEC-0015: one or more outputs of game generator 2), one more
output of game generator 2 is drawn and turned into `x` as above; the
attack is evaded if `x < DataA` of the level (0.1 at level 1): no
damage and no damage event. A unit that is paused (`PauseUnit`) does
not roll and does not evade.

Measured (test RYN, a demon hunter of level 1 attacked by three
footmen for 30 s): 66 hits, each with its dice draw followed by the
evasion draw in the same tick; the three with `x` 0.099, 0.044 and
0.064 had no damage event, the others theirs. With the hero paused, no
evasion draws were made in 33 hits.

## Edge cases

- A roll is made at a swing start even if the swing then does not hit:
  the first proc of one blademaster in RYC (at its first swing) had no
  damage event, and `N` was reset all the same.
- Levels change `DataA` and so `i`, not `N`.

## Open questions

1. The evasion part of drunken brawler (`DataD`), presumably as
   evasion above.
2. Bash's stun: duration measured in ticks, and how it interrupts
   orders.
3. Critical strike with missiles and splash, and whether the
   multiplier applies before or after armor (measured only against
   armor 0).
4. The `targs` check in detail (allies, structures, magic immune).

## Sources

- `Units\AbilityData.slk` of 1.27b (`DataA`, `DataB`, `DataC`, `Dur`,
  `HeroDur`, `targs`).
- Test maps RYB (targets of `Player(15)`) and RYC (targets of
  `Player(1)`) on 1.27b under Wine, 2026-10-21: the generator 3 state at
  every draw, the chance, the drawn value and the outcome of every
  roll, damage events written by the map's script.

## Verification

1. A blademaster of level 1 attacking a paused peasant: the chance of
   the n-th swing since the last proc is `0.032 * n` (0.032, 0.064,
   ...); with the generator states of SPEC-0007 the procs fall exactly
   on the swings where `0.032 * n >= x`.
2. Each crit proc hit does twice the normal damage; each bash proc
   adds a damage event of 25 in the tick of the hit, before it.
3. Over many swings a 15 percent ability procs on about 15 percent of
   swings (the constant `C[3]` gives that average), but never more than
   32 swings apart (`0.032 * 32 >= 1`).
