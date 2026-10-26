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
- Flight: the missile is a path agent of its own (commits only at the
  launch and at the impact) and flies a fixed straight line.
  - Launch in the effect tick `e`, from the caster's position floored to
    1/128 cell, towards the target's per-tick position (SPEC-0018) in
    that tick.
  - It moves at the missile speed (1000 = 0.15625 cells per tick) along
    that direction. Every 0.1 s after the launch a homing update looks
    at the target: if the target is not moving at that moment (it
    stands, even if it moved since the last aim), nothing changes (no
    new measure, no hit test, also with the target less than one step
    away), and the straight flight goes on to its end; if it is moving,
    the bolt is aimed anew
    from its logical point (the launch point moved 100 towards the aim
    point per 0.1 s step), and if the target's per-tick position is
    then less than one step (100) away the bolt hits (a 0.0001 s timer,
    so in the same tick): its agent is placed at the target, makes one
    last step and the impact follows. `c-SAK-1`: the first bolt (a
    standing kobold, `d` = 20.5378) had six updates without effect,
    also the sixth with 1.79 cells left, and landed at `e + 132`; the
    second (a gnoll walking away, launch q 12.969920, 4.21 cells to its
    per-tick position) was re-aimed at its first update, found the gnoll
    1.909 cells (61.1) away and hit in that tick (`e + 20`), not after
    the straight count.
  - A target already within one step (100) at the launch is hit at
    once, in the effect tick. `c-SAK-1` (log): the effect on gnoll 17,
    89.3 away, and its damage events (0.00, then 100.00) are all in
    tick 7982.
  - The straight flight is a timer of `d / 1000` seconds (`d` the
    launch distance in world units), counted from the world clock of
    the effect tick. When it fires, the agent is placed at the flight's
    end (the launch point plus `n = ceil(d / 0.15625)` steps for `d`
    in cells, so it slightly overshoots) and a 0.0001 s timer starts;
    that timer gives the impact (damage and stun). So the impact is in
    the first tick whose queue time is at least `clock(e) + d / 1000 +
    0.0001`. This is mostly tick `e + n`, but it is one tick later when
    the 0.0001 s crosses a tick's queue time (bolt 2 below).
  Read in the game, `c-RZH-1`, both bolts on creep 8, ticks by the
  movement clock:
  - bolt 1: the target was walking, `d` = 8.1708, `n` = 53, launch 891,
    impact 944 (the map's log labels the damage 943); creep 8 walked on
    for 10 ticks and stopped (tick 901) for a swing. At both homing
    updates (q 4.554908 and 4.654901) it stood, so no re-aim and no hit
    test, although at the second the bolt's logical point was 50.8 from
    it;
  - bolt 2: standing target, `d` = 4.99756 (flight 0.159922 s from
    clock 13.898668, launch tick 2781), `n` = 32: the agent reaches the
    flight's end in tick 2813 (q 14.058641), 0.0016 cells past the
    target's centre, but the impact timer (14.058690) is past that
    tick's queue time, so the impact is in tick 2814 (q 14.063641).
  The two earlier tests (damage in `e + 41` and `e + 20`, targets
  walking towards the caster) also fit this rule, checked by the
  implementer.
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
  `max(speed x (1 - DataC), MinUnitSpeed)` (`DataC` 0.5, `MinUnitSpeed`
  150 from the game constants): 270 -> 150 for two ogres and a paladin
  (`c-SBG-1`; the clamp is read from the data, a base speed above 300
  would show the cut itself).
  - Slow start on a walking unit (`c-SBG-1`, ogre 3): inside the buff's
    application its walk is made again (search, cluster, path), its
    agent is committed at its per-tick position, and it goes on at the
    new speed from there at once.
  - Slow end (the buff's timer): no new search and no commit; the unit
    takes the old speed back at its next update, so the time from the
    end to that update is still walked at the slowed speed.
  - For a creep the buff's end gives the same immediate order as the
    end of a creep's stun (one object; a second one when it is carried
    out). A standing creep carries it out at once; a walking one only
    when its move order ends (`c-SBG-1`: ogre 3 at its move's end,
    0.92 s later).
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

### The order after a stun

While stunned the unit makes no updates of its walk (no member step,
no look-ahead check). When the stun ends its order is given again as
if new: a new cluster is made (SPEC-0025), and at the unit's next
update the cluster's search (limit 5000), its own search (limit 400)
and the local search are made from where it stands, and it walks on
from there. Replay `c-RXD-1`, worker 5 walking to the mine (read in
the game): stopped in tick 4337 at (439.3159, 386.1949) cells (speed
0, order stunned); no updates until the order came back in tick 4739
(402 ticks later, as above); in 4741 a new cluster, the three searches
from (439.3159, 386.1949) towards the mine's point (448, 384), local
path of 5 points; it walked from 4747.

### The end of a stun, step by step

Read in the game (`c-SBI-1`, war stomp at q 4.970000 with `Dur` 3 s;
`c-SAK-1`, storm bolt and bash; the same steps for all three). With
`E` the effect's queue time:
1. `E + Dur`: the stun buff's timer ends. At that moment the unit takes the
   buff-end object, posts the free of the stun object, and starts a
   0.01 s timer on the unit (also a 5.0 s timer on a creep's camp AI and
   a 3.333 s timer on an effect object). Several stuns ending at the
   same expiry run in the order the stuns were made.
2. `E + Dur + 0.01`: the unit's timer ends and its order comes back:
   the order the unit had before the stun, with its target (an attack
   order on a unit comes back as that attack order; no order gives
   order 0). It makes the order-back object, starts the AI's 2.0 s
   timer and 0.01 s check timer (SPEC-0019), and a 0.01 s timer of the
   stun object.
3. `E + Dur + 0.02`: both 0.01 s timers end, in the order they were
   started. The AI check runs its acquisition (calls for help
   included). The stun object's timer makes a damage event of 0.00 from
   the caster on the unit, then posts the free of the order-back
   object (so that free comes right after the 0.00 hit's frees). When
   the restored order starts the check timer again (an attack order
   coming back), the check is started a second time after the stun
   object's timer and so runs after it.
A unit that dies while stunned: the buff goes at the death, the
order-back object is made, and the stun object's 0.01 s timer frees it
0.01 s later with no 0.00 hit (`c-SAK-1` creeps 11, 14, 16).
This explains the 2 or 3 ticks of "Stun timing": the order comes back
0.01 s after `E + Dur`.

### A stun and the unit's attack

Read in `c-SBI-1` (four creeps under war stomp, in different states of
their attacks) and `c-SAK-1`:
- the attack loses its target at the stun: from then on it holds no
  target;
- a swing not yet past its damage point is dropped whole: its hit never
  comes and its cooldown timer is cancelled (a kobold that swung 0.02 s
  before the stun);
- the timers of a finished swing run on (after-swing event, cooldown
  end); each finds no target and does nothing;
- the AI's check timer is cancelled: no acquisition is made for the
  stunned unit until the 0.01 s check after the order comes back;
- the unit's order is put aside and comes back with its target (step 2
  above); a creep with an attack order on its stunner (`c-SAK-1`, ogre
  18 bashed while attacking the mountain king) attacks him again
  through that order, while its attack's own target is gone.

## Edge cases

- Storm bolt and bash damage ignore the armor value; storm bolt against
  a hero is reduced by the hero factor of spells.

## Open questions

1. The launch point of storm bolt (the caster's centre in both tests).
2. The exact attack interval of thunder clap's slow while a
   slow starts or ends during a cooldown.
3. (Settled: the order comes back 0.01 s after the buff's end, see
   "The end of a stun, step by step".)
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
