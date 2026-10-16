---
id: SPEC-0005
title: JASS operators, literals, strings and thread limits
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis and test maps)"]
---

## Behaviour

Arithmetic on `integer` and `real` values is specified in SPEC-0004. This
spec covers how expressions are parsed and evaluated, literals, string
values and what stops a thread.

### Operator precedence

From tightest to loosest:

1. `not`, unary `-`
2. `*`, `/`
3. binary `+`, `-`
4. `==`, `!=`, `<`, `<=`, `>`, `>=`
5. `or`
6. `and`

**`or` binds tighter than `and`.** `a and b or c` means `a and (b or c)`,
and `a or b and c` means `(a or b) and c`. Binary operators of the same
level are left-associative (`10 - 4 - 3` is 3, `100 / 10 / 5` is 2).
Comparisons can be chained: `1 < 2 == true` is valid and true.
Parentheses group as usual.

### Evaluation of `and` and `or`

Both are short-circuit: the right operand of `and` is not evaluated when
the left one is false, and the right operand of `or` is not evaluated when
the left one is true. Operands are evaluated left to right.

### Integer literals

| Form | Example | Value |
|---|---|---|
| decimal | `42` | 42 |
| hexadecimal, prefix `0x`, `0X` or `$`, digits in either case | `0x1f`, `0X1F`, `$1f` | 31 |
| octal, leading `0` | `017` | 15 |
| character code, 1 character | `'a'` | 97 |
| character code, 4 characters | `'abcd'` | 'a' * 2^24 + 'b' * 2^16 + 'c' * 2^8 + 'd' = 1633837924 |

- A literal outside the 32-bit range wraps modulo 2^32: `4294967296` is 0,
  `0xffffffff` is -1.
- An octal literal with a digit 8 or 9 (`08`) is a syntax error.
- A character code of 2 or 3 characters (`'ab'`) is a syntax error.
- A character code may contain an escape: `'\n'` is 10.

### Real literals

`5.` and `.25` are valid and mean 5.0 and 0.25. The bit pattern of a real
literal equals the result of `S2R` on the same text (checked for `0.1`).

### String literals

Escapes: `\"` (quote), `\\` (backslash), `\t` (tab, 9), `\n` (line feed,
10), `\r` (carriage return, 13). Any other character after a backslash
(for example `\q`) is a syntax error.

### Strings at run time

- `==` and `!=` compare contents, case-sensitively:
  `"abc" == "ab" + "c"` is true, `"A" == "a"` is false.
- `null` and `""` are different: `null == ""` is false, `null != ""` is
  true, `null == null` is true.
- `null` behaves as empty text in concatenation and length:
  `null + "x"` is `"x"`, `StringLength(null)` is 0.

### S2I and S2R

`S2I(s)`:

- `null` or `""` gives 0.
- Leading spaces are skipped, an optional `+` or `-` follows, then decimal
  digits up to the first other character: `" 12"` and `"12abc"` give 12,
  `"+5"` gives 5, `"abc"` gives 0.
- No hexadecimal: `"-0x10"` gives 0 (it reads `-0`).
- Values outside the 32-bit range saturate: `"99999999999"` gives
  2147483647, `"-99999999999"` gives -2147483648.

`S2R(s)`:

- `null` or `""` gives 0.0.
- Leading spaces are **not** skipped: `" 2"` gives 0.0.
- An optional sign, digits and one optional `.`, up to the first other
  character: `"3abc"` gives 3.0, `"1,5"` gives 1.0, `".5"` gives 0.5.
- No exponent: `"1e3"` gives 1.0.
- The value is computed with the engine arithmetic of SPEC-0004 and can
  differ from the nearest float: `"123456789012"` gives 123456793600.0.
  The exact algorithm is open question 1.

### Errors that stop a thread

These end the current thread silently. Nothing is shown to the player.

| Cause | Observed |
|---|---|
| Integer division by zero | thread ends at the division |
| Real division by `0.0` | thread ends at the division |
| Operation limit reached | thread ends |
| `TriggerSleepAction` where sleeping is not allowed | see below |

What survives the end of a thread:

- A function started with `ExecuteFunc` runs in its own thread. When that
  thread ends with an error, the caller of `ExecuteFunc` continues with its
  next statement.
- A plain function call runs in the caller's thread, so an error ends the
  caller too.
- When a trigger action thread ends with an error, the trigger's next
  action still runs.

`-2147483648 / -1` terminates the game process (a crash, observed under
Wine). An implementation should not reproduce the crash. Treating it as a
thread-ending error is the closest safe behaviour.

### Operation limit

Each thread may execute 300000 operations of the script machine. A thread
started by `ExecuteFunc` gets its own budget; a plain function call uses
the caller's. The cost of a statement depends on how the script is
compiled into operations (open question 2). As a calibration point,
`loop` / `set i = i + 1` / `endloop` with a global `i` runs 42856 times in a
fresh thread.

### TriggerSleepAction outside trigger actions

| Where | Observed |
|---|---|
| Trigger action | sleeps, continues later (SPEC-0002) |
| Function started with `ExecuteFunc` | sleeps; the caller of `ExecuteFunc` continues at once, and the sleeping function resumes later |
| Timer callback | the callback ends at the sleep |
| `ForForce` callback | the callback ends at the sleep, and `ForForce` calls no further callbacks |
| Trigger condition | the condition ends at the sleep and counts as false |

### Return values of the wrong type

A function that returns a value of another type than it declares (the
"return bug", for example `return h` with a handle `h` in a function that
returns `integer`) is rejected: the map does not load.

## Edge cases

- Negation of -2147483648 and its multiplication by -1 give -2147483648
  (wrap). The tests built the value as `-2147483647 - 1`; the literal
  `-2147483648` was not tested.
- Tested groupings: `not` against `and` and `or`, `*` against `+`, `or`
  against `and`, chained comparisons, left associativity of `-` and `/`.
  The places of unary `-` and of comparisons against arithmetic in the
  list follow the usual order but were not tested separately.
- `not` applied to a comparison: for booleans `not a == b` gives the same
  result whichever way it is grouped, so the grouping was not tested.

## Open questions

1. Exact `S2R` algorithm (digit accumulation and scaling), to be specified
   with test vectors in SPEC-0004.
2. Operation costs: how many operations each statement and expression
   costs. Needed only for bit-exact behaviour of scripts that hit the
   limit.
3. Which other callbacks forbid sleeping (`ForGroup`, enum filters,
   `TriggerEvaluate` called from an action, unit event callbacks).
4. When the operation limit ends a trigger action thread, does the
   trigger's next action run (as it does after a division by zero)?

## Sources

- Test maps on 1.27b under Wine, run by the analyst workspace, 2026-10-16.
  Each map writes its output to a file with `PreloadGenStart`, `Preload`
  and `PreloadGenEnd`; the file appears in `CustomMapData\ow3out\`.
- Analysis of the 1.27b game library for the operation limit (300000).

## Verification

Test maps built from a 12-player melee map without victory conditions.
Each prints lines with `BJDebugMsg`, mirrored to a Preload file. Expected
1.27b output:

- **Precedence and short-circuit.** With helper functions `T(name)` and
  `F(name)` that print `eval name` and return true or false:

  | Expression | Evaluated | Result |
  |---|---|---|
  | `T(a) and F(b) or T(c)` | a, b, c | true |
  | `F(a) and T(b) or T(c)` | a | false |
  | `T(a) or F(b) and F(c)` | a, c | false |
  | `F(a) or T(b) and F(c)` | a, b, c | false |
  | `(F(a) and F(b)) or T(c)` | a, c | true |
  | `F(a) and (F(b) or T(c))` | a | false |
  | `T(a) or F(b) and F(c) or T(d)` | a, c, d | true |
  | `F(a) and T(b) or T(c) and T(d)` | a | false |
  | `not F(a) and F(b)` | a, b | false |
  | `not T(a) or T(b)` | a, b | true |

  Also `not true and false` is false, `true or true and false` is false,
  `false and false or true` is false.
- **Literals.** `0x10` 16, `0xFF` 255, `0xffffffff` -1, `$10` 16, `$ff`
  255, `0X1f` 31, `010` 8, `017` 15, `4294967296` 0, `2147483647 + 1`
  -2147483648, `'a'` 97, `'abcd'` 1633837924, `'hfoo'` 1751543663,
  `'\n'` 10, `5.` 5.0, `.25` 0.25. Maps containing `08`, `'ab'` or `"\q"`
  do not load.
- **Strings.** As listed under "Strings at run time" and "S2I and S2R".
- **Division by zero.** A trigger action calls
  `ExecuteFunc("IntDiv")`, prints, `ExecuteFunc("RealDiv")`, prints, then
  calls `IntDiv()` directly and prints; a second action prints `D9`. Output:
  the line before each division, both lines after `ExecuteFunc`, no line
  after the direct call, then `D9`.
- **Operation limit.** `ExecuteFunc` of an endless increment loop returns to
  the caller after 42856 iterations; the same loop called directly ends the
  action.
- **Sleeps.** The table under "TriggerSleepAction outside trigger actions".
