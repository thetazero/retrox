# retrox — Design Document

Status: living document.

This document holds the vision and the decisions later work should follow:
how the game works, and the look and sound of each kind of thing. It is not
a log. Tuning numbers, measurements, function names and bug histories live
in the code, its tests and benches, and the PRs and issues. Where the game
doesn't yet do what a rule here says, the rule names the issue that will
make it so.

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
   Money, work and danger come out of the sim, never from payout tables or
   random generators.
6. **History is simulated, not authored.** Generation builds the physical
   galaxy and a single cradle of civilization, then runs the real sim
   forward for centuries. Every faction, border, colony, ruin and grudge
   the player finds is the output of that history (§4.2).
7. **Everything is reachable from the screen.** Anything the player can do,
   they can find and do from what the game shows them, without outside
   knowledge. Places that matter are never hidden below the fold or behind
   a list nobody reads.

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

**Save-long (years of galaxy history over ~100h of play):** grow from a single small
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
| In-system fast travel | seconds | crossing a system ≈ 1–3 days |
| Hyperlane jump | ~10–20 s transit sequence | 2–14 days by lane length |
| Docked actions | instant / menus | per action, see below |
| "Wait" (docked only) | fast-forward | weeks–months |

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

- **No pace target.** How much galaxy time an hour of play covers is
  whatever comes out of what the player does: an hour at the rocks passes
  about an hour, an hour of hauling passes the days its trips take. Nothing
  is tuned to hit a number of galaxy days per played hour; a trip takes
  what the drive or engines really take (the user's ruling).
- **Waiting is docked-only.** You can't skip danger by waiting in space.
- **Fast-forward auto-interrupts** on: interdiction, hostiles in range,
  contract deadline approaching, cargo spoilage, and news matching the
  player's filters.
- **Contracts carry galaxy-time deadlines**, which is what makes time
  matter to the player.
- **Skipped days play out, they don't freeze the screen.** A wait shows the
  days ticking by with a stop button; a jump's days tick across its transit
  sequence, so you arrive with the clock caught up. The sim still steps
  one day at a time (§5.1).
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
  traversed by fast travel.
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
- Prehistory runs the **same sim rules as play** (§5), so the galaxy the
  player enters is a state the live sim can sustain — not a hand-balanced
  snapshot.
- What the player inherits is all output: factions and their family tree,
  borders, populations, facilities, markets, tech levels (and so which lane
  tiers they can use), ship designs, remnants (ruins, wrecks, abandoned
  colonies), and the history record itself (§5.7).
- **The player starts at that fixed present day and picks where.** A
  start screen shows the map and its history (shared with the legends
  tool, §9), so choosing a starting system is choosing a place with a
  past. Until it exists, a new game starts at the biggest faction capital
  whose system has an asteroid field, so mining is one fast travel away.
- Until prehistory exists, the game starts a month after generation, so
  markets have drifted and the first boards have work.
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
- **Multithreaded** via rayon within phases where it pays — but the cost
  model below is designed for one core first; threads are headroom, not
  the plan.
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
  - Results must match across platforms, so the sim avoids platform math
    (`powf`, `exp`) where it decides anything.
- **The sim ticks daily, everywhere, always.** Coarser steps would break
  rules with timescales under a month (famines, trips, contracts). Speed
  comes from what each actor does per day, not from skipping days (§5.2).
- Headless runs ("simulate 500 years, dump history") are a first-class
  tool for balancing and testing.

### 5.1.1 Cost model

Seeding a world means simulating centuries of history (§4.2): 400 years is
144,000 days, and a mature galaxy might hold ~10,000 settlements and
~50,000 traders. So the daily budget is well under a millisecond, which
only works if every actor follows its kind's rule:

| actor | runs | per-run work |
|---|---|---|
| settlement | daily | its own books: production, consumption, population, prices, ledger transfers |
| settlement, faction | monthly, each on its own day | decisions: construction, colonization, politics, pirates |
| trader | on events only (depart, arrive) | move goods and money through the ledger |
| trader | monthly, on its own day | commit to a route for the month |
| colony ship | on events only | found a settlement on arrival |

- **Daily work is arithmetic.** No searching, no allocation, no per-day
  decisions.
- **Decisions read data prepared for them.** Searching happens when that
  data is made — monthly, or when something changes — not at each
  decision: cached lane distances, each system's short lists of where
  goods are wanted or spare and where the shipyards are, each system's
  settleable sites in order.
- **Traders don't tick.** A calendar queue hands each day its departures
  and arrivals; between events a trader costs nothing.
- **Monthly work is spread over the month**, each actor on its own day,
  so no day carries everyone's reviews.
- **Measured, not assumed.** `retrox bench` and a prehistory benchmark
  track the costs; they decide where the next work goes.

### 5.2 Two tiers + LOD bubble

- **Coarse tier (everywhere outside the bubble, and all of prehistory):**
  the actors of §5.1.1. No individual ships, no flights.
- **Notable assets are always individual**, even in the coarse tier:
  capital ships, flagships, named stations, convoy leaders.
- **Agent tier (the bubble):** within 3 jumps of the player, ships are
  individual agents with goals (trade, mine, patrol, raid) **flying real
  routes under the same flight model and autopilot as the player**. None
  in prehistory.
- Agents **materialize** from coarse state when entering the bubble —
  placed along their trip by how far through it they are — and
  **collapse** back when they leave or when time compresses. Both
  directions conserve state (cargo, damage, losses).
- **Time compresses** when the player waits docked or is in the lanes:
  the bubble goes away for those days. Fast travel in a system keeps it —
  traders flying past while you cruise are what piracy and interdiction
  are made of.
- **In the bubble, the galaxy reacts to the player.** Fleets pick a fresh
  route at every port and the bubble's systems refresh their trade lists
  daily, so traders near the player answer the markets the player
  changes.
- **The player sees the bubble's traffic.** Fleets are drawn as their
  ships in formation, nose along their thrust, engines lit by how hard
  they burn (§7.1.2), and marked on the HUD.
- **Every goal says how it really moves**, so the autopilot can reach it;
  a bubble day must stay cheap however busy the system is.
- **Not yet in the bubble:** colony ships, patrols.

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
  prices, chronic shortages and gluts, black markets in the gaps. A
  planned faction pays a delivery premium to whoever brings goods to a
  settlement below target, and a planned market sells only its surplus.
- **Market economies:** prices float; booms and crashes.
- Borders between the two are natural trade & smuggling opportunities.
- NPC traders, miners and haulers operate on the same timeline as the
  player, so observed price gaps really do close.

#### 5.4.1 Rules that keep it healthy

`retrox simulate` ends with a health report that checks these on a long
run.

- **People before industry.** A settlement's population takes what it
  needs first; facilities share what's left in proportion to what they
  want. Nothing gets first pick because of where it sits in a list.
- **Ships follow freight.** Companies buy ships only when their fleets come
  in full and none is idle; settlements found companies only with surplus
  to move and no idle ships nearby. A fleet that carries nothing for
  months sells ships for scrap; a company with no fleets winds up.
- **Shipyards build to orders.** Captains who need a hull leave orders at
  a yard in reach; yards keep a little stock plus the orders and otherwise
  stand idle. New yards are built only where orders outrun supply.
- **Projects give up.** A construction project starved of materials for
  months is cut short at what it has built.
- **Printing follows need, and debt reins in spending.** A broke faction
  prints only for settlements below their reserve, within its debt limit,
  and what it spends by choice (premiums, wages, relief) shrinks as its
  debt grows. That brake is what bounds planned economies' structural
  deficit.
- **Trips cost what flying them costs.** One estimate of a trip's days
  serves the coarse tier, the bubble, contracts and the player's
  planning.
- **All travel is flown.** No ship, NPC or player, moves by timer or
  teleport where the bubble can see it: trips in the bubble are real
  flights on the shared autopilot. Today a hop between two settlements on
  one world is a one-day timer that is never flown (#190).
- **Cargo always gets off.** A fleet with cargo aboard at its review routes
  to wherever it sells best, or leaves it at the dock if nobody in reach
  can buy it.

#### 5.4.2 Harvesting

Raw materials also come from space, gathered by ships, not only from
facilities on the ground.

- **Sources.** Ice comes from icy asteroid fields, icy moons and ice
  worlds; gas from gas giants. Ore stays with mines.
- **Gas is a good of its own**, refined into fuel more cheaply than ice,
  so gas giants matter to industry.
- **Harvesters are companies** like traders, working sources in their own
  system and selling there. A run is out to the source, work it until the
  hold is full, in to a market.
- **Sources don't run dry; they strain.** A source remembers what was taken
  from it lately, recovering over months; crowded sources work slower, so
  crews spread out. The player's mining and skimming strain a source like
  anyone's.
- **In the bubble a run is flown**, and seen: ice crews cutting a boulder
  with their laser, gas skimmers trailing a plume over the cloud tops,
  each labelled with what it's doing.

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

#### 5.5.1 Stability and secession

- **Stability** is per settlement and drifts over a couple of years toward
  a target set by lane distance from the capital (a long bridge strains
  loyalty more than a short hop), the size of the realm (great powers'
  middle provinces get restless too, so they split in large pieces, not
  only at the fringe), hunger and famine. Outside shocks (§5.7) act
  through the same target. The capital is always loyal.
- **Regions break away whole.** Neighbouring unstable systems form a
  region, which may secede; a system goes whole (a city and its highport
  don't split), and only what's nearer the new capital than the old one
  goes with it. Small regions can't nibble a great power to dust. A new
  faction starts hopeful.
- **Breakaways look like family**: a colour near the parent's, so families
  read as families on the map, and usually the parent's economic model.
- **History records** foundings and secessions, each with a named leader.
- **Target shape after 400 years:** an effective number of 3–10 factions,
  the largest under half of all people, no more than about 60 standing.
  `retrox prehistory` and `retrox simulate` report it.
- **Nothing merges or dies yet**; war and conquest (§5.7) are the
  counterweight.

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
- **Ship designs evolve.** Factions field designs suited to their tech
  and economy, in lineages; old hull classes stay in service. Every hull
  model is still designed by hand, never rescaled or reused (§8.1).
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
- **Fast travel** for in-system trips, hyperlane jumps between systems.
- **Fast travel is real flight**: the shared autopilot flies the real
  flight model under sped-up time, so it can be intercepted, dropped out
  of with [J], and is refused with hostiles near. Interdiction can pull
  ships out of it (piracy, patrols).
- **Arrival puts you where the next action is**: in docking range of a
  station ([F]), in landing range of a city ([L]), among the rocks of a
  field within laser reach of a good one, above a gas giant's cloud deck
  for skimming. No minutes of hand flying at either end of a trip; flying
  by hand still works for anyone who wants to.

#### 7.1.1 Gravity wells and planet stops

Wells cost **galaxy time and a stop**, not real minutes (pillar 4):

- **Jumps need open space**, outside every gravity well.
- **Inside a well the autopilot only flies within it**: down to anything
  in the same well (the planet's cities, a station in orbit), or **up and
  out** — [J] climbs to just past the well's edge on the side facing the
  target, and a second [J] goes on. That stop, the galaxy time it takes,
  and the fact that every launch starts inside one are what make wells
  matter.
- **Most city planets have a highport above them**, a station settlement
  of its own owned by the planet's biggest city, so the hop from a station
  down to the city really happens: undock, [J] down, land; launch, [J] up.
  Frontier outposts, the cradle, and cities founded later in history go
  without unless colonization builds one.

#### 7.1.2 Fast travel as seen

- **The ship flies its trip for real**: burn out nose-first, turn over,
  brake tail-first, then turn round with its own attitude control at the
  hull's own rates to face where it was going. No rotation is set by a
  timer; touching the stick hands the turn to the pilot. An intercept ends
  facing the convoy.
- **The camera looks where the trip is going.** Through fast travel and
  the turn-over after it, the view (chase or cockpit) faces the place the
  trip goes to, not the nose: braking, the ship faces the camera
  tail-first with the place beyond it. The view eases onto that heading
  and back to the nose, never snapping; a climb out of a well looks
  outward, an intercept at the convoy. Flying by hand it follows the
  nose.
- **The engines show what the drive is doing**, for every autopilot ship,
  the player's and NPCs': plumes lit by thrust along the nose, dark while
  turning over or holding still.

#### 7.1.3 Following a course with [J]

- **Taking work sets the course.** ACCEPT on a contract plots the course
  to its system, as BUY & GO does for a trade run (§7.8.1), unless a
  course is already set.
- **[J] is the one key for going on.** Inside a well it climbs out
  (§7.1.1); with a target in reach it fast travels; in open space on a
  course with nothing to fast travel to, it jumps the course's next lane,
  one [J] per system. A target picked in the system comes first, and the
  prompt strip says which [J] will do.
- **HYPERLANES opens whenever a new course is set**, so the next lane is
  in sight. Hints and the course panel name the destination system and
  the next key at every step.

### 7.2 Combat

- Small/medium ships: direct dogfighting.
- Capital ships: **combat changes** — subsystem / compartment targeting,
  turret direction, damage per compartment, possibly crew-less
  "station" switching. Exact design open (§11).

#### 7.2.1 Dogfighting

- **Guns are safe or hot** (#189). One trigger (LMB or R). On safe it is
  the mining laser's or the gas scoop's and no gun fires, so mining never
  hits anyone near a field. The guns go hot, and the trigger is theirs
  alone, while a ship fighting the player or a pirate running from it is
  near, or when the pilot arms them by hand with [B] to fire on a ship
  that isn't hostile (the Pirate path, §7.2.2); [B] again, docking or
  landing makes them safe. A hostile faction's traders passing by don't
  make them hot. The flight panel shows which, and the boresight shows
  only while hot.
- **Aiming.** Fixed guns swing a few degrees to meet the target's lead, so
  putting the boresight on the HUD's **lead pip** hits. Y picks the next
  hostile; the nearest is picked for you.
- **Budgets (§7.4) in flight.** Engines, shields and guns draw power and
  make heat by what they're doing; the reactor feeds the guns last, so an
  over-budget fit fires slower while it burns and recharges, and heat the
  radiators can't shed locks the guns out until it falls.
- **Damage.** Shields take hits first and recharge shortly after the last
  one. Past them a hit lands on the section it struck: wreck a section and
  its modules die with it (lose the stern, lose the engines). Damage stays
  until a dock or city repairs it, for money.
- **Raiders** are the ships of the sim's pirate gangs (§7.2.2), flying the
  player's own flight model with stock fits under the same budgets: they
  attack along the lead, break off after overshooting, jink when someone
  is on their tail, fire in bursts, and run when badly hurt.
- **Stakes.** With a hostile near there's no fast travel, no jump and no
  saving: fight or run. Wrecks spill components and metal for the
  tractor.
- **The starting courier** carries a light gun besides its mining laser,
  so a new pilot can defend themselves.
- **The target is never hidden by your own ship.** Where the hull covers
  the target it is cut away round it, like a cutaway on a technical
  drawing, edged in the target's amber. The camera doesn't move and the
  hull doesn't fade.
- **Looks and sound (§8).** Tracers are streaks of light, cream for the
  player's guns and red for everyone else's; lasers are brief beams;
  shields a faint bubble of scan bands that flares round each hit. Ships
  blow up in a churning fireball with secondaries and a shockwave, and
  leave a burnt-out wreck tumbling. Every weapon, hit and explosion has
  its own synthesized sound.
- **Losing the ship** restores the last checkpoint (§7.7).

#### 7.2.2 Pirates, bounties and standing

Pirates are part of the living galaxy (pillars 1 and 5): sim actors that
come from somewhere, live somewhere, and fly for real near the player.

- **Where gangs come from.** Hardship (food and fuel going short) turns
  some crews pirate, so gangs rise where the economy fails. There is a cap
  on gangs per settled place: past it, robbed freight idles the trade
  fleet. A city and the highport above it count as one place.
- **Where they live.** A lair is an asteroid field near the crews' home,
  never in a faction capital's system, chosen by prey (busy traffic
  nearby, chokepoints most of all) over law. So gangs sit by the busy
  lanes in the quiet systems.
- **What they are.** A name built from the lair and a crew ("Ash Rats of
  Bralum"), a couple of fighters to start, and a record of what they've
  taken from whom. Raids buy ships, up to a gunship; a gang that goes
  without raiding loses ships and finally breaks up. Founding, breaking up
  and bounties go in the history record.
- **Raids.** In the coarse tier, trips through a lair's neighbourhood risk
  losing cargo, and sometimes a ship. In the bubble a gang flies out to
  intercept laden traders and robs them for real; a robbery in the
  player's system comes over comms as a mayday.
- **Hunting the player.** A gang near the player's ship goes after it,
  and a stalking gang that reaches a ship in fast travel **interdicts**
  it: the ship drops out with an INTERDICTED band across the view, and
  the gang's ships come in. The stalk is real flight on the shared
  autopilot like every other ship's; today it uses a simpler movement of
  its own (#190). After a fight the survivors go home and keep away for a
  few days.
- **Bounties.** The faction whose people a gang came from, or whose
  traders it has robbed most, posts a warrant if it can afford one, and
  pays per ship destroyed from its treasury. A bounty should pay better
  than contracts for the time, against the risk of the fight.
- **The bounty office**, on every station's and city's menu, lists nearby
  warrants with PLOT COURSE, and the player's standing. NAVIGATION tags a
  field with a lair RAIDERS; the HUD marks gangs in the system with red
  diamonds (doubled for a gang after the player).
- **Standing** with each faction reads as a rank: hostile, wary, neutral,
  trusted, honoured. Killing pirates in its space and delivering for it
  raise it; failing its contracts lowers it; **attacking its peaceful
  ships wrecks it** — one attack and a second burst make a faction
  hostile. Grudges fade faster than favours.
- **Hostility you can feel.** A hostile faction's ports refuse the ship,
  and its traders, when the player comes alongside, fight or run.
- **Faction ships near the player** are ships in the fight's scene,
  peaceful unless their faction counts the player hostile. Shot, the
  whole fleet turns; a trader destroyed is gone from the sim with its
  cargo, some of which the tractor can take.
- **Intercept** ([I]) fast travels to the trader fleet nearest the
  crosshair and stops off its quarter, moving with it. That is the start
  of the Pirate path. Fencing stolen cargo, black markets and bounties on
  the player are later work.

### 7.3 Mining

- An **activity**, not a timer: fracture asteroids, cut into them, collect
  chunks, manage heat/hold.
- One of the three ways into the game (§8.3), and the one that must feel
  good on its own with no capital: a new game starts within a fast travel
  of a field.
- **Gear:** a *mining laser* is a module for a weapon slot (the starting
  courier has one). Its cut rate and reach come from the module; its heat
  is a gauge of its own, shed by the radiators. Run it to the top and it
  locks out while it vents.
- **Prospect:** a ping reads every rock nearby: ore or ice, a grade (A–D),
  volatile or not, and where its veins are (§8.7).
- **Cut:** the laser cuts faster and cooler on a vein, and a rock fractured
  through a vein yields more. Rocks spin, so veins turn away; the skill is
  chasing them.
- **Fracture:** smaller rocks split into smaller rocks and shed ore; the
  smallest crumble. Big rocks are **quarries**: each fracture calves a
  boulder off where the laser hit, until the quarry is worked out.
  **Volatile** rocks are richer but burst when they fracture, scattering
  ore and shoving the ship.
- **Collect:** a tractor pulls loose chunks into the hold when you fly
  close; when the hold is full, they stay floating. Sell at any market.
- **Rock is solid where it's drawn**: the ship, the laser and loose ore all
  meet the rock's real shape, so ore always comes to rest where the ship
  can reach it.
- **Getting there:** fast travel to a field stops among its rocks, facing a
  good mineable rock within laser reach: arrive, [G], fire.
- **Where it sells:** the mining panel shows the best markets in reach for
  the hold's load and what the whole load fetches. Choosing where to sell
  is the miner's main lever.
- **Finding a field:** wherever the player is, the hints name the nearest
  field, in this system or along the lanes.
- Fields remember: broken and worked rocks stay that way for months, then
  the field is whole again.

#### 7.3.1 Gas skimming

The player's way to harvest a gas giant, the same sources NPC skimmers
work.

- **Getting there:** fast travel to a gas giant stops above its cloud deck.
- **The band:** the gas thickens toward a solid deck you bounce off.
- **The scoop** is the hold's intake; any ship with a hold can skim. Deep
  and slow beats high and fast, and deep and fast overheats it in seconds;
  at full heat it shuts and vents. The gas drags and shoves the ship near
  the deck.
- **The HUD** swaps the mining panel for a skimming panel over a giant: an
  altimeter strip shaded by density, height, speed through the gas,
  intake, the scoop's heat, the hold, and what the gas fetches nearby.

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

#### 7.4.1 Finding a shipyard

- **Which settlements build ships is public knowledge**, like a pilot's
  almanac, not learned by visiting. The star chart lists the nearest
  shipyards with the trip and what's on the lot, and a click plots a
  course; NAVIGATION tags a yard YARD; a station with no yard says where
  the nearest is.
- **Every settlement can be visited** (#169). Until settlements on moons
  and small bodies can be, yards there aren't listed, and neither are
  yards whose faction would refuse the ship.

### 7.5 Fleet & automation

- Own multiple ships, hire faceless NPC captains, assign automated trade
  routes. Stations later.

#### 7.5.1 Freight contracts

Hour-to-hour work for the Trader path (§2), drawn from the economy, never
from a random generator (pillar 5).

- **Where they come from.** A settlement's dispatch board pairs a good it
  can spare with a settlement one to a few jumps away that is short of it,
  counting what traders already have on the way, so goods flow from
  fuller markets to emptier ones. Lots are sized to the player's hold.
  Boards change daily as markets do; some days a port has nothing.
- **Short runs first.** The board is ordered by pay per day, then by
  fewest jumps, and keeps a short run on it when one is on offer, so a new
  pilot's first payment comes after one jump, not five.
- **Who pays, and what.** The consignee buys the goods at the origin's
  price and pays the fee from its own treasury, only what it can afford.
  The fee pays what the galaxy's haulers cost for the days the trip
  takes, its lanes and a day of handling (§5.4.1); a deadline's slack
  isn't paid. A hop within one system is a few credits of carriage, so no
  board posts one: that is local shuttle work. The fee is a wage, not the
  trade's profit: the price gap and any delivery premium belong to
  whoever buys the goods and carries them at their own risk (§7.8). The
  wage must still feel worth the trip; today it is tens of credits a job,
  and what contracts should pay for is an open question (§11).
- **Deposit.** Taking a contract costs a deposit of the cargo's value,
  returned on delivery. Contract cargo rides in the hold but can't be
  sold.
- **Deadlines** allow some slack over the expected trip. Late deliveries
  pay less; very late or abandoned contracts fail: the consignee keeps the
  deposit, the goods are the player's, and its faction holds it against
  them (§7.2.2).
- **Delivery** happens on docking or landing at the consignee, and says
  what came back: the fee and the deposit. Waiting while docked stops on
  the day a contract falls due (§3).
- **A taken contract leaves the board** (#172), and **the way to its
  destination is on screen from the moment it's taken**, docked or in
  space (#188).

### 7.6 Planets

- No seamless landing. Planets have **predefined landing points**; landing
  plays an animation and opens the location's UI.
- Fast travel brings the ship into landing range of a planet's city, and
  [J] climbs back out after launch (§7.1.1).
- **A planet may have several cities**, each its own row in NAVIGATION
  under its planet, and each a place [L] can land. Hints name a city with
  its planet ("Kiolgristio, on Mufath Prime III") so the player knows
  where to fly.
- **What the sim founds while you're there is there at once**: a new city
  or station appears in its place in NAVIGATION after the day it's
  founded, with no message of its own. Founding is the sim's ordinary
  business.

### 7.7 Death

- On ship destruction, **restore the last checkpoint** (written on every
  docking and landing). Because the game is deterministic, the same future
  plays out again unless you act differently — what you learned before
  dying is yours to use. A checkpoint carries nothing over from before it
  (heat, cooldowns, a fight's dice), so the future really is the same.
- **The moment.** The HUD dies with the ship and the camera draws back from
  the fireball. A red constructivist band slams across the view: **SHIP
  LOST**, with when and where, the checkpoint's date and place, and how
  much time going back costs.
- **The choice is the player's.** No automatic restore: a key restores;
  Esc opens the pause menu and its saves.
- **The rewind.** Restoring rolls the tape back rather than cutting: film
  running backwards through a projector gate, a reel-to-reel counter
  rolling the galaxy date back, a tape-rewind whine ending in a clunk.
  Then **CHECKPOINT RESTORED** at the dock.

### 7.8 Earnings and progression

Every career in §2 the player can follow today earns a living, and the
ship ladder is long enough to save up along. Money comes out of the
economy, never from a payout table (pillar 5), so the levers are the sim's
own numbers.

- **Each career earns from its own thing:** contracts from hauling (a
  wage), trading from price gaps and delivery premiums (a profit, at the
  trader's own risk), mining from what it digs, bounties from warrants.
  A trader who guesses wrong should lose money.
- **No career dominates.** Contracts pay surely but modestly; trading pays
  most for skill and judgement, and can lose; mining needs no capital and
  never meets a bare board; bounties pay for risk. Pay comes from the sim,
  never from rates tuned to a target. Today trading out-earns everything
  many times over, because short markets never fill (#196).
- **The ladder is long.** Ships cost about twice their materials, and NPCs
  pay the same. A new pilot owns the courier and a little money: enough
  for a first deposit or a hold of cargo, not another ship. The next small
  ship is about an hour of work away, a medium ship many hours, a large
  ship tens of hours, which leaves fleets and capital ships (§7.5) the
  rest of the ~100-hour arc (§2).
- **Growth is by refitting as much as buying**: a miner grows by fitting
  more lasers and a bigger hold, not only by a bigger ship.
- **Measured** with the career bench (`cargo test -p retrox-client careers
  -- --ignored --nocapture`): bots play an hour of each career in a fresh
  game, paying the travel a player pays. Bots work far faster than a
  person (a contract a minute), so what a person earns is the bench's pay
  per job at a human pace, not its pay per hour.

#### 7.8.1 Trade runs

- **A pilot hears prices from this system and one jump out.** Prices
  further out are unknown; a remembered price list for ports the player
  has visited is not built yet.
- **TRADE RUNS**, beside the market, shows the best runs a pilot can hear
  of from here: for each good, the lot the hold, purse and stock allow,
  where it sells best in reach, what it costs, what it fetches, and the
  profit. Prices move on the way, so a run can come in over or under.
- **BUY & GO** buys the lot and heads for where it sells, and the hold
  remembers where it's bound, so hints and NAVIGATION lead there as they
  do for a contract.
- **Where nothing's worth carrying**, the panel names the nearest market
  space (which economy a faction runs is public) and says contracts and
  mining pay anywhere.

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
- **Text:** English. No fake Cyrillic. Plain and specific: no filler
  taglines or empty phrasing (#187).
- **Rendering:** flat / lightly lit hulls, strong silhouettes, glow,
  film grain, period film-stock color grading.
- **Audio:** analog-synth score, radio static and crackle on comms, heavy
  mechanical sounds for docking, switches, and hull stress.
- **The established look** is PR #45's; everything new matches it and
  looks good.

### 8.1 Ship models and liveries

- **No reused models** (the user's ruling): every hull is designed by hand.
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

- The map is seen **through a round chart glass** set in an enamelled
  console, not as a full-screen canvas: the console frame is what makes it
  read as a physical instrument, and the round glass keeps the eye on the
  focus. The view is 3D.
- **Names fit the zoom** (#193): zoomed out, the chart names large
  structures, not a scatter of single systems; single systems are named
  only when close enough to tell apart. Labels never overlap.
- Every view control has a **physical scale**: a bearing ring graduated in
  degrees turns as the view orbits, read against a fixed red index;
  elevation and range have their own arc scales with amber pointers; range
  is logarithmic.
- **Drum counters** (cream on black) repeat the chart drive's settings.
  Mechanical readouts belong to instruments set by hand; live ship data
  stays on teal VFD.
- The chart's plane carries **range rings and bearing spokes** round the
  focus in faint phosphor teal.
- Moving the chart's focus **slews** it with easing, as a motor-driven
  chart would, with a SLEW lamp lit while it runs.
- The course panel shows a **jump interlock** of lamps (course set, well
  clear, jump ready), so why a jump is unavailable reads at a glance.
- Systems keep their faction colours: the chart is information first.
- **What matters is lit, the rest is faint** (#192). At full strength:
  where you are, what's one jump away, the course, contracts'
  destinations (with the way there), shipyards, search hits, and
  whatever the pointer is on. Everything else stays readable but faint,
  and never competes. Later map work keeps this split rather than adding
  full-strength layers.
- The chart **plots on the plane level with its focus**, so height lines
  stay short, and **opens at a range that takes in what matters**.
- **FIND SYSTEM** finds a system by typing part of its name and plots a
  course to it; empty, it lists the contracts' destinations. While a
  text field has the keyboard, no game key acts.

### 8.3 Help for new pilots

Pillar 5 applies to teaching too: no scripted tutorial campaign, nothing
that waits on the player. Pillar 7 is the test: a new player gets through
each loop from the screen alone.

- **Three ways in:** contracts, mining and trading are offered side by side
  from the first screen; none is forced. Each leads the player through
  launching, fast travel and selling or delivering.
- **First-steps notes**: a HUD panel of indicator lamps, one per first
  (take on work, launch, pick a target, fast travel, jump a hyperlane,
  dock or land, get paid), lit teal as the player does each one in any
  order, the next one amber. Under the lamps, **one hint** read off the
  situation every frame (where you are, what you carry, your work's
  destination, what you're aimed at). It goes for good once every lamp is
  lit; a setting turns it off.
- **Hints link to what they name**: a place in a hint is an amber,
  underlined link; clicking a field or station targets it, clicking a
  system plots a course. Keys are drawn as keycaps.
- **Hints name the way out by its button**: docked it is UNDOCK, landed it
  is LAUNCH.
- **Controls card** on H (or F1): every key over the view, without pausing.
  The HUD corner shows only the keys for help and the menu.
- **Pilot's handbook**: on the title screen and in the pause menu, the one
  place that explains the game in prose. The handbook, the controls card
  and the hints share one key table, so they can't disagree.
- **Edge arrows**: a target off screen gets an arrow at the edge of the
  view (amber for the target, red for hostiles) pointing the short way
  round to it.
- **Nothing covers the help**: toasts and banners never sit over the
  first-steps panel (#177).

### 8.4 Screens and controls

- **No stock widgets.** Every list, button and table is painted from the
  shared kit: lists are **instrument rows** with a hairline under each, a
  red wash on hover, an amber wash on the selected row; buttons are **push
  keys** with a darker lip that sink when pressed (red for the one primary
  action, dark enamel otherwise).
- **Colour carries meaning, consistently, everywhere:**
  - amber: what you pay, buy, target or have selected;
  - teal: live readings, and what you're paid or sell for;
  - red: the primary action, alarms and anything hostile (dimmer for
    leaning hostile: a wary faction, a gang about its own business);
  - cream: names and plain facts.
  So trade has an amber bank of buy keys and a teal bank of sell keys, and
  a standing rank never borrows amber.
- **Joined key banks** (`+1 +10 MAX`) rather than loose buttons for steps
  of one action, like a row of switches on a panel.
- **One lamp and one keycap**: every indicator lamp and every keycap is
  painted the same way, so they match from screen to screen.
- **Chart symbols** in lists of places: a rayed star, ringed gas giants,
  stations as teal diamonds, fields as scattered dots, cities as a little
  skyline; moons, stations and cities hang off their body on a bracket.
  Rows carry tags for why they matter: CARGO (amber) where the hold is
  bound, RAIDERS (red) at a lair, YARD (cream) at a shipyard.
- **NAVIGATION is a tree of where you go**: the star, then each body with
  its cities, stations and moons. Fields are places you go as much as
  stations — mining and bounties happen there — so they are never out of
  sight below the rest. A list longer than the panel always shows its
  scroll bar, and a row picked from elsewhere scrolls into view. Names are
  short (the panel gives the context) and cut to the room left, never to a
  fixed count.
- **Counts and estimates read as words**: "1 jump", "this system", "about
  39 d"; distances in the unit that reads (km near, AU far). Never "~" or
  "≈", which the mono face draws as other glyphs.
- **Text over the scene sits on a plate**: a tight opaque plate behind each
  label, a dark plate behind blocks of text, so text stays whole over a
  sunlit rock or a star. No drop shadows: they read as a ghosted second
  copy. Panels and menus are opaque.
- **Labels never pile up**: markers, rock tags and readouts make room for
  each other and for the HUD's panels (#173).
- **The HUD fits the window**: in a smaller window the whole UI scales down
  rather than letting panels pile onto each other.
- **The message banner fits its text**, one line where it can, under the
  top edge.

### 8.5 Sound

- **All synthesized, no samples.** Every sound is made in code from
  oscillators, noise, filters and resonators, so the whole game shares one
  analog character and cues can be retuned like any other code.
- **The score is generative**: a small analog ensemble — detuned-saw
  string pad, filtered bass, sequencer arpeggio through a tape delay, a
  theremin-like lead, glassy FM bells, a soft rhythm box — with tape wow on
  everything, in D Dorian / Aeolian (hopeful, with melancholy). Moods
  crossfade by situation: *title*, *space* (sparse, resting in silence now
  and then), *station* (warmer), *transit* (the sequencer driving).
- **The palette:** heavy mechanisms for anything physical — docking clamps
  and airlock hiss, the jump drive's breaker, the star chart's motors and
  relays, bakelite toggles for every switch, hull plates groaning; radio
  static and squelch whenever a message comes over comms; VFD beeps for
  instruments. Engines rise with acceleration rather than speed, so
  cruising is quiet and burns are loud.
- **The sound follows the game**: the client turns what changed in the
  world each frame into cues; game code only reports what the sound can't
  see.
- **Volume** sliders in Settings; pausing silences the ship but not the
  music. No audio device means silence, never an error.

### 8.6 The sky

- **Every other system is a star in the sky**, carrying the light it really
  sends us.
- **Stars are sharp points**: a hard core and a halo of a few pixels at
  most, never a smudge. Past that, light is compressed like film's
  shoulder into a core that blooms and **four thin diffraction spikes** on
  the diagonals, fixed to the frame, that grow longer, never wider. A close
  neighbour is a hard white point with glare and rays, not a fog bank.
- **A near star shows its surface**: exposed down as its disc grows, with
  limb darkening, granulation and spots, and a faint corona.

### 8.7 Asteroid fields

- **Rocks are broken pieces, not eggs**: lumpy, stretched, with flat
  fracture faces, craters and chips, in many shapes. The shape the ship,
  laser and ore meet is the one drawn.
- **Rock is dark stone**, each rock leaning umber, slate or rust, mottled,
  and **sharp at any distance**: its relief and grit hold from its own size
  down to the pixel, so a quarry up close has no straight edges or
  stretched texture.
- **Rock in shade still reads as rock**, and a backlit rock shows its
  facets against the black.
- **Veins are seams of ore in the rock**, not lamps on it: a stained patch
  with stringers glowing out of it, amber ore, pale blue ice, pulsing red
  if volatile.
- **A belt is dense and deep**: gravel and boulders too small to mine fill
  the space between the rocks, thinning toward the edge, and sunlit dust
  veils the distance. None of it ever looks like a rock to mine.
- **Laser heat glows round the cut**, never over a whole rock face. A
  breaking rock throws a spark and a dusty puff, never a flash that covers
  the view.
- **Rock tags**, not letters on rocks: a read rock gets a short leader to
  a plated tag, *ORE C* or *ICE B* (and *VOLATILE* in red), bright in
  laser reach, dim beyond, never overlapping; the nearest few, plus
  quarries further out as landmarks. The rock under the reticle has its
  readout instead.
- **Loose ore** is a small diamond: filled while the tractor has it,
  hollow out of reach, dim when the hold is full; the nearest piece out of
  reach says how far it is.

## 9. Technical architecture

Pure Rust, Linux first.

```
retrox/
  crates/
    sim/      # galaxy state, tick pipeline, economy, factions, events, news
    gen/      # procedural generation (galaxy, systems, factions)
    content/  # storylets, goods, module & hull definitions (data-only Rust)
    client/   # rendering, input, audio, local physics, UI
    tools/    # headless runner, history dumps, legends, balancing
```

- **sim:** no engine dependencies. Typed arenas, explicit phases, rayon.
  Serializable with serde — a save is (mostly) one serialized struct.
- **Spatial queries:** galaxy level = hyperlane graph + kd-tree over system
  positions; in-system = f64 coordinates per system frame with a spatial
  index for "what's near X".
- **client:** thin custom client on `wgpu` + `winit`; `kira` for audio;
  custom in-game UI matching the art direction (painted with egui, never
  stock widgets). Floating origin / camera-relative rendering for large
  scales. The client owns only what's near the camera; it reads sim state
  and submits player actions.
- **No ECS for the galaxy.** If the client wants an ECS-like structure for
  the local scene it's an implementation detail, since that state is a
  disposable view of the sim.
- **One flight model and one autopilot** for every ship, player and NPC
  (§5.4.1).
- **tools** gains `retrox legends` (browse and query the history record)
  and a time-lapse map for tuning the history engine; the in-game start
  screen (§4.2) reuses its views. `--remote` lets an agent play a real game
  turn by turn, for playtests.
- **Saves:** one long save per game plus a checkpoint rewritten on every
  docking and landing; *Continue* loads the newest. A save is the whole
  sim state plus the player's, and loading it replays the same future
  given the same actions. No saving in transit or with hostiles near.
  A save from another format, or one that doesn't parse, is refused with
  a message, never a crash.
- **Menus:** a title screen (Continue, New game, Load, Settings, Quit);
  New game takes a seed and generates behind a loading card. Esc opens the
  pause menu, which stops galaxy time. Menus work with the mouse or the
  keyboard. Settings persist.
- **Controls:** the arrow keys pitch like a stick, as the mouse does (↓
  nose up); *Invert pitch* flips both.

## 10. Milestones (rough)

1. **Sim skeleton (headless):** galaxy gen + hyperlanes, factions, basic
   economy with a few goods, tick pipeline, history dump tool.
2. **Flight sandbox:** wgpu client, one system rendered at scale, arcade
   flight, fast travel, docking, floating origin.
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
11. **History engine:** cradle generation, the §5.7 rules, determinism
    rules, legends tool and time-lapse map, start screen. Order relative to
    the others is flexible.

## 11. Open questions

- Capital-ship combat design in detail.
- Nature of the aliens and the semi-scripted anomalies/discoveries.
- Faction count and size at generation; agent budget in the bubble.
- Gamepad support / control scheme.
- Does the player's character age over a ~30-year save? (Probably ignored.)
- History tuning: default `history_years`, and how strong the secession
  loop and shock rates should be.
- How tech is lost and recovered in detail.
- How many lane tiers, and how lanes are split between them.
- How historical figures are generated (lifespans, which events earn a
  name).
- What a freight contract should pay for: hauling at the galaxy's cheap
  carriage rates pays tens of credits a job.
