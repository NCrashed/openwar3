---
id: SPEC-0008
title: Map loading, player slots and world bounds
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)"]
---

## Behaviour

### Archive order

Game files are looked up in the archives below. When several contain the
same file, the one with the highest priority wins:

| Archive | Priority |
|---|---|
| the map | above all game archives |
| `War3Patch.mpq` | 8 |
| `War3xLocal.mpq` | 2 |
| `War3x.mpq` | 1 |
| `War3.mpq` | 0 |

A map may replace `Scripts\common.j` and `Scripts\Blizzard.j`; the game
compiles and runs the map's copies. With a map copy of `Blizzard.j` in
which `GetPlayerStartLocationX` returns `12345.0`, the map script saw
12345.0. With a map copy of `common.j` in which
`PLAYER_NEUTRAL_AGGRESSIVE` is 13, the map script saw 13.

When the registry value `Allow Local Files` (key `Warcraft III`) is set,
loose files under the game directory are also used; this was not tested.

### config and main

`config` and `main` run in the same script instance: a global set in
`config` keeps its value in `main`.

Inside `config` (single player, started with `-loadfile`), any call to a
native that returns a `string` (`I2S`, `GetPlayerName`, also with the
result discarded) makes the map fail to start. Natives returning integers,
booleans, reals and handles work, and string literals and `+` work.

### Player slot state

The tables give the values read in four places of a 12-player map:
before anything else in `config` ("before config"), at the end of
`config`, at the start of `main`, and 0.5 s into the game. Player numbers
are 0-based.

**Before config**, for every map tested:

| Value | Players 0..11 | Players 12..15 |
|---|---|---|
| `GetPlayerController` | `MAP_CONTROL_NONE` | `MAP_CONTROL_CREEP` |
| `GetPlayerSlotState` | `PLAYER_SLOT_STATE_EMPTY` | `PLAYER_SLOT_STATE_PLAYING` |
| `GetPlayerRace` | handle id 0 (no race) | handle id 0 |
| race preference | `RACE_PREF_RANDOM` only | `RACE_PREF_RANDOM` only |
| `GetPlayerSelectable` | true | true |
| `GetPlayerStartLocation` | -1 | -1 |
| `GetPlayerTeam` | 0 | -1 |
| `GetPlayerColor` | color with the player's number | same |
| `GetPlayerHandicap` | 1.0 | 1.0 |
| `PLAYER_STATE_GIVES_BOUNTY` | 0 | 1 |

`GetPlayers()` is 0, `GetStartLocationX(0)` and `Y` are 0, and no map
flag is set. `GetTeams()` read 12 and `GetGamePlacement()` read
`MAP_PLACEMENT_TEAMS_TOGETHER` before the map's `SetTeams` and
`SetGamePlacement` ran; these may be left over from an earlier run of
`config` for the same map and are not specified.

**Alliances before config.** Each player has `ALLIANCE_PASSIVE` toward
itself and toward player 15; player 15 has it toward everyone.
`ALLIANCE_SHARED_VISION` and `ALLIANCE_HELP_REQUEST` are set only toward
the player itself. These values do not change in `config` or before
`main`. In `main`, `InitBlizzard` of `Blizzard.j` then adds the
alliances it sets up for the neutral players (player 12 passive toward
13; player 13 passive toward 0..13 and no bounty).

**Allies and enemies.** `IsPlayerAlly(a, b)` is exactly
`GetPlayerAlliance(a, b, ALLIANCE_PASSIVE)` and `IsPlayerEnemy(a, b)` is
its negation. Only the direction from `a` to `b` counts, and other
alliance flags do not matter (shared vision and help response without
passive: not allies). A player is its own ally only through the default
passive flag toward itself: after `SetPlayerAlliance(p, p,
ALLIANCE_PASSIVE, false)`, `IsPlayerAlly(p, p)` is false and
`IsPlayerEnemy(p, p)` is true. With the defaults, players 0..11 are
allies of player 15 and enemies of players 12..14.

**Units.** `IsUnitAlly(u, p)` is `IsPlayerAlly(p, owner of u)` and
`IsUnitEnemy(u, p)` is `IsPlayerEnemy(p, owner of u)`: they ask how the
player `p` regards the unit's owner, the direction opposite to the
argument order of `IsPlayerAlly`. After `SetPlayerAlliance(Player(0),
Player(1), ALLIANCE_PASSIVE, true)` alone, a unit of player 1 is an ally
for player 0 (`IsUnitAlly(u1, Player(0))` true), while a unit of player 0
is still an enemy for player 1.

**At the end of config** the values are exactly what the map's `config`
set: controllers, race preferences, selectable flags, start locations and
teams from its `SetPlayer...` calls. Slot states are still empty and
`GetPlayerRace` is still 0.

**At the start of main** the game has filled the slots (next sections).
The values are the same 0.5 s into the game, except for the changes
made by `main` itself.

### Who plays in single player

**Map with fixed player settings** (`war3map.w3i` flag 0x20):

- The local player takes the first slot that `config` gave
  `MAP_CONTROL_USER`. With players 0 and 2 set to computer and the others
  to user, the local player was player 1. The slot types in
  `war3map.w3i` do not matter: with all slots typed user in
  `war3map.w3i` and the same `config`, the result was the same.
- Every slot that `config` gave `MAP_CONTROL_COMPUTER` plays.
- Every other user slot stays empty.
- Playing slots keep the team set by `config`. **Empty slots get team -1.**
- Each slot's race is its race preference from `config` (human 1, orc 2,
  undead 3, night elf 4), also for empty slots.
- Names: the local player has the player's profile name; a computer
  slot has the slot name from `war3map.w3i` with trigger strings resolved
  (SPEC-0003); an empty slot has the default name "Player <n+1>"
  (observed in the Russian game as "Игрок 4" for player 3, also when the
  `war3map.w3i` name was different).
- `GetPlayerSelectable` keeps the value from `config`.
- Start locations: as set by `config`.

**Melee map** (`war3map.w3i` flag 0x04, original `(12)IceCrown.w3m`):

- The local player plays in slot 0, and one computer plays in slot 1. All
  other slots 0..11 are empty (team -1).
- Both have race preference `RACE_PREF_RANDOM` in `main` (the map's
  `config` had set human for player 0 and orc for player 1). The local
  player got human and the computer got undead in the run observed.
  How the race is picked, and whether the local player's race is the one
  last chosen in the lobby, is open.
- The computer is named "Computer (Normal)" (observed in the Russian
  game as "Компьютер (средн" plus one stray byte: the name was cut to its
  first 31 bytes).
- The local player kept start location 0 and team 0; the computer got
  start location 4 and team 1.

The neutral players 12..15 play in every case (SPEC-0006).

**Map flags in main.** `IsMapFlagSet` reported `MAP_LOCK_RANDOM_SEED` for
both maps and `MAP_LOCK_ALLIANCE_CHANGES` also for the fixed-settings map.

### World bounds

`GetWorldBounds()` spans the whole terrain grid of `war3map.w3e`: from
its origin `(x0, y0)` to `(x0 + (width - 1) * 128, y0 + (height - 1) * 128)`.
For `(12)IceCrown` (193 x 193 corners, origin -12288, -12288) it is
-12288 .. 12288 on both axes. It is wider than the playable area
(`bj_mapInitialPlayableArea`, -11520 .. 11520 in x).

Each call returns a new rect: two calls gave two different handle ids.

## Edge cases

- A `config` that calls a string native does not run to an error message:
  the game does not start the map at all.

## Open questions

1. How the random race of a melee slot is chosen, and where the local
   player's race comes from.
2. Which slot gets the computer opponent in a melee map, and how start
   locations are assigned to it (player 1 got start location 4 here).
3. Whether `MAP_LOCK_ALLIANCE_CHANGES` comes from the map's force flags.
4. Archive order for files other than the scripts (object data, models)
   was not tested; the priorities above apply to all files.
5. The behaviour of string natives in `config` in a lobby game.

## Sources

- Analysis of the 1.27b game library for the archive names and priorities.
- Test maps on 1.27b under Wine, single player with `-loadfile`, run by the
  analyst workspace, 2026-10-17.

## Verification

All tests use `(12)IceCrown` (the fixed-settings copy saved by the editor,
flags 0xD472, and the original melee map, flags 0x9416). A probe records
the values of the tables above in `config` (integers only, see above),
at the start and end of `main` and after 0.5 s, and writes them with
`Preload` / `PreloadGenEnd` at 0.5 s.

1. Fixed-settings map as saved: only player 0 plays, named after the
   profile; players 1..11 empty with team -1.
2. Same with `config` setting players 0 and 2 to computer: local player 1;
   players 0 and 2 computer and playing, team 0, named after their
   `war3map.w3i` slots; player 3 named "Player 4".
3. Melee map: player 0 user and player 1 computer playing, both with
   preference random; players 2..11 empty.
4. Map with replaced `Scripts\Blizzard.j` and `Scripts\common.j`: the
   values above (12345.0 and 13).
5. `GetWorldBounds` twice: -12288 .. 12288, two different ids.
6. Alliances between players 0 and 1 changed with `SetPlayerAlliance`
   one flag at a time: `IsPlayerAlly` and `IsPlayerEnemy` follow the
   passive flag as described.
7. **Units and alliances.** Footmen of players 0, 1, 12 and 15, checked
   against players 0, 1, 12, 15 with `IsUnitAlly`, `IsUnitEnemy`,
   `IsPlayerAlly(owner, p)` and `IsPlayerEnemy(owner, p)`, at the
   defaults and after each of: 0 passive toward 1; 1 shared vision
   toward 0 (no change); 1 passive toward 0; 0 no longer passive toward
   1; 0 and 15 no longer passive toward each other. In all 96 cases the
   unit natives equal the player natives with the players swapped.
