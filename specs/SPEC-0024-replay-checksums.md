---
id: SPEC-0024
title: Replay checksums (turn checksum)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, memory reads and replays of test maps)"]
---

## Behaviour

### Notation

All values are unsigned 32-bit, arithmetic is modulo 2^32.

- `rol3(x)`: rotate left by 3 bits.
- `fold(h, x) = rol3(h XOR x)`.
- `bh(x)` of a 32-bit value: the sum over its four bytes `b` of
  `(b * 0x7A2D) XOR b`. Of a byte string: the same sum over all its
  bytes. The byte order does not matter (it is a sum).
- The id of an object is a pair `(i, n)`: `i` is the number of the entry
  the object holds in the engine's object table, `n` is its creation
  number (see "Object table"). `idsum(obj) = rol3(i XOR n)`.

### Where the checksum is

In a replay every time slot block (id 0x1F, also 0x1E) is followed by a
block 0x22 with a length byte of 4 and a 32-bit little-endian value. The
value is the turn checksum the recording player computed for that time
slot. The same value goes into the player's network message for the
turn. A game shows no message when a replay is played back and its own
checksums differ from the recorded ones: playback continues.

Turns are numbered `k = 1, 2, ...` in replay order. `t_k` is the sum of
the time increments (ms) of time slots `1..k`, `t_0 = 0`.

### Turn checksum

    v = rol3(S XOR 6059)
    v = fold(v, B)
    v = fold(v, C)
    v = fold(v, D)
    checksum = fold(v, E)

- 6059 is the build number of 1.27b.
- `S`: the system sum (below).
- `B`: in single player, the game time in ms that has run when the
  checksum is computed, which is `25 * floor(t_(k-1) / 25)` (the game
  runs in steps of 25 ms, SPEC-0002). In network games a different value
  may take its place (open question 1).
- `C`: the number of the entry the next object will get in the object
  table: the first entry of the free list, or the size of the table when
  the free list is empty.
- `D`: the next creation number.
- `E = bh(a) + bh(b)` of the script generator state `a`, `b` (SPEC-0007).

### System sum S

Twenty slots, numbered 0 to 19, each empty or holding one value `c_i`.
`S` folds them from slot 19 down to slot 0, starting from 0 and skipping
empty slots: `h = fold(h, c_i)`.

| Slot | Value |
|---|---|
| 0 | cheat flags: the set of active cheats, with bit 12 cleared (0 without cheats) |
| 1 | empty |
| 2 | `N`, the network sum (below) |
| 3 | `R`: the sum of `bh(a) + bh(b)` over the 45 game generators of SPEC-0007 |
| 4 | static (game data table) |
| 5 | static (ability data) |
| 6 | `G`, the game sum (below) |
| 7 | static (game data table) |
| 8 | empty |
| 9 | static (upgrade data) |
| 10, 11 | empty |
| 12 to 18 | static: object data of units, items, destructables, (15 empty), abilities, buffs, upgrades |
| 19 | static (other custom data, 0 in the tests) |

Static values are computed once from the loaded data and do not change
during the game. Measured on test maps made from `(12)IceCrown` without
changed object data:

- slots 19 down to 7 folded: `S7 = 0xEA73D958`, so that
  `S = fold(fold(fold(fold(fold(fold(S7, G), c5), c4), R), N), c0)`;
- `c4 = 0x414C0D79`, `c5 = 0xA996F046`.

Every step above is linear over XOR, so the checksum can also be written
as `F XOR K`, where `F` is the checksum computed with all static values
set to 0 and `K` depends only on the static values. An implementation
that does not reproduce the static values can take `K` from the first
recorded checksum whose other parts it knows, and use it for the rest of
the game.

### Network sum N

`N` starts at 0. For every time slot block of the replay, in order, with
`p` the block's payload (the 2-byte time increment followed by the
action data, that is, all bytes after the block's length field):

    w = 0
    for each whole little-endian 32-bit word x of p: w = fold(w, x)
    for each remaining byte y of p (at most 3):    w = fold(w, y)
    N = N + bh(id + bh(w))

with `id` the block id (0x1F). The checksum of turn `k` includes the
blocks `1..k`. A few other network messages (leaving the game and two
more) also add `bh(id)`; this was not tested.

### Game sum G

    G = 12
    for player = 0 .. 11: G = fold(G, P(player))

Players 12 to 15 (neutral) do not take part. All 12 slots counted in the
tests, including empty ones.

The player sum:

    h = fold(T, h1)
    h = fold(h, h2)
    for each filled entry obj of the player's 7-entry list: h = fold(h, idsum(obj))
    P = fold(Y, h)

- `h1`, `h2`: two 32-bit fields of the player, both the float 1.0
  (`0x3F800000`) for every player in the tests (open question 3).
- The 7-entry list was empty in all tests (open question 3).
- `T`: the type record sum. Each player has a list of records, one per
  unit type, kept in the order they were created. With the record's
  values `q0 .. q6`:

      x = rol3(q2 XOR q1)
      y = rol3(q4 XOR q0)
      z = rol3(q5 XOR q3)
      r = rol3(rol3(rol3(x XOR y) XOR z) XOR q6)

  `T` starts at 0 and does `T = fold(T, r)` for each record in list
  order, then `T = fold(T, u)` for each value `u` of a second list that
  was empty in the tests. The type id itself is not part of the sum.
  Measured values: a footman record has `q0` = 2147483647 (no limit),
  `q1` and `q4` = the number of footmen (1, then 2 with two footmen),
  `q2`, `q3`, `q5` = 0, `q6` = 8. Every player had three more records
  made at game start, before any unit: `hrtt` (all 0, `q6` = 8), `otbk`
  (the same) and `uske` (`q0` = 25, the rest 0, `q6` = 8).
- `Y`: the selection sum of the player, from the selection state that
  is kept in sync between players (SPEC-0006 for `SyncSelections`):

      Y = number of selected units
      for each selected unit u, in selection order: Y = fold(Y, idsum(u))
      Y = fold(Y, j)    j = number of the active subgroup (from 0), or 0xFFFFFFFF if none
      for each subgroup, in order:
          Y = fold(Y, number of units in it)
          for each unit u in it: Y = fold(Y, idsum(u))
      for each of the 10 control groups: for each unit u: Y = fold(Y, idsum(u))
      if there is a lead unit: Y = fold(Y, idsum(lead))
      if there is a second marked unit: Y = fold(Y, idsum(it))

  Subgroups are the groups of same-type units within a selection. The
  lead unit was the first selected unit in the test; the second marked
  unit was never set (open question 4).

### Object table

The engine keeps one table of internal objects. Each object holds one
entry; freed entries go to a free list and are reused, the last freed
first. Every new object also gets the next creation number. The table
holds many objects besides units: on an empty test map it had about 870
entries before the script created anything (path finding data, flags
and numbers kept in sync, regions and others). A unit's id pair is its
entry in this table and its creation number, for example `(825, 828)`
and `(857, 863)` for two footmen. `C`, `D` and every `idsum` therefore
depend on the creation and freeing of all these objects (open
question 2).

## Edge cases

- Only the sum `R` over the game generators is used, so two generator
  states with the same byte sums give the same checksum.
- The first turns of a game (before about 1.2 s on the test map) had
  other `C` and `D` values while the script created objects.
- The cheat slot clears one bit, so one cheat flag never changes the
  checksum.

## Open questions

1. Network games: a flag that was 0 in single player makes the game use
   a sum over the objects of the object table in place of `B`. When the
   flag is set and what that sum is was not studied.
2. Which actions create and free entries of the object table, and in
   which order. Without this `C`, `D` and the id pairs cannot be
   reproduced; see Verification 4 for a way to use the checksum anyway.
3. The meaning of `h1`, `h2` (possibly handicaps) and of the 7-entry
   list of the player.
4. The second marked unit of the selection; the key order of the 10
   control groups (groups were empty in the tests).
5. The static values on other maps and with changed object data.

## Sources

- Analysis of the 1.27b game library: how the turn checksum and its
  parts are computed.
- Memory reads of the running game (1.27b under Wine): the stored turn
  checksum matched the replay's 0x22 values (68 of 68); dumps taken
  while the game was paused by the script at different times in two
  games gave the same `S7`.
- Replays of test maps recorded with `EndGame(false)` (they are in
  `game-data/1.27b/replays/`, see its `README.txt`).

## Verification

1. **Byte hash:** `bh(0x00000001) = 0x7A2C`, `bh(0x00000100) = 0x7A2C`,
   `bh(0x000000FF) = 0x79B22C`, `bh(0x12345678) = 0x83B8F0`,
   `bh(0xFFFFFFFF) = 0x1E6C8B0`.
2. **Network sum from a replay:** for `c-RSF-1.w3g`, the column `N` of
   `SPEC-0024-vectors.tsv` gives `N` after each turn; it depends only on
   the replay's time slot blocks.
3. **Full checksum:** for each row of `SPEC-0024-vectors.tsv` (turns 12
   to 206 of `c-RSF-1.w3g`), the formula above with the row's `B`, `N`,
   `C`, `D`, `E`, `R`, `G`, the constants `S7`, `c4`, `c5` and `c0 = 0`
   gives the row's `checksum`, which is the replay's 0x22 value. All 195
   rows match.
4. **Using the checksum without the object table:** between two turns
   in which no object is created or freed, no random number is drawn and
   the selection and type records do not change, only `B` and `N`
   change, and both follow from the replay. Undoing them from two
   consecutive checksums gives the same remainder; a turn where the
   remainder changes is a turn in which the game state changed in one of
   the other parts. In `c-RSF-1.w3g` this happens at turn 41 (4286 ms),
   when `C` went from 851 to 858.
