# retrox

A single-player space game in a simulated, procedural galaxy. See [docs/DESIGN.md](docs/DESIGN.md).

## Crates

| crate | what |
|---|---|
| `crates/sim` | the galaxy simulation: state, the daily tick, economy, traders, routes |
| `crates/gen` | procedural generation of the galaxy, its systems and its starting factions |
| `crates/content` | data-only Rust: ship hulls, sections and modules |
| `crates/client` | the game: rendering, flight, the HUD and station screens |
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

The game opens on its title screen. Starting flags (`--seed`, `--station`,
`--undocked`, …) skip it and drop straight into a fresh game.

| flag | does |
|---|---|
| `--seed N` | generate galaxy `N` (default 1) |
| `--station` | start docked at a station, in the first system with one |
| `--undocked` | start in space: just out of the station, or launched |
| `--field` | start in an asteroid field facing a rock, prospected |
| `--fire` | hold the mining laser's trigger down |
| `--map` | start with the galaxy map open |
| `--course JUMPS` | plot a course to the nearest system at least that many jumps away |
| `--ship HULL` | start in that stock ship (`retrox ship` lists them) |
| `--sections A,B,…` | with `--ship`: put these sections in its mounts, in order, fully fitted |
| `--contracts` | start on the dispatch board |
| `--contract` | also take the first contract on it |
| `--ship-view DEG` | look at your ship from that angle, close up (0 = from behind) |
| `--screenshot PATH.ppm` | save the 90th frame and quit |
| `--menu PAGE` | open a menu page: `title`, `new`, `load`, `settings`, or `pause` / `quit` over a fresh game |
| `--keys K1,K2,…` | press these keys, one a frame (`waitN` waits N frames), then quit |
| `--remote PATH` | play turn by turn from steps appended to PATH: keys, `hold:KEY:N`, `click:X:Y`, `scroll:X:Y:DY`, `shot:PATH.ppm`, `info:PATH`; the world waits between steps (for playtests) |

### Tools

```
retrox simulate [--seed N] [--years N] [--csv PATH]   run the sim headless, print timings
retrox map [--seed N] > galaxy.svg                    render the galaxy as SVG
retrox stats [--seed N]                               generation statistics
retrox prehistory [--seed N] [--years N]              grow the galaxy from the cradle
retrox bench [--seed N] [--years N]                   time the sim per actor
retrox ship [HULL]                                    list stock ships, or show one
```
