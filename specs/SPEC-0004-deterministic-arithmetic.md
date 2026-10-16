---
id: SPEC-0004
title: Deterministic real arithmetic and JASS number operations
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

The game does not use the processor's floating-point instructions for
simulation arithmetic. It uses its own operations on 32-bit values that
have the IEEE 754 single-precision layout (1 sign bit, 8 exponent bits,
23 fraction bits) but round differently. The same operations implement
JASS `real` arithmetic and timer times (SPEC-0002).

Notation: for a 32-bit pattern `x`, `s(x)` is the sign bit, `e(x)` the
exponent field (0..255) and `f(x)` the fraction field (0..2^23-1).
`nlz(v)` is the number of leading zero bits of a 32-bit unsigned value.
All bit patterns are 32-bit unsigned; "mod 2^32" marks where a result
wraps.

### Zero

Any value with `e(x) = 0` counts as zero in addition and subtraction,
whatever its fraction. There are no special rules for infinities or NaN:
patterns with `e(x) = 255` go through the same formulas as other values.

### Addition: add(a, b)

1. If `e(a) = 0`, the result is `b` (its bit pattern, unchanged).
   Otherwise, if `e(b) = 0`, the result is `a`.
2. Let `d = e(b) - e(a)`. If `d >= 23`, the result is `b`. If `d <= -23`,
   the result is `a`.
3. Form signed significands `Ma = (2^23 + f(a)) * 2` and
   `Mb = (2^23 + f(b)) * 2`, negated when the sign bit is set.
4. Shift the significand of the operand with the smaller exponent right by
   `|d|` bits, rounding toward minus infinity (an arithmetic shift:
   `floor(M / 2^|d|)`). Let `E` be the larger exponent field
   (`e(a)` when `d = 0`).
5. `S = Ma + Mb`. If `S = 0`, the result is `0x00000000` (+0).
6. Let `A = |S|` and `n = nlz(A)`. Normalise: `N = A * 2^(n-8)` if `n >= 8`,
   otherwise `N = floor(A / 2^(8-n))`.
7. The result is
   `((E + 7 - n) * 2^23 mod 2^32) OR (N mod 2^23) OR (0x80000000 if S < 0)`.

Compared with IEEE addition: bits shifted out are dropped (no rounding),
negative operands are shifted toward minus infinity, an operand 23 or more
binary orders of magnitude smaller is ignored, and exponent overflow or
underflow is not handled.

### Subtraction: sub(a, b)

`sub(a, b) = add(a, b XOR 0x80000000)`.

### Multiplication: mul(a, b)

Let `sign = (a XOR b) AND 0x80000000` and `X = e(a) + e(b) - 127`.

- If `f(a) != 0` and `f(b) != 0`:
  `P = (2^31 + f(a) * 2^8) * (2^31 + f(b) * 2^8)` (a 64-bit product),
  `t = 1` if `P >= 2^63` else `0`,
  `F = floor(P / 2^(39 + t)) mod 2^23`, and the candidate result is
  `F OR ((X + t) * 2^23 mod 2^32) OR sign`.
- Otherwise, if `e(a) != 0` and `e(b) != 0` (one significand is an exact
  power of two): the candidate result is
  `(X * 2^23 mod 2^32) OR f(a) OR f(b) OR sign`.
- Otherwise the result is `0x00000000`.

In the first two cases the result is `0x00000000` instead of the candidate
when `(X - 1) * 2^23`, taken as a signed 32-bit integer after wrapping, is
negative; that is when `X <= 0` or `257 <= X <= 383`.

The product significand is truncated, never rounded.

### Integer to real: i2r(v)

For a signed 32-bit `v`: if `v = 0` the result is `0x00000000`. Otherwise
`A = |v|` (as unsigned; `|-2^31| = 2^31`), `n = nlz(A)`,
`N = A * 2^(n-8)` if `n >= 8` else `floor(A / 2^(8-n))`, and the result is
`((158 - n) * 2^23) OR (N mod 2^23) OR (0x80000000 if v < 0)`.

Integers up to 2^24 in magnitude convert exactly; larger ones are
truncated toward zero (IEEE would round to nearest).

### Real to integer: R2I

The JASS native `R2I(x)`:

- `e(x) < 127` (magnitude below 1, including zero): `0`.
- `e(x) >= 158` (magnitude at least 2^31, including infinities and NaN):
  `2147483647` if the sign bit is clear, `-2147483648` if it is set.
- Otherwise the value truncated toward zero:
  `(2^23 + f(x))` shifted right by `150 - e(x)` bits, or left by
  `e(x) - 150` bits, then negated if the sign bit is set.

### Floor

`floor(x)`, used by the engine for rounding:

- `e(x) < 127`: `0xBF800000` (-1.0) if the sign bit is set and
  `x != 0x80000000`; otherwise `0x00000000`. So `-0.0` gives `+0.0`, and a
  negative value with exponent field 0 gives -1.0.
- `127 <= e(x) < 150`: the mathematical floor of the value, which is
  exactly representable.
- `e(x) >= 150`: `x` unchanged.

### Reciprocal: recip(x)

`recip` uses the table `R[0..1024]` given in `SPEC-0004-recip-table.tsv`
(1025 unsigned 32-bit values, measured in the game; see Verification). For a pattern
`x`:

1. `i = floor(f(x) / 2^13)` (0..1023) and `r = f(x) mod 2^13`.
2. Weight `w = r * 2^19 + r * 2^6 + floor(r / 2^7)` (a 32-bit value; the
   three terms do not overlap).
3. `h = floor(((R[i] - R[i+1]) mod 2^32) * w / 2^32)`.
4. `v = (R[i] - h - e(x) * 2^23) mod 2^32`.
5. If `(v + 0x7E000000) mod 2^32` has its top bit set, the result is
   `0x00000000`. Otherwise the result is
   `((v + 0x7E800000) mod 2^32) OR (sign bit of x)`.

### Division: div(a, b)

`div(a, b)` is `0x3F800000` (exactly 1.0) when `a` and `b` have the same
bit pattern, and `mul(a, recip(b))` otherwise. For example
`div(1.0, 0.005)` gives 200.00003, where IEEE gives 200.0.

### Fractional part and remainder

- `frac(x)`: if `e(x) < 127` (magnitude below 1) the result is `x`. If
  `e(x) >= 150` the result is `0x00000000`. Otherwise let `k = e(x) - 127`
  and `I` be `x` with its lowest `23 - k` fraction bits cleared (the integer
  part, same sign); the result is `sub(x, I)`.
- `fmod(x, y)` for `x >= 0` (the only case the game is known to use):
  `r = mul(frac(mul(x, recip(|y|))), |y|)`; if `r > 0` and `r >= |y|`, the
  result is `sub(r, |y|)`, otherwise `r`. Example: `fmod(310, 120)` gives
  `0x428BFFFE` (69.99998).

### Decimal text to real: s2r(text)

Used by `S2R` and, as far as tested, for real literals in scripts. `S2R`
returns 0.0 for `null` and `""` without parsing.

1. An optional first character `-` (sign -1) or `+` is consumed.
2. Characters are read one at a time. Reading stops at the end of the
   text, at a space, `;` or tab, at a second `.`, and at any other
   character that is not a decimal digit.
3. Three counters start at 0: the integer `m`, the significant digit
   count `c` and the exponent adjustment `k`; a step `q` starts at 0.
   - A digit is significant once any non-zero digit has been seen (leading
     zeros are not significant). For each digit, `c` is increased by 1 if
     the digit is significant.
   - While `c <= 9`: `m = m * 10 + digit`, then `k = k + q`.
   - When `c` reaches 10, `q` is decreased by 1. For every digit with
     `c >= 10` (including that one), `k = k + q` and `m` is unchanged.
   - The first `.` increases `q` by 1.
   So `q` is 1 after the point and 0 before it, minus 1 once more than nine
   significant digits have been read; `k` counts digits after the point and
   subtracts digits beyond the ninth significant one.
4. `v = i2r(m * sign)` (integer product; `-0` gives `+0.0`).
5. `p(n)` is 10 to the power `n`, computed by squaring: start with
   `r = 1.0`, `b = 10.0`; for each bit of `n` from the lowest, if the bit
   is set `r = mul(r, b)`; then `b = mul(b, b)`.
6. The result is `div(v, p(k))` if `k >= 0`, else `mul(v, p(-k))`.

Examples: `"0.1"` gives `0x3DCCCCCE` (IEEE nearest: `0x3DCCCCCD`), `"1.5"`
gives `0x3FC00001` (1.5000001), `"123456789012"` gives `0x51E5F4C8`,
`" 2"` gives 0.0, `"1e3"` gives 1.0, `"12;34"` gives 12.0.

### Real to text: R2S and R2SW

`R2S(x)` is `R2SW(x, 0, 3)`. `R2SW(x, w, p)` builds the text as follows
(`a` is `x` with the sign bit cleared, `f2i` is the truncating conversion
of the integer part, without saturation):

1. If the sign bit of `x` is set and `x` is not `-0.0`, the text starts
   with `-`. The sign comes before any padding.
2. **Large values**, `e(a) >= 158` (`a >= 2^31`):
   - if `a` is not NaN and `a >= 0x7F7FFFFF` (the largest float or
     infinity), the rest is `inf`;
   - otherwise (NaN included) divide by ten with `div(t, 10.0)`, counting
     the divisions `k`, until the pattern of `t` is at most `0x4EFFFFFF`
     (at least one division); the rest is the decimal integer `f2i(t)`,
     `k` zeros, `.`, and `max(p, 1)` zeros. `w` is ignored.
3. **Other values**:
   - `q = p`, or 6 if `p < 0` (and then trailing zeros are removed, see
     below); `q` is limited to at most 9;
   - integer part `i = f2i(a)`; `P = 10^q` as an integer;
   - fraction digits `n = f2i(floor(add(mul(frac(a), i2r(P)), 0.5)))`;
   - if `n >= P`: `i = i + 1`, `n = n - P` (carry);
   - if `p < 0`: while `q > 1` and `n` is a multiple of 10, `q = q - 1`
     and `n = n / 10`;
   - the rest is `i` right-aligned in `max(w - q, 1)` characters, `.`, and
     `n` with leading zeros to `q` digits (for `q = 0`, `n` itself, which
     is 0 after a carry or the fraction rounded down).

Consequences: `R2S(-0.0)` is `0.000` while `R2S(-0.0004)` is `-0.000`;
`R2SW(2.5, 1, 0)` is `3.0` (one fraction digit even for precision 0);
`R2SW(-1.5, 10, 2)` is `-       1.50`; `R2SW(1.0/3.0, 1, 9)` is
`0.333333312` (the fraction is multiplied in engine arithmetic, not
printed exactly); `R2S(99.9995)` is `99.999` because the float nearest to
99.9995 is 99.99949646. `R2SW(x, w, p)` with `p > 9` prints 9 digits.

### JASS integer operations

- `+`, `-`, `*` and unary `-` wrap modulo 2^32 (two's complement).
- `/` truncates toward zero: `-7 / 2 = -3`, `7 / -2 = -3`.
- Integer division by zero ends the running thread (SPEC-0005).
  `-2147483648 / -1` terminates the game process in 1.27b (SPEC-0005).
- `<`, `<=`, `>`, `>=`, `==`, `!=` compare as signed 32-bit integers.

### JASS real operations

- `+`, `-`, `*`, `/` are `add`, `sub`, `mul`, `div` above.
- Division by a real equal to `0.0` (including `-0.0`) ends the running
  thread, like integer division by zero.
- Unary `-` flips the sign bit.
- `a == b` is true when `|sub(a, b)| < 0.001`: the sign bit of
  `sub(a, b)` is cleared, and the result is compared as an IEEE float
  with the float nearest to 0.001 (`0x3A83126F`).
- `a != b` is true when the bit patterns of `a` and `b` differ. It is
  therefore **not** the negation of `==`: for `a = 1.0` and `b = 1.0005`
  both `a == b` and `a != b` are true, and `0.0 != -0.0` is true.
- `<`, `<=`, `>`, `>=` are ordinary IEEE comparisons of the two values.
- `I2R(i)` is `i2r(i)`. `R2I(r)` is described above. `S2R(s)` is `s2r(s)`.

## Edge cases

- `1.0 - 0.005` gives `0x3F7EB851` (0.99499994), where IEEE gives
  0.995000005.
- `100000.0 * 0.005` gives `0x43F9FFFF` (499.99997), where IEEE gives
  500.0.
- `add(x, y)` with `y` 23 or more binary orders of magnitude smaller than
  `x` returns `x` exactly, so small increments to large values are lost.
- `i2r(16777217)` gives 16777216.0 (truncation).

## Open questions

None at the moment. Answered by test maps (SPEC-0005): division by zero
ends the thread; `-2147483648 / -1` terminates the game process; `and` and
`or` are short-circuit; `S2I` saturates and stops at the first non-digit.

## Sources

- Analysis of the 1.27b game library (method kept in the analyst
  workspace). The operations above were checked against the game's own
  code executed in an emulator: one million random and boundary inputs
  per operation, with no differences.
- `SPEC-0004-recip-table.tsv` was measured in the game with a test map
  (see Verification). No formula reproduces it exactly: the closest found,
  `round(2^34 / (1024 + i) - 2^23 + 0.095 * 2^24 / (1 + i/1024)^3 / 2^20)`,
  differs in 80 entries.

## Verification

- `SPEC-0004-vectors.tsv`: 27000 input/result pairs produced by executing
  the game's own code, for `add`, `sub`, `mul`, `i2r`, `r2i` (the `R2I`
  native), `floor`, plus 7000 lines for `recip` and `div` and about 3000
  for `s2r` (text in double quotes) and about 3000 for `r2sw` (pattern,
  width and precision, text). Floats are hex bit patterns. An implementation must
  reproduce every line.
- JASS test maps (to be made): print the bit patterns of `1.0 - 0.005`,
  `100000.0 * 0.005` and the results of `==` and `!=` on 1.0 and 1.0005.
- **Reciprocal table (test map R1).** Every value is computed in JASS from
  integers with `I2R`, so no real literal is involved:
  `bits(x) = R2I((I2R(1) / x) * I2R(16777216)) - 8388608`.
  - For `i = 0..1023`, `x = I2R(1024 + i) / I2R(1024)`; `bits(x)` is
    `R[i]`. Every step is exact: division by a power of two, the reciprocal
    of a value with zero interpolation weight, multiplication by 2^24.
  - For `r = 1..8191`, `x = I2R(2^23 + 1023 * 2^13 + r) / I2R(2^23)`;
    `bits(x)` follows the interpolation between `R[1023]` and `R[1024]`, which
    fixes `R[1024]`.
  - The map writes the 9215 values with `Preload` / `PreloadGenEnd`. On
    1.27b all of them agree with the table and with `recip` above.
