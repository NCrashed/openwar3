---
id: SPEC-0048
title: Target rating
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (ratings read in the running game, test maps and replays)"]
---

## Behaviour

Whenever a unit chooses among enemies, it computes a **rating key** for
each candidate and takes the candidate with the highest key; between
equal keys the strictly nearer one wins, a tie keeping the current
target or the one found first (SPEC-0019, "Target rating"). This covers:
- acquisition checks;
- checks while attacking;
- the comparison of a call's attacker with the current target.

The key is a list of parts compared in order, from the most to the
least important. Each part is either a yes/no part (yes ranks higher)
or a small number (higher ranks higher). A unit that cannot attack the
candidate at all gives it no key (it is not a candidate).

Who rates matters:
- **Every unit** uses the common parts (list A).
- **Creeps** (units of the neutral hostile player) also use the creep
  parts (list B).
- **Big creeps** (creeps of level 7 or more) also use list C.
- Lists D and E need game-wide conditions that are off in all games
  read so far (`c-RXD-1`, `c-RZH-1`, `c-RZJ-1`). They are given for
  completeness.

The parts of the four lists are interleaved in one order. The table
gives that order; the "list" column says who uses a part.

### The order

| # | Part | List |
|---|---|---|
| 1 | the candidate is not rooted (no `BEer` buff) and not in a second state that was not identified (true for every unit seen) | B |
| 2 | big creeps: the candidate is a hero within the rater's range + 150; lists D/E: damage multiplier at least 0.7, or the building rule of E | C, D |
| 3 | the candidate belongs to a non-neutral player (player number below 12) | B |
| 4 | the candidate is not a "special" unit (a state not identified) | D |
| 5 | the candidate is not attacking an enemy of the rater: it is idle, attacks the rater, or attacks an ally of the rater | A |
| 6 | as 2 for big creeps (the same rule gives both 2 and 6) | C |
| 7 | constant, equal for every candidate (three parts) | - |
| 8 | the candidate can attack (it has an attack that is not disabled) | A |
| 9 | as 5, but always "no" for creeps of level 6 or below | A |
| 10 | the candidate attacks an allied building of the rater | D |
| 11 | the candidate has a unit state not identified (from a game-wide setting) | B |
| 12 | damage multiplier at least 2.0, the candidate cannot move | big creeps, under the condition of D |
| 13 | damage multiplier at least 1.5, the candidate cannot move | as 12 |
| 14 | the candidate attacks an allied hero of the rater | B |
| 15 | the candidate can hit the rater (as 31, copied up) | B |
| 16 | in range (the range test of SPEC-0019), unless 2 applies | C |
| 17 | the candidate has a poison ability without the poison buff | B |
| 18 | the candidate is not ensnared (no `Bens` buff) | B |
| 19 | damage multiplier at least 2.0, the candidate can move | as 12 |
| 20 | damage multiplier at least 1.5, the candidate can move | as 12 |
| 21 | life below 100 | C |
| 22 | damage multiplier at least 1.0 | as 12 |
| 23 | the candidate's level, a number 0 to 15, only for candidates of the neutral players | B |
| 24 | damage multiplier above 0.6 | as 12 |
| 25 | life below 200 | C |
| 26 | life below 300 | C |
| 27 | damage multiplier above 0.4 | as 12 |
| 28 | damage multiplier above 0.2 | as 12 |
| 29 | the candidate's armour type is not "fortified" | A |
| 30 | the candidate can move; or (for a candidate that cannot move) it is a Serpent Ward | A |
| 31 | big creeps: the candidate is a hero | C |
| 32 | the candidate can hit the rater (one of its weapons can target it) | A |
| 33 | in range | A |
| 34 | slower: the candidate can move and its movement speed is below the rater's; a candidate that cannot move (other than a Serpent Ward) also gets this part | B |
| 35 | current target and in range | A |
| 36 | the candidate attacks the rater itself | A |
| 37 | the candidate attacks the rater, or an ally of the rater, or is busy with an ability with no target (a state not identified) | A |
| 38 | both the rater and the candidate are heroes | A |
| 39 | building priorities by unit type (town halls, towers, and others), ten parts | E |

The damage multiplier parts (12, 13, 19, 20, 22, 24, 27, 28) are checked only by big creeps, and only under a condition of the creep player's AI that was false in all games read. Parts 2 (D/E variant), 4, 10 and 39 need a game-wide setting of 2 or more (it was 1), which is raised to 2 for big creeps under that same condition.

### What decides in the cases read

- `c-RZJ-1` 1019, gnoll 13 (level 1): the Water Elemental and the
  Paladin, both out of range, both attacking a creep. They are equal up
  to part 34. The elemental is slower (220 against 270), so it wins.
- `c-RZJ-1` 1851, the Archmage (a player unit): only list A counts.
  Among his in-range candidates the nearest (the Ogre Lord, 454) wins.
- `c-RZJ-1` 2352, the Ogre Lord (level 7): footmen 2 and 5 are both in
  range and both attack ogre 14. Footman 5 has life 285, which gives
  part 26 "yes", and that ranks above part 35 (footman 2 is the current
  target). Footman 2 had about 334 life.
- `c-RXD-1`, `c-RZH-1`: the raters are creeps of level 1 to 3 or player
  units, so parts 16, 21, 25, 26 and 31 (list C) do not occur.

## Edge cases

- Part 34 for a candidate that cannot move: Serpent Wards get part 30
  instead.
- A rater that is not a creep stops comparing as soon as a candidate's
  upper parts (5, 8, 9) are below those of the best found so far,
  so nothing below them changes the choice.

## Open questions

1. The unidentified candidate states in parts 1, 4, 11 and 37.
2. The game-wide setting and the creep AI condition behind lists D and
   E, and the full building list of E (unit type codes of human, orc,
   night elf and undead buildings).
3. Part 2 for big creeps: the hero rule has a further condition on
   the candidate's state that was not identified.

## Sources

- The ratings of every candidate read in the running game: `c-RZJ-1`
  ticks 1019 (all eight creeps), 1851 (Archmage), 2352 (Ogre Lord).
- The game-wide setting and the creep AI condition read in `c-RXD-1`,
  `c-RZH-1` and `c-RZJ-1`.
- Constants read in the game: life bands 100, 200, 300; damage
  multiplier bands 2.0, 1.5, 1.0, 0.7, 0.6, 0.4, 0.2; near distance range
  + 150.

## Verification

1. `c-RZJ-1` 1019: creeps 13, 14, 15 switch to the elemental (part 34).
2. `c-RZJ-1` 2352: the Ogre Lord switches from footman 2 to footman 5
   (part 26).
3. `c-RXD-1` and `c-RZH-1` stay as they are (no list C parts for their
   raters).
