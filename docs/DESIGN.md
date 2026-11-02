# retrox — Design Document

Status: draft v0.1 · living document

## 1. Vision

A single-player 3D space game set in a procedurally generated, continuously
simulated galaxy of ~5000 star systems. You fly your own ship — mining,
hauling, trading, hunting bounties or turning pirate — while factions rise,
plan, trade, wage war and collapse around you, with or without your
involvement.

The tone is **hopeful Soviet-inspired retro-futurism**: collectivist
optimism about reaching the stars together, worn-in industrial hardware,
and a galaxy that is bigger than you. The look and mood are inspiration,
not a theme park — everything in-world is fictional, nothing is overdone.

### Pillars

1. **The galaxy lives without you.** The simulation is the game's heart.
   History happens whether or not you watch it. Most of the time, the player
   is one small participant.
2. **Small, but not powerless.** Players can change things — kill a
   flagship, break a blockade, starve a market — but it's expensive, and
   you will never single-handedly turn a major war.
3. **Hands on the stick.** Moment-to-moment play is direct, arcade 3D
   flight: mining, combat and docking are things you *do*, not menus.
4. **Time is earned by travel, not by fighting.** Action happens in real
   time; years of galaxy history pass during the connective tissue of
   travel, hauling and waiting.
5. **Systems over scripts.** Prefer emergent outcomes. Authored content
   (storylets) sets up situations; the simulation decides how they play out.

### Non-goals (for now)

- Multiplayer.
- Seamless planetary landing (see §7.6).
- Deterministic history replay (generation *is* deterministic; history isn't).
- Named NPC characters / crew personalities. Ships and factions are faceless.
- Permadeath.

## 2. Core loops

**Minute-to-minute:** fly, mine, fight, dock, scan, haul.

**Hour-to-hour:** pick work (contracts, trade runs, bounties, raids), travel
the hyperlane network, sell, refit, upgrade.

**Save-long (10–30 galaxy years over ~100h):** grow from a single small
ship to a fleet, build reputation (or infamy) with factions, rise through a
faction's ranks, automate trade routes, eventually command a capital ship —
while the galaxy's map, economy and politics shift underneath you.

Career paths are not classes; they are just what you spend time on:

| Path | Core activity | Feeds on |
|---|---|---|
| Miner | Flying the mining activity, selling ore | Supply chains, demand shocks |
| Trader | Price gaps, contracts, later automated routes | Economy, news, planned vs market borders |
| Bounty hunter | Hunting flagged pilots/pirates | Faction law, reputation |
| Pirate | Interdicting on lanes, fencing cargo | Hyperlane chokepoints, black markets |
| Officer | Faction military ranks, battle participation | Wars, fronts |

## 3. Time model

The galaxy sim ticks in **galaxy days**. Galaxy time is driven by *what
the player is doing*, not by wall-clock seconds.

| Activity | Real time | Galaxy time |
|---|---|---|
| Combat, mining, docking, local flight | 1:1 | effectively paused (minutes ≈ 0 days) |
| In-system supercruise | ~1 s ≈ 1 h | crossing a system ≈ 1–3 days |
| Hyperlane jump | ~10–20 s transit sequence | 2–14 days by lane length |
| Docked actions | instant / menus | per action, see below |
| Autopilot route / "wait" (docked only) | fast-forward | weeks–months |

Docked action durations:

| Action | Galaxy time |
|---|---|
| Trade, contracts, news | ~instant |
| Refuel / rearm | hours |
| Repairs | hours → weeks (scales with damage & hull size) |
| Refits | scales with module size |
| Buying a ship off the lot | ~1 day |
| Commissioning a custom build | weeks |

Rules:

- **Waiting is docked-only.** You can't skip danger by waiting in space.
- **Fast-forward auto-interrupts** on: interdiction, hostiles in range,
  contract deadline approaching, cargo spoilage, and news matching the
  player's filters.
- **Contracts carry galaxy-time deadlines**, which is what makes time
  matter to the player.
- Rough target pace: a trading-heavy hour ≈ 2–6 galaxy months; a
  combat-heavy hour ≈ a few days. (Tunable.)
- **Detail level is a function of both distance and time rate** — the same
  LOD mechanism (§5.2) handles "far away" and "time is moving fast".

## 4. Galaxy & procedural generation

- **~5000 star systems**, generated from a seed. Generation is
  deterministic: same seed → same starting galaxy.
- **Hyperlanes** connect systems into a graph. The graph is deliberately
  uneven: dense cores, sparse frontiers, chokepoints and long bridges.
  Lane topology drives trade routes, war fronts and piracy.
- Each system: star(s), planets, moons, asteroid belts/fields, stations,
  lane entry points, optional anomalies. Real-ish scale ("huge worlds"),
  traversed by supercruise.
- Resources are distributed by system type so that supply chains must
  cross space.
- **Factions are procedural**: origin region, ideology, **economic model**
  (planned or market, §5.4), tech level, aesthetic variant, attitudes
  toward others. Factions can form, split, merge and die over time.
- Starting history: optionally run the sim for N years at generation time
  so the galaxy begins with a past (wars already fought, ruins, borders).

## 5. Simulation

### 5.1 Principles

- The sim is a **plain-Rust, engine-free crate**. It runs headless.
- State is plain data in **typed arenas** (`SystemId`, `FactionId`,
  `FleetId`, `ShipId`, `MarketId` …) — relational, not ECS.
- Each tick is an **explicit phase pipeline**, readable top to bottom:
  1. Production & consumption
  2. Planning (faction AI, plan quotas, fleet orders)
  3. Trade & logistics
  4. Movement
  5. Conflict resolution
  6. Events & storylets
  7. News generation
- **Multithreaded** via rayon within phases (read previous state, write
  phase outputs, merge). History is not deterministic, and that's fine.
- Headless runs ("simulate 500 years, dump history") are a first-class
  tool for balancing and testing.

### 5.2 Two tiers + LOD bubble

- **Coarse tier (everywhere):** factions, systems, markets, stockpiles,
  fleet strengths, threat levels, as numbers. Small craft are counts.
- **Notable assets are always individual**, even in the coarse tier:
  capital ships, flagships, named stations, convoy leaders.
- **Agent tier (the bubble):** in systems near the player, ships are
  individual agents with goals (trade, mine, patrol, raid) flying real
  routes.
- Agents **materialize** from coarse state when entering the bubble and
  **collapse** back when they leave or when time compresses. Both
  directions conserve state (cargo, damage, losses).

### 5.3 Battles

- Major battles are resolved abstractly over days in the coarse tier.
- If the player joins, the bubble spawns **the slice near the player** from
  the actual fleets — the specific capital ships present plus a sampled
  share of escorts.
- Outcomes **write back exactly**. Kill a flagship and it's gone: the
  fleet's strength, morale and its commander's plans change. This is how
  the player can matter — expensively.

### 5.4 Economy

- **Real supply chains**, ~15–30 goods:
  ore/ice/gas → refined materials → components → modules, ships, consumer
  goods, food, fuel.
- Production facilities are stations/planets with inputs, outputs and
  stockpiles. Prices emerge from stockpiles and flows.
- **Planned economies:** quotas and five-year-ish plans. Stable official
  prices, chronic shortages and gluts, black markets in the gaps.
- **Market economies:** prices float; booms and crashes.
- Borders between the two are natural trade & smuggling opportunities.
- NPC traders, miners and haulers operate on the same timeline as the
  player, so observed price gaps really do close.

### 5.5 Factions

- Procedural, can form/split/merge/die.
- Have territory, fleets, economy, tech, relations, and goals.
- Player can join a faction and hold **military rank**.
- **Reputation** per faction; **bounties** issued by faction law.

### 5.6 News & propaganda

- The sim knows what actually happened. **The player never sees it
  directly.**
- Each faction runs its own state news, which is **biased**: inflates its
  victories, downplays losses, omits the inconvenient.
- Independent outlets exist (as a faction kind) and are more accurate but
  patchier.
- Information is something you can act on — and, eventually, trade in.
- The news feed is the main window into the off-screen simulation and
  should be diegetic (broadcasts/papers received at stations or on comms).

## 6. Events

Two sources, same plumbing:

1. **Emergent:** wars, booms/crashes, shortages, migrations, pirate
   surges, faction collapse, colonization — produced by sim rules.
2. **Semi-scripted storylets:** authored setups such as discovering a
   new kind of mining resource, breakthrough tech, the alien arrival.
   The sim decides *when and where*; authored content decides *what*.

Storylets are **data-only Rust**, no DSL:

```rust
Storylet {
    id: "first_contact",
    trigger: Condition::All(vec![
        Condition::GalaxyYearAtLeast(12),
        Condition::FactionTechAtLeast { tech: Tech::DeepScan, level: 3 },
        Condition::SystemTagged(Tag::FrontierEdge),
    ]),
    effects: vec![
        Effect::SpawnFaction(FactionTemplate::Visitors),
        Effect::News(NewsTemplate::UnknownSignal),
    ],
    once: true,
}
```

Conditions and effects are enums, not closures, so storylets stay
serializable, inspectable in debug tools and testable.

**Aliens** arrive via storylet, then run on the normal faction sim. Their
nature is open (see §11) — leaning toward strange and hard to understand
rather than space bugs.

## 7. Player systems

### 7.1 Flight

- **Arcade** 3D flight model; cockpit and third-person views.
- Supercruise for in-system travel; hyperlane jumps between systems.
- Interdiction can pull ships out of supercruise (piracy, patrols).

### 7.2 Combat

- Small/medium ships: direct dogfighting.
- Capital ships: **combat changes** — subsystem / compartment targeting,
  turret direction, damage per compartment, possibly crew-less
  "station" switching. Exact design open (§11).

### 7.3 Mining

- An **activity**, not a timer: fracture asteroids, cut into them, collect
  chunks, manage heat/hold.
- First thing a new player does; must feel good on its own.

### 7.4 Ships

- Ships are built from **sections**, each with its own slots and each its
  own damage location. **Small ships** are one built-in section. **Medium
  and large ships** are a frame with bow, mid and stern mounts; any section
  of the matching position and *exact* size goes in a mount (a gun deck or
  a cargo bay amidships, a sprint stern or a cruise stern aft). The frame
  itself differs per hull: number of mid mounts, mass, structure.
- Slot kinds: weapon, engine, RCS, power, shield, radar, radiator, cargo.
  A slot takes a module of its kind at its size or smaller.
- Two budgets tie a fit together: **power** (reactors vs. everything that
  draws) and **heat** (active modules vs. sections + radiators, with a heat
  capacity that buffers surplus). Fits may be over budget at full load —
  e.g. an interceptor that can't fire, shield and burn all at once — and
  the pilot manages that in flight.
- **Refitting only while docked.** Swapping a section takes longer than
  swapping a module, and unfits the modules inside it.
- Turning comes from **RCS thrusters** working against the ship's moment
  of inertia (mass × length²), so heavy or long ships turn slowly.
- Roles: combat, trade, speed, at small / medium / large. No hangars yet.
- **Capital ships** come later: a stack of modules (habitation, cargo,
  reactor, hangar, weapons…), each with its own slots, and each a damage
  location — the same section model at a larger scale.
- Procedural generation of ship visuals is built from the same module kit.

### 7.5 Fleet & automation

- Own multiple ships, hire faceless NPC captains, assign automated trade
  routes. Stations later.

### 7.6 Planets

- No seamless landing. Planets have **predefined landing points**; landing
  plays an animation and opens the location's UI.

### 7.7 Death

- On ship destruction, **restore the last checkpoint** (last dock /
  autosave). Because history isn't deterministic, the galaxy may play out
  differently after a restore — the future isn't fixed.

## 8. Art direction & audio

**Hopeful Soviet-inspired retro-futurism**, a mix of tones across
factions: collectivist optimism, worn-in melancholy, occasional absurdity.
Inspiration, not imitation — fictional throughout.

- **Ships:** orbital-hardware vocabulary — cylinders, spheres, trusses,
  radiators, solar wings, thermal blankets, stenciled markings. Capital
  ships and big stations go monumental / brutalist.
- **Cockpit:** diegetic — painted panels, VFD and nixie-style readouts,
  analog dials, toggles. Third-person uses the same information as a HUD.
- **Galaxy map:** an electromechanical star-chart instrument rather than a
  flat menu.
- **Graphic design:** constructivist-inspired typography and composition
  for faction branding, loading screens and station UI.
- **Text:** English. No fake Cyrillic.
- **Rendering:** flat / lightly lit hulls, strong silhouettes, glow,
  film grain, period film-stock color grading.
- **Audio:** analog-synth score, radio static and crackle on comms, heavy
  mechanical sounds for docking, switches, and hull stress.

## 9. Technical architecture

Pure Rust, Linux first.

```
retrox/
  crates/
    sim/      # galaxy state, tick pipeline, economy, factions, events, news
    gen/      # procedural generation (galaxy, systems, factions, ship visuals)
    content/  # storylets, goods, module & hull definitions (data-only Rust)
    client/   # rendering, input, audio, local physics, UI
    tools/    # headless runner, history dumps, balancing
```

- **sim:** no engine dependencies. Typed arenas, explicit phases, rayon.
  Serializable with serde — a save is (mostly) one serialized struct.
- **Spatial queries:** galaxy level = hyperlane graph + kd-tree over system
  positions; in-system = f64 coordinates per system frame with a
  spatial index (grid/BVH) for "what's near X".
- **client:** thin custom client on `wgpu` + `winit`; `rapier3d` for local
  physics; `kira` for audio; `egui` for dev/debug tools; custom in-game UI
  matching the art direction. Floating origin / camera-relative rendering
  for large scales. The client owns only what's near the camera; it reads
  sim state and submits player actions.
- **No ECS for the galaxy.** If the client wants an ECS-like structure for
  the local scene it's an implementation detail, since that state is a
  disposable view of the sim.
- **Saves:** single long save + checkpoints/autosaves at docking.

## 10. Milestones (rough)

1. **Sim skeleton (headless):** galaxy gen + hyperlanes, factions, basic
   economy with a few goods, tick pipeline, history dump tool.
2. **Flight sandbox:** wgpu client, one system rendered at scale, arcade
   flight, supercruise, docking, floating origin.
3. **Mining & trading loop:** mining activity, markets, cargo, jumps,
   time model wired to the sim.
4. **Bubble & LOD:** agents materialize/collapse, NPC traffic, interdiction.
5. **Combat & reputation:** dogfighting, bounties, piracy, faction rep.
6. **News & propaganda; planned vs market economies.**
7. **Ships & fleets:** hull/compartment/slot design, owning fleets,
   automated routes.
8. **Battles & capital ships:** battle slices, exact write-back, capital
   ship combat.
9. **Storylets & aliens.**
10. **Art pass:** cockpit, galaxy chart instrument, post-processing, audio.

## 11. Open questions

- Exact target pace (galaxy months per play-hour) — tune after milestone 3.
- Capital-ship combat design in detail.
- Nature of the aliens and the semi-scripted anomalies/discoveries.
- Mining activity mechanics in detail.
- Faction count and size at generation; agent budget in the bubble.
- Gamepad support / control scheme.
- Does the player's character age over a ~30-year save? (Probably ignored.)
