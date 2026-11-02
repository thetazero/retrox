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
6. **History is simulated, not authored.** Generation builds the physical
   galaxy and a single cradle of civilization, then runs the real sim
   forward for centuries. Every faction, border, colony, ruin and grudge
   the player finds is the output of that history (§4.2).

### Non-goals (for now)

- Multiplayer.
- Seamless planetary landing (see §7.6).
- Named NPC characters / crew personalities. Ships and factions are
  faceless. Historical figures exist only as names in the history record
  (founders, admirals, rebel leaders) — no personalities, no dialogue.
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

## 4. Galaxy & world generation

Generation has two stages: build the physical galaxy, then simulate its
history. The whole thing is deterministic — same seed and config, same
starting galaxy (§5.1).

### 4.1 The physical galaxy

- **~5000 star systems**, generated from a seed.
- **Hyperlanes** connect systems into a graph. The graph is deliberately
  uneven: dense cores, sparse frontiers, chokepoints and long bridges.
  Lane topology drives trade routes, war fronts and piracy.
- **Every lane has a tier.** A ship's drive has a tier too, and can use
  every lane at or below it. Low tiers form a sparse, slow backbone;
  higher tiers add shortcuts and long bridges. Which network you can use
  depends on your drive.
- Each system: star(s), planets, moons, asteroid belts/fields, stations,
  lane entry points, optional anomalies. Real-ish scale ("huge worlds"),
  traversed by supercruise.
- Resources are distributed by system type so that supply chains must
  cross space.
- Nothing civilizational is placed here: no factions, territory,
  population or facilities. Those come from history.

### 4.2 Simulated prehistory

- History starts from **one cradle** — a single homeworld at the dawn of
  FTL, shared by **2–4 rival nations**. Every later faction descends from
  one of them, so the galaxy shares an origin but starts with rivalry: a
  space race outward, and the first wars before space is widely settled.
- The sim then runs for a fixed **`history_years`** (tunable; ~400 to
  start). How far settlement has spread when it stops varies by seed.
- Prehistory runs the **same sim rules as play** (§5), at a coarser time
  step (§5.1), so the galaxy the player enters is a state the live sim can
  sustain — not a hand-balanced snapshot.
- What the player inherits is all output: factions and their family tree,
  borders, populations, facilities, markets, tech levels (and so which lane tiers they can use),
  ship designs, remnants (ruins, wrecks, abandoned colonies), and the
  history record itself (§5.7).
- **The player starts at that fixed present day and picks where.** A
  start screen shows the map and its history (shared with the legends
  tool, §9), so choosing a starting system is choosing a place with a
  past.
- Target: prehistory finishes in about a minute on a desktop (tunable).

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
  phase outputs, merge) — but the cost model below is designed for one
  core first; threads are headroom, not the plan.
- **Deterministic, for the whole game:** same seed + same player actions →
  same history. Rules that keep it so:
  - Randomness comes from RNG streams derived from (seed, purpose, time,
    entity), never from a shared generator or thread order.
  - Parallel phases produce per-item results and merge them in a fixed
    order; no order-dependent reductions.
  - The client–sim boundary is an **input log**. Local flight physics
    needn't be deterministic; what gets written back to the sim ("flagship
    destroyed on day N", "cargo sold") is recorded as input. Saves are
    snapshots for speed, but seed + log reproduces them.
  - Linux first. Same-seed results across platforms will eventually need a
    portable math library for `powf`/`exp`/etc.
- **The sim ticks daily, everywhere, always.** Coarser steps would break
  rules with timescales under a month (famines, trips, contracts). Speed
  comes from what each actor does per day, not from skipping days (§5.2).
- Headless runs ("simulate 500 years, dump history") are a first-class
  tool for balancing and testing.

### 5.1.1 Cost model

Seeding a world means simulating centuries of history (§4.2), so the
coarse tier is designed around a per-day cost, not tuned toward one
later. Roughly: 400 years is 144,000 days; a mature galaxy might hold
~10,000 settlements and ~50,000 traders. Aim for prehistory in about a
minute on one core; a few minutes is acceptable while the shape settles.
That puts the daily budget well under a millisecond, which only works if
every actor follows its kind's rule:

| actor | runs | per-run work | aim (one core) |
|---|---|---|---|
| settlement | daily | its own books: production, consumption, population, prices, ledger transfers | tens of ns |
| settlement, faction | monthly, each on its own day | decisions: construction, colonization, politics | ~100 ns |
| trader | on events only (depart, arrive) | move goods and money through the ledger | tens of ns |
| trader | monthly, on its own day | commit to a route for the month | ~100 ns |
| colony ship | on events only | found a settlement on arrival | — |

Consequences:

- **Daily work is arithmetic.** No searching, no allocation, no per-day
  decisions. Anything that needs a search happens monthly, from data
  prepared for it: cached lane distances, short per-region lists of
  where each good is cheap or wanted, a settlement's stored distance to
  its capital.
- **Traders don't tick.** A calendar queue (a ring of day buckets)
  hands each day its departures and arrivals. Between events a trader
  costs nothing. A trader commits to a route monthly and runs it until
  its next review.
- **Within a day:** settlements' local tick, then that day's events in a
  fixed order, then whoever's monthly review falls today; prices update
  once at the end. Every random draw uses a stream (§5.1), so the queue
  doesn't change history.
- **Measured, not assumed.** Microbenchmarks track the per-settlement,
  per-event and per-decision costs, and a prehistory benchmark tracks the
  whole. The aims above will move as we learn where things land.

### 5.2 Two tiers + LOD bubble

- **Coarse tier (everywhere outside the bubble, and all of prehistory):**
  the actors of §5.1.1. Settlements keep their books daily; factions and
  settlements decide monthly; traders are companies that own counts of
  ships, commit to routes monthly and move goods only on departure and
  arrival events. No individual ships, no flights.
- **Notable assets are always individual**, even in the coarse tier:
  capital ships, flagships, named stations, convoy leaders.
- **Agent tier (the bubble):** in systems within a few jumps of the
  player (radius to be tuned; ~3 jumps is ~20 systems), ships are
  individual agents with goals (trade, mine, patrol, raid) flying real
  routes under the flight model. None in prehistory.
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

- Procedural, can form/split/merge/die. The only factions placed by hand
  are the cradle's founding nations (§4.2); every other faction arises in
  the sim.
- **Secession is how new factions are born.** Holding territory costs
  more the further it is from the capital; shortages and overextension
  erode stability; unstable regions break away. This loop is what stops
  history converging on one empire or an empty galaxy.
- **Factions have a family tree.** A breakaway inherits language, economic
  model, ideology and tech from its parent, then drifts. Names follow the
  lineage (the Free Ports of Kessa broke from the Union of Kessa).
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
- **The past is reported with bias too.** Prehistory reaches the player
  through each faction's version of it: one faction's hero of the Sereth
  bridge is another's war criminal.

### 5.7 History engine

What prehistory (§4.2) needs from the sim — and what keeps running during
play:

- **Population** grows when fed and shrinks under shortfall.
- **Colonization:** factions expand along the lanes their drives can use,
  toward resources and room.
- **Construction:** facilities are built where prices and shortfalls call
  for them.
- **Politics:** stability, overextension and secession (§5.5); relations,
  war and conquest, with battles resolved abstractly (§5.3).
- **Outside shocks** at purely random rates — plagues that spread along
  trade flows, resource discoveries, lanes collapsing or opening. They use
  the storylet plumbing (§6) and act *through* the sim (a plague hits busy
  trade hubs hardest), never as one-off stat changes. No director: the
  internal loops have to keep history moving on their own.
- **Tech** per faction, inherited on secession, spread by trade and
  contact — and **losable**. Collapse brings dark ages, so ruins of an
  earlier era can hold better tech than anyone has now. Modules carry a
  tech requirement.
- **Lane tiers follow drive tech.** There is no per-faction lane
  discovery: a faction can use every lane up to its drive tier. Reaching a
  new drive tier opens a new layer of the network at once; losing it in a
  dark age cuts regions off from the routes that held them together.
- **Ship designs evolve.** Factions generate designs from the section and
  module kit (§7.4) within their tech and economy, in lineages; old hull
  classes stay in service. Stock ships become examples, not the catalog.
- **Remnants** persist where history left them: ruins of collapsed
  colonies, wreck fields at battle sites, abandoned stations, fallen
  capitals — each tagged with its origin event, era, faction and tech.
  They survive ownership changes and are salvage, exploration and
  storylet hooks.
- **The history record** is rich and queryable: every founding,
  secession, war, battle, collapse, famine and shock, linked to the
  systems, factions, ships and historical figures involved.

Rough build order, each step adding to the world on its own: factions and
politics → remnants → tech and lane tiers → ship designs (which needs
tech).

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
- **Gear:** a *mining laser* is a module for a weapon slot (the starting
  courier has one in its chin mount). Its cut rate and reach come from the
  module; its heat is a gauge of its own, shed by the ship's radiators.
  Run it to the top and it locks out while it vents.
- **Prospect:** a ping reads every rock nearby: ore or ice, a grade (A–D),
  volatile or not, and its *veins* glow on the surface. Rocks you cut are
  read too.
- **Cut:** the laser cuts four times as fast on a vein and runs cooler
  there, and a rock fractured through a vein yields half as much again.
  Rocks spin, so veins turn away; the skill is chasing them.
- **Fracture:** rocks up to ~60 m split into smaller rocks and shed ore
  chunks; the smallest crumble. Bigger rocks are **quarries**: each
  fracture calves a boulder off where the laser hit, until the quarry is
  worked out. **Volatile** rocks are richer but burst when they fracture,
  scattering ore and shoving the ship.
- **Collect:** a tractor pulls loose chunks into the hold (as the market
  goods ore and ice) when you fly close; when the hold is full, they stay
  floating. Sell at any market.
- Fields remember: broken and worked rocks stay that way for ~120 galaxy
  days, then the field is whole again.

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

### 7.5.1 Freight contracts

Hour-to-hour work for the Trader path (§2), drawn from the economy, never
from a random generator (pillar 5).

- **Where they come from.** A settlement's dispatch board pairs a good it
  can spare with a settlement within 5 jumps that is short of it,
  counting what traders already have on the way. A consignee posts
  once it holds less than 75% of its target, and the origin releases
  stock only down to 75% of its own target, so goods always flow from
  fuller markets to emptier ones. Lots are sized to the player's hold.
  A board holds 8 contracts at most, 2 per destination, best pay per day
  first. Boards change daily as markets do; some days a port has nothing.
- **Who pays, and what.** The consignee buys the goods at the origin's
  price and pays the fee from its own treasury: the price gap between
  the two markets, its faction's delivery premium if planned (the same
  premium NPC haulers earn), and freight for the expected trip (a rate
  per tonne-day plus the crew's wage), scaled up by how short it is. It
  only commits what it can afford.
- **Deposit.** Taking a contract costs a deposit of the cargo's value,
  returned on delivery. Contract cargo rides in the hold but can't be
  sold.
- **Deadlines** allow 1.5× the expected trip (jumps plus ~3 days per
  system). Each day late takes 10% off the fee. Ten days late, or
  abandoned, the contract fails: the consignee keeps the deposit and its
  fee, and the goods are the player's to sell. No reputation cost yet;
  that comes with faction reputation (milestone 5).
- **Delivery** happens on docking or landing at the consignee. Waiting
  while docked stops on the day a contract falls due (§3).
- **The client starts 30 days after generation**, so markets have
  drifted off their generated targets and the first board has work on
  it. Prehistory (§4.2) will replace this warm-up.

### 7.6 Planets

- No seamless landing. Planets have **predefined landing points**; landing
  plays an animation and opens the location's UI.

### 7.7 Death

- On ship destruction, **restore the last checkpoint** (last dock /
  autosave). Because the game is deterministic, the same future plays out
  again unless you act differently — what you learned before dying is
  yours to use.

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

### 8.1 Ship models and liveries

Decided while hand-building every hull (PR #86):

- **No reused models** (the user's ruling): every hull has its own design.
  Large hulls are not scaled-up medium ones, and each framed hull has its
  own version of every section type. Only module parts (gun barrels, radar
  dishes, engine bells, radiator panels) are shared between hulls.
- **Liveries by role:** courier cream and red; trader and hauler worn
  mustard with hazard stripes; interceptor, gunship and frigate gunmetal
  or olive with red accents; blockade runner matte dark; express freighter
  a cream airliner livery with a cheatline; bulk freighter oxide red;
  racer cream and sky blue.
- Stencilled seven-segment hull numbers, and weathering on enamel paint.

### 8.2 Galaxy map instrument

Decided while building the star chart (issue #71):

- The map is seen **through a round chart glass** set in an enamelled
  console, not as a full-screen canvas: the console frame is what makes it
  read as a physical instrument, and the round glass keeps the eye on the
  focus.
- Every view control has a **physical scale**: a bearing ring graduated
  in degrees turns as the view orbits and is read against a fixed red
  index (galactic bearing, 0° along +x); elevation and range have their own
  arc scales with amber pointers; range is logarithmic.
- **Drum counters** (cream on black, rolling over at each whole number)
  repeat the chart drive's settings and the focus position. Mechanical
  readouts belong to instruments that are set by hand; live ship data stays
  on teal VFD.
- The galactic plane carries **range rings and bearing spokes** round the
  focus in faint phosphor teal, replacing the square grid.
- Moving the chart's focus (double-click, CENTRE) **slews** it with
  easing, as a motor-driven chart would, and a SLEW lamp lights while it
  runs.
- The course panel shows a **jump interlock** of lamps (course set, well
  clear, jump ready), so why a jump is unavailable reads at a glance.
- Systems keep their faction colours: the chart is information first.

## 9. Technical architecture

Pure Rust, Linux first.

```
retrox/
  crates/
    sim/      # galaxy state, tick pipeline, economy, factions, events, news
    gen/      # procedural generation (galaxy, systems, factions, ship visuals)
    content/  # storylets, goods, module & hull definitions (data-only Rust)
    client/   # rendering, input, audio, local physics, UI
    tools/    # headless runner, history dumps, legends, balancing
```

- **tools** gains `retrox legends` (browse and query the history record)
  and a time-lapse map (borders every N years) for tuning the history
  engine. The in-game start screen (§4.2) reuses the legends views.

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
  - Each game has its own folder (`<data dir>/saves/<started>-<seed>/`,
    data dir `$XDG_DATA_HOME/retrox`) with two files: `save.json`, the one
    long save, written from the pause menu; and `checkpoint.json`, rewritten
    on every docking and landing. A death will restore the checkpoint
    (§7.7). *Continue* loads the newest file of any game.
  - A save is the whole sim state (RNG streams, calendar queue, free lists)
    plus the player's: date, ship and fit, cargo, credits, stored ships,
    and docked / landed / in space and where. Loading it replays the same
    future given the same actions (tested).
  - No saving in transit (fast travel, a hyperlane jump, the landing
    animation): the pause menu says why, and checkpoints fall on arrival.
  - File: a JSON header line (what the load screen shows) then the player's
    state and the galaxy, gzipped. Fits are saved as content ids, not
    indices. A `FORMAT` number is bumped when saved state changes shape;
    other formats, or files that don't parse, are refused with a message,
    never a crash. Migrating old saves can come once there are players
    with saves worth keeping.
- **Menus:** launch to a title screen (Continue, New game, Load,
  Settings, Quit) over a backdrop that needs no galaxy. New game takes a
  seed (a number, or a word hashed to one; random by default) and
  generates on a worker thread behind a loading card. Esc in play opens
  the pause menu (Resume, Save, Load, Settings, Quit to title), which
  stops galaxy time; it first closes the map or backs out of a station
  screen. Quit to title offers save-and-quit. Menus work with the mouse or
  the arrow keys and Enter. Settings (mouse sensitivity, invert pitch,
  fullscreen, interface size) persist in `$XDG_CONFIG_HOME/retrox/
  settings.json`. Development flags (`--seed`, `--station`, …) still skip
  the title; `--menu` and `--keys` drive menus for screenshots and checks.

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
11. **History engine:** cradle generation, the §5.7 rules, `dt`-sized
    steps, determinism rules, legends tool and time-lapse map, start
    screen. Order relative to the others is flexible.

## 11. Open questions

- Exact target pace (galaxy months per play-hour) — tune after milestone 3.
- Capital-ship combat design in detail.
- Nature of the aliens and the semi-scripted anomalies/discoveries.
- Faction count and size at generation; agent budget in the bubble.
- Gamepad support / control scheme.
- Does the player's character age over a ~30-year save? (Probably ignored.)
- History tuning: default `history_years`, prehistory step size, and how
  strong the secession loop and shock rates should be.
- How tech is lost and recovered in detail.
- How many lane tiers, and how lanes are split between them.
- How historical figures are generated (lifespans, which events earn a
  name).
