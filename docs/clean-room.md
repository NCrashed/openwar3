# Clean-room process

openwar3 reimplements the Warcraft III engine without copying Blizzard's code.
Knowledge about the original game's behaviour reaches the codebase only
through written specifications in `specs/`. This document defines who may see
what, and how a spec gets accepted.

## Reference version

Behaviour is specified against **Warcraft III 1.27b**. Map, script and data
compatibility later extends to everything that ran on 1.26-1.31 (Lua,
24 players, new natives), but the behavioural reference stays 1.27b unless a
spec says otherwise.

Balance values live in the game's data files (SLK, INI, TXT) and are read from
the user's installation, so specs describe mechanics and leave balance numbers
to the data.

## Roles

| | Analyst | Implementer |
|---|---|---|
| May use | Game binaries, disassembly/decompilation (Ghidra or similar tools), Warsmash source, memory-research posts, in-game experiments | `specs/`, public format documentation, this repository, test vectors |
| Must not use | (no restrictions) | Game binaries or their disassembly, decompiler output, Warsmash source (AGPL), analysts' private notes |
| Produces | Specs (pull requests to `specs/`) | Code |
| Works in | A separate private workspace | This repository |

A contributor who has seen decompiled code of a subsystem must
not implement that subsystem. Switching roles is allowed only for unrelated
subsystems, and must be recorded in the spec's `analysts` field.

Separate workspaces only separate context. Under one OS account
an implementer can still read analysts' files, so the boundary is a rule that
contributors keep and the tooling does not enforce. When the project has more
contributors, analysts and implementers should be different people.

## What a spec may contain

Allowed: facts and behaviour.

- formulas, constants, tables, units and rounding rules;
- event order, timing (in game ticks), state machines described in prose or diagrams;
- edge cases and how the original game resolves them;
- file layouts (field order, sizes, encodings);
- names of JASS natives and their observable semantics;
- how the claim was verified (replay, in-game experiment, test map).

Not allowed: expression of Blizzard's code.

- disassembly, decompiler output, or pseudocode that mirrors their control flow;
- function or variable names invented by the decompiler, binary addresses or offsets;
- text, assets or data tables copied from game files (cite the file and field instead).

When in doubt, describe what the game *does* as seen from the outside.

## Accepting a spec

A spec pull request needs a reviewer who checks:

1. The template in `specs/TEMPLATE.md` is complete, including sources and verification.
2. Nothing from the "not allowed" list is present.
3. The behaviour is testable: either a test vector, a replay-based oracle, or a
   reproducible in-game experiment is described.

## Game data

Users supply data from their own legally obtained copy. The repository never
contains Blizzard files. Tests that need real game data read it from
`game-data/` (git-ignored) and are skipped when it is absent. Fixtures
committed to the repository are authored by the project.
