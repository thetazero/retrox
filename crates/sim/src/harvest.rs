//! Harvesters (issue #68, DESIGN §5.4.2): companies whose ships gather raw
//! materials from space instead of buying them, ice from icy asteroid
//! fields and moons, gas from gas giants, and sell them to a market.
//!
//! A harvester is an ordinary [`Company`](crate::haul::Company) whose
//! fleets carry a [`Harvest`] job. It lives on the same calendar as
//! traders. At its monthly review a fleet picks a **source** in its port's
//! system and a market in that system to sell to. Each **run** is then one
//! trip: out to the source, a few days working it until the hold is full,
//! and in to the market, where arriving sells the cargo like any other.
//! Harvesters never use the lanes.
//!
//! - **Coarse tier:** a run is a single trip on the calendar. The hold
//!   fills when the run's arrival comes round, just before the sale.
//! - **Bubble:** the run is flown for real (see [`crate::bubble`]). Ships
//!   fly to the source, hold station there while they work, so they can be
//!   seen at the fields and skimming the giants, and fly to the market.
//!
//! **Sources don't run dry.** Each keeps a [`Strain`]: the tonnes taken
//! from it lately, fading with a [`STRAIN_DAYS`] time constant. Working a
//! strained source takes longer, so harvesters spread out over a system's
//! sources and a crowded one recovers if they leave it alone.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::bodies::{BodyId, BodyKind, PlanetClass};
use crate::economy::{Good, STORAGE_MULTIPLE};
use crate::fields::FieldId;
use crate::flight::{self, Goal};
use crate::haul::{self, CompanyId, Location};
use crate::ledger::{self, Account, Supply};
use crate::settlement::SettlementId;
use crate::{GalaxyState, GalaxyTime, Rng, SystemId};

use crate::bubble::METRES_PER_AU;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Somewhere ships can harvest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Source {
    /// An asteroid field, for its ice.
    Field(FieldId),
    /// A gas giant for its gas, or an icy moon or ice world for its ice.
    Body(BodyId),
}

/// A fleet's harvesting job.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Harvest {
    /// What it's committed to until its next review.
    pub plan: Option<Plan>,
    /// Out on a run and the hold not yet filled.
    pub working: bool,
}

/// A month's commitment: work `source` for `good`, sell at `market`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub source: Source,
    pub good: Good,
    pub market: SettlementId,
}

/// Richness below this isn't worth a run.
const MIN_RICHNESS: f64 = 0.1;
/// Days to fill a hold at a source of richness 1.0, before strain.
const FILL_DAYS_AT_FULL: f64 = 6.0;
/// The longest a run spends at the source.
const MAX_WORK_DAYS: f64 = 30.0;
/// Strain fades with this time constant, in days: the same memory fields
/// have of the player's mining (DESIGN §7.3).
pub const STRAIN_DAYS: f64 = 120.0;
/// Tonnes of recent takings at which a source's work takes twice as long.
fn strain_capacity(state: &GalaxyState, source: Source) -> f64 {
    match source {
        Source::Field(_) => 6_000.0,
        Source::Body(b) => match state.bodies[b].kind {
            BodyKind::GasGiant => 40_000.0,
            _ => 10_000.0,
        },
    }
}
/// A run has to earn at least this many credits a day.
const MIN_PROFIT_PER_DAY: f64 = 1.0;
/// A run is only worth it for a market with room for this share of a hold.
const MIN_HOLD_SHARE: f64 = 0.5;
/// A buyer spends at most this share of its treasury on one delivery.
const MAX_SPEND_SHARE: f64 = 0.5;

/// What a source gives and how richly, if anything.
pub fn yields(state: &GalaxyState, source: Source) -> Option<(Good, f64)> {
    let (good, richness) = match source {
        Source::Field(f) => (Good::Ice, state.fields[f].resources.ice),
        Source::Body(b) => {
            let body = &state.bodies[b];
            match body.kind {
                BodyKind::GasGiant => (Good::Gas, body.resources.gas),
                BodyKind::Moon | BodyKind::Planet(PlanetClass::Ice) => {
                    (Good::Ice, body.resources.ice)
                }
                _ => return None,
            }
        }
    };
    (richness >= MIN_RICHNESS).then_some((good, richness))
}

/// The system a source is in.
pub fn system(state: &GalaxyState, source: Source) -> SystemId {
    match source {
        Source::Field(f) => state.bodies[state.fields[f].belt].system,
        Source::Body(b) => state.bodies[b].system,
    }
}

/// A source's name, for display.
pub fn name(state: &GalaxyState, source: Source) -> &str {
    match source {
        Source::Field(f) => &state.fields[f].name,
        Source::Body(b) => &state.bodies[b].name,
    }
}

/// Every source in each system, in a fixed order, worked out once. The
/// bodies and fields never change, so nothing ever needs clearing. Not
/// saved.
#[derive(Default)]
pub struct SourceIndex(RwLock<Option<Sources>>);

/// Each system's sources.
type Sources = Arc<HashMap<SystemId, Vec<Source>>>;

impl Clone for SourceIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for SourceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceIndex")
    }
}

/// The sources in `system`, fields first, then bodies inner to outer.
pub fn sources_in(state: &GalaxyState, system: SystemId) -> Vec<Source> {
    let index = {
        let read = state.source_index.0.read().expect("source index poisoned");
        read.clone()
    };
    let index = index.unwrap_or_else(|| {
        let mut map: HashMap<SystemId, Vec<Source>> = HashMap::new();
        for (id, _) in state.fields.iter() {
            let source = Source::Field(id);
            if yields(state, source).is_some() {
                map.entry(self::system(state, source))
                    .or_default()
                    .push(source);
            }
        }
        for (id, body) in state.bodies.iter() {
            let source = Source::Body(id);
            if yields(state, source).is_some() {
                map.entry(body.system).or_default().push(source);
            }
        }
        let map = Arc::new(map);
        *state.source_index.0.write().expect("source index poisoned") = Some(map.clone());
        map
    });
    index.get(&system).cloned().unwrap_or_default()
}

/// Recent takings from each source.
#[derive(Clone, Debug, Default)]
pub struct Strain {
    /// Tonnes, as of the day stored with them.
    taken: HashMap<Source, (f64, u64)>,
}

impl Strain {
    /// Tonnes taken lately from `source`, as of `today`.
    pub fn get(&self, source: Source, today: GalaxyTime) -> f64 {
        self.taken
            .get(&source)
            .map_or(0.0, |&(t, day)| faded(t, day, today))
    }

    fn add(&mut self, source: Source, today: GalaxyTime, tonnes: f64) {
        let now = self.get(source, today) + tonnes;
        self.taken.insert(source, (now, today.days()));
    }
}

fn faded(tonnes: f64, day: u64, today: GalaxyTime) -> f64 {
    let days = today.days().saturating_sub(day) as f64;
    tonnes * (-days / STRAIN_DAYS).exp()
}

// JSON maps need string keys, so strain saves as a sorted list.
impl Serialize for Strain {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<(Source, f64, u64)> =
            self.taken.iter().map(|(&k, &(t, d))| (k, t, d)).collect();
        entries.sort_by_key(|e| e.0);
        entries.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Strain {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let entries: Vec<(Source, f64, u64)> = Vec::deserialize(d)?;
        Ok(Self {
            taken: entries.into_iter().map(|(k, t, d)| (k, (t, d))).collect(),
        })
    }
}

/// Record `tonnes` taken from `source` today, as a run or the player does.
pub fn strain(state: &mut GalaxyState, source: Source, tonnes: f64) {
    let today = state.time;
    state.strain.add(source, today, tonnes);
}

/// How rich a source works out right now, after strain: what sets how long
/// filling a hold takes.
pub fn effective_richness(state: &GalaxyState, source: Source) -> f64 {
    let Some((_, richness)) = yields(state, source) else {
        return 0.0;
    };
    let strain = state.strain.get(source, state.time) / strain_capacity(state, source);
    richness / (1.0 + strain)
}

/// Mean strain over every source worked lately, as a share of its
/// capacity: 1.0 means a typical worked source takes twice as long.
pub fn mean_strain(state: &GalaxyState) -> f64 {
    let n = state.strain.taken.len();
    if n == 0 {
        return 0.0;
    }
    let mut keys: Vec<Source> = state.strain.taken.keys().copied().collect();
    keys.sort();
    keys.iter()
        .map(|&s| state.strain.get(s, state.time) / strain_capacity(state, s))
        .sum::<f64>()
        / n as f64
}

/// Days at the source to fill a hold.
pub fn work_days(state: &GalaxyState, source: Source) -> f64 {
    let r = effective_richness(state, source).max(1e-3);
    (FILL_DAYS_AT_FULL / r).clamp(1.0, MAX_WORK_DAYS)
}

/// How far a source is from its star, in AU, for travel estimates.
fn radius_au(state: &GalaxyState, source: Source) -> f64 {
    let orbit = match source {
        Source::Field(f) => &state.fields[f].orbit,
        Source::Body(b) => &state.bodies[b].orbit,
    };
    match orbit.parent {
        crate::Orbiting::Star => orbit.semi_major_au,
        crate::Orbiting::Body(p) => state.bodies[p].orbit.semi_major_au,
    }
}

/// Days of in-system cruise between a port and a source.
fn cruise(state: &GalaxyState, accel: f64, port: SettlementId, source: Source) -> f64 {
    let d = (haul::radius_au(state, port) - radius_au(state, source))
        .abs()
        .max(0.002);
    crate::bubble::cruise_days(d, accel)
}

/// The stretches of a run from `here`, in days: out, work, and in to the
/// market (including handling).
pub fn run_days(
    state: &GalaxyState,
    accel: f64,
    here: SettlementId,
    plan: &Plan,
) -> (f64, f64, f64) {
    (
        cruise(state, accel, here, plan.source),
        work_days(state, plan.source),
        cruise(state, accel, plan.market, plan.source) + haul::HANDLING_DAYS,
    )
}

/// Where a ship works a source at `t_days`, in metres from the star:
/// spread round the source by `slot` (a turn, 0..1) so a fleet's ships
/// and different fleets don't sit on one point. In a field, somewhere in
/// it; at a gas giant, skimming its sunlit upper atmosphere; at a moon,
/// low over its surface.
pub fn work_goal(state: &GalaxyState, source: Source, slot: f64, t_days: f64) -> Goal {
    let system = self::system(state, source);
    let angle = slot * std::f64::consts::TAU;
    let (centre, vel, offset_m) = match source {
        Source::Field(f) => {
            let field = &state.fields[f];
            let (p, v) = state.orbit_state(system, &field.orbit, t_days);
            // Inside the field, a third of the way out: among the rocks.
            (p, v, field.radius_km * 1000.0 * 0.35)
        }
        Source::Body(b) => {
            let body = &state.bodies[b];
            let (p, v) = state.orbit_state(system, &body.orbit, t_days);
            let altitude = match body.kind {
                BodyKind::GasGiant => 1.04,
                _ => 1.15,
            };
            (p, v, body.radius_km * 1000.0 * altitude)
        }
    };
    let centre = flight::scale(centre, METRES_PER_AU);
    // Toward the star, turned round the pole by the slot: the dayside.
    let sun = flight::scale(centre, -1.0);
    let len = flight::length(sun).max(1.0);
    let (sx, sy) = (sun[0] / len, sun[1] / len);
    let turn = (angle - 0.5 * std::f64::consts::PI) * 0.5;
    let (c, s) = (turn.cos(), turn.sin());
    let dir = [sx * c - sy * s, sx * s + sy * c, 0.15 * angle.sin()];
    let dl = flight::length(dir).max(1e-9);
    let offset = flight::scale(dir, offset_m / dl);
    let vel = flight::scale(vel, METRES_PER_AU / SECONDS_PER_DAY);
    // The offset faces the star, so it turns as the source goes round it,
    // at the source's angular rate about the pole. Left out, a gas giant's
    // ~70,000 km offset moves ~13 m/s faster than the goal says: more than
    // the autopilot's arrival tolerance, so ships closing on it never
    // arrive and crawl at its smallest step all day (issue #115).
    let r2 = (centre[0] * centre[0] + centre[1] * centre[1]).max(1.0);
    let omega = (centre[0] * vel[1] - centre[1] * vel[0]) / r2;
    let turning = [-omega * offset[1], omega * offset[0], 0.0];
    Goal {
        pos: flight::add(centre, offset),
        vel: flight::add(vel, turning),
    }
}

/// What `ships` of `class` would clear a day running `plan` from `here`,
/// selling at the good's base price: how generation and founding choose a
/// hull for the work.
pub fn base_profit_per_day(
    state: &GalaxyState,
    class: crate::haul::ShipClassId,
    ships: u32,
    here: SettlementId,
    plan: &Plan,
) -> f64 {
    let c = &state.ship_classes[class];
    let fleet = haul::Fleet {
        id: 0,
        class,
        ships,
        at: Location::Docked(here),
        cargo: None,
        route: None,
        review_due: false,
        idle: false,
        loaded: false,
        slack: 0,
        harvest: None,
    };
    let (out, work, back) = run_days(state, c.accel_laden, here, plan);
    let cargo = c.cargo_t * f64::from(ships);
    let fuel = haul::fuel_per_day(state, &fleet) * (out + back) * Good::Fuel.base_price();
    (cargo * plan.good.base_price() - fuel) / (out + work + back)
        - haul::upkeep_per_day(state, &fleet)
}

/// Whether `here` is in the company's home system, where it harvests.
fn at_home(state: &GalaxyState, key: CompanyId, here: SettlementId) -> bool {
    let home = state.companies[key].home;
    state
        .settlements
        .get(home)
        .is_none_or(|h| h.system == state.settlements[here].system)
}

/// Whether a fleet is a harvester.
pub fn is_harvester(state: &GalaxyState, key: CompanyId, n: usize) -> bool {
    state.companies[key].fleets[n].harvest.is_some()
}

/// What a market will pay per unit for `amount` more of `good`, once
/// what's already coming has arrived.
fn expected_sale(state: &GalaxyState, to: SettlementId, good: Good, amount: f64) -> f64 {
    let s = &state.settlements[to];
    let market = &s.economy.market;
    let stock = market.stock[good] + state.traffic.inbound(to, good) + amount / 2.0;
    market.price_at(good, stock, state.factions[s.owner].economy)
}

/// The best run from `here`: every source in the system for every market
/// in it that wants the good, by profit per day.
fn choose(state: &GalaxyState, key: CompanyId, n: usize, here: SettlementId) -> Option<Plan> {
    let fleet = &state.companies[key].fleets[n];
    let class = &state.ship_classes[fleet.class];
    let capacity = class.cargo_t * f64::from(fleet.ships);
    let system = state.settlements[here].system;
    let fuel_price = state.settlements[here].economy.market.price[Good::Fuel];
    let fuel_per_day = haul::fuel_per_day(state, fleet);
    // Harvesting has no cargo to buy, so the crew's pay is most of its
    // cost: a run has to cover it.
    let upkeep = haul::upkeep_per_day(state, fleet);
    let mut best: Option<(f64, Plan)> = None;
    for source in sources_in(state, system) {
        let Some((good, _)) = yields(state, source) else {
            continue;
        };
        for (market, s) in state.settlements_in(system) {
            if s.economy.market.demand[good] <= 0.0 {
                continue;
            }
            let plan = Plan {
                source,
                good,
                market,
            };
            let (out, work, back) = run_days(state, class.accel_laden, here, &plan);
            let days = out + work + back;
            // Markets store no more than facilities would fill them to.
            let m = &s.economy.market;
            let room = m.target(good) * STORAGE_MULTIPLE
                - m.stock[good]
                - state.traffic.inbound(market, good);
            if room < capacity * MIN_HOLD_SHARE {
                continue;
            }
            let sell = expected_sale(state, market, good, capacity);
            let treasury = s.economy.treasury.max(0.0);
            let amount = capacity
                .min(room)
                .min(treasury * MAX_SPEND_SHARE / sell.max(1e-9));
            let per_day =
                (amount * sell - (out + back) * fuel_per_day * fuel_price) / days - upkeep;
            if per_day > MIN_PROFIT_PER_DAY && best.as_ref().is_none_or(|(b, _)| per_day > *b) {
                best = Some((per_day, plan));
            }
        }
    }
    best.map(|(_, plan)| plan)
}

/// A harvester's review, or its arrival with one due: commit to the best
/// run from here, or idle if none pays.
pub(crate) fn review(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) {
    if !at_home(state, key, here) {
        // Not home yet (a new crew fetching its hull): it plans there.
        let fleet = &mut state.companies[key].fleets[n];
        fleet.idle = false;
        if let Some(h) = fleet.harvest.as_mut() {
            h.plan = None;
        }
        return;
    }
    let plan = crate::decisions::time(crate::decisions::Decision::Harvest, || {
        choose(state, key, n, here)
    });
    let fleet = &mut state.companies[key].fleets[n];
    fleet.idle = plan.is_none();
    if let Some(h) = fleet.harvest.as_mut() {
        h.plan = plan;
    }
}

/// A harvester docked at `here` sets out on its next run, if it has a plan.
pub(crate) fn run(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) {
    let fleet = &state.companies[key].fleets[n];
    if !matches!(fleet.at, Location::Docked(_)) {
        return;
    }
    if !at_home(state, key, here) {
        let home = state.companies[key].home;
        let class = fleet.class;
        let Some(days) = haul::trip_days(state, class, here, home) else {
            return;
        };
        state.companies[key].fleets[n].review_due = true;
        haul::depart(state, key, n, here, home, days.ceil() as u32, true);
        return;
    }
    let Some(plan) = fleet.harvest.and_then(|h| h.plan) else {
        return;
    };
    if !state.settlements.contains_key(plan.market) {
        if let Some(h) = state.companies[key].fleets[n].harvest.as_mut() {
            h.plan = None;
        }
        return;
    }
    let class = &state.ship_classes[fleet.class];
    let capacity = class.cargo_t * f64::from(fleet.ships);
    let accel = class.accel_laden;
    // Unsold cargo of another good, from before a change of plan: left at
    // the dock for nothing, to make room.
    if let Some(c) = fleet.cargo
        && c.good != plan.good
    {
        ledger::donate(state, here, c.good, c.amount);
        state.companies[key].fleets[n].cargo = None;
    }
    let (out, work, back) = run_days(state, accel, here, &plan);
    let days = (out + work + back).ceil().max(1.0) as u32;
    let today = state.time;
    // Fuel for the flying, not the working.
    let fleet = &state.companies[key].fleets[n];
    let fuel = haul::fuel_per_day(state, fleet) * (out + back);
    let fuel_price = state.settlements[here].economy.market.price[Good::Fuel];
    ledger::buy(
        state,
        Account::Company(key),
        here,
        Good::Fuel,
        fuel,
        fuel_price,
        Supply::Stock,
        1.0,
    );
    let fleet = &mut state.companies[key].fleets[n];
    let id = fleet.id;
    if let Some(h) = fleet.harvest.as_mut() {
        h.working = true;
    }
    // What the run will bring, so traders see it coming: a full hold.
    state.traffic.add(plan.market, plan.good, capacity);
    let system = state.settlements[here].system;
    if crate::bubble::contains(state, system) {
        crate::bubble::launch_harvest(state, (key, id), here, plan.market, plan.source);
        return;
    }
    let arrive = GalaxyTime::from_days(today.days() + u64::from(days));
    state.companies[key].fleets[n].at = Location::Travelling {
        from: here,
        depart: today,
        to: plan.market,
        arrive,
        jumps: false,
    };
    state.calendar.schedule(
        today,
        arrive,
        haul::Event {
            company: key,
            fleet: id,
            kind: haul::EventKind::Arrive,
        },
    );
}

/// The hold fills at the end of a run's work: the goods come into being
/// in the ships, and the source is strained by them. Does nothing unless
/// the fleet is out working.
pub(crate) fn fill(state: &mut GalaxyState, key: CompanyId, n: usize) {
    let fleet = &state.companies[key].fleets[n];
    let Some(Harvest {
        plan: Some(plan),
        working: true,
    }) = fleet.harvest
    else {
        return;
    };
    let capacity = state.ship_classes[fleet.class].cargo_t * f64::from(fleet.ships);
    let aboard = fleet
        .cargo
        .filter(|c| c.good == plan.good)
        .map_or(0.0, |c| c.amount);
    let tonnes = (capacity - aboard).max(0.0);
    let today = state.time;
    state.strain.add(plan.source, today, tonnes);
    let fleet = &mut state.companies[key].fleets[n];
    fleet.cargo = Some(haul::Cargo {
        good: plan.good,
        amount: aboard + tonnes,
    });
    fleet.loaded = true;
    if let Some(h) = fleet.harvest.as_mut() {
        h.working = false;
    }
}

/// Which way round its source a fleet works: a fixed turn from its key.
pub fn slot(key: CompanyId, fleet: u32) -> f64 {
    let entity = slotmap::Key::data(&key).as_ffi() ^ (u64::from(fleet) << 32);
    Rng::stream(0, "harvest-slot", 0, entity).f64()
}

/// Founding: at its monthly review, a settlement with money to spare and
/// a source in its system whose good it is short of may found a harvesting
/// company with one ship, unless harvesters already sit idle there. The
/// crew fetches the hull from the nearest yard that has one, or orders one,
/// the way trading companies do (see [`haul`]).
pub(crate) fn found(state: &mut GalaxyState) {
    const CHANCE: f64 = 0.25;
    const WEALTH: f64 = 3.0;
    const CREDITS_PER_T: f64 = 10.0;
    let today = state.time;
    for i in crate::settlement::review_slots(today.days(), 0, state.settlements.slot_count()) {
        let Some(id) = state.settlements.key_at(i) else {
            continue;
        };
        let system = state.settlements[id].system;
        let entity = slotmap::Key::data(&id).as_ffi();
        if state.settlements[id].economy.population <= 0.0
            || !Rng::stream(state.seed, "found-harvester", today.days(), entity).chance(CHANCE)
        {
            continue;
        }
        let market = &state.settlements[id].economy.market;
        let Some(good) = sources_in(state, system)
            .into_iter()
            .filter_map(|s| yields(state, s).map(|y| y.0))
            .find(|&g| market.demand[g] > 0.0 && market.fill(g) < 1.0)
        else {
            continue;
        };
        let treasury = state.settlements[id].economy.treasury;
        let cost = |c: &haul::ShipClass| {
            c.mass_t * crate::economy::HULL_PRICE_PER_T + c.cargo_t * CREDITS_PER_T
        };
        // The hull that clears the most a day on the richest source of the
        // good, among those it can afford.
        let Some(&source) = sources_in(state, system)
            .iter()
            .filter(|&&s| yields(state, s).is_some_and(|y| y.0 == good))
            .max_by(|&&a, &&b| {
                effective_richness(state, a).total_cmp(&effective_richness(state, b))
            })
        else {
            continue;
        };
        let plan = Plan {
            source,
            good,
            market: id,
        };
        let Some((class, founding)) = state
            .ship_classes
            .iter()
            .filter(|(_, c)| cost(c) * WEALTH <= treasury)
            .map(|(cid, c)| (cid, cost(c), base_profit_per_day(state, cid, 1, id, &plan)))
            .filter(|&(_, _, p)| p > MIN_PROFIT_PER_DAY)
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(cid, cost, _)| (cid, cost))
        else {
            continue;
        };
        // Harvesters already sat idle here this month.
        if state
            .idle_ports
            .as_ref()
            .is_some_and(|p| p.harvesters(system))
        {
            continue;
        }
        let mass = state.ship_classes[class].mass_t;
        // The crew fetches its hull from the nearest yard with one and
        // flies it home.
        let yard = match haul::hull_source(state, &state.board, id, class) {
            Ok(yard) => yard,
            Err(order) => {
                state.settlements[order].economy.hull_wanted += mass;
                continue;
            }
        };
        let base = state.settlements[id]
            .name
            .split(' ')
            .next()
            .unwrap_or("Free")
            .to_owned();
        state.settlements[yard].economy.hull_t -= mass;
        let mut new = company(format!("{base} {}", trade_name(good)), id, class, 1);
        new.fleets[0].at = Location::Docked(yard);
        let company = haul::add_company(state, new);
        let (home, company) = (Account::Settlement(id), Account::Company(company));
        ledger::pay(state, home, company, founding);
        ledger::pay(
            state,
            company,
            Account::Settlement(yard),
            mass * crate::economy::HULL_PRICE_PER_T,
        );
    }
}

/// What a harvesting company calls itself, by what it gathers.
pub fn trade_name(good: Good) -> &'static str {
    match good {
        Good::Gas => "Gas Skimmers",
        _ => "Ice Haulers",
    }
}

/// A new harvesting company with one fleet docked at `home`.
pub fn company(
    name: String,
    home: SettlementId,
    class: crate::haul::ShipClassId,
    ships: u32,
) -> haul::Company {
    haul::Company {
        name,
        home,
        credits: 0.0,
        fleets: vec![haul::Fleet {
            id: 0,
            class,
            ships,
            at: Location::Docked(home),
            cargo: None,
            route: None,
            review_due: false,
            idle: false,
            loaded: false,
            slack: 0,
            harvest: Some(Harvest::default()),
        }],
        next_fleet: 0,
    }
}
