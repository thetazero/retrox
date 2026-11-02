# retrox

A single-player space game in a simulated, procedural galaxy. See [docs/DESIGN.md](docs/DESIGN.md).

## Crates

| crate | what |
|---|---|
| `crates/sim` | the galaxy simulation: state, the daily tick, economy, traders, routes |
| `crates/gen` | procedural generation of the galaxy, its systems and its starting factions |
| `crates/content` | data-only Rust: ship hulls, sections and modules |
| `crates/client` | the game: rendering, flight, the HUD and station screens, mining, skimming, combat, saves and the synthesized sound |
| `crates/tools` | `retrox`, headless tools for running and inspecting the sim |
| `crates/viewer` | a developer map that runs the sim live |

CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`
and `cargo test --workspace`, with warnings as errors.

## Running

```sh
cargo run --release -p retrox-client            # the game
cargo run --release -p retrox-viewer -- --seed 1 # the live sim map
cargo run --release -p retrox-tools -- simulate  # headless (see below)
```

### Game flags (for development)

The game opens on its title screen. Flags that set up a start (`--seed`,
`--station`, `--undocked`, `--field`, `--combat`, `--map`, `--screenshot`,
…) skip it and drop straight into a fresh game; `--fire`, `--sections`,
`--contract` and `--keys` only change a start that something else sets up,
and `--menu pause|quit` opens over a fresh game.

| flag | does |
|---|---|
| `--seed N` | generate galaxy `N` (default 1) |
| `--station` | start docked at a station, in the first system with one |
| `--undocked` | start in space: just out of the station, or launched |
| `--field` | start in an asteroid field facing a rock, prospected |
| `--sunward` | start in the field with the biggest sun in its sky, the sun behind the rock |
| `--quarry [back]` | start in the field a couple of hundred metres off a quarry, the sun behind the ship (or behind the quarry) |
| `--fire` | hold the mining laser's trigger down |
| `--traffic` | start in space alongside a trader fleet |
| `--skim` | start skimming a gas giant |
| `--cities` | start in orbit of a planet with two cities, cargo bound for one |
| `--interdict` | fly past a pirate gang's lair until it interdicts the ship |
| `--convoy` | start alongside a trader fleet, in its frame |
| `--hostile-convoy` | with `--convoy`: its faction counts you hostile |
| `--harvesters [gas\|ice]` | start by a harvester at work (default `gas`) |
| `--combat` | start in the nearest asteroid field with raiders, for them to find |
| `--brawl` | with the band close in front, battered, fighting at once |
| `--lost SECS` | the ship blown up that many seconds ago (implies `--brawl`) |
| `--rewind` | show the rewind to the last checkpoint |
| `--arrival` | show the arrival back at a checkpoint, at a station |
| `--map` | start with the galaxy map open |
| `--course JUMPS` | plot a course to the nearest system at least that many jumps away |
| `--ship HULL` | start in that stock ship (`retrox ship` lists them) |
| `--sections A,B,…` | with `--ship`: put these sections in its mounts, in order, fully fitted |
| `--contracts` | start on the dispatch board |
| `--contract` | also take the first contract on it |
| `--screen NAME` | start docked on `trade`, `hangar`, `shipyard`, `contracts` or `bounties` |
| `--size WxH` | open the window this size (logical pixels) rather than fitted to the screen |
| `--ship-view DEG` | look at your ship from that angle, close up (0 = from behind) |
| `--screenshot PATH.ppm` | save the 90th frame and quit |
| `--menu PAGE` | open a menu page: `title`, `new`, `load`, `settings`, or `pause` / `quit` over a fresh game |
| `--keys K1,K2,…` | press these keys, one a frame (`waitN` waits N frames), then quit (`--screenshot` stops it early) |
| `--mute` / `--sound` | silence any run / give sound to a screenshot or `--keys` run, which are silent by default |
| `--remote PATH` | play turn by turn from steps appended to PATH: keys, `hold:KEY:N`, `click:X:Y`, `scroll:X:Y:DY`, `shot:PATH.ppm`, `info:PATH` (bearings, rocks, raiders and their lead pips, gangs); the world waits between steps (for playtests) |

### Tools

```
retrox simulate [--seed N] [--years N] [--csv PATH]   run the sim headless, print timings and economy health
retrox map [--seed N] > galaxy.svg                    render the galaxy as SVG
retrox stats [--seed N]                               generation statistics
retrox prehistory [--seed N] [--years N]              grow the galaxy from the cradle
retrox bench [--seed N] [--years N]                   time the sim per actor
retrox ship [HULL]                                    list stock ships, or show one
```

### Benches and dumps

```sh
cargo test -p retrox-client careers -- --ignored --nocapture      # credits per hour for each career
RETROX_AUDIO_DUMP=dir cargo test -p retrox-client dump -- --ignored # every cue and a minute of each mood, as WAVs
```
