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
- The value is computed with the engine arithmetic; the exact algorithm
  is `s2r` in SPEC-0004. It can differ from the nearest float:
  `"0.1"` gives `0x3DCCCCCE`, one step above the nearest float.

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
- When a trigger action thread ends with an error (division by zero or
  the operation limit), the trigger's next action still runs.

`-2147483648 / -1` terminates the game process (a crash, observed under
Wine). An implementation should not reproduce the crash. Treating it as a
thread-ending error is the closest safe behaviour.

### Operation limit

A thread runs at most **300000 operations** before it is stopped. The count
starts at 0 when the thread starts and again each time it resumes after
`TriggerSleepAction` (or another wait), so a thread that sleeps regularly
can run any number of operations in total. A thread started by
`ExecuteFunc` has its own count; a plain function call runs in the
caller's thread and counts toward the caller's limit. Exactly 300000
operations are executed; the 300001st is not.

### Operation costs

The script is compiled to simple operations and each operation executed
counts once. The tables give the number of operations executed. `c(e)` is
the cost of computing expression `e`, `c(S)` the cost of executing the
statements `S`.

Expressions:

| Expression | Cost |
|---|---|
| literal of any type (`1`, `1.5`, `"a"`, `true`, `null`, `'hfoo'`), `function f` | 1 |
| variable: global, local or parameter | 1 |
| `a[i]` | c(i) + 1 |
| `-e`, `not e` | c(e) + 1 |
| `l op r` for `+ - * /` (also string `+`) and `== != < <= > >=` | c(l) + c(r) + 3 |
| `( e )` | c(e) |
| integer value used where a real is expected (implicit conversion) | c(e) + 1 |
| `l and r`, `l` true | c(l) + c(r) + 2 |
| `l and r`, `l` false | c(l) + 3 |
| `l or r`, `l` false | c(l) + c(r) + 2 |
| `l or r`, `l` true | c(l) + 3 |
| native call `n(a1, ..., ak)` | (c(a1) + 1) + ... + (c(ak) + 1) + 1 |
| function call `f(a1, ..., ak)` | (c(a1) + 1) + ... + (c(ak) + 1) + 1 + (1 if k > 0) + cost of running `f` |

Running a function `f` once costs:

- 1 per parameter;
- per `local` declaration: 1, plus c(e) + 1 if it has an initial value `= e`;
- the statements executed in its body;
- at the end: `return e` costs c(e) + 2; `return` without a value and
  reaching `endfunction` cost 2.

Statements:

| Statement | Cost |
|---|---|
| `set v = e` | c(e) + 1 |
| `set a[i] = e` | c(i) + c(e) + 3 |
| `call ...` | cost of the call expression |
| `if c then S endif`, `S` not empty, `c` true | c(c) + c(S) + 2 |
| `if c then S endif`, `S` not empty, `c` false | c(c) + 1 |
| `if c then endif` (empty), and `if c then else endif` (both empty) | c(c) |
| `if c then S1 else S2 endif`, `c` true | c(c) + c(S1) + 2 |
| `if c then S1 else S2 endif`, `c` false | c(c) + c(S2) + 2 |
| `if c1 then S1 elseif c2 then S2 endif`, `c1` true | c(c1) + c(S1) + 2 |
| same, `c1` false, `c2` true | c(c1) + c(c2) + c(S2) + 4 |
| same, both false | c(c1) + c(c2) + 4 |
| `loop S endloop` | 1 on entry, then c(S) + 1 per pass through the body |
| `exitwhen c` | c(c) + 1, whether it exits or not |

A longer `elseif` chain behaves as `if c1 then S1 else` followed by the
rest of the chain written as `if ... endif`, with the last `elseif` acting
as an `if` with an empty `else`.

Example: in a fresh thread, a function with `local integer lx = 0`,
`set g = 0` and the loop `loop` / `set g = g + 1` / `endloop` (globals `g`)
costs 5 before the loop, 1 to enter it and 7 per pass (6 for the `set`, 1
to repeat). The 42856th store to `g` is the last one that fits in 300000
operations.

### TriggerSleepAction outside trigger actions

| Where | Observed |
|---|---|
| Trigger action, including actions started by `TriggerExecute` and by events (unit death tested) | sleeps, continues later (SPEC-0002); the code that caused the action continues at once |
| Function started with `ExecuteFunc` | sleeps; the caller of `ExecuteFunc` continues at once, and the sleeping function resumes later |
| Timer callback | the callback ends at the sleep |
| `ForForce`, `ForGroup` and `EnumDestructablesInRect` callbacks | each callback ends at the sleep; the enumeration continues with the next element |
| Filter of `GroupEnumUnitsInRange` | each filter call ends at the sleep; the enumeration continues |
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

1. Costs of constructs not measured yet: `if` with an empty `then` and a
   non-empty `else`, and the exact place of implicit integer to real
   conversions in mixed expressions (assumed at the integer operand).

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
- **Sleeps.** The table under "TriggerSleepAction outside trigger actions",
  measured with 3 units, a force of 3 players and the destructables of the
  map. In a single-player test `GetPlayersAll()` holds one player.
- **Operation limit and next action.** An action that loops forever is
  followed by an action that prints; the print appears.

- **Operation costs.** 36 loop bodies, each run in its own thread with
  `ExecuteFunc` until the limit stops it; the number of passes `N` is
  recorded. With body cost `C` (the base loop plus the tested statement),
  the model above predicts `N = floor(299988 / C) + 1` for every one of
  them, for example 42856 for the plain loop (`C = 7`), 33333 for an added
  `set x = 1` (`C = 9`), 21428 for an added `call f(1)` with an empty `f`
  (`C = 14`) and 37499 for an added `call GetLocalPlayer()` (`C = 8`).
- **Budget after a sleep.** A thread that runs 10000 passes of a 19-step
  loop body, then `TriggerSleepAction(0)`, four times (about 760000
  operations in total) finishes.
