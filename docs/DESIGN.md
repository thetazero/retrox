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
- **Skipped days play out, they don't freeze the screen** (code-health
  audit, #75). A wait ticks as many days per frame as fit a small frame
  budget, with a day counter and a stop button on the station screen;
  undocking ends it. A hyperlane jump's days tick across its transit
  sequence, so you arrive with the clock already caught up. Either way
  the sim still steps one day at a time (§5.1).
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
  8. Prices: every market reprices once, at the end of the day
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

#### 5.1.2 Settlement books as built (#56)

- **The production phase is the daily books; planning is decisions.**
  `settlement::books` walks the settlements once, in slot order. Each
  one produces and consumes, its population changes by what it ate,
  its project takes today's materials (planned factions pay wages), and
  it settles tax and welfare with its faction as one ledger transfer.
  Then companies pay their taxes and factions close their books. The
  planning phase only holds monthly reviews: what to build, and whether
  to send colonists.
- **One walk, one thread.** Money moves through shared faction accounts,
  which fixes the order. Running the part each settlement does alone
  (production, population) on threads needs a second walk. Measured,
  that costs more on one core (250 against 170 ns per settlement-day)
  than threads win back on twenty, so the books stay one walk.
- **Review days are by slot.** A settlement reviews when
  `day ≡ slot index (mod 30)`, so a day's reviewers are found by a
  stride through the pool, not by testing every settlement. Construction
  and company founding review on that day, colonization fifteen days
  later. A settlement founded in a reused slot takes that slot's day.
  Founding draws its random stream only for settlements reviewing today.
  The scan for idle ships in a system runs only once a reviewer is
  otherwise ready to found a company.
- **Prices without `powf`.** The scarcity curve (`scarcity^0.6`, held
  between a quarter and five times base) comes from two written-out
  tables: one for the power of two, one interpolated for the rest. It is
  within 1e-5 of `powf`, and because the tables are literals it gives the
  same price on every platform, which §5.1 asked for.
- **Welfare runs a day behind.** Each day's books pay out the budget the
  day before set aside, shared by the claims staked the day before, and
  stake today's claims for tomorrow. That keeps the books to one walk.
  The round is saved with the faction, so a loaded game pays the same.
  A faction's head count is totted up during the walk and takes effect
  when the books close, so its debt limit holds still within the day.
- **Nothing allocated daily.** Per-faction sums live on the faction, and
  the trade board (each system's best buyer and seller) is rebuilt in
  place in a per-system table kept between days.
- **Where it stands** (seed 0, static galaxy of ~3 750 settlements, one
  core): books 170 ns per settlement-day, prices 17, monthly reviews 57
  (spread over the month, about 2 µs per review). Before: about 240,
  60–100 and 180. The books are not yet tens of ns. Production itself
  is ~100 ns of it, and the cost is memory more than arithmetic: the same
  code runs in ~50 ns on settlements packed together. The next lever is
  layout. A settlement is ~480 bytes with its facilities in a separate
  heap list. Facilities could sit inline, one slot per kind (there are
  eight), and the fields the books touch could be kept together, apart
  from names and history. Making reviews cheaper was #58's job
  (§5.1.3).

#### 5.1.3 Monthly decisions as built (#58)

Decisions read data prepared for them; the searching happens when the
data is made, once a month or when something changes. `retrox bench`
times each kind of decision.

- **A "region" is a system's trading neighbourhood:** its 20 nearest
  settled systems by lane (`ROUTE_SYSTEMS`, the same reach traders
  always had). Lists hang off systems, so a trader reads its port's.
- **Trade lists.** Each settled system keeps, for every good, the four
  nearby settlements paying most for it; the four most valuable
  surpluses nearby; and the four nearest shipyards, each with its lane
  distance. It refreshes them once a month, on its own day (system index
  mod 30), from the day's listings: each system's best buyer of every
  good and its best surplus, still rebuilt daily from the books. A
  trader at its review looks only at its port's lists, but prices what it
  finds there at today's prices, and keeps to its own class's range. With
  nothing worth carrying, it heads for the listed surplus that would
  fetch most today. Systems in the bubble refresh daily, so traders near
  the player still answer the markets the player changes (§5.2.1). A
  system settled since its day came round makes its lists on the spot.
  Lists are saved: they hold the prices of the day they were made, and a
  loaded game must choose as the running one would.
- **Hulls.** New crews fetch hulls from, and captains leave orders at,
  the yards on their port's lists, rather than searching for one. Each
  system notes its own yards when it refreshes, and its neighbours read
  that instead of every facility.
- **Idle ports.** Founding a trading or harvesting company checks a set
  of the systems where ships sat docked idle, made in one scan of the
  fleets on the first day of each month (and saved), not by each
  settlement that asks.
- **Colonization.** Each system keeps its settleable sites best first
  (sites and their scores don't change) and how many at the front are
  taken. A launch compares the first free site of each system in reach,
  which is exactly what searching every body in reach found: a test
  checks the two agree while sites are taken. Taking a site moves its
  system's list past it; one that comes free (colonists who scatter)
  sends only its own system's list back to the start. This follows from
  the state, so it isn't saved.
- **Construction** compares margins in the settlement's own books: one
  pass over its facilities sums each kind, and the system's bodies are
  looked at only for a kind that needs a resource.
- **Pirates.** A settlement's crews consider turning pirate a week after
  its other reviews, found by the same stride through the slots. The
  count of settled places that caps gangs is kept until a settlement is
  founded, and a gang choosing a lair reads fields from an index by
  system instead of scanning every field.
- **Politics** isn't built yet, but each settlement already stores its
  lane distance to its capital (`to_capital`): worked out when it is
  founded, and again only when its owner or its faction's capital
  changes (`politics::set_owner`, `set_capital`), never at a review.
  Saves made before it was stored work it out on loading.
- **Where it stands** (seed 0, static galaxy of ~4,450 settlements, one
  core, ns per decision, before → after):

  | decision | before | after |
  |---|---|---|
  | trader's route | 7,300 | 1,360 |
  | trade lists, per system-month | — | 2,000 |
  | founding a company | 1,780 | 65 |
  | colonization | 1,260 | 310 |
  | pirates (now every due settlement, ports included) | 6,900 | 310 |
  | construction | 330 | 310 |
  | harvester's run (unchanged) | 1,770 | 1,710 |

  A day went from 3.1 to 1.8 ms: planning from 115 to 35 ns per
  settlement-day, trade from 350 to 180 ns per fleet-day. A 60-year
  `retrox simulate` went from 76 to 57 s, its planning from 17 to 3.5 s,
  and its health report barely moved (fewer idle traders, fuller holds).
  Still above ~100 ns: a route, which prices a dozen or so listed
  buyers at today's prices; a list refresh, which reads 20 neighbours'
  listings; and a harvester's run, which still walks its system's
  sources and markets. Those are the next levers.

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

#### 5.2.1 The bubble as built (#59)

- **Radius: 3 jumps** from the player's system, counted in lanes. On
  the default galaxy that is ~24 systems and ~15–20 trader fleets in
  flight, with no measurable cost per day.
- **Time compresses** when the player waits docked or is in the lanes:
  the bubble goes away for those days and comes back around wherever the
  player is next. Fast travel in a system keeps it — traders flying past
  while you cruise are what piracy and interdiction will be made of.
- **Materialize:** a coarse trip that passes through the bubble becomes
  a flight on the day it gets there, placed along its trip (burn out,
  lane, burn in) by how far through its calendar it is. Fleets leaving
  port inside the bubble fly from the start. Docked, idle fleets stay
  records.
- **Collapse:** a flight that jumps toward a system outside the bubble,
  or that the bubble leaves behind, becomes a coarse trip again, its
  remaining days estimated the way coarse trips are.
- **Behaviour:** in the bubble a fleet picks a fresh route at every port
  instead of waiting for its monthly review, and the bubble's systems
  refresh their trade lists daily (§5.1.3), so traders near the player
  react to the markets the player is changing.
- **Losses** roll once per trip, coming out of its last lane, in both
  tiers.
- **Planet ports:** ships dock at a holding point above a planet's north
  pole (1.5 radii out), not at its centre.
- **Pirates** fly in the bubble too (§7.2.2).
- **What a bubble day costs** (#115): a flight flies a few dozen
  autopilot steps a day. A goal must say how it really moves: one whose
  velocity is off by more than the arrival tolerance (5 m/s) is never
  reached, and the ship steps at the smallest step all day. Harvesters'
  work spots once did (their star-facing offset turns with the source),
  and a busy bubble's day took most of a second. So moving goals carry
  every part of their motion, a step is never shorter than a tenth of
  the time to match the goal's speed, and `a_busy_bubble_day_flies_few_steps`
  holds the busiest system to under 1,000 steps per flight-day.
  `retrox bench` reports the bubble's ms per day and steps per flight-day.
  The player's fast travel follows the same rule (#109): a climb out of a
  well heads for the edge facing the next target, which swings round as
  the planet orbits, and its goal carries that swing. Before, the
  autopilot chased the spot ~5 m/s off for over 100 galaxy days in its
  start-of-trip rehearsal (a freeze on [J], and 94 s of every test run),
  and the climb itself cost 18 hours of galaxy time instead of minutes.
- **Not yet in the bubble:** colony ships, patrols. Harvesters fly in it
  (§5.4.2).
- **Seen by the player:** the client carries each flight on between the
  sim's daily ticks with the same autopilot, and draws each fleet as its
  ships in a loose echelon, nose along its thrust (a braking ship flies
  tail first), engines lit by how hard it burns. The HUD marks every
  fleet in the system with a small VFD-teal square, labelled with
  company, ships, cargo and range within 2 million km.

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

#### 5.4.1 Rules that keep it healthy (#72)

Decisions from the economy audit; `retrox simulate` ends with a health
report to check them on a long run.

- **People before industry.** Each day a settlement's population takes
  what it needs first; facilities then share what is left of each input
  in proportion to what they want of it. Nothing gets first pick because
  of where it sits in a list. A facility stopped because its output store
  is full wants nothing that day, so it isn't counted as short.
- **Ships follow freight.** A company buys a ship only when a fleet came
  in with its holds ≥ 90 % full and none of its fleets is idle. A
  settlement founds a company only when it has a full hold's worth of
  surplus and no ships sat idle in its system at the start of the month;
  the crew fetches its hull from the nearest listed yard in trading reach
  that has it (§5.1.3). A fleet that has
  carried nothing for three monthly reviews running (cargo aboard counts
  as carrying) sells half its ships, at least one, for scrap: half their
  mass, as metal, to the port it's in. A company whose last fleet goes
  winds up.
- **Shipyards build to orders.** A captain who wants a ship where there
  isn't enough hull leaves an order (`hull_wanted`, halving monthly) at
  the nearest yard in reach, or at home if there's none. Yards
  keep about a month of their own output on hand plus the orders, and
  otherwise stand idle and decay like any idle facility. A shipyard is
  built only where orders exceed the hull on hand; planned settlements
  treat hull like any other good they're short of.
- **Projects give up.** A construction project still short of materials
  after 180 days is cut short: the settlement keeps the share it has built
  (if that's at least a tenth of a unit) and can plan again.
- **Printing follows need.** A faction with money pays welfare out of it as
  before. A faction without money prints only for settlements below their
  reserve, in proportion to how far below they are, and within its debt
  limit.
- **Debt reins in spending.** What a faction spends by choice (delivery
  premiums, construction wages, printed relief) scales with its *thrift*:
  full with money in hand, falling to nothing as its debt reaches a quarter
  of its limit. Planned economies run a structural deficit (they pay
  premiums and wages, and the money drains to market economies through
  trade), so this brake is what bounds their debt: on a 60-year run it
  settles just under a quarter of the limit.
- **Trips cost what flying them costs.** One estimate serves the coarse
  tier and the bubble: the burn out to a jump point, each lane in whole
  days, each crossing of a system on the way (jump point to jump point),
  the burn in, half a day lost at each change of stretch, and a day of
  handling. In-system distances are typical, not radial, since bodies sit
  anywhere on their orbits. A hop between two settlements on one world is
  a climb to orbit and back down (a day), burns fuel, and is never flown
  in the bubble.
- **Cargo always gets off.** A fleet with cargo still aboard at its review
  routes to wherever that cargo sells best; if nobody in reach can buy it,
  it leaves the cargo at the dock for nothing.

#### 5.4.2 Harvesting (#68)

Raw materials also come from space, gathered by ships, not only from
facilities on the ground. Decided while building it:

- **Gas is a good of its own.** Gas giants hold gas (richness 0.2–1.0,
  more in bigger giants); nothing else does. A **gas plant** refines it,
  1 t of gas to 1.4 t of fuel, which makes it cheaper to run than cracking
  ice in the old fuel plant (1:1), which stays as the fallback where
  there's no giant. Gas plants are built only in systems with a giant.
  Gas's base price is 10.
- **Sources.** Ice comes from icy asteroid fields, icy moons and ice
  worlds; gas from gas giants. Ore stays with mines.
- **Harvesters are companies** like traders, on the same calendar, whose
  fleets carry a harvest job. They work only sources in their port's
  system and never use the lanes. At its monthly review a fleet picks the
  source and the market in the system that pay best per day, after fuel
  and crew upkeep, for a market with room for at least half a hold (no
  market buys past the storage cap that would stop its own facilities).
  A **run** is one trip: out to the source, work it until the hold is
  full, in to the market, where it sells like any arrival. In the coarse
  tier the hold fills on the day it arrives.
- **Sources don't run dry; they strain.** Each source remembers the tonnes
  taken from it lately, fading with a 120-day time constant (the same
  memory fields have of the player's mining). Work takes
  `6 days / richness`, stretched by `1 + strain / capacity` (capacity
  6,000 t for a field, 10,000 t for a moon, 40,000 t for a giant), so
  crowded sources slow down and crews spread out. The player's skimming
  and ice mining strain a source like anyone's.
- **Founding.** A settlement short of a good it has a source of may found
  a harvesting company with one ship: the hull that pays best at base
  prices that it can afford, fetched from the nearest yard in reach, which
  then flies home. Generation seeds harvesters for every settlement's gas,
  and for ice where nobody in the system mines it on the ground.
- **In the bubble** a run is flown: out to the source, holding station
  there while it works (in a field, among the rocks; at a giant, skimming
  its sunlit cloud tops; at a moon, low over it), and in to sell. Ice crews
  are drawn cutting a boulder with their laser, gas skimmers trailing a
  plasma plume and contrail. Their HUD label says what they're doing and
  for how many more days.

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

#### 7.1.1 Gravity wells and planet stops (#95)

A planet city stop used to cost about five real minutes of plain flying:
in from the well's edge to landing range, and back out again, at the
150 km/s cruise cap. Wells now cost **galaxy time and a stop**, not real
minutes (pillar 4):

- **Fast travel to a planet with a city stops low over it**, at 1.6
  radii, inside the 2-radii landing range: arrive, [L] to land. Gas giants
  stop just above their cloud deck, for skimming (§7.3.1). Other planets
  without a city still stop just past their well's edge.
- **Inside a gravity well the autopilot only flies within it**: down to
  anything whose arrival point is in the same well (the planet's city, a
  station in orbit), or else **up and out**: [J] climbs the ship to just
  past the well's edge and stops it there, on the side facing the
  selected target. A second [J] goes on. A climb is fast travel in every
  other way: the shared autopilot flying the real flight model under
  sped-up time, interceptable, droppable with [J], refused with hostiles
  near.
- **Jumps still need open space**, and a trip that leaves a well always
  breaks at its edge. That stop, the galaxy time the climb takes, and the
  fact that every launch starts inside one are what make wells matter.
- Flying out by hand still works, for anyone who wants to.
- **Every city planet has a highport above it** (#112), so the hop from a
  station down to the city really happens: undock, [J] down, land; launch,
  [J] up. The galaxy generator puts one station over each planet with a
  city of at least 1 population (on seed 1, 708 of 754 city planets; the
  frontier outposts go without). It is a settlement of its own, named
  "*Planet* Highport", owned by the planet's biggest city and holding 8%
  of that city's people, so a system's population is unchanged. It has
  its own market but does no mining (the planet below does that); it can
  run a gas plant if the system has gas. It orbits at 2.6–3.2 radii:
  above the 2-radii landing range, with its arrival point (in docking
  range, §7.1.2) well inside the 4-radii well. A colony that finds its site taken settles a
  station at 3 radii instead of on the well's edge, for the same reason.
  Cities founded later in history get no port by rule; that is left to
  colonization. The cradle (§4.2) starts without one: at the dawn of FTL
  the nations have only their cities.

A planet stop now takes about as long as a station stop: a few seconds
of autopilot each way plus the landing sequence.

#### 7.1.2 Arriving nose-on (#118)

Fast travel used to hand the ship back tail-first: it faces its thrust,
and the last thing the drive does is brake, so the place you travelled
to was always directly behind you.

- **The ship turns over at the end of every trip** to face where it was
  going. The autopilot brakes tail-first to a stop as before, then its
  attitude hold turns the ship round with the ship's own controls: the
  stick a pilot would hold, through the same turning as hand flight, at
  the hull's own rates. A nimble ship swings round in a couple of
  seconds, a heavy one takes longer; no rotation is set by a timer.
  Touching the stick hands the turn to the pilot. A climb out of a well
  ends facing the way it's headed next; an intercept ends facing the
  convoy.
- **Fast travel to a station stops in docking range**: 1.2 station sizes
  + 200 m from it, inside the [F] dock prompt's reach (1.5 sizes + 400 m)
  and at rest. Arrive, [F] to dock, like [L] at a city planet. It used to
  stop 20 km out, which meant minutes of hand flying twice per trip. The
  station is still the far end of a real flight, so piracy is unchanged.

### 7.2 Combat

- Small/medium ships: direct dogfighting.
- Capital ships: **combat changes** — subsystem / compartment targeting,
  turret direction, damage per compartment, possibly crew-less
  "station" switching. Exact design open (§11).

#### 7.2.1 Basic combat as built (#70)

- **Guns.** One trigger (LMB or R) fires every gun in a weapon slot,
  and the mining laser too except while hostiles are near (then the
  trigger is the guns' alone; the laser keeps its own gauge, §7.3).
  Autocannons throw shells at 1.6 km/s on top of the ship's velocity;
  lasers hit along their line at once. Fixed guns swing up to 4° to
  meet the target's lead, so putting the boresight on the HUD's **lead
  pip** hits. Y picks the next hostile; the nearest is picked for you.
- **Budgets (§7.4) in flight.** Engines draw power and make heat in
  proportion to how hard they burn, shields while recharging, guns while
  firing; the reactor feeds the rest first and the guns last, so an
  over-budget fit fires slower while it burns and recharges. Heat the
  radiators can't shed fills the sections' capacity; at the top the guns
  lock out until half of it is gone.
- **Damage.** Shields take hits first and recharge 3 s after the last
  one. Past them a hit lands on the section it struck, by where along the
  hull. A section run out of structure is wrecked and its modules die
  with it (lose the stern, lose the engines). A small ship is one section;
  a framed ship's further hits on wreckage go to the frame, and it is lost
  when the frame gives out or every section is gone. Hull damage stays
  with the ship until a dock or city repairs it, automatically, at
  3 cr per point, paid to the settlement's treasury.
- **Raiders** are the ships of the sim's pirate gangs (§7.2.2), flying the
  player's own arcade flight model (shared code) with stock fits under
  the same budgets: they attack along the lead, break off after
  overshooting, jink when someone is on their tail, fire in bursts, run
  when badly hurt, and give up 16 km out from what they guard. (Until
  #80 they were bands the client rolled per field and date; those are
  gone.)
- **Stakes.** With a hostile within 20 km there's no fast travel, no
  jump and no saving: fight or run. Wrecks in a field spill components
  and metal for the tractor.
- **The starting courier** carries a light autocannon in a belly mount
  besides its chin mining laser, so a new pilot can defend themselves.
- **Looks (§8).** Tracers are streaks of light, cream for the player's
  guns and red for everyone else's; lasers are brief beams. Shields are
  a faint bubble of scan bands that flares round each hit. Hull hits
  spark. Damage scorches the paint in soot patches. Ships blow up in a
  churning fireball with secondaries, a white core flash, a shockwave
  ring, flung hull plates and embers, and leave a burnt-out wreck with
  embers in its cracks, tumbling, for three minutes.
- **The target is never hidden by your own ship** (#123). The chase
  camera sits above and behind, so a raider a few degrees low, where it
  usually is as it passes under you, is behind your hull. Where the hull
  covers the target it is **cut away** round it, like a cutaway on a
  technical drawing: a clean round hole a little wider than the target
  (never smaller than its bracket), its edge inked in thin amber, the
  target's colour. Only the hull in front of the target is cut, so most
  of the time nothing changes. Chosen over moving the camera (the view
  would shift just as a fight starts, and a target can pass under any
  framing) and over fading the whole hull (the ship would vanish, and a
  dither picks up the film's colour fringing). The target's labels carry
  the §8.4 drop shadow and its bars a dark edge, since they often land
  on the hull.
- **Sounds (§8).** Synthesized like the rest: a thump and crack per
  autocannon round (raiders' quieter and lower), a falling zap for
  lasers, an electric fizz on the shield, a metal clang and hull creak
  when shots get through, and the volatile-rock blast for ships going up.
- **Losing the ship** shows a "ship lost" card and restores the last
  checkpoint (§7.7.1).

#### 7.2.2 Pirates, bounties and standing (#80)

The Bounty hunter path and the start of the Pirate one (§2). Pirates are
part of the living galaxy (pillars 1 and 5): sim actors that come from
somewhere, live somewhere, and fly for real near the player.

- **Where gangs come from.** Each settlement looks at itself monthly on
  its own day: a small base chance plus a share of its *hardship* (the
  worst of its food and fuel demand going unmet) that some crews go over
  (§5.1.1 cost: arithmetic, with a search only on the rare founding). At
  most one gang per fifteen settlements, plus three: denser than that,
  robbed freight leaves a quarter of the trade fleet idle. The starting
  galaxy seeds gangs from 6% of settlements until prehistory grows them.
- **Where they live.** A lair is an asteroid field within two jumps of
  the crews' home, never in a system with a faction capital, one gang
  per system, chosen at random weighted by *prey* (settlements in and
  next to it, whose traffic passes) over *law* (settlements in the lair
  system itself), with chokepoints (systems of one or two lanes) worth
  1.6×. So gangs sit by the busy lanes in the quiet systems.
- **What they are.** A name ("Ash Rats of Bralum", "Free Crews of Baalem":
  the lair system and a crew name, no fake anything), two light fighters
  (interceptors, heavy fighters) to start, and a record: tonnes taken,
  last raid, and how much it has taken from each faction's traders.
  Every 80 t taken buys another ship, up to five; the fourth is a gunship.
  150 days without a raid costs a ship, and each further 75 another (at
  the monthly reviews, so about one every three months); a gang with none
  is gone.
  Founding, breaking up and bounties go in the history record.
- **Raids, coarse tier.** A trader trip that starts or ends in a lair
  system (or a system next to one) may be robbed on arrival: 5% per gang
  ship in the lair system, 2% next door, so at most 25%. The gang takes up to
  25 t per ship of the cargo, and one time in twenty a ship (much more
  often and replacement ships outrun the freight: the economy test's
  idle share goes past a fifth).
- **Highports (#112) don't add pirates.** A city and the port above it are
  one settled place: the gang cap, the starting gangs, the monthly
  founding rolls and a lair's prey and law all count places, not
  settlements. A run between a port and the city below never leaves the
  planet's well and stays under the port, so coarse raids skip it. With
  both, the share of loaded freight robbed stays as tuned (1.47% against
  1.40% before highports, over four seeds of a year on the small test
  galaxy; within seed noise).
- **Raids, in the bubble.** A gang whose lair is in the bubble flies,
  under the shared autopilot and flight model like every other ship:
  it holds at its field, spots the nearest laden trader within 0.4 AU,
  flies out to intercept it, robs it when it catches it (the same take
  as a coarse raid), gives up after three days, and flies home. A
  robbery in the player's system comes over comms as a mayday.
- **Hunting the player.** A gang not resting from a fight with the
  player, within 0.4 AU of the player's ship in space, goes after it.
  The client flies that stalk (only it knows where the player is between
  ticks): full burn on an intercept course that matches the ship's
  velocity across the line of sight while closing along it, on the
  fast-travel drive the gang's slowest ship has (the same formula as the
  player's). A stalking gang that comes within 4,000 km of a ship in fast
  travel **interdicts** it: the ship drops out with its speed dragged down
  to 250 m/s, an INTERDICTED band slams across the view, and the gang's
  ships come in from where it was. At 1:1 a gang closes in for real and
  its ships come out at 30 km. A gang at home meets a ship that comes to
  its field.
- **After a fight** the client tells the sim which ships died; the rest fly
  home and keep away from the player for three days.
- **Bounties.** When a gang forms, the faction whose settlement the crews
  came from posts a warrant on them, if it can afford one. A gang without
  one draws one at its monthly review from the faction whose traders it
  has robbed most, if that faction can afford the whole gang twice over.
  A warrant pays 2,000 cr per fighter, 15 cr more per tonne the gang has
  taken, up to 8,000, and double for a gunship, from the posting
  faction's treasury the moment a ship goes down (as far as it can pay).
  A fresh two-fighter gang pays 4,000: about 1.4 hours of contracts in a
  courier (§7.8), or two-thirds of an interceptor, for a trip of a few
  jumps and a fight that can cost the ship and pays for its repairs.
- **The bounty office**, on every station's and city's menu, lists the
  warrants on gangs within six jumps (name, lair, ships, who posted it,
  what it has taken, pay per ship and for the gang, PLOT COURSE), and the
  player's standing with the local faction and every faction they have
  one with. NAVIGATION marks a field where a gang lairs; the HUD marks
  gangs flying in the system with red diamonds (a doubled one for a gang
  after the player), labelled with name, ships and warrant.
- **Standing** with each faction runs from −100 to +100, read as a rank:
  hostile (−25 and below), wary (below −5), neutral, trusted (15 and
  up), honoured (50 and up). It moves by: +3 for a ship with that
  faction's warrant on it (paid or not: a broke treasury is still
  glad), +1 for any pirate destroyed in its space (where it has the most
  people; not on top of the +3 when it posted the warrant), +1 per delivery, −5 per failed
  contract; **−20 for opening fire on one of its peaceful ships**, −6 for
  each further hit (at most one per 1.5 s per fleet) and −30 for each of
  its ships destroyed. So one attack and a second burst make a faction
  hostile. Grudges fade by 0.25 a day, favours by 0.05. Standing is
  saved with the player.
- **Hostility you can feel.** A hostile faction's stations and cities
  refuse clearance to dock or land, and its trader ships, when the
  player comes alongside, are hostile: armed ones fight, the rest run.
- **Faction ships near the player.** Within 12 km, each ship of a trader
  fleet is a ship in the fight's scene, holding its place in the
  formation, peaceful unless its faction counts the player hostile.
  Shot, the whole fleet turns. A trader destroyed is gone from the sim's
  fleet with its share of the cargo, and the tractor takes up to 80% of
  that share into the hold.
- **Intercept** ([I]) fast travels to the trader fleet nearest the
  crosshair and brings the ship to rest 2.5 km off its quarter, in a
  frame that moves with the fleet (so flight assist, and any fight, are
  relative to the convoy). That is the start of the Pirate path:
  interdicting on the lanes. Fencing stolen cargo, black markets and
  bounties on the player are later work.
- **Saves:** format 2 (the galaxy carries gangs, and the player
  standing); a convoy's frame is saved as the star's.

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

### 7.3.1 Gas skimming (#68)

The player's way to harvest a gas giant, the same sources NPC skimmers
work.

- **Getting there:** fast travel to a gas giant stops 160 km above its
  cloud deck, not at the edge of its well.
- **The band:** the gas thickens by a factor of *e* every 20 km down, from
  nothing at 120 km to full at the deck. The deck is solid: you bounce
  off it.
- **The scoop** is the hold's intake; any ship with a hold can skim. Hold
  the trigger in the band and it opens. It takes in gas as density × speed
  × richness, scaled by the square root of the hold's size, and friction
  heats it as density × speed². So deep and slow beats high and fast, and
  deep and fast cooks it in seconds. At full heat it shuts and vents until
  it has cooled to a fifth; radiators shed the heat, as they do the mining
  laser's.
- **The gas pushes back:** drag through it, and turbulent shoves near the
  deck that grow with density and speed.
- **The HUD** swaps the mining panel for a skimming panel over a giant: an
  altimeter strip shaded by density with the band's top marked, readouts
  of height, speed through the gas and intake, the scoop's lamps and heat,
  the hold, and what the gas fetches nearby. The open scoop trails the
  same plume NPC skimmers do.

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
  price and pays the fee from its own treasury: freight for the expected
  trip (a rate per tonne-day plus the crew's wage, both the galaxy's own
  hauler upkeep): a wage, not a profit. The price gap and a
  planned faction's delivery premium go to traders, not to a captain
  carrying someone else's goods (§7.8). It only commits what it can
  afford.
- **Deposit.** Taking a contract costs a deposit of the cargo's value,
  returned on delivery. Contract cargo rides in the hold but can't be
  sold.
- **Deadlines** allow 1.5× the expected trip (jumps plus ~3 days per
  system). Each day late takes 10% off the fee. Ten days late, or
  abandoned, the contract fails: the consignee keeps the deposit and its
  fee, and the goods are the player's to sell, and the consignee's
  faction counts it against the player (§7.2.2); a delivery counts a
  little for them.
- **Delivery** happens on docking or landing at the consignee. Waiting
  while docked stops on the day a contract falls due (§3).
- **The client starts 30 days after generation**, so markets have
  drifted off their generated targets and the first board has work on
  it. Prehistory (§4.2) will replace this warm-up.

### 7.6 Planets

- No seamless landing. Planets have **predefined landing points**; landing
  plays an animation and opens the location's UI.
- Fast travel brings the ship into landing range of a planet's city, and
  [J] climbs back out after launch (§7.1.1).
- **A planet may have several cities** (issue #116). Each is a row of its
  own in NAVIGATION, under its planet; picking one (click, or [T], which
  steps through city rows too) targets the planet and makes that city the
  one [L] lands at. With none picked, [L] lands where the hold's cargo is
  bound, else at the first city listed. While more than one city is in
  reach, the prompts say so and point at NAVIGATION. Hints name a city with
  its planet ("Kiolgristio, on Mufath Prime III") so the player knows
  where to fly.

### 7.7 Death

- On ship destruction, **restore the last checkpoint** (last dock /
  autosave). Because the game is deterministic, the same future plays out
  again unless you act differently — what you learned before dying is
  yours to use.

#### 7.7.1 Losing the ship, as built (#78)

- **The same future, really.** A checkpoint is written on docking or
  landing, and a ship at rest in a dock or city is exactly what a loaded
  checkpoint has: cool, shields up, guns ready, nothing in flight. So
  nothing left over from before the dock (gun cooldowns, heat, an earlier
  fight's dice) changes what comes after it. Each fight's own dice (shot
  scatter, wreckage) are also reseeded from the band when it comes out,
  so a fight is decided by the field, the date and what the player does
  from then on. A test plays a lost fight from a checkpoint twice, frame
  by frame, and they match to the bit; flying differently changes it.
- **The moment.** The instruments die with the ship: the HUD goes, and
  the camera draws back and round from the fireball in its last roll,
  the view darkening at the edges. After 1.6 s a red constructivist band
  slams across the view, **SHIP LOST** in display type with the ship's
  name plate riveted beside it, over a panel of readouts: when and where
  it was destroyed (alarm red), the last checkpoint's date and place
  (teal), and how much time going back costs (amber, "1 d 04 h
  earlier"). A falling two-note synth tone, like a station signing off,
  plays under it.
- **The choice is the player's.** No automatic restore: Enter or the red
  key restores; Esc opens the pause menu (whose Load page has every save
  and checkpoint). Without a checkpoint the key reads *title screen*.
- **The rewind.** Restoring rolls the tape back rather than cutting: film
  runs backwards through the gate (frame lines, sprocket holes,
  scratches, grain, an unsteady projector lamp) behind a reel-to-reel
  transport plate whose drum counter rolls the galaxy date and hour back
  to the checkpoint's, with a tape-rewind whine ending in the brake's
  clunk. It lasts at least 2.8 s and the checkpoint loads behind it.
  Then the black lifts off the dock with **CHECKPOINT RESTORED** and the
  place and date.
- Saves' headers carry the exact galaxy clock (for the counter); older
  headers without it still load.

### 7.8 Earnings and progression

Every career in §2 the player can follow today earns a living, and the
ship ladder is long enough to save up along (issue #84). Money comes out
of the economy, never from a payout table (pillar 5), so the levers are
the sim's own numbers:

- **Ships cost about twice their materials.** Finished hull sells at
  120 cr/t: a tonne takes 1 t of metal, 0.3 t of components and 0.1 t of
  fuel (56 cr at base prices), and a shipyard marks that up as a factory
  marks up components (§5.4). Every ship and part is priced from it
  (§7.4). At half that, ships cost barely more than their raw metal and
  a few hours of any work bought a freighter. NPC companies buy hulls at
  the same price; five simulated years end with 4% fewer NPC ships and
  the same shortages.
- **A new pilot owns the courier and 2 000 cr.** That covers a first
  contract's deposit or a hold of cargo, not another ship. (It was
  25 000 cr, which bought any medium ship on day one.)
- **A contract pays carriage.** Its fee is the hauler's keep for the
  trip at the galaxy's own upkeep rates: 0.02 cr per tonne-day plus a
  crew's 2 cr a day (§7.5.1). How short the consignee is decides whether
  it posts and how the board is ordered, not the pay. The price gap
  between the two markets, and a planned faction's delivery premium,
  belong to whoever buys the goods and carries them at their own risk:
  a trader, or an NPC hauler. So each career earns from its own thing:
  contracts from hauling, trading from price gaps, mining from what it
  digs. Contracts used to pay the gap, the premium and a shortage
  markup too, at 3 cr/t-day and 30 cr/day (50–150× NPC upkeep), so a
  courier on contracts made 35–78k cr/h while mining made 1–3k, and
  nobody would mine.
- **Mining is unchanged:** ore sells at its market price. A faster laser
  or a longer tractor barely moves it (+0–15% in the bench). In a courier
  the 12 t hold forces ~25 station trips an hour, and gluts at the local
  market set the price. A miner grows by refitting: a gunship with mining
  lasers in the bow and a cargo bay amidships (43 500 cr) cuts five times
  as fast. A miner mines in fields the nav list shows no gang lairs in
  (§7.2.2).
- **Bounties** pay from the posting faction's treasury per pirate ship
  destroyed (§7.2.2), a little above contracts for the time, against the
  risk of the fight.

**Measured** with the career bench (`cargo test -p retrox-client careers
-- --ignored --nocapture`, `crates/client/src/world/careers.rs`). Bots
play an hour of each career in a fresh game, seeds 1–4. They fly the real
world, with turning, nudging around a field and menus charged at a
practised pilot's pace. They play well, so a new player should expect
somewhat less, contracts most of all (the bot reads a board instantly).

| Career, ship | cr per hour (seeds 1–4) |
|---|---|
| Mining, courier (12 t, light laser) | 1 950 · 2 990 · 970 · 1 670 |
| Freight contracts, courier | 2 840 · 3 920 · 2 340 · 2 530 |
| Trading, courier | 16 510 · 4 360 · 8 920 · −980 |
| Mining, gunship refit (180 t, 2 lasers) | 1 520 · 8 080 · 1 120 · 1 720 |
| Freight contracts, hauler (360 t) | 8 900 · 7 180 · 7 620 · 8 210 |
| Trading, hauler | 27 280 · 9 870 · 17 110 · 7 470 |

In a courier, contracts earn about 1.5× mining on average (1.3× to 2.4×
by seed). Mining needs no capital and never meets a bare board. Trading
pays best and swings most. Planned economies hold one price everywhere,
so the money is in market space and across the border (§5.4). A trader
who guesses wrong loses money. The bot trader also knows prices two jumps
out, which a player has to learn.

**The ladder** (stock fits): racer 4 060, courier 5 930, interceptor
6 180, light trader 6 590, heavy fighter 10 150; blockade runner 38 800,
hauler 41 200, gunship 50 500; express freighter 243 000, bulk freighter
302 000, frigate 413 000. From the courier and 2 000 cr, the light
trader (twice the hold) is about 2 hours of contracts or 2–4 of mining
away; a medium ship is ~15–20 hours of either, less for a good trader. A
medium freighter on contracts takes ~30 hours to buy a large ship (a
trader, half that), which leaves fleets and capital ships (§7.5) the
rest of the ~100-hour arc (§2).

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

### 8.3 Help for new pilots

Decided while making the game learnable (issue #79). Pillar 5 applies to
teaching too: no scripted tutorial campaign, nothing that waits on the
player.

- **First-steps notes**: a HUD panel of indicator lamps, one per first
  (take on work, launch, pick a target, fast travel, jump a hyperlane,
  dock or land, get paid), lit teal as the player does each one in any
  order, the next one amber. Under the lamps, **one hint** read off the
  situation every frame (where you are, what you carry, your contract's
  destination, what you're aimed at), with keys drawn as keycaps. The
  panel goes for good once every lamp is lit; a setting turns it off.
  The firsts are saved with the player.
- **Controls card** on H (or F1): every key in three columns (flight,
  travel, work & view) over the view, without pausing. The corner of the
  HUD shows only the keys for help and the menu, as keycaps, not a
  paragraph of plain text.
- **Pilot's handbook**: a page on the title screen and in the pause menu,
  in chapters (first steps, controls, making a living). It is the one
  place that explains the game in prose. The handbook and the controls
  card draw from the same key tables, so they can't disagree.
- The suggested first run is a freight contract: it pays for sure and
  walks the player through launching, the galaxy map, a jump, fast travel
  and docking. Mining and trade are offered as the other ways in.
- **Edge arrows** (#117): a target off screen gets an arrow at the edge
  of the view (amber for the target, red for hostiles) pointing the short
  way round to turn toward it, whether it is beside, behind or below the
  nose. Something dead astern has no short way, and its arrow points
  down.
### 8.4 Screens and controls

Decided while bringing every screen into one style (issue #77):

- **No stock widgets.** Every list, button and table is painted from the
  shared kit in `crates/client/src/widgets.rs` (and the HUD helpers in
  `hud.rs`): lists are **instrument rows** with a hairline under each, a
  red wash and tab on hover, an amber wash and tab on the selected row;
  buttons are **push keys** with a darker lip that sink when pressed (red
  for the one primary action, dark enamel otherwise, outlined in a colour
  where the colour means something).
- **Colour carries meaning, consistently:** amber is what you pay, buy,
  target or have selected; teal is live readings and what you're paid or
  sell for; red is the primary action and alarms; cream is names.
  So trade has an amber bank of buy keys and a teal bank of sell keys.
- **Joined key banks** (`+1 +10 MAX`) rather than loose buttons for steps
  of one action, like a row of switches on a panel.
- **Chart symbols** in lists of places: a rayed star, ringed gas giants,
  stations as teal diamonds, fields as scattered dots; moons hang off their
  planet on a bracket, and cities (a little skyline of three blocks) hang
  off the body they're on the same way, one step further in. A city row
  carries no range (its planet's row has it). Where the hold's cargo is
  bound, the row is tagged CARGO in amber, as a pirate lair is tagged
  RAIDERS in red. Under TRAFFIC, trader fleets are teal squares.
- **Hostiles and standing** (audit 3, #137): anything hostile is alarm
  red, and anything leaning that way is the same red dimmer: a pirate gang
  about its own business is a dim red diamond (bright, and doubled, when
  it's after the player), and a faction that counts you wary reads in dim
  red. Neutral is dim cream, trusted and honoured teal. Amber stays for
  paying, buying and selecting, so a rank never borrows it.
- **Slot plates:** loadout slots carry a stencilled kind plate coloured by
  role (weapons red; engines, power and jump amber; shields, radar and RCS
  teal; radiators and cargo cream).
- **Key legends** (the corner strip from §8.3, the star chart's legend)
  are cream keycaps with the action in uppercase mono beside them.
- **One lamp and one keycap** (audit 2, #101): every indicator lamp (HUD,
  star chart, rewind deck) and every keycap (corner strip, hints, legends)
  is painted by the same kit function, keycaps in mono 13, so they match
  from screen to screen.
- **Counts and estimates read as words:** "1 jump", "this system",
  "1 settlement"; an estimated trip is "about 39 d". Never "~": in the
  mono face it reads as a minus sign. Nor "≈": the mono face has no such
  glyph and draws "=" (so "ABOUT 0.6 T PER FRACTURE"). Every count goes
  through `hud::count` (or `hud::count_as` for "cities"), every jump
  count through `hud::jumps_text` and every distance through
  `hud::distance`, so a near target reads in km, not "0.00 AU".
- **NAVIGATION is a tree of where you go** (#119): the star, then each
  body with what hangs off it, one step in per level: its cities, the
  stations in orbit round it, then its moons with theirs. Stations round
  the star follow under STATIONS; fields come last under FIELDS, since a
  system can have a dozen and nobody docks at one. In the list and on the
  markers in space a field or station drops the system's name (the
  panel's own context) and a field drops "field": *Outer Belt A*. A
  station under its body drops the body's name too (*Highport*), the
  bracket says whose it is. Names that still don't fit are cut to the
  pixel room left by the range and any tag, never to a fixed count.
- **Text over the scene gets a backing** (#124): blocks of free-floating
  text (the corner key strip, the readout on the rock under the reticle)
  sit on a dark plate; single labels on markers and rocks get a dark drop
  shadow, which keeps a crowded field light. The rock readout stands
  beside the rock's brackets (which close round the rock as it looks on
  screen), flipping to its left near the screen's edge, never over it.
  HUD panels are near opaque, menu pages fully opaque: the surface blends
  in linear light, so a bright station shows through far more than a
  panel's alpha suggests.
- **The message banner fits its text** (#120): one line up to 760 px,
  wrapped only at spaces past that, centred under the top edge. It is
  painted straight onto a layer, not laid out in an egui `Area`, whose
  remembered width otherwise narrows the next message.
- **Keys painted into a row stay inside it** (#125): a row's push key is
  placed from the row's own rect, and the rest of the row shares out what
  is left. Panels never use a centred horizontal layout, which stretches
  to the panel's previous height and leaves a band of empty panel.

### 8.5 Sound

Decided while building the soundscape (#76):

- **All synthesized, no samples.** Every sound is made in code from
  oscillators, noise, filters and resonators (`client/src/audio/`), so
  the whole game shares one analog character, nothing needs licensing,
  and cues can be retuned like any other code. One-shot cues render once
  at startup; continuous sounds run live on the audio thread through
  kira.
- **The score is generative**: a small analog ensemble — detuned-saw
  string pad, filtered bass, sequencer arpeggio through a ping-pong tape
  delay, a theremin-like lead with blooming vibrato, glassy FM bells, a
  soft rhythm box — with tape wow on everything. It plays progressions in
  D Dorian / Aeolian (hopeful, with melancholy), a section at a time.
  Moods crossfade by situation: *title* (the full theme), *space*
  (sparse, and it rests in silence now and then), *station* (warmer,
  with the rhythm box), *transit* (fast travel and jumps: the sequencer
  driving).
- **The palette:** heavy mechanisms for anything physical — docking
  clamps then airlock hiss, the jump drive's breaker, the star chart's
  motors and relays, bakelite toggles for every switch and button, hull
  plates groaning on impacts and in transit; radio static with crackle
  and a squelch tail whenever a message comes over comms; VFD-panel beeps
  for instruments (menu cursor, targeting, the prospector's sonar ping).
  Engines are a low roar plus a turbine note that rise with acceleration
  rather than speed, so cruising is quiet and burns are loud.
- **The sound follows the game**, not the other way round: the client
  watches the world each frame and turns what changed into cues and loop
  levels. Game code only reports what the sound can't see (the speed of a
  collision).
- **Volume:** master, music and effects sliders in Settings, heard while
  dragging. Pausing silences the ship but not the music. Screenshot and
  scripted runs are silent unless `--sound`; `--mute` silences any run.
  No audio device means silence, never an error.

### 8.6 The sky

Decided while fixing nearby stars that drew as blurred discs (issue #121):

- **Every other system is a star in the sky**, carrying the light it really
  sends us (luminosity over distance squared). Faint ones are a sharp
  core plus a halo kept under fixed peaks, so they fade smoothly.
- **The halo never grows past a few pixels** (sigma 2.5 px). A wide soft
  halo reads as a smudge on the lens, not a star.
- **Past that, the light is compressed like film's shoulder**
  (logarithmically), and what's left goes half to the core, which burns
  over the bloom threshold and glows in the bloom pass (with its red
  halation), and half to **four thin diffraction spikes** that grow
  longer with each e-fold of light, never wider or brighter. So a close
  neighbour is a hard white point with glare and rays, and an O star ten
  light-years off is a bigger version of the same, not a fog bank.
- **The spikes lie on the diagonals**, fixed to the frame like a real
  lens's, so they never line up with the HUD's square-on reticle ticks.

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
- **Arrow-key pitch** (decided in #128): the arrow keys pitch like a
  stick, as the mouse does by default: ↓ raises the nose, ↑ lowers it.
  The controls card and handbook say so (“↓ nose up · ↑ nose down”), and
  *Invert pitch* flips the keys along with the mouse, so one setting
  covers both ways of steering and the card always shows the keys as set.
- **Playtests** (decided in #103): `--remote PATH` lets an agent play a
  real game turn by turn. Steps appended to the file (keys, held keys,
  clicks, the mouse wheel, `shot:` for a screenshot, `info:` for a text
  readout of bearings, rocks and raiders) run as in `--keys`, and the
  world stands still between them, so a slow player loses nothing. Run it
  with `RETROX_DATA` pointed somewhere scratch, so its saves stay out of
  the player's own. A playtest files each problem as its own issue, in
  the player's words, labelled **blocks play** (a new player can't get
  through the loop without outside knowledge, or loses work or money to
  it) or **polish** (playable, but rough to look at or slow to feel).

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
