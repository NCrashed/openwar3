---
id: SPEC-0045
title: Divine Shield (Paladin)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (script test maps, buff level, orders, attack and damage events read every tick)"]
---

## Behaviour

Divine Shield (`AHds`, order `divineshield`, no target) makes the
Paladin invulnerable for a time. The buff is `BHds`.

### Data

From `AbilityData.slk` (1.27b patch):

| Level | Duration (`Dur`, `HeroDur`) | Cooldown | Mana |
|---|---|---|---|
| 1 | 15 | 35 | 25 |
| 2 | 30 | 50 | 25 |
| 3 | 45 | 65 | 25 |

### Timing

The spell effect comes in the same tick as the order, with no cast
delay. The buff (`GetUnitAbilityLevel(u, 'BHds')`) is present from the
next tick. It is removed `Duration` seconds after the effect: at level 1
the effect was at tick 399 and the buff was gone at tick 3400
(3001 ticks).

### While the shield is up

- **No damage.** Units that were attacking the Paladin stop. No attack
  on him starts (`EVENT_PLAYER_UNIT_ATTACKED` does not fire) and no
  damage event fires for him.
- **Attackers drop him.**
  - Units that attacked him on their own (acquired target, no order)
    drop him as a target and look for another one. In c-RZJ-1 the
    shield came at tick 4801. The attacking kobold acquired a new
    target at 4807 and the gnoll at 4839.
  - Units with an explicit `attack` order on him lose the order (current
    order 0) in the next tick. One footman lost it 51 ticks later,
    likely at the end of its swing.
- **New orders on him.** An `attack` order with the shielded Paladin as
  target is accepted (`IssueTargetOrder` returns true), and so is an
  attack-move to his position. Neither leads to an attack. A `smart`
  (right click) order on him is refused.
- **The Paladin fights on.** He acquires and attacks enemies as usual.
  An enemy footman hit by him received a `move` order (851986) in the
  next tick and was idle again 166 ticks later (it walked off). Further
  hits repeated this.

### The Paladin's own target checks

The cast ends the Paladin's attack and leaves him with no order. No
target check of his runs after it until he gets a new order. Read in
the game, `c-RZJ-1`: no check and no check timer of his between 4780
and 4899, although kobold 8, which he had attacked just before the
shield, stood close. The script's attack-move at 4901 set his check
to 0.01 s, the check of 4902 took a target, and his reaction delay
(0.0557 s) followed. A cast with a backswing behaves differently: the
Archmage's Water Elemental order ended at 5042, and his check of 5043,
0.01 s later, took the Ogre Lord (SPEC-0035, item 6).

### When the shield ends

In the tick the buff is removed (3400 in the test), every idle enemy
around that can reach him acquires the Paladin again
(`EVENT_UNIT_ACQUIRED_TARGET` for all of them in that tick), and attacks
resume.

## Edge cases

- Explicit orders on the shielded Paladin end without an attack: a
  footman ordered to attack him from 700 away had order `attack` from
  tick 803 to 1535, and one with attack-move from 803 to 1355. Where it
  walked and why the order ended there was not logged.

## Open questions

1. The direction of the walk of a unit hit by an invulnerable attacker,
   and its distance (166 ticks of walking at footman speed).
2. Area damage on the shielded Paladin (Blizzard, SPEC-0043) and
   friendly spells on him (Holy Light).
3. Whether the next acquisition after the shield follows the acquisition
   timer of SPEC-0019 or happens at once (c-RZJ-1: +6 and +38 ticks).

## Sources

- Test map c-RZT on 1.27b under Wine, single player, 2026-10-25, in the
  analyst workspace: three level 1 Paladins with hostile footmen
  (attacking on their own, with an explicit attack order, and ordered
  in while the shield is up).
- Replay c-RZJ-1 (game-data/1.27b/replays): Divine Shield at tick 4801.

## Verification

1. c-RZJ-1.log.tsv: cast at tick 4801. No damage to unit 1 after that
   tick until the end of the shield; unit 8 acquires unit 4 at 4807,
   unit 12 acquires unit 16 at 4839.
2. A level 1 Paladin with three hostile footmen next to him: buff from
   the tick after the order for 3000 ticks; no attack on him in that
   time; all footmen acquire him in the tick the buff ends.
