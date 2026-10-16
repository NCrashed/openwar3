# openwar3

An open-source reimplementation of the Warcraft III engine, written in Rust.

The goal is to give the modding community an engine it controls: existing
maps keep working, long-standing engine limits go away, and new APIs are
decided in the open.

openwar3 is an engine only. You need your own copy of Warcraft III to provide
the game data; this repository contains no Blizzard assets or code.

## Status

Early setup. Nothing is playable yet.

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
