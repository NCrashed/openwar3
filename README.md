# openwar3

An open-source reimplementation of the Warcraft III engine, written in Rust.

The goal is to give the modding community an engine it controls: existing
maps keep working, long-standing engine limits go away, and new APIs are
decided in the open.

openwar3 is an engine only. You need your own copy of Warcraft III to provide
the game data; this repository contains no Blizzard assets or code.

## Status

The engine loads every map of the 1.27b install, runs its scripts, and
plays units, orders, combat, creeps, gathering, building and several hero
abilities. A viewer shows the maps in 3D, and you can select units and
order them. There is no full melee game yet, and no multiplayer.

What is checked against the game:

- file formats, the JASS language and VM, the number arithmetic and the
  random numbers match the game bit for bit;
- 24 of 31 replays of our test maps (one mechanic each) match the game turn
  by turn, including the turn checksum the game uses to detect desyncs;
- in the three hero tests (Mountain King, Blademaster, Archmage, Paladin,
  footmen, water elementals, creeps) every one of the engine's internal
  object changes and every turn checksum match the game: 11848 changes
  and 1614 turns (to 161 s), 4881 changes and 629 turns, and 7596 changes
  and 828 turns (to 83 s).

## Roadmap

1. **Foundations.** File formats, JASS, deterministic arithmetic, timers,
   map loading. Done.
2. **Viewer.** See a map, select units, give orders. Usable, with known
   differences from the game.
3. **Simulation that matches the game exactly.** Every mechanic checked
   against replays, down to the turn checksum. In progress: movement,
   combat, creeps, Human economy and the first heroes are done.
4. **Full melee game.** All four races, buildings, upgrades, items, all
   abilities, computer opponents. Not started beyond Human basics.
5. **Multiplayer with the original 1.27b client.** Not started.
6. **Beyond compatibility.** Lifted engine limits, new script APIs and
   modding, decided in the open.

There are no dates. Stages 3 and 4 are by far the largest, and parts of
them cannot be sized yet. [docs/roadmap.md](docs/roadmap.md) lists every
area with its state and marks where the amount of work is still unknown.

## Principles

- **Compatibility first.** Behaviour is specified against Warcraft III 1.27b;
  maps and scripts from 1.26-1.31 should run unchanged.
- **Clean-room.** Knowledge from reverse engineering enters the code only
  through written specs. See [docs/clean-room.md](docs/clean-room.md).
- **Deterministic simulation.** The game core produces bit-identical results
  on every machine, so lockstep multiplayer and replay-based testing work.

## Getting the source

The main repository is on GitHub:

```sh
git clone https://github.com/NCrashed/openwar3.git
```

A mirror is published over the [hbs2](https://github.com/NCrashed/hbs2)
peer-to-peer network and stays available if the GitHub repository goes away.
With a running `hbs2-peer` and `hbs2-git3` installed:

```sh
git clone hbs23://7VM1rtxrTmpXYVidNFHu5ZbNqRKMhKoRAV8vJebGaRN9 openwar3
```

## Development

```sh
nix develop      # or install a Rust toolchain >= 1.85
cargo test
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

Warcraft is a trademark of Blizzard Entertainment, Inc. This project is not
affiliated with or endorsed by Blizzard Entertainment.
