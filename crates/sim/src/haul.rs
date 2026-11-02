//! Traders in the coarse tier (DESIGN §5.1.1, §5.2, issue #57).
//!
//! A trader is a company: money, a home, and fleets of one ship class each,
//! tracked as counts. A fleet commits to a **route** on a monthly review:
//! buy a good at one settlement, sell it at another. Until its next review
//! it shuttles: load and depart, arrive and sell, go back empty, repeat.
//!
//! Traders don't tick. Everything a fleet does happens on an **event** in
//! the [`Calendar`]: arriving somewhere, or its monthly review. Between
//! events a fleet costs nothing. Trip times come from lane distances and
//! the class's laden jump speed, plus a rough in-system estimate; there are
//! no flights here (those are the bubble's, see [`crate::bubble`]).
//!
//! Every movement of money and goods goes through [`crate::ledger`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use slotmap::{SecondaryMap, new_key_type};

use crate::economy::{Good, GoodMap};
use crate::ledger::{self, Account, Supply};
use crate::routes::{self, Visit};
use crate::settlement::{SettlementId, Site};
use crate::{EconomyModel, FactionId, GalaxyState, GalaxyTime, Id, Orbiting, Rng, SystemId};

pub type ShipClassId = Id<ShipClass>;

new_key_type! {
    /// Companies can go under, so they use generational keys.
    pub struct CompanyId;
}

/// A kind of trade ship, as the economy sees it. Generated from the ship
/// designs in `retrox-content`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShipClass {
    /// The hull's id in the ship content.
    pub hull: String,
    pub cargo_t: f64,
    /// Empty mass, which is also how much hull a shipyard must build.
    pub mass_t: f64,
    /// m/s², with a full hold.
    pub accel_laden: f64,
    /// Light-years per day along a hyperlane, with a full hold.
    pub jump_laden: f64,
    pub drive_tier: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Company {
    pub name: String,
    /// Where the crews live; upkeep is paid here.
    pub home: SettlementId,
    pub credits: f64,
    pub fleets: Vec<Fleet>,
    /// The id the next new fleet gets.
    pub next_fleet: u32,
}

/// Ships of one class that travel together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fleet {
    /// Stable within its company; events refer to fleets by it.
    pub id: u32,
    pub class: ShipClassId,
    pub ships: u32,
    pub at: Location,
    pub cargo: Option<Cargo>,
    pub route: Option<Route>,
    /// Its monthly review fell while it was away; it reviews on arrival.
    pub review_due: bool,
    /// Its last review found no route worth running. Idle fleets mean
    /// there are already more ships than freight.
    pub idle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Location {
    Docked(SettlementId),
    Travelling {
        to: SettlementId,
        arrive: GalaxyTime,
        /// Whether the trip goes through the lanes, where ships are lost.
        jumps: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cargo {
    pub good: Good,
    pub amount: f64,
}

/// A month's commitment: carry `good` from `from` to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub from: SettlementId,
    pub to: SettlementId,
    pub good: Good,
    /// One-way trip, in days, including handling.
    pub leg_days: u32,
    pub jumps: bool,
}

/// Something that will happen to a fleet on a given day.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub company: CompanyId,
    pub fleet: u32,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Arrive,
    Review,
}

/// Days of events the calendar holds ahead: longer than any trip and than
/// the review interval.
const HORIZON: usize = 64;

/// A ring of day buckets: each day's events, in the order they were
/// scheduled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calendar {
    buckets: Vec<Vec<Event>>,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            buckets: vec![Vec::new(); HORIZON],
        }
    }
}

impl Calendar {
    /// Schedule `event` for `day`, clamped to between tomorrow and the
    /// horizon.
    pub fn schedule(&mut self, today: GalaxyTime, day: GalaxyTime, event: Event) {
        let ahead = day
            .days()
            .saturating_sub(today.days())
            .clamp(1, HORIZON as u64 - 1);
        let slot = ((today.days() + ahead) % HORIZON as u64) as usize;
        self.buckets[slot].push(event);
    }

    /// Everything scheduled for `day`.
    pub fn take(&mut self, day: GalaxyTime) -> Vec<Event> {
        std::mem::take(&mut self.buckets[(day.days() % HORIZON as u64) as usize])
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(Vec::len).sum()
    }

    /// Every scheduled event, in no particular order.
    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.buckets.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Cargo on its way to each market, so traders choosing a route can see
/// what's already coming.
#[derive(Clone, Debug, Default)]
pub struct Traffic {
    inbound: HashMap<(SettlementId, Good), f64>,
}

impl Traffic {
    pub fn inbound(&self, to: SettlementId, good: Good) -> f64 {
        self.inbound.get(&(to, good)).copied().unwrap_or(0.0)
    }

    fn add(&mut self, to: SettlementId, good: Good, amount: f64) {
        let entry = self.inbound.entry((to, good)).or_default();
        *entry += amount;
        if *entry <= 1e-9 {
            self.inbound.remove(&(to, good));
        }
    }
}

// JSON maps need string keys, so traffic saves as a sorted list.
impl Serialize for Traffic {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<(SettlementId, Good, f64)> = self
            .inbound
            .iter()
            .map(|(&(to, g), &a)| (to, g, a))
            .collect();
        entries.sort_by_key(|a| (a.0, a.1));
        entries.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Traffic {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let entries: Vec<(SettlementId, Good, f64)> = Vec::deserialize(d)?;
        Ok(Self {
            inbound: entries.into_iter().map(|(to, g, a)| ((to, g), a)).collect(),
        })
    }
}

type Contracts = SecondaryMap<FactionId, bool>;

/// Each system's best buyer of every good, and its most valuable surplus:
/// what route choice reads instead of scanning every market. Rebuilt at the
/// start of every trade phase in one pass over settlements.
/// Only settled systems have an entry.
#[derive(Clone, Debug, Default)]
struct Board {
    systems: HashMap<SystemId, Listing>,
}

#[derive(Clone, Copy, Debug)]
struct Listing {
    /// The settlement paying the most for each good.
    buyers: GoodMap<Option<(SettlementId, f64)>>,
    /// The settlement whose surplus would fetch the most at base prices,
    /// with that surplus and its margin per unit.
    source: Option<(SettlementId, f64, f64)>,
}

impl Board {
    fn build(state: &GalaxyState) -> Self {
        let mut systems: HashMap<SystemId, Listing> =
            HashMap::with_capacity(state.settlements.len());
        for (id, s) in &state.settlements {
            let listing = systems.entry(s.system).or_insert(Listing {
                buyers: GoodMap::splat(None),
                source: None,
            });
            let market = &s.economy.market;
            for good in Good::ALL {
                let price = market.price[good];
                if listing.buyers[good].is_none_or(|(_, best)| price > best) {
                    listing.buyers[good] = Some((id, price));
                }
                let surplus = market.surplus(good);
                let margin = (good.base_price() - price).max(0.0);
                let value = surplus * margin;
                if value > 0.0 && listing.source.is_none_or(|(_, s, m)| value > s * m) {
                    listing.source = Some((id, surplus, margin));
                }
            }
        }
        Self { systems }
    }
}

/// Route choice looks at no more than this many settled systems, nearest
/// first.
const ROUTE_SYSTEMS: usize = 20;

/// Days between monthly reviews.
const REVIEW_DAYS: u64 = 30;
/// Trips longer than this aren't considered.
const MAX_TRIP_DAYS: f64 = 40.0;
/// Loading and unloading, added to every leg.
const HANDLING_DAYS: f64 = 1.0;
/// A route has to earn at least this many credits per day.
const MIN_PROFIT_PER_DAY: f64 = 1.0;
/// Fuel burned per ship per day of travel: a base plus a share of the hold.
const FUEL_PER_SHIP_DAY: f64 = 0.5;
const FUEL_PER_CARGO_T_DAY: f64 = 0.002;
/// Crew upkeep per ship per day, in credits, paid monthly.
const UPKEEP_PER_SHIP_DAY: f64 = 2.0;
const UPKEEP_PER_CARGO_T_DAY: f64 = 0.02;
/// A buyer spends at most this share of its treasury on one delivery.
const MAX_SPEND_SHARE: f64 = 0.5;
/// A company spends at most this share of its credits on one cargo.
const MAX_STAKE_SHARE: f64 = 0.9;
/// A company buys another ship when it holds this many times its price.
const BUY_SHIP_AT_PRICES: f64 = 4.0;
/// Chance a settlement with hull and money founds a company at its
/// monthly review, if no idle fleets sit in its port.
const FOUND_COMPANY_CHANCE: f64 = 0.25;
/// A new company starts with enough to fill its hold this many times.
const FOUNDING_HOLDS: f64 = 2.0;
const FOUNDING_CREDITS_PER_T: f64 = 20.0;
/// A settlement founds companies only from treasury above this many times
/// the cost of founding one.
const FOUNDING_WEALTH: f64 = 3.0;
/// Chance a ship is lost on any one trip through the lanes.
const LOSS_PER_SHIP_JUMP: f64 = 0.001;
/// Rough distance of a station from what it orbits, for travel estimates.
const STATION_ORBIT_AU: f64 = 0.001;

/// How far a settlement is from its star, in AU: its body's orbit (or its
/// planet's, for a moon). Sizes, not positions: good enough for coarse
/// travel times.
fn radius_au(state: &GalaxyState, id: SettlementId) -> f64 {
    let body_radius = |b: crate::BodyId| {
        let body = &state.bodies[b];
        match body.orbit.parent {
            Orbiting::Star => body.orbit.semi_major_au,
            Orbiting::Body(p) => state.bodies[p].orbit.semi_major_au,
        }
    };
    match state.settlements[id].site {
        Site::Region(r) => body_radius(state.regions[r].body),
        Site::Surface(b) => body_radius(b),
        Site::Space(orbit) => match orbit.parent {
            Orbiting::Star => orbit.semi_major_au,
            Orbiting::Body(b) => body_radius(b) + STATION_ORBIT_AU,
        },
    }
}

/// One-way days from `from` to `to` for `class`, given the lane distance
/// between their systems, including handling.
fn leg_days(
    state: &GalaxyState,
    class: &ShipClass,
    from: SettlementId,
    to: SettlementId,
    lane_ly: f64,
) -> f64 {
    let cruise = |au: f64| crate::bubble::cruise_days(au, class.accel_laden);
    let (ra, rb) = (radius_au(state, from), radius_au(state, to));
    let travel = if state.settlements[from].system == state.settlements[to].system {
        cruise((ra - rb).abs().max(STATION_ORBIT_AU))
    } else {
        let jp = crate::bubble::JUMP_POINT_AU;
        cruise((ra - jp).abs()) + lane_ly / class.jump_laden + cruise((rb - jp).abs())
    };
    travel + HANDLING_DAYS
}

/// Lane distances already worked out, by origin and range. The lane
/// network doesn't change, so [`reach`] remembers its answers. Not saved;
/// a loaded game starts with an empty cache. Clear it if lanes change.
#[derive(Default)]
pub struct ReachCache(RwLock<HashMap<(SystemId, u64), Reached>>);

/// Systems within range, with their lane distance, nearest first.
pub type Reached = Arc<[(SystemId, f64)]>;

impl ReachCache {
    pub fn clear(&self) {
        self.0.write().expect("reach cache poisoned").clear();
    }
}

impl Clone for ReachCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for ReachCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReachCache")
    }
}

/// Shortest lane distance from `origin` to every system within `max_ly`,
/// nearest first.
pub fn reach(state: &GalaxyState, origin: SystemId, max_ly: f64) -> Reached {
    let key = (origin, max_ly.to_bits());
    if let Some(hit) = state
        .reach_cache
        .0
        .read()
        .expect("reach cache poisoned")
        .get(&key)
    {
        return hit.clone();
    }
    let found: Reached = search(state, origin, max_ly).into();
    state
        .reach_cache
        .0
        .write()
        .expect("reach cache poisoned")
        .insert(key, found.clone());
    found
}

fn search(state: &GalaxyState, origin: SystemId, max_ly: f64) -> Vec<(SystemId, f64)> {
    let mut out = Vec::new();
    routes::search(
        state,
        origin,
        |lane| lane.length_ly,
        |step| {
            if step.cost > max_ly {
                return Visit::Stop;
            }
            out.push((step.system, step.cost));
            Visit::Expand
        },
    );
    out
}

fn model(state: &GalaxyState, id: SettlementId) -> EconomyModel {
    state.factions[state.settlements[id].owner].economy
}

fn capacity(state: &GalaxyState, fleet: &Fleet) -> f64 {
    state.ship_classes[fleet.class].cargo_t * f64::from(fleet.ships)
}

fn fuel_per_day(state: &GalaxyState, fleet: &Fleet) -> f64 {
    f64::from(fleet.ships) * FUEL_PER_SHIP_DAY + capacity(state, fleet) * FUEL_PER_CARGO_T_DAY
}

/// Add a company, giving its fleets ids and scheduling their first
/// reviews, spread over the month. Every company enters through here.
pub fn add_company(state: &mut GalaxyState, mut company: Company) -> CompanyId {
    for fleet in &mut company.fleets {
        fleet.id = company.next_fleet;
        company.next_fleet += 1;
    }
    let ids: Vec<u32> = company.fleets.iter().map(|f| f.id).collect();
    let key = state.companies.insert(company);
    let today = state.time;
    for id in ids {
        let entity = slotmap::Key::data(&key).as_ffi() ^ u64::from(id);
        let offset = Rng::stream(state.seed, "review-day", today.days(), entity)
            .below(REVIEW_DAYS as usize) as u64
            + 1;
        state.calendar.schedule(
            today,
            GalaxyTime::from_days(today.days() + offset),
            Event {
                company: key,
                fleet: id,
                kind: EventKind::Review,
            },
        );
    }
    key
}

fn fleet_index(state: &GalaxyState, company: CompanyId, fleet: u32) -> Option<usize> {
    state
        .companies
        .get(company)?
        .fleets
        .iter()
        .position(|f| f.id == fleet)
}

/// Trade phase: today's events, in the order they were scheduled, then
/// settlements' monthly founding of new companies. Prices update at the
/// end.
pub fn trade(state: &mut GalaxyState) {
    let today = state.time;
    let contracts = crate::fiscal::open_contracts(state);
    let board = Board::build(state);
    for event in state.calendar.take(today) {
        let Some(n) = fleet_index(state, event.company, event.fleet) else {
            continue;
        };
        match event.kind {
            EventKind::Arrive => arrive(state, &contracts, &board, event.company, n),
            EventKind::Review => review(state, &contracts, &board, event.company, n),
        }
    }
    found_companies(state);
    crate::economy::update_all_prices(state);
}

fn arrive(state: &mut GalaxyState, contracts: &Contracts, board: &Board, key: CompanyId, n: usize) {
    let today = state.time;
    let Location::Travelling { to, arrive, jumps } = state.companies[key].fleets[n].at else {
        return;
    };
    if arrive != today {
        return;
    }
    if let Some(cargo) = state.companies[key].fleets[n].cargo {
        state.traffic.add(to, cargo.good, -cargo.amount);
    }
    if jumps && !lose_ships(state, key, n) {
        return;
    }
    if !state.settlements.contains_key(to) {
        // Its destination is gone, and the fleet with it.
        remove_fleet(state, key, n);
        return;
    }
    state.companies[key].fleets[n].at = Location::Docked(to);
    sell(state, contracts, key, n, to);
    buy_ship(state, key, n, to);
    if state.companies[key].fleets[n].review_due {
        state.companies[key].fleets[n].review_due = false;
        choose_route(state, contracts, board, key, n, to);
    }
    run_route(state, contracts, key, n, to);
}

/// Roll for losses coming out of the lanes. Returns whether any of the
/// fleet survived.
fn lose_ships(state: &mut GalaxyState, key: CompanyId, n: usize) -> bool {
    let fleet = &state.companies[key].fleets[n];
    let entity = slotmap::Key::data(&key).as_ffi() ^ (u64::from(fleet.id) << 40);
    let mut rng = Rng::stream(state.seed, "ship-loss", state.time.days(), entity);
    let ships = fleet.ships;
    let survivors = (0..ships)
        .filter(|_| !rng.chance(LOSS_PER_SHIP_JUMP))
        .count() as u32;
    if survivors == ships {
        return true;
    }
    if survivors == 0 {
        remove_fleet(state, key, n);
        return false;
    }
    let fleet = &mut state.companies[key].fleets[n];
    // Lost ships take their share of the cargo with them.
    if let Some(cargo) = fleet.cargo.as_mut() {
        cargo.amount *= f64::from(survivors) / f64::from(ships);
    }
    fleet.ships = survivors;
    true
}

/// A fleet is gone. A company with no fleets left winds up, its money
/// going home.
fn remove_fleet(state: &mut GalaxyState, key: CompanyId, n: usize) {
    state.companies[key].fleets.remove(n);
    if !state.companies[key].fleets.is_empty() {
        return;
    }
    let home = state.companies[key].home;
    let credits = state.companies[key].credits;
    let heir = if state.settlements.contains_key(home) {
        Some(Account::Settlement(home))
    } else {
        state.factions.keys().next().map(Account::Faction)
    };
    if let Some(heir) = heir {
        ledger::pay(state, Account::Company(key), heir, credits);
    }
    state.companies.remove(key);
}

/// The monthly review: pay a month's upkeep, then choose a route if docked,
/// or on arrival if away.
fn review(state: &mut GalaxyState, contracts: &Contracts, board: &Board, key: CompanyId, n: usize) {
    let today = state.time;
    let event = Event {
        company: key,
        fleet: state.companies[key].fleets[n].id,
        kind: EventKind::Review,
    };
    state.calendar.schedule(
        today,
        GalaxyTime::from_days(today.days() + REVIEW_DAYS),
        event,
    );
    pay_upkeep(state, key, n);
    match state.companies[key].fleets[n].at {
        Location::Travelling { .. } => state.companies[key].fleets[n].review_due = true,
        Location::Docked(here) => {
            choose_route(state, contracts, board, key, n, here);
            run_route(state, contracts, key, n, here);
        }
    }
}

/// What a market will pay per unit for `amount` of `good` once what's
/// already coming has arrived, plus any contract premium.
fn expected_sale(
    state: &GalaxyState,
    contracts: &Contracts,
    to: SettlementId,
    good: Good,
    amount: f64,
) -> f64 {
    let market = &state.settlements[to].economy.market;
    let stock = market.stock[good] + state.traffic.inbound(to, good) + amount / 2.0;
    market.price_at(good, stock, model(state, to))
        + crate::fiscal::contract_premium(state, contracts, to, good, stock)
}

/// Pick the month's route for a fleet docked at `here`: the best margin per
/// day for a loaded leg out and an empty leg back. With nothing worth
/// carrying from here, it heads empty for the nearby port with the most
/// valuable surplus, and chooses again there.
fn choose_route(
    state: &mut GalaxyState,
    contracts: &Contracts,
    board: &Board,
    key: CompanyId,
    n: usize,
    here: SettlementId,
) {
    let company = &state.companies[key];
    let fleet = &company.fleets[n];
    let class = &state.ship_classes[fleet.class];
    let capacity = capacity(state, fleet);
    let origin = &state.settlements[here];
    let market = &origin.economy.market;
    let fuel = fuel_per_day(state, fleet) * market.price[Good::Fuel];
    let offers: Vec<(Good, f64, f64)> = Good::ALL
        .into_iter()
        .filter_map(|good| {
            let price = market.price[good];
            let amount = capacity
                .min(market.surplus(good))
                .min(company.credits.max(0.0) * MAX_STAKE_SHARE / price);
            (amount >= 1.0).then_some((good, amount, price))
        })
        .collect();

    let mut best: Option<(f64, Route)> = None;
    let mut fallback: Option<(f64, SettlementId, u32, bool)> = None;
    let mut considered = 0;
    for &(system, ly) in reach(state, origin.system, MAX_TRIP_DAYS * class.jump_laden).iter() {
        let Some(listing) = board.systems.get(&system) else {
            continue;
        };
        considered += 1;
        if considered > ROUTE_SYSTEMS {
            break;
        }
        let jumps = system != origin.system;
        // No leg to this system is shorter than its jump plus handling.
        let shortest = ly / class.jump_laden + HANDLING_DAYS;
        for &(good, amount, buy) in &offers {
            let Some((to, price)) = listing.buyers[good] else {
                continue;
            };
            if to == here {
                continue;
            }
            // Cheap bound first: today's price is at least what it will be
            // once more cargo arrives, and the premium is at most its cap.
            let owner = state.settlements[to].owner;
            let premium_cap = if contracts.get(owner).copied().unwrap_or(false) {
                crate::fiscal::max_premium(good)
            } else {
                0.0
            };
            let bound = amount * (price + premium_cap - buy) / (2.0 * shortest);
            if bound <= MIN_PROFIT_PER_DAY || best.as_ref().is_some_and(|(b, _)| bound <= *b) {
                continue;
            }
            let leg = leg_days(state, class, here, to, ly);
            if leg > MAX_TRIP_DAYS {
                continue;
            }
            let sell = expected_sale(state, contracts, to, good, amount);
            let treasury = state.settlements[to].economy.treasury.max(0.0);
            let amount = amount.min(treasury * MAX_SPEND_SHARE / sell.max(1e-9));
            if amount < 1.0 {
                continue;
            }
            let per_day = (amount * (sell - buy) - 2.0 * leg * fuel) / (2.0 * leg);
            if per_day > MIN_PROFIT_PER_DAY && best.as_ref().is_none_or(|(b, _)| per_day > *b) {
                let route = Route {
                    from: here,
                    to,
                    good,
                    leg_days: leg.ceil() as u32,
                    jumps,
                };
                best = Some((per_day, route));
            }
        }
        // Somewhere better to load? Rough guide: what the system's best
        // surplus would fetch at base prices.
        if let Some((to, surplus, margin)) = listing.source
            && to != here
            && best.is_none()
            && fallback
                .as_ref()
                .is_none_or(|(b, ..)| capacity.min(surplus) * margin / (2.0 * shortest) > *b)
        {
            let leg = leg_days(state, class, here, to, ly);
            let per_day = (capacity.min(surplus) * margin - leg * fuel) / (2.0 * leg);
            if leg <= MAX_TRIP_DAYS
                && per_day > MIN_PROFIT_PER_DAY
                && fallback.as_ref().is_none_or(|(b, ..)| per_day > *b)
            {
                fallback = Some((per_day, to, leg.ceil() as u32, jumps));
            }
        }
    }

    let fleet = &mut state.companies[key].fleets[n];
    match (best, fallback) {
        (Some((_, route)), _) => {
            fleet.route = Some(route);
            fleet.idle = false;
        }
        (None, Some((_, to, days, jumps))) => {
            fleet.route = None;
            fleet.idle = false;
            fleet.review_due = true;
            depart(state, key, n, here, to, days, jumps);
        }
        (None, None) => {
            fleet.route = None;
            fleet.idle = true;
        }
    }
}

/// Carry on with the route from `here`: load and go if this is where it
/// starts, go back empty if not. A leg that would lose money drops the
/// route until the next review.
fn run_route(
    state: &mut GalaxyState,
    contracts: &Contracts,
    key: CompanyId,
    n: usize,
    here: SettlementId,
) {
    let fleet = &state.companies[key].fleets[n];
    if !matches!(fleet.at, Location::Docked(_)) {
        return;
    }
    let Some(route) = fleet.route else {
        return;
    };
    if !state.settlements.contains_key(route.from) || !state.settlements.contains_key(route.to) {
        state.companies[key].fleets[n].route = None;
        return;
    }
    if here != route.from {
        // At the far end, or somewhere else: back to the start. Cargo it
        // couldn't sell stays aboard for next time.
        depart(state, key, n, here, route.from, route.leg_days, route.jumps);
        return;
    }
    if fleet.cargo.is_some() {
        // Leftovers from last time: take them to the route's market.
        depart(state, key, n, here, route.to, route.leg_days, route.jumps);
        return;
    }
    let capacity = capacity(state, fleet);
    let fuel =
        fuel_per_day(state, fleet) * state.settlements[here].economy.market.price[Good::Fuel];
    let company = Account::Company(key);
    let price = state.settlements[here].economy.market.price[route.good];
    let affordable = ledger::available(state, here, route.good, Supply::Surplus)
        .min(capacity)
        .min(ledger::spendable(state, company) * MAX_STAKE_SHARE / price);
    let sell = expected_sale(state, contracts, route.to, route.good, affordable);
    let margin = affordable * (sell - price) - 2.0 * f64::from(route.leg_days) * fuel;
    if affordable < 1.0 || margin <= 0.0 {
        state.companies[key].fleets[n].route = None;
        return;
    }
    let bought = ledger::buy(
        state,
        company,
        here,
        route.good,
        affordable,
        price,
        Supply::Surplus,
        1.0,
    );
    state.companies[key].fleets[n].cargo = Some(Cargo {
        good: route.good,
        amount: bought,
    });
    depart(state, key, n, here, route.to, route.leg_days, route.jumps);
}

/// Buy fuel for the trip and set off; schedule the arrival.
fn depart(
    state: &mut GalaxyState,
    key: CompanyId,
    n: usize,
    here: SettlementId,
    to: SettlementId,
    days: u32,
    jumps: bool,
) {
    let today = state.time;
    let fuel = fuel_per_day(state, &state.companies[key].fleets[n]) * f64::from(days);
    let fuel_price = state.settlements[here].economy.market.price[Good::Fuel];
    // What the port can't supply, or the company can't pay for, is burned
    // from the ships' own reserves.
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
    let arrive = GalaxyTime::from_days(today.days() + u64::from(days.max(1)));
    let fleet = &mut state.companies[key].fleets[n];
    fleet.at = Location::Travelling { to, arrive, jumps };
    let event = Event {
        company: key,
        fleet: fleet.id,
        kind: EventKind::Arrive,
    };
    if let Some(cargo) = fleet.cargo {
        state.traffic.add(to, cargo.good, cargo.amount);
    }
    state.calendar.schedule(today, arrive, event);
}

/// Sell as much cargo as the market can afford: the settlement pays the
/// price, its faction any contract premium.
fn sell(
    state: &mut GalaxyState,
    contracts: &Contracts,
    key: CompanyId,
    n: usize,
    here: SettlementId,
) {
    let Some(cargo) = state.companies[key].fleets[n].cargo else {
        return;
    };
    let market = &state.settlements[here].economy.market;
    let stock = market.stock[cargo.good] + cargo.amount / 2.0;
    let price = market.price_at(cargo.good, stock, model(state, here));
    let premium = crate::fiscal::contract_premium(state, contracts, here, cargo.good, stock);
    let owner = state.settlements[here].owner;
    let amount = ledger::sell(
        state,
        Account::Company(key),
        here,
        cargo.good,
        cargo.amount,
        price,
        MAX_SPEND_SHARE,
    );
    if amount <= 0.0 {
        return;
    }
    ledger::pay(
        state,
        Account::Faction(owner),
        Account::Company(key),
        amount * premium,
    );
    let left = cargo.amount - amount;
    state.companies[key].fleets[n].cargo = (left > 1e-6).then_some(Cargo {
        good: cargo.good,
        amount: left,
    });
}

/// A prosperous company with no idle fleets, docked at a shipyard, adds a
/// ship to the fleet.
fn buy_ship(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) {
    let company = &state.companies[key];
    if company.fleets.iter().any(|f| f.idle) {
        return;
    }
    let mass = state.ship_classes[company.fleets[n].class].mass_t;
    let price = mass * crate::economy::HULL_PRICE_PER_T;
    if company.credits < price * BUY_SHIP_AT_PRICES || state.settlements[here].economy.hull_t < mass
    {
        return;
    }
    let paid = ledger::pay(
        state,
        Account::Company(key),
        Account::Settlement(here),
        price,
    );
    debug_assert_eq!(paid, price);
    state.settlements[here].economy.hull_t -= mass;
    state.companies[key].fleets[n].ships += 1;
}

/// A month of crew upkeep for one fleet, paid at home.
fn pay_upkeep(state: &mut GalaxyState, key: CompanyId, n: usize) {
    let company = &state.companies[key];
    let fleet = &company.fleets[n];
    let class = &state.ship_classes[fleet.class];
    let per_day =
        f64::from(fleet.ships) * (UPKEEP_PER_SHIP_DAY + class.cargo_t * UPKEEP_PER_CARGO_T_DAY);
    let home = company.home;
    ledger::pay(
        state,
        Account::Company(key),
        Account::Settlement(home),
        per_day * REVIEW_DAYS as f64,
    );
}

/// At their monthly review, settlements with hull to sell and money to
/// spare may found a company with one ship of the biggest class they can
/// build, unless idle fleets already sit in their port.
fn found_companies(state: &mut GalaxyState) {
    let today = state.time;
    let day = today.days() % REVIEW_DAYS;
    let reviewing: Vec<SettlementId> = state
        .settlements
        .iter()
        .filter(|(id, s)| {
            s.economy.hull_t > 0.0 && slotmap::Key::data(id).as_ffi() % REVIEW_DAYS == day
        })
        .map(|(id, _)| id)
        .collect();
    if reviewing.is_empty() {
        return;
    }
    let idle_ports: HashSet<SettlementId> = state
        .companies
        .values()
        .flat_map(|c| c.fleets.iter())
        .filter_map(|f| match f.at {
            Location::Docked(at) if f.idle => Some(at),
            _ => None,
        })
        .collect();
    for id in reviewing {
        let entity = slotmap::Key::data(&id).as_ffi();
        if idle_ports.contains(&id)
            || !Rng::stream(state.seed, "found-company", today.days(), entity)
                .chance(FOUND_COMPANY_CHANCE)
        {
            continue;
        }
        let economy = &state.settlements[id].economy;
        let cost = |c: &ShipClass| {
            c.mass_t * crate::economy::HULL_PRICE_PER_T
                + c.cargo_t * FOUNDING_HOLDS * FOUNDING_CREDITS_PER_T
        };
        let Some((class, founding)) = state
            .ship_classes
            .iter()
            .filter(|(_, c)| {
                c.mass_t <= economy.hull_t && cost(c) * FOUNDING_WEALTH <= economy.treasury
            })
            .max_by(|a, b| a.1.cargo_t.total_cmp(&b.1.cargo_t))
            .map(|(cid, c)| (cid, cost(c)))
        else {
            continue;
        };
        let mass = state.ship_classes[class].mass_t;
        let base = state.settlements[id]
            .name
            .split(' ')
            .next()
            .unwrap_or("Free")
            .to_owned();
        state.settlements[id].economy.hull_t -= mass;
        let company = add_company(
            state,
            Company {
                name: format!("{base} Haulers {}", today.year()),
                home: id,
                credits: 0.0,
                fleets: vec![Fleet {
                    id: 0,
                    class,
                    ships: 1,
                    at: Location::Docked(id),
                    cargo: None,
                    route: None,
                    review_due: false,
                    idle: false,
                }],
                next_fleet: 0,
            },
        );
        // The settlement stakes the company, which buys its hull from the
        // same yard.
        let (home, company) = (Account::Settlement(id), Account::Company(company));
        ledger::pay(state, home, company, founding);
        ledger::pay(
            state,
            company,
            home,
            mass * crate::economy::HULL_PRICE_PER_T,
        );
    }
}

/// Goods held anywhere: in markets and aboard ships.
pub fn total_goods(state: &GalaxyState) -> GoodMap<f64> {
    let mut total = GoodMap::splat(0.0);
    for s in state.settlements.values() {
        for good in Good::ALL {
            total[good] += s.economy.market.stock[good];
        }
    }
    for company in state.companies.values() {
        for fleet in &company.fleets {
            if let Some(cargo) = fleet.cargo {
                total[cargo.good] += cargo.amount;
            }
        }
    }
    total
}
