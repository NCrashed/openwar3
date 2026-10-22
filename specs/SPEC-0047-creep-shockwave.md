---
id: SPEC-0047
title: Creep spells - Shockwave of the Ogre Lord
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, spell, order and damage events read every tick, values read in the running game)"]
---

## Behaviour

Neutral hostile creeps use some of their spells on their own. This
spec covers the creep Shockwave (`ACsh`, base code `AOsh`, order
`shockwave` 852125) as the Ogre Lord (`nogl`) of c-RZJ-1 uses it. The
Ogre Lord's other ability, `ACav`, is the creep Devotion Aura (base code
`AHad`, +3 armour, area 900, SPEC-0038).

### Data

`ACsh` (`AbilityData.slk`, 1.27b patch): damage 75 (`DataA`), maximum
damage 99999 (`DataB`), distance 800 (`DataC`), final area 150
(`DataD`), area 150, cast range 700, cooldown 8, mana 100, targets
ground, structure, enemy. Ogre Lord: 950 life, 500 mana, mana
regeneration 1.25 per second.

### When the creep considers the spell

The spell is considered at two kinds of events. Each has a **victim**
(a unit of the creep's side) and an **attacker** (an enemy unit):

1. **A hit.** An enemy attack hits the creep itself, at the moment the
   damage is dealt. The victim is the creep, the attacker is the unit
   that hit. With several attackers this happens once per hit, so at
   the period of each attacker's cooldown.
2. **A call for help** (SPEC-0030). When a creep of the camp acquires
   or is hit, the call reaches the Ogre Lord, and it considers the
   spell against the attacker named by the call once for each creep the
   call reaches. In c-RZJ-1 this happened at tick 636, when the whole
   camp acquired the Paladin, before any damage.

### The decision

At each such event the spell is cast at the attacker when all of the
following hold:

1. **Ready.** The cooldown is over and the creep can cast the spell
   now. In particular it has the mana and is not casting already.
2. **Enough enemies.** Count the enemy units whose distance from the
   attacker's position is smaller than `150 + collision`, with each
   unit's own collision size; the attacker counts itself. The spell
   is cast if the count is at least the **threshold**:
   - 2, if the victim is the creep itself and its life is below 400;
   - 3 otherwise.
3. **One at a time.** While another creep of the same player has a
   creep spell request in progress (from its order to the end of its
   cast), the request is refused. The creep tries again at its next
   event.

Examples: one knight hitting an Ogre Lord never makes it cast. Two
knights side by side (120 apart) make it cast only once its life is
below 400. Three knights in a row 100 apart make it cast at the first
hit of the middle knight. In c-RZJ-1 at 636 the count around the
Paladin was 3 (the Paladin and two footmen 110 and 119 from him).

The count boundary was probed with a knight and two paused footmen
(collision 31) at distance `d` from it. With `d` up to 178 the Ogre
Lord cast; with 182, 186 and 190 it did not (count 1). The bound is
`150 + 31 = 181`.

### The cast

The order is a unit target order on the attacker. The creep does not
move for it in the cases seen. The spell effect comes 111 ticks after
the order, and the order ends 214 ticks after it (the creep then has no
order). After the cast the cooldown of 8 s runs from the effect.
Repeated casts with three knights came 1800 ticks apart: the cooldown
ends 1600 ticks after the effect, and the next hit after that is
taken.

### The wave

The wave starts at the effect and moves from the caster toward the
target's position at the effect, along that line, for `DataC` (800).
It is updated every 20 ticks (0.1 s) after the effect, and once more
when it reaches its end:

- At update `k = 0, 1, 2, ...` (tick effect `+ 20k`) its centre is
  `150 + 105k` from the caster along the line (speed 1050 per second).
- The last update comes at effect `+ 152` (800 / 1050 s), with the
  centre at `150 + 800 = 950`.
- At each update every enemy unit whose distance from the centre is
  smaller than `150 + collision` (its own collision size) and that the
  wave has not hit yet takes the damage, 75. A unit is hit at most once.

Hit ticks for footmen (collision 31) at `(x, y)`, with `x` along the
line from the caster and `y` across it:

| Footman at | Hit at effect + |
|---|---|
| (300, 0) | 0 |
| (350, 0), (400, 0) | 20 |
| (500, 0) | 40 |
| (600, 0), (600, 60), (600, 90), (600, 120) | 60 |
| (600, 150), (600, 170), (700, 0) | 80 |
| (800, 0), (850, 0) | 100 |
| (850, 150), (900, 0), (950, 0) | 120 |
| (1000, 0), (1000, 100), (1050, 0), (950, 100) | 140 |
| (1000, 150), (1100, 0) | 152 |
| (400, 180), (600, 190), (850, 200), (1150, 0) | not hit |

So (600, 120) is hit by the circle at 465 (distance 180.6) and
(400, 180) is missed, between the circles at 360 (184.4) and 465
(191.1).

A hero takes 75% of it (the Paladin took 56.25), other units the full
75 (footman, knight; no armour reduction in the damage event).

## Edge cases

- **Several Ogre Lords** of player 12 that all qualify in the same
  round of hits cast one after another, one per round. Six such
  creeps cast in a fixed rotation, each every 1800 ticks; a seventh
  that also qualified got its first turn only after 5780 ticks.
- In c-RZJ-1 the Paladin was 730 from the Ogre Lord (cast range 700)
  and the Ogre Lord cast without moving.

## Open questions

1. The exact rule of the per-player limit (which request wins, and
   when the next one may start).
2. Which creep spells besides Shockwave follow the same pattern
   (thresholds and radius come from the ability).
3. The range check against the cast range.

## Sources

- Test maps c-RZY, c-RZZ, c-SAB, c-SAC and c-SAD .. c-SAG on 1.27b
  under Wine, single player, 2026-10-25 and 26, in the analyst
  workspace: Ogre Lords with knights, footmen and paused probes;
  orders, spell effects, acquisitions and damage logged every 0.005 s.
  The count, the threshold and the life bound were read in the running
  game at each decision.
- Replay c-RZJ-1 (game-data/1.27b/replays).

## Verification

1. c-RZJ-1: the Ogre Lord (unit 11) is ordered to Shockwave the
   Paladin (unit 1) at tick 636 (log o row at 637). Later casts at
   orders 2424, 4212 and 6372.
2. An Ogre Lord with three knights on hold position at (-160, -100),
   (-160, 0), (-160, 100) from it: Shockwave at the first hit of the
   middle knight, then every 1800 ticks.
3. With two knights: the first cast comes at the first hit after its
   life drops below 400.
