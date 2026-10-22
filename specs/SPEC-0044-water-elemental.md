---
id: SPEC-0044
title: Water Elemental (Archmage)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, summon events, positions and orders read every tick)"]
---

## Behaviour

Water Elemental (`AHwe`, order `waterelemental`, no target) summons one
unit next to the Archmage for a limited time.

### Data

From `AbilityData.slk` (1.27b patch):

| Level | Unit | Duration (`Dur`, `HeroDur`) | Cooldown | Mana | Area |
|---|---|---|---|---|---|
| 1 | `hwat` | 60 | 20 | 125 | 200 |
| 2 | `hwt2` | 60 | 20 | 125 | 200 |
| 3 | `hwt3` | 60 | 20 | 125 | 200 |

`hwat` (`UnitBalance.slk`, `UnitWeapons.slk`): collision 32 (size
class 2), acquisition range 500, missile attack, range 300, cooldown
1.5.

### The cast

The order is accepted at once. The spell effect
(`EVENT_UNIT_SPELL_EFFECT`) follows 60 ticks (0.3 s) later for an
Archmage that stands still. At the effect, in the same tick:

- the 125 mana are spent;
- the elemental is created and `EVENT_PLAYER_UNIT_SUMMON` fires for the
  caster's player.

The caster's order ends about 541 ticks after the order (2.7 s, the rest of
the cast animation). It then has no order.

### Where it appears

Let the caster stand at `(x, y)` with facing `f` (degrees) at the
effect. The elemental is created as by `CreateUnit` (SPEC-0013,
"Position") at

    (x + 200 * cos f, y + 200 * sin f)

with facing `f`. The distance 200 equals the ability's `Area`. The
rounding is the one of `CreateUnit`. Examples for a caster at x = -7600,
-5600, -3600, -1600, 400, 4400 and y = 0:

| Facing | Elemental |
|---|---|
| 0 | (-7400, 0) and (4599.75, 0) |
| 45 | (-5458.75, 141.25) |
| 90 | (-3600, 199.75) |
| 180 | (-1800, 0) |
| 270 | (400, -200) |

If the place is taken, the free-cell search of SPEC-0013 ("Occupied
ground") moves it. A paused footman at (2500, -173.25), in front of a
caster at (2400, 0) with facing 300, moved the elemental (class 2) to
the centre of the first free cell, (2576, -240).

### Life time

The elemental dies 12006 ticks (60.03 s) after the effect. This held
for eight elementals in one test and for the first elemental of
c-RZJ-1 (summon 460, dead in the log at 12466). Its life does not drop
before that.

### What it does

The elemental has no order when it appears. It is an ordinary unit of
the caster's player without an order:

- It does not follow the caster. A caster that walked 5200 away left
  the elemental standing where it appeared.
- It acquires enemies within its acquisition range (500) like any idle
  unit, attacks them, and walks back toward its place after the target
  dies.

## Edge cases

- **Life.** The elementals of the tests had 450 life at creation and
  kept it, while `UnitBalance.slk` gives `hwat` 525 HP. The reason was
  not looked at.
- The facing used is the caster's facing at the effect. In c-RZJ-1 the
  first elemental appeared with facing 348.7 at distance 199.9 from the
  caster.

## Open questions

1. Whether the walk back after a fight ends at the exact point where
   the elemental appeared (the log of that test was cut short).
2. Levels 2 and 3 (other unit types; same rules expected).
3. Why the summoned unit has 450 life.
4. The exact tick of death relative to the game clock (12006 ticks of a
   0.005 s script timer: 60 s plus a few ticks).

## Sources

- Test map c-RZR on 1.27b under Wine, single player, 2026-10-25, in the
  analyst workspace: eight Archmages with facings 0, 45, 90, 180, 270
  and 300, one with a paused footman in front, one that walks away,
  one with a hostile creep nearby; orders, positions and deaths logged
  every 0.005 s.
- Replay c-RZJ-1 (game-data/1.27b/replays), two casts in a fight.

## Verification

1. c-RZJ-1.log.tsv: summons at ticks 460 (unit 16 at (-395, 55.25)) and
   4561 (unit 17 at (-409.5, -64.5)). Unit 16 dies at 12466 (log).
2. A caster at (-5600, 0) facing 45 summons at (-5458.75, 141.25); at
   (-3600, 0) facing 90 at (-3600, 199.75).
3. A caster at (2400, 0) facing 300 with a paused footman at
   (2500, -173.25): the elemental stands at (2576, -240).
