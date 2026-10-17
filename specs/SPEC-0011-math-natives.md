---
id: SPEC-0011
title: Math natives (Sin, Cos, Tan, Asin, Acos, Atan, Atan2, SquareRoot, Pow, Deg2Rad, Rad2Deg)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

All these natives work on the engine reals of SPEC-0004 and use only its
operations (`add`, `sub`, `mul`, `div`, `recip`, `i2r`) plus integer
arithmetic. They do not use the processor's floating point or a C math
library, so the results are fully deterministic. Notation as in
SPEC-0004: `e(x)` is the exponent field, `f(x)` the fraction field of a
32-bit pattern; "mod 2^32" marks unsigned 32-bit wrap. Comparisons
written with `<`, `>`, `<=`, `>=` between reals are ordinary comparisons
of the float values.

Additional helpers:

- `f2i(x)`: truncation toward zero **without saturation**. If `e(x) < 127`
  it is 0. Otherwise `m = 2^23 + f(x)`; if `e(x) <= 150` the magnitude is
  `floor(m / 2^(150 - e(x)))`, else it is `(m * 2^((e(x) - 150) mod 32))
  mod 2^32`; it is negated (mod 2^32) when the sign bit is set, and read
  as a signed 32-bit integer.
- `trunc(x)`: `x` with the fraction bits below its integer part cleared:
  0 if `e(x) < 127`, `x` unchanged if `e(x) >= 150`, otherwise `x` with
  its lowest `150 - e(x)` bits cleared.
- `scale(v, k)`: `i2r(v)` with `k` subtracted from its exponent field
  when the result is not zero (an exact division by `2^k`).
- `hi(a, w)`: `floor(a * w / 2^32)` for unsigned 32-bit `a` and `w`.
- `neg0(x)`: true when `x` is negative and not `-0.0` (sign bit set and
  some other bit set).

### Tables

Three integer tables are defined by formulas. `SPEC-0011-tables.tsv`
lists their values, computed from these formulas (with 60 significant
digits) and checked against the game.

- `S[i] = floor(2^31 * sin(i * pi / 2048))` for `i = 0..1023`, and
  `S[1024] = 2^31 - 1`.
- `A[i] = floor(2^29 * acos(i / 1024))` for `i = 0..1023`.
- `N[i]` for `i = 0..137`:
  - `N[0] = 2^31 - 1`;
  - for `1 <= i <= 119`, with `z = floor((i + 120) / 8)`,
    `k = (i + 120) mod 8` and `d = 2^(31 - z) * (16 - k) / 8`:
    `N[i] = trunc(F(2^34 * acos(1 - d / 2^24)))`, where `F` rounds to the
    nearest 32-bit float;
  - `N[120..123] = 11863283` (the same formula at `d = 4`),
    `N[124..127] = 8388608` (`d = 2`), `N[128..135] = 5931641` (`d = 1`),
    `N[136] = N[137] = 0`.

### Constants

| Name | Pattern | Value |
|---|---|---|
| `K_ANG` | `0x4822F983` | 2^20 / (2 pi) |
| `PI` | `0x40490FDB` | pi |
| `PI_2` | `0x3FC90FDB` | pi / 2 |
| `M_PI_2` | `0xBFC90FDB` | -pi / 2 |
| `PI_6` | `0x3F060A92` | pi / 6 |
| `T12` | `0x3E8930A3` | tan(pi / 12) |
| `T6` | `0x3F13CD3A` | tan(pi / 6) |
| `M_T6` | `0xBF13CD3A` | -tan(pi / 6) |
| `P0`, `P1`, `Q1` | `0x3F7FFFF0`, `0x3E8415A6`, `0x3F17592E` | atan coefficients |
| `L1`, `L2` | `0xBE88D424`, `0xBF19BF59` | log coefficients |
| `LN2`, `ILN2` | `0x3F317218`, `0x3FB8AA3B` | ln 2, 1 / ln 2 |
| `E4` | `0x3FA45AF2` | e^(1/4) |
| `C1..C4` | `0x3C1A534C`, `0x3D296EC9`, `0x3E2AB479`, `0x3EFFFFBD` | exp coefficients |
| `EPS` | `0x3A83126F` | 0.001 |
| `DEG`, `RAD` | `0x3C8EFA35`, `0x42652EE1` | pi / 180, 180 / pi |

### Sin, Cos, Tan

`sinq(x, s)`:

1. `n = f2i(mul(x, K_ANG))` (one full turn is 2^20 steps).
2. `j = n mod 256`, `k = floor(n / 256) mod 1024`,
   `q = (floor(n / 2^18) + s) mod 4` (floors of the signed `n`).
3. `w = j * 0x01010101` (the weight `j / 255` as a 32-bit fraction).
4. If `q` is even: `r = S[k] + hi(S[k+1] - S[k], w)`.
   If `q` is odd: `r = S[1024-k] - hi(S[1024-k] - S[1023-k], w)`.
5. If `q >= 2`, `r = -r`.
6. The result is `scale(r, 31)`.

`Sin(x) = sinq(x, 0)`, `Cos(x) = sinq(x, 1)`,
`Tan(x) = div(Sin(x), Cos(x))`.

Examples: `Sin(0) = 0`, `Cos(0) = Sin(1.5707964) = 0.99999994`
(`0x3F7FFFFF`), `Sin(3.1415927) = 2^-31` (`0x30000000`),
`Tan(0.7853982) = 1.0`.

### Atan and Atan2

`atan(x)`:

1. `a = |x|` (sign bit cleared). If `a > 1`, `a = recip(a)` and remember
   "inverted".
2. If `a > T12`, `a = div(add(a, M_T6), add(1.0, mul(T6, a)))` and
   remember "shifted".
3. `s = mul(a, a)`;
   `r = div(mul(a, add(P0, mul(P1, s))), add(1.0, mul(Q1, s)))`.
4. If shifted, `r = add(r, PI_6)`. If inverted, `r = sub(PI_2, r)`.
5. If `neg0(x)`, the sign bit of `r` is flipped.

`atan2(y, x)` (core):

1. If `e(x) != 0`: `r = atan(div(y, x)` with the sign bit cleared`)`;
   otherwise `r = PI_2`.
2. If `neg0(x)`, `r = sub(PI, r)`.
3. If `neg0(y)`, the sign bit of `r` is flipped.

Natives: `Atan(x) = atan(x)`. `Atan2(y, x)` is 0 when both
`|sub(x, 0)| < EPS` and `|sub(y, 0)| < EPS`, otherwise `atan2(y, x)`.
So `Atan2(0.0005, -0.0005) = 0`, `Atan2(0, -1) = PI`,
`Atan2(1, 1) = 0.78539824` (`0x3F490FDC`, one unit above `pi/4`).

### Asin and Acos

For `|x| <= 0x3F7E8000` (comparing the patterns with the sign cleared;
about 0.99414):

1. `n = f2i(x * 2^30)` (the pattern of `x` plus `15 * 2^23`), clamped to
   `-(2^30 - 1) .. 2^30 - 1`.
2. `c = -n` for asin, `c = n` for acos.
3. `u = (c * 2^12) mod 2^32`, `w = u OR floor(u / 2^20)`,
   `k = floor(c / 2^20) mod 1024`.
4. If `c >= 0`: `r = A[k] - hi(A[k] - A[k+1], w)`.
   If `c < 0`: `r = 0x6487ED51 - A[1024-k] - hi(A[1023-k] - A[1024-k], w)`
   (`0x6487ED51` is `pi * 2^29`). All mod 2^32.
5. For asin, subtract `0x3243F6A8` (`pi/2 * 2^29`) mod 2^32.
6. The result is `scale(r, 29)` with `r` read as signed.

For larger `|x|`:

1. `D = 0x3F800000 - pattern(|x|)` if `|x| <= 1`, else `D = 0`
   (the distance from 1.0 in units of 2^-24).
2. `z = nlz(D)` (32 for `D = 0`); `b = (NOT D) * 2^z mod 2^32`, or 0 if
   `z = 32`.
3. `i = 8z - 120 + floor(b / 2^28)`, `w = (b * 16) mod 2^32`.
4. `r = N[i] - hi(N[i] - N[i+1], w)`; `t = scale(r, 34)`, which is
   `acos(|x|)`.
5. asin: `add(M_PI_2, t)` if `x` is negative, else `sub(PI_2, t)`.
   acos: `sub(PI, t)` if `x` is negative, else `t`.

Natives: `Asin(x)` and `Acos(x)` are 0 when `x < -1` or `x > 1`,
otherwise the functions above. `Asin(1) = PI_2`, `Acos(-1) = PI`,
`Acos(0.5) = 1.0471975` (`0x3F860A91`), `Asin(1.5) = 0`.

### SquareRoot

`sqrt(x)` for `x > 0` (0 for zero and negative patterns):

1. `m = f(x)`, `E = e(x) - 127`.
2. `v = 2^31 + m * 2^8 + floor(m / 2^15)`;
   `s = floor(sqrt(v))` (exact integer square root, 46340..65535).
3. `t` is the pattern `0x3F800000 OR floor((((s - 46340) * 46341) mod
   2^32) / 2^8)`.
4. `h = floor(E / 2)` (rounding down). The scale pattern is
   `(h + 127) * 2^23`, plus `0x3504F3` when `E` is odd (that is
   `2^h * 1.4142135`).
5. The result is `mul(t, scale)`.

Native: `SquareRoot(x)` is 0 when `|sub(x, 0)| < EPS` or `x < 0`,
otherwise `sqrt(x)`. So `SquareRoot(0.0005) = 0`, `SquareRoot(4) = 2`,
`SquareRoot(2) = 1.4142135` (`0x3FB504F3`), and
**`SquareRoot(9) = 3.0000577`** (`0x404000F2`).

### Pow

Helpers:

- `ln(x)`: `m` is the pattern `f(x) OR 0x3F800000` (the significand in
  [1, 2), the sign is ignored); `z = div(add(m, -1.0), add(m, 1.0))`,
  `z2 = mul(z, z)`, `v = mul(z, add(1.0, mul(L1, z2)))`, then 1 is added
  to the exponent field of `v` if `e(v) != 0` (times 2);
  `L = div(v, add(1.0, mul(L2, z2)))`. The result is
  `mul(LN2, add(i2r(e(x) - 127), mul(L, ILN2)))`.
- `powi(b, n)` for `n >= 0`: `r = 1.0`; while `n != 0`: if `n` is odd,
  `r = mul(r, b)`; `b = mul(b, b)`; `n = floor(n / 2)`.
- `expp(t)` for `t >= 0`: if `e(t) != 0`, add 2 to the exponent field of
  `t` (times 4). `i = trunc(t)`, `f = sub(t, i)`. `g` is `f` with 2
  subtracted from its exponent field (`f / 4`) when `e(f) >= 3`, else 0.
  `p = mul(C1, g)`, then for `c` in `C2, C3, C4, 1.0`: `p = mul(add(p, c),
  g)`; then `p = add(p, 1.0)`. The result is `mul(p, powi(E4, f2i(i)))`.
- `exp(t)`: `recip(expp(-t))` if `neg0(t)`, else `expp(t)`.

`pow(x, y)`:

1. If `trunc(y) == y` (float comparison) and not `neg0(y)`:
   `powi(x, f2i(y))`.
2. Otherwise, if `e(x) != 0`: `exp(mul(y, ln(x)))`.
3. Otherwise 0.

Native `Pow(x, y)`:

- if `|sub(x, 0)| < EPS`: 0 when `y < 0`, else `pow(x, y)`;
- else if `|sub(y, 0)| < EPS`: 1.0;
- else `pow(x, y)`.

Examples: `Pow(2, 10) = 1024`, `Pow(2, 0.5) = 1.4142135`,
`Pow(2, -1) = 0.50000012` (`0x3F000002`: negative integer powers take the
logarithm path), `Pow(-8, 0.5) = 2.8284264` (the sign of `x` is ignored
on that path), `Pow(-2, 3) = -8`, `Pow(5, 0.0005) = 1.0`,
`Pow(0, -1) = 0`, `Pow(0.0005, 2) = 2.5e-7`, `Pow(0, 0) = 1.0`.

### Deg2Rad and Rad2Deg

`Deg2Rad(x) = mul(x, DEG)`, `Rad2Deg(x) = mul(x, RAD)`.
`Deg2Rad(180) = 3.1415925` (`0x40490FDA`), `Rad2Deg(3.1415927) = 180`.

## Edge cases

- **`Pow` with a huge integer exponent hangs the game.** When `y` is an
  integer value of at least 2^31 (for example `Pow(2.0, 3000000000.0)`),
  `f2i(y)` wraps to a negative number and the squaring loop never ends:
  the game froze in the test, with no script error (the operation limit
  of SPEC-0005 does not apply inside a native). An implementation should
  not hang; any result is acceptable there, since no map can rely on it.
- `Atan2`, `SquareRoot` and `Pow` treat arguments below 0.001 in
  magnitude as zero (see the natives above). `Sin`, `Cos`, `Tan`, `Atan`,
  `Asin`, `Acos` do not.
- Angles: `Sin` and `Cos` use only `f2i(x * 2^20 / (2 pi))` modulo 2^20,
  so the input resolution is 2^-20 of a turn, and inputs whose product
  exceeds 2^31 wrap (no error).

## Open questions

None known.

## Sources

- Analysis of the 1.27b game library for the structure of each function
  and the constants; the tables were matched against the formulas above
  entry by entry.
- Test map MT1 on 1.27b under Wine, 2026-10-17: 174 calls of the eleven
  natives, read back as bit patterns, all as specified. Test map PH2:
  `Pow(2.0, 3000000000.0)` never returned.

## Verification

- `SPEC-0011-vectors.tsv`: 12300 lines produced by executing the game's
  own code for each native (as for SPEC-0004), followed by the 174 lines
  measured in the game with test map MT1. Each line is
  `native, arg1, arg2 (or -), result` as hex bit patterns. An
  implementation must reproduce every line.
- `SPEC-0011-tables.tsv`: the tables `S`, `A`, `N`.
- Reading a real's bit pattern in a JASS test: find the exponent by
  repeated `* 2.0` / `* 0.5` until the value is in [1, 2) (exact in the
  engine arithmetic), then `R2I(x * 8388608.0) - 8388608` is the
  fraction field.
