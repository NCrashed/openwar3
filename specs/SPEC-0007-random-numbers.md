---
id: SPEC-0007
title: Script random numbers (GetRandomInt, GetRandomReal, SetRandomSeed)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

### Generator

The generator has a state of two 32-bit words, `a` and `b`, and uses a
table `T` of 61 unsigned 32-bit values (`SPEC-0007-rng-table.tsv`). `b`
holds four byte offsets into the table, one per byte:
`b = (o0 << 24) | (o1 << 16) | (o2 << 8) | o3`. All arithmetic is modulo
2^32; `rotl(x, n)` rotates left by `n` bits.

One step:

1. Each offset moves down by its stride and wraps around its cycle:

   | Offset | Stride (bytes) | If the result is negative, add | Cycle |
   |---|---|---|---|
   | `o0` | 4 | 188 | 47 entries |
   | `o1` | 12 | 212 | 53 entries |
   | `o2` | 24 | 236 | 59 entries |
   | `o3` | 28 | 244 | 61 entries |

   The new offsets replace the old ones in `b`.
2. With `w(o) = T[o / 4]` for the new offsets:
   `a = a + (rotl(w(o2), 3) XOR rotl(w(o1), 2) XOR w(o3) XOR rotl(w(o0), 1))`.
3. The step's output is the new `a`.

### Seeding

`SetRandomSeed(s)`, with `s` taken as an unsigned 32-bit value:

- `a = s`;
- `b = ((s mod 59) << 10) | ((s mod 61) << 2) | ((s mod 53) << 18) |
  (((s / 47) * 17 + s) << 26)` (integer division, the last term modulo
  2^32);
- then one step is made and its output is discarded.

The first random value after `SetRandomSeed(s)` is the output of the
second step. With this seeding `o0` starts at a multiple of 4 between 0
and 184, so all offsets stay below 244 and only the table entries 0..60
are ever read.

### Natives

Each native call that needs a random value makes one step and uses its
output `r`.

- `GetRandomInt(lo, hi)`:
  - if `lo == hi`, the result is `lo` and **no step is made**;
  - otherwise `d = |hi - lo|` (as an unsigned 32-bit value) and the
    result is `lo + floor(r * (d + 1) / 2^32)` (64-bit product, 32-bit
    wrap of the sum). For `hi < lo` the result is between `lo` and
    `lo + d`, above `lo`: `GetRandomInt(6, 1)` returns 6..11. When `d + 1`
    wraps to 0 (`lo = -2147483648`, `hi = 2147483647`) the result is
    always `lo`.
- `GetRandomReal(lo, hi)` (operations of SPEC-0004):
  - `d = sub(hi, lo)` with the sign bit cleared;
  - if `d < 2e-7` (`0x3456BF95`), the result is `lo` and **no step is
    made**;
  - otherwise `x = add((r AND 0x7FFFFF) OR 0x3F800000, -1.0)` (a value in
    [0, 1) with 23 random bits) and the result is
    `add(mul(x, d), lo)`. As for integers, for `hi < lo` the result lies
    above `lo` (`GetRandomReal(7.25, 2.5)` returns values from 7.25 to
    12.0).

### Game generators

Besides the script generator there are 45 game generators, numbered
0..44, with the same step and table. `SetRandomSeed(s)` also reseeds them:

1. The script generator is seeded with `s` and one step is made (as
   above); call that step's output `r1`.
2. A helper generator is seeded with `r1` by the seeding formula above,
   without the discarded step.
3. 45 steps of the helper give `v0..v44`; game generator `i` is seeded
   with `v_i`, again without a discarded step.

The script natives (`GetRandomInt`, `GetRandomReal`) do not use the game
generators, and the game generators do not advance the script generator.
Generator 2 rolls the damage dice of unit attacks (SPEC-0016), and each
unit with a weapon draws one output of it when it is created (the phase
of its acquisition checks, SPEC-0019); generator 9 draws the reaction
delay of attack orders and of acquisitions (SPEC-0019). The other uses
are not known yet.

### State at game start

Every game starts from the same state, before `main` of the map script
runs. With `s0 = 0x77617233` (the bytes of the text "3raw" read as a
little-endian word):

- the script generator is seeded with `s0` by the seeding formula,
  **without** the discarded step: `a = 0x77617233`, `b = 0x2C08AC40`;
- a separate helper generator is also seeded with `s0` (no discarded
  step); its first 45 outputs `w0..w44` seed the game generators: game
  generator `i` is seeded with `w_i`, without a discarded step. Game
  generator 0 is seeded with 0x2C46961D, generator 1 with 0xB9614EFC.

So the first script random value of a game that has not drawn any
before is the output of one step from that state: three
`GetRandomInt(0, 2147483647)` at the start give 371411726, 1555081086,
1646406250.

The random seed stored in the replay header differs from game to game,
but it does not take part in this: the state was the same in three
single player games with different header seeds, and a replay played
back with its header seed changed drew the same values. A replay played
back starts from the same state, so playback repeats all random values.
Network games were not tested.

### Table equivalence

The table was recovered from measured outputs. Any table that differs
from it by XOR of one constant `c` into every entry, where
`rotl(c, 3) XOR rotl(c, 2) XOR c XOR rotl(c, 1) = 0` (three such bits
exist), produces exactly the same outputs; the published table is one
member of that class.

## Edge cases

- `SetRandomSeed(-1)` and `SetRandomSeed(-2147483648)` use the unsigned
  values 4294967295 and 2147483648.
- `GetRandomInt` and `GetRandomReal` with equal bounds do not advance the
  generator, so they do not change later random values.

## Open questions

1. The state at game start in network games. (An earlier note gave
   57869864 as the first `GetRandomInt(0, 2147483647)` in single player;
   that map drew random numbers during its initialisation. On a map that
   does not, the first value is 371411726, see "State at game start".)
2. What the game generators other than numbers 2 and 9 are used for.

## Sources

- Analysis of the 1.27b game library for the structure of the generator,
  the seeding and the natives.
- Test maps on 1.27b, 2026-10-17. The table was not copied from the game:
  it was solved from 704 differences of consecutive outputs measured for
  64 seeds (a linear system over GF(2), since the mixing is XOR of
  rotations), and then checked against 250 further outputs of 25 other
  seeds, including the first output after each `SetRandomSeed`.

## Verification

- `SPEC-0007-vectors.tsv`: 89 seeds with the 10 or 12 generator outputs
  that follow `SetRandomSeed`, measured in the game. Each full output was
  read in two runs with the same seed: `GetRandomInt(0, 2147483647)`
  gives its top 31 bits and `R2I(GetRandomReal(0.0, 1.0) * 8388608.0)`
  its low 23 bits.
- With `SetRandomSeed(777)`: `GetRandomInt(6, 1)` six times gives
  7 10 8 7 11 7; `GetRandomInt(1, 6)` gives 2 5 3 2 6 2;
  `GetRandomReal(2.5, 7.25)` printed with `R2SW(x, 1, 9)` gives
  4.846750208 5.005834102 3.415878752 5.382181632 4.171306608
  2.869395968.
- Game start: on a map that draws no random numbers before, the script
  generator state is `a = 0x77617233`, `b = 0x2C08AC40` and game
  generators 0 and 1 are `(0x2C46961D, 0x3CB404A0)` and
  `(0xB9614EFC, 0x2C1C141C)` (states read from memory); the first three
  `GetRandomInt(0, 2147483647)` are 371411726, 1555081086, 1646406250, in
  the game and when its replay is played back.
