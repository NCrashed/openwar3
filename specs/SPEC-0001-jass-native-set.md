---
id: SPEC-0001
title: JASS native set of the map and AI script environments
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary)", "Anton Gushcha (black-box test maps only)"]
---

## Behaviour

The game exposes native functions to two script environments:

- the **map environment**, which runs `war3map.j` together with `common.j`
  and `Blizzard.j`;
- the **AI environment**, which runs AI scripts (`*.ai`) together with
  `common.ai`.

Declarations come from the script files in the user's installation, read
with the usual MPQ priority (`War3Patch.mpq` over `War3x.mpq` over
`War3.mpq`). In 1.27b the patched files declare:

| File | Natives | `type` declarations |
|---|---|---|
| `Scripts\common.j` | 1160 | 91 |
| `Scripts\common.ai` | 123 | 0 |

No native is declared in both files.

### Map environment

The engine provides an implementation for every native declared in
`common.j`, with exactly the parameter and return types written there.
An implementation should treat `common.j` as the authoritative list of
names and signatures and should fail loudly when a `common.j` native has no
implementation, so that the list stays complete.

A map script may also declare natives itself, in its declaration part
(before the first function), and call them. The engine provides at least
the following natives that `common.j` does not declare:

- natives declared in `common.ai`. Observed for `UnitAlive`: a map that
  declares `native UnitAlive takes unit u returns boolean` loads, and the
  call returns `true` for a living unit and `false` right after `KillUnit`
  on it.
- five natives that no shipped script declares. Their signatures, written
  as JASS declarations with parameter names chosen by this spec:

```
native DialogSetAsync          takes dialog d returns nothing
native SetStackedSound         takes string soundLabel, real width, real height returns nothing
native ClearStackedSound       takes string soundLabel, real width, real height returns nothing
native SetStackedSoundRect     takes string soundLabel, rect r returns nothing
native ClearStackedSoundRect   takes string soundLabel, rect r returns nothing
```

A map that declares them loads, and each call returns normally (test maps
E and F). `DialogSetAsync` on a new dialog does not prevent a following
`DialogDisplay` from showing it. Any other observable effect is not
specified yet (open question 2). Their names suggest they belong to the
same groups as `DialogDisplay` and
`RegisterStackedSound`/`UnregisterStackedSound`.

The engine also has natives named `GetPlayerStartLocationX` and
`GetPlayerStartLocationY` (`takes player, returns real`). Scripts cannot
reach them: `Blizzard.j` defines ordinary functions with the same names,
which return `GetStartLocationX/Y(GetPlayerStartLocation(p))`. A script
calling these names gets the `Blizzard.j` functions. An implementation does
not need these two natives.

### AI environment

The engine provides every native declared in `common.ai`, with the types
written there, plus one undeclared native:

```
native DebugBreak takes integer i returns nothing
```

A map script can declare and call `DebugBreak` too. In the release game
`DebugBreak(0)` returns normally and has no visible effect (test map H3).

AI scripts shipped with the game also call natives from `common.j`, so the
AI environment must provide those too. Whether it provides the full
`common.j` set or a subset is open question 1.

### Signature rules

- A native's name maps to one signature. No name is provided twice with
  different types, in either environment.
- Types are the JASS primitives `integer`, `real`, `string`, `boolean`,
  `code`, `nothing` (return only) and the handle types declared by `type`
  lines in `common.j`.

## Edge cases

- A `native` declaration after the first function of a script is a syntax
  error. World Editor places "Custom Script Code" after generated
  functions, so a native cannot be declared there without editing
  `war3map.j` directly.
- A map script that declares a native whose name is already used by a
  `Blizzard.j` function fails to load. Observed with
  `GetPlayerStartLocationX/Y`, even without any call: in the lobby the
  player list stays empty, and starting the game returns to the menu. The
  same failure for any script that does not compile is expected but only
  this case was tested.
- A map script that declares a native the engine does not provide fails
  to load, even if the native is never called (test maps G1 and G2).
- Parameter names in a `native` declaration are identifiers: a keyword such
  as `code` there is a syntax error, and the map does not load.
- `common.j` from `War3.mpq` (ROC) and `War3x.mpq` (TFT) declares fewer
  natives than the patched file. The engine provides the full 1.27b set
  regardless of which declaration file a map ends up using.

## Open questions

1. Does the AI environment provide every `common.j` native?
2. Observable semantics of the five undeclared map natives and of
   `DebugBreak`.
3. Whether `constant native` (the `constant` keyword on a declaration)
   changes any behaviour of the call.
4. Do all `common.ai` natives work when declared by a map, or only some?

## Sources

- `Scripts\common.j`, `Scripts\common.ai`, `Scripts\Blizzard.j` from the
  user's `War3Patch.mpq` (1.27b), used for counts, signatures and name
  clashes. This spec cites them and does not reproduce them.
- Analysis of the 1.27b game library for the list of provided natives
  (method kept in the analyst workspace).
- Test maps on 1.27b, 2026-10-16 (see Verification, item 3).

## Verification

1. Declaration counts: parse `common.j` and `common.ai` from a 1.27b
   `War3Patch.mpq`. Expect 1160 and 123 `native` declarations, 91 `type`
   lines in `common.j`, no name in both files.
2. Completeness: a test in the implementation enumerates all `native`
   declarations of `common.j` and `common.ai` and checks that each has an
   implementation with the same parameter and return types, and that the
   five undeclared map natives and `DebugBreak` exist with the types above.
3. Test maps, made from a 12-player melee map. Each `war3map.j` declares
   natives after `endglobals` and runs the test 1 second after start,
   printing with `BJDebugMsg`:

   | Map | Declares | Calls | 1.27b result |
   |---|---|---|---|
   | A | nothing | a message only | not reported separately; B, which adds to A, loads |
   | B | `UnitAlive` | on a new footman, before and after `KillUnit` | loads, prints true then false |
   | D | `GetPlayerStartLocationX/Y` | nothing | does not load, empty player list |
   | C | `GetPlayerStartLocationX/Y` | for players 0-11 | does not load, empty player list |
   | E | `DialogSetAsync` | on a new dialog with one button, then `DialogDisplay` for player 0 | loads, all messages, dialog shown |
   | F | the four stacked sound natives | each once, label "OW3Test", 256 x 256 and a rect | loads, all messages |
   | G1 | a name the engine does not provide | nothing | does not load |
   | G2 | the same | that native | does not load |
   | H2 | `DebugBreak` | nothing | loads |
   | H3 | `DebugBreak` | `DebugBreak(0)` | loads, messages before and after the call |

   An implementation reproduces the same load results and output.
