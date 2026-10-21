---
id: SPEC-0033
title: Hero experience and levels
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, values read by JASS natives)"]
---

## Behaviour

Heroes gain experience (XP) when enemy units die near them, and their
level follows their XP. The constants are in `Units\MiscGame.txt`,
section `[Misc]` (values of 1.27b below). Unit levels are `level` of
`Units\UnitBalance.slk` as `GetUnitLevel` reads them (peasant 1,
footman 2, knight 4, gryphon rider 5; kobold 1, gnoll brute 2, ogre 3,
forest troll 4).

### Tables beyond their listed values

A table of `MiscGame.txt` lists the first values; later ones follow
`f(x) = A * f(x - 1) + B * x + C` with the constants `...FormulaA`,
`...FormulaB`, `...FormulaC` of the same name:

| Table | Listed | A, B, C | Values (x = 1, 2, ...) |
|---|---|---|---|
| `NeedHeroXP` (XP to reach level x + 1) | 200 | 1, 100, 0 | 200, 500, 900, 1400, 2000, 2700, 3500, 4400, 5400 |
| `GrantNormalXP` (kill of a unit of level x) | 25 | 1, 5, 5 | 25, 40, 60, 85, 115, ... |
| `GrantHeroXP` (kill of a hero of level x) | 100, 120, 160, 220, 300 | 1, 0, 100 | 100, 120, 160, 220, 300, 400, 500, 600, 700, 800 |

### Levels

A hero of level `L` has at least `NeedHeroXP(L - 1)` XP (0 for level
1); `MaxHeroLevel` is 10, reached at 5400. Measured with `SetHeroXP` on
a new paladin for XP 0, 100, ... 6000: levels 1 to 10 begin exactly at
0, 200, 500, 900, 1400, 2000, 2700, 3500, 4400 and 5400, and XP stays
at 5400 above it. `SetHeroXP` does not lower a hero's XP (set to 0 after
5400 it stayed 5400), and `SetHeroLevel` does not lower its level.

### Experience from a death

When a unit dies that is an enemy of a player, each hero of that player
within `HeroExpRange` (1200) of the dying unit gets a share:

1. The base is `GrantHeroXP(level)` for a hero, else
   `GrantNormalXP(level)` of the dying unit.
2. The base is divided equally among the player's heroes within the
   range (including heroes at the highest level, which get nothing
   themselves: `MaxLevelHeroesDrainExp` 1).
3. For a unit of the neutral hostile player (creeps, `Player(12)`) each
   hero's share is multiplied by `HeroFactorXP` for that hero's level:
   80, 70, 60, 50 percent for levels 1 to 4 and 0 from level 5 on. Kills
   of other players' units are not reduced by the hero's level, and
   neither are kills of heroes.
4. The result is rounded to the nearest integer, halves up (17.5 -> 18,
   42.5 -> 43, 12.5 -> 13), and no hero goes beyond 5400.

Measured (tests RXU, RXV, RXW, all kills with `UnitDamageTarget` from
the paladin, values read with `GetHeroXP` right after):

| Case | Hero level | XP |
|---|---|---|
| peasant, footman, knight, gryphon rider of `Player(1)` | 1 to 9 | 25, 40, 85, 115 at every level |
| kobold, gnoll brute, ogre, forest troll of `Player(12)` | 1 | 20, -, 48, - (x 0.8) |
| the same | 2 | 18, 28, -, 60 (x 0.7) |
| the same | 3 | 15, 24, 36, 51 (x 0.6) |
| the same | 4 | 13, 20, 30, 43 (x 0.5) |
| the same | 5 to 10 | 0 |
| blademaster of `Player(1)`, levels 1 to 10 | 1 to 8 | 100, 120, 160, 220, 300, 400, 500, 600, 700, 800 |
| knight (`Player(1)`), two heroes in range (levels 1 and 3) | 1, 3 | 43 and 43 |
| ogre (`Player(12)`), two heroes in range | 1, 3 | 24 and 18 (30 x 0.8, 30 x 0.6) |
| the same, at level 2 and 3 | 2, 3 | 21 and 18 |

Who kills does not matter: a kill by one of the player's footmen 300
from the dying unit, or by one 3000 away, gave the hero in range the
same XP as its own kill. A hero 1300 from the dying unit got nothing,
one 1100 from it its share. A hero of another player that was only
`ALLIANCE_PASSIVE` with the killer got nothing.

## Edge cases

- In one test, heroes taken out with `RemoveUnit` kept taking their
  shares of later kills near the place where they had been (the XP of
  the remaining hero was the base divided by one more for each removed
  hero). This may concern only heroes removed by script.

## Open questions

1. Shared experience between allied players (`ALLIANCE_SHARED_XP`),
   `SummonedKillFactor` (0.5) for summoned units, `BuildingKillsGiveExp`
   (0) and kills of buildings, and `GlobalExperience`.
2. Experience from deaths without a killer (`KillUnit`), from
   allies' kills of units of a third player, and from creeps of other
   neutral players.
3. Attributes and their effects (`StrHitPointBonus`, `AgiDefenseBonus`,
   `AgiAttackSpeedBonus`, ...), the gains per level, skill points and
   hero abilities: next specs.

## Sources

- `Units\MiscGame.txt` and `Units\UnitBalance.slk` of 1.27b.
- Test maps RXT, RXU, RXV and RXW on 1.27b under Wine, 2026-10-21: a
  paladin of `Player(0)` at (0, 0), victims created 200 from it and
  killed by `UnitDamageTarget`, XP and levels read with JASS natives
  and written to a log.

## Verification

1. Levels: `SetHeroXP` on a new hero to 0, 100, ..., 6000; the levels
   follow the `NeedHeroXP` row of the table.
2. One hero of level 1, kills of a footman of an enemy player: 40 each;
   of a kobold of `Player(12)`: 20 each until level 2 (then 18), 0 from
   level 5.
3. Two heroes of the same player 1100 and 200 from a dying knight of an
   enemy: 43 each; with the second 1300 away: 85 and 0.
