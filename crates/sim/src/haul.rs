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
//! no flights here. Near the player, fleets fly for real instead: see
//! [`crate::bubble`], which hands them back here when they leave.
//!
//! Every movement of money and goods goes through [`crate::ledger`].

use std::collections::HashMap;
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
    /// Whether it has taken on cargo since its last review.
    #[serde(default)]
    pub loaded: bool,
    /// Reviews in a row it has carried nothing: ships with no freight to
    /// carry are sold for scrap (see [`SLACK_REVIEWS`]).
    #[serde(default)]
    pub slack: u32,
    /// Set for harvesters, which gather goods instead of buying them; see
    /// [`crate::harvest`].
    #[serde(default)]
    pub harvest: Option<crate::harvest::Harvest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Location {
    Docked(SettlementId),
    Travelling {
        /// Where the trip began, and when: what the bubble needs to place
        /// the fleet along it.
        #[serde(default)]
        from: SettlementId,
        #[serde(default)]
        depart: GalaxyTime,
        to: SettlementId,
        arrive: GalaxyTime,
        /// Whether the trip goes through the lanes, where ships are lost.
        jumps: bool,
    },
    /// Flying for real in the bubble, bound for `to`; the trip itself is
    /// in [`crate::bubble::Bubble::flights`].
    Flying {
        to: SettlementId,
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
    /// A trip reaches the bubble and starts flying for real.
    Enter,
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

    pub(crate) fn add(&mut self, to: SettlementId, good: Good, amount: f64) {
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

pub(crate) type Contracts = SecondaryMap<FactionId, f64>;

/// What traders read when they choose a route (DESIGN §5.1.1). Two
/// layers:
///
/// - **Today's listings:** each settled system's best buyer of every good,
///   and its most valuable surplus. Rebuilt at the start of every trade
///   phase in one pass over settlements, in place, so the pass allocates
///   nothing. Not saved.
/// - **Trade lists:** for each settled system, the best few buyers of
///   every good and the best few surpluses among its nearest settled
///   systems, with their lane distances. Each system refreshes its lists
///   from the listings once a month, on its own day, and a trader picks
///   from them instead of searching. Saved, since they depend on the
///   prices of the day they were made.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Board {
    /// Today's listings, by system index.
    #[serde(skip)]
    systems: Vec<Option<Listing>>,
    /// Trade lists, by system index.
    #[serde(default)]
    lists: Vec<Option<TradeLists>>,
}

#[derive(Clone, Copy, Debug)]
struct Listing {
    /// The settlement paying the most for each good, and that price.
    /// Single precision: listings only rank, and kept small, a month's
    /// trade lists read fewer cache lines.
    buyers: GoodMap<Option<(SettlementId, f32)>>,
    /// The settlement whose surplus would fetch the most at base prices,
    /// with the good, and what it would fetch.
    source: Option<(SettlementId, Good, f32)>,
}

/// Buyers listed per good, surpluses and shipyards listed, in a system's
/// trade lists.
const LISTED_BUYERS: usize = 4;
const LISTED_SOURCES: usize = 4;
const LISTED_YARDS: usize = 4;

/// A system's trade lists; see [`Board`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct TradeLists {
    /// For each good, the nearby settlements paying most for it, best
    /// first.
    wanted: GoodMap<[Option<Listed>; LISTED_BUYERS]>,
    /// The nearby surpluses that would fetch most at base prices, best
    /// first.
    sources: [Option<Listed>; LISTED_SOURCES],
    /// The nearest settlements with a shipyard, nearest first: where new
    /// crews fetch hulls, and where orders for them go.
    #[serde(default)]
    yards: [Option<Listed>; LISTED_YARDS],
    /// The system's own settlements with a shipyard, as of these lists:
    /// what its neighbours' lists read, rather than every neighbour's
    /// facilities.
    #[serde(default)]
    own_yards: [Option<SettlementId>; LISTED_YARDS],
}

/// A settlement on a trade list, with how far it is.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Listed {
    at: SettlementId,
    /// Lane distance from the list's system, and the lanes crossed.
    ly: f64,
    jumps: u32,
    /// What it ranked by when listed: price for a buyer, value at base
    /// prices for a surplus.
    rank: f64,
}

/// Put `entry` into `list`, kept best first; ties keep the nearer.
fn insert_ranked<const N: usize>(list: &mut [Option<Listed>; N], entry: Listed) {
    // Most candidates don't make a full list: check the last place first.
    if list[N - 1].is_some_and(|last| entry.rank <= last.rank) {
        return;
    }
    let Some(at) = list
        .iter()
        .position(|e| e.is_none_or(|e| entry.rank > e.rank))
    else {
        return;
    };
    list[at..].rotate_right(1);
    list[at] = Some(entry);
}

impl Board {
    fn rebuild(&mut self, state: &GalaxyState) {
        self.systems.clear();
        self.systems.resize(state.systems.len(), None);
        for (id, s) in state.settlements.iter() {
            let listing = self.systems[s.system.index()].get_or_insert(Listing {
                buyers: GoodMap::splat(None),
                source: None,
            });
            let market = &s.economy.market;
            for good in Good::ALL {
                let price = market.price[good] as f32;
                if listing.buyers[good].is_none_or(|(_, best)| price > best) {
                    listing.buyers[good] = Some((id, price));
                }
                let surplus = market.surplus(good);
                let margin = (good.base_price() - market.price[good]).max(0.0);
                let value = (surplus * margin) as f32;
                if value > 0.0 && listing.source.is_none_or(|(_, _, v)| value > v) {
                    listing.source = Some((id, good, value));
                }
            }
        }
    }

    fn get(&self, system: SystemId) -> Option<&Listing> {
        self.systems.get(system.index())?.as_ref()
    }

    /// Monthly: the settled systems whose day it is refresh their trade
    /// lists from today's listings. A stride through the systems, like the
    /// settlements' reviews. Systems in the bubble refresh daily, so
    /// traders near the player answer the markets the player is changing.
    fn refresh_lists(&mut self, state: &GalaxyState) {
        let n = state.systems.len();
        if self.lists.len() != n {
            self.lists.resize(n, None);
        }
        let first = (state.time.days() % REVIEW_DAYS) as usize;
        let due = (first..n).step_by(REVIEW_DAYS as usize);
        let near_player = state.bubble.systems.iter().map(|s| s.index());
        for i in due.chain(near_player) {
            let system = SystemId::from_index(i as u32);
            self.lists[i] = if self.systems[i].is_some() {
                Some(crate::decisions::time(
                    crate::decisions::Decision::TradeLists,
                    || self.make_lists(state, system),
                ))
            } else {
                None
            };
        }
    }

    /// `system`'s trade lists as of today's listings.
    fn make_lists(&self, state: &GalaxyState, system: SystemId) -> TradeLists {
        let mut lists = TradeLists::default();
        let mut considered = 0;
        for &(near, ly, jumps) in reach(state, system, listing_reach_ly(state)).iter() {
            let Some(listing) = self.get(near) else {
                continue;
            };
            considered += 1;
            if considered > ROUTE_SYSTEMS {
                break;
            }
            for good in Good::ALL {
                if let Some((at, price)) = listing.buyers[good] {
                    let entry = Listed {
                        at,
                        ly,
                        jumps,
                        rank: f64::from(price),
                    };
                    insert_ranked(&mut lists.wanted[good], entry);
                }
            }
            if let Some((at, _, value)) = listing.source {
                let entry = Listed {
                    at,
                    ly,
                    jumps,
                    rank: f64::from(value),
                };
                insert_ranked(&mut lists.sources, entry);
            }
            // Nearest first already, so yards just fill up in order.
            let theirs = match self.lists.get(near.index()) {
                Some(Some(l)) if near != system => l.own_yards,
                _ => own_yards(state, near),
            };
            if near == system {
                lists.own_yards = theirs;
            }
            let free = lists.yards.iter().filter(|y| y.is_none()).count();
            let start = LISTED_YARDS - free;
            for (slot, at) in lists.yards[start..]
                .iter_mut()
                .zip(theirs.into_iter().flatten())
            {
                *slot = Some(Listed {
                    at,
                    ly,
                    jumps,
                    rank: -ly,
                });
            }
        }
        lists
    }

    /// `system`'s trade lists, or, for a system settled since its day came
    /// round, ones made from today's listings.
    fn lists(&self, state: &GalaxyState, system: SystemId) -> std::borrow::Cow<'_, TradeLists> {
        match self.lists.get(system.index()) {
            Some(Some(lists)) => std::borrow::Cow::Borrowed(lists),
            _ => std::borrow::Cow::Owned(self.make_lists(state, system)),
        }
    }
}

/// The settlements in `system` with a shipyard, as many as a list holds.
fn own_yards(state: &GalaxyState, system: SystemId) -> [Option<SettlementId>; LISTED_YARDS] {
    let mut out = [None; LISTED_YARDS];
    let yards = state.settlements_in(system).filter(|(_, s)| {
        s.economy
            .facilities
            .iter()
            .any(|f| f.kind == crate::economy::FacilityKind::Shipyard)
    });
    for (slot, (id, _)) in out.iter_mut().zip(yards) {
        *slot = Some(id);
    }
    out
}

/// How far trade lists look: as far as the longest-legged class goes in a
/// trip. Each trader keeps to its own class's range.
fn listing_reach_ly(state: &GalaxyState) -> f64 {
    let fastest = state
        .ship_classes
        .iter()
        .map(|(_, c)| c.jump_laden)
        .fold(0.0, f64::max);
    MAX_TRIP_DAYS * fastest
}

/// Route choice looks at no more than this many settled systems, nearest
/// first.
const ROUTE_SYSTEMS: usize = 20;

/// Days between monthly reviews.
const REVIEW_DAYS: u64 = 30;
/// Trips longer than this aren't considered.
pub(crate) const MAX_TRIP_DAYS: f64 = 40.0;
/// Loading and unloading, added to every leg.
pub(crate) const HANDLING_DAYS: f64 = 1.0;
/// A route has to earn at least this many credits per day.
const MIN_PROFIT_PER_DAY: f64 = 1.0;
/// Fuel burned per ship per day of travel: a base plus a share of the hold.
const FUEL_PER_SHIP_DAY: f64 = 0.5;
const FUEL_PER_CARGO_T_DAY: f64 = 0.002;
/// Crew upkeep per ship per day, in credits, paid monthly.
pub(crate) const UPKEEP_PER_SHIP_DAY: f64 = 2.0;
pub(crate) const UPKEEP_PER_CARGO_T_DAY: f64 = 0.02;
/// A buyer spends at most this share of its treasury on one delivery.
const MAX_SPEND_SHARE: f64 = 0.5;
/// A company spends at most this share of its credits on one cargo.
const MAX_STAKE_SHARE: f64 = 0.9;
/// A company buys another ship when it holds this many times its price,
/// and the fleet came in with its holds at least this full.
const BUY_SHIP_AT_PRICES: f64 = 4.0;
const FULL_HOLDS: f64 = 0.9;
/// A fleet that has carried nothing for this many reviews in a row sells
/// half its ships for scrap at the next port: a share of their mass, as
/// metal.
pub const SLACK_REVIEWS: u32 = 3;
const SCRAP_METAL_SHARE: f64 = 0.5;
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
pub(crate) const STATION_ORBIT_AU: f64 = 0.001;

/// How far a settlement is from its star, in AU: its body's orbit (or its
/// planet's, for a moon). Sizes, not positions: good enough for coarse
/// travel times.
pub(crate) fn radius_au(state: &GalaxyState, id: SettlementId) -> f64 {
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
/// between their systems and the lanes crossed, including handling: the
/// burn out to a jump point, each lane, each crossing of a system on the
/// way, the burn in. In-system distances are typical ones, since where
/// things are along their orbits changes; see [`crate::bubble`].
fn leg_days(
    state: &GalaxyState,
    class: &ShipClass,
    from: SettlementId,
    to: SettlementId,
    lane_ly: f64,
    jumps: u32,
) -> f64 {
    use crate::bubble::{STAGE_ROUNDING_DAYS, crossing_days, jump_point_days, local_days};
    let travel = if state.settlements[from].system == state.settlements[to].system {
        local_days(state, class, from, to)
    } else {
        let lanes = f64::from(jumps.max(1));
        let crossings = lanes - 1.0;
        let stages = 2.0 + lanes + crossings;
        jump_point_days(state, class, from)
            + lane_ly / class.jump_laden
            + crossings * crossing_days(class)
            + jump_point_days(state, class, to)
            // Lanes take whole days, and every change of stretch waits
            // for the next.
            + lanes * 0.5
            + (stages - 1.0) * STAGE_ROUNDING_DAYS
    };
    travel + HANDLING_DAYS
}

/// Planning estimate of a one-way trip for ships of `class`, in days,
/// including handling; `None` if it's further than any trip a trader
/// would make.
pub fn trip_days(
    state: &GalaxyState,
    class: ShipClassId,
    from: SettlementId,
    to: SettlementId,
) -> Option<f64> {
    let c = &state.ship_classes[class];
    let target = state.settlements.get(to)?.system;
    let origin = state.settlements.get(from)?.system;
    let &(_, ly, jumps) = reach(state, origin, MAX_TRIP_DAYS * c.jump_laden)
        .iter()
        .find(|r| r.0 == target)?;
    Some(leg_days(state, c, from, to, ly, jumps))
}

/// Lane distances already worked out, by origin and range. The lane
/// network doesn't change, so [`reach`] remembers its answers. Not saved;
/// a loaded game starts with an empty cache. Clear it if lanes change.
#[derive(Default)]
pub struct ReachCache(RwLock<HashMap<(SystemId, u64), Reached>>);

/// Systems within range, with their lane distance and the lanes crossed
/// to get there, nearest first.
pub type Reached = Arc<[(SystemId, f64, u32)]>;

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

fn search(state: &GalaxyState, origin: SystemId, max_ly: f64) -> Vec<(SystemId, f64, u32)> {
    let mut out = Vec::new();
    routes::search(
        state,
        origin,
        |lane| lane.length_ly,
        |step| {
            if step.cost > max_ly {
                return Visit::Stop;
            }
            out.push((step.system, step.cost, step.jumps));
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

pub(crate) fn fuel_per_day(state: &GalaxyState, fleet: &Fleet) -> f64 {
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

pub(crate) fn fleet_index(state: &GalaxyState, company: CompanyId, fleet: u32) -> Option<usize> {
    state
        .companies
        .get(company)?
        .fleets
        .iter()
        .position(|f| f.id == fleet)
}

/// Trade phase: today's events, in the order they were scheduled, then
/// settlements' monthly founding of new companies. Prices update at the
/// end of the day ([`crate::phases::prices`]).
pub fn trade(state: &mut GalaxyState) {
    let today = state.time;
    let contracts = crate::fiscal::open_contracts(state);
    let mut board = std::mem::take(&mut state.board);
    board.rebuild(state);
    board.refresh_lists(state);
    for event in state.calendar.take(today) {
        let Some(n) = fleet_index(state, event.company, event.fleet) else {
            continue;
        };
        match event.kind {
            EventKind::Arrive => arrive(state, &contracts, &board, event.company, n),
            EventKind::Review => review(state, &contracts, &board, event.company, n),
            EventKind::Enter => crate::bubble::enter(state, (event.company, event.fleet)),
        }
    }
    crate::bubble::step(state, &contracts, &board);
    state.board = board;
    crate::pirates::step(state);
    found_companies(state);
    crate::harvest::found(state);
}

fn arrive(state: &mut GalaxyState, contracts: &Contracts, board: &Board, key: CompanyId, n: usize) {
    let today = state.time;
    let Location::Travelling {
        from,
        to,
        arrive,
        jumps,
        ..
    } = state.companies[key].fleets[n].at
    else {
        return;
    };
    if arrive != today {
        return;
    }
    if !crate::pirates::coarse_raid(state, key, n, from, to) {
        return;
    }
    if jumps && !roll_losses(state, key, n, to) {
        return;
    }
    crate::harvest::fill(state, key, n);
    dock(state, contracts, board, key, n, to, false);
}

/// A fleet reaches `to`, in either tier: sell, maybe buy a ship, and carry
/// on. `replan` chooses a fresh route here whether or not a review is due,
/// which is how fleets in the bubble trade.
pub(crate) fn dock(
    state: &mut GalaxyState,
    contracts: &Contracts,
    board: &Board,
    key: CompanyId,
    n: usize,
    to: SettlementId,
    replan: bool,
) {
    if let Some(cargo) = state.companies[key].fleets[n].cargo {
        state.traffic.add(to, cargo.good, -cargo.amount);
    }
    if !state.settlements.contains_key(to) {
        // Its destination is gone, and the fleet with it.
        remove_fleet(state, key, n);
        return;
    }
    state.companies[key].fleets[n].at = Location::Docked(to);
    let fleet = &state.companies[key].fleets[n];
    let full = fleet
        .cargo
        .is_some_and(|c| c.amount >= FULL_HOLDS * capacity(state, fleet));
    sell(state, contracts, key, n, to);
    buy_ship(state, board, key, n, to, full);
    let review_due = state.companies[key].fleets[n].review_due;
    if review_due && retire(state, key, n, to) {
        return;
    }
    if crate::harvest::is_harvester(state, key, n) {
        if replan || review_due {
            state.companies[key].fleets[n].review_due = false;
            crate::harvest::review(state, key, n, to);
        }
        crate::harvest::run(state, key, n, to);
        return;
    }
    if replan || review_due {
        state.companies[key].fleets[n].review_due = false;
        choose_route(state, contracts, board, key, n, to);
    }
    run_route(state, contracts, key, n, to);
}

/// Roll for losses coming out of the lanes on the way to `to`, keeping
/// [`Traffic`] in step with the cargo that's left. Returns whether any of
/// the fleet survived.
pub(crate) fn roll_losses(
    state: &mut GalaxyState,
    key: CompanyId,
    n: usize,
    to: SettlementId,
) -> bool {
    let before = state.companies[key].fleets[n].cargo;
    let survived = lose_ships(state, key, n);
    if let Some(cargo) = before {
        let left = if survived {
            state.companies[key].fleets[n]
                .cargo
                .map_or(0.0, |c| c.amount)
        } else {
            0.0
        };
        state.traffic.add(to, cargo.good, left - cargo.amount);
    }
    survived
}

/// One ship of `fleet` is destroyed (the player shot it down): it takes
/// its share of the cargo with it. Returns what it was carrying.
pub fn ship_destroyed(state: &mut GalaxyState, company: CompanyId, fleet: u32) -> Option<Cargo> {
    let n = fleet_index(state, company, fleet)?;
    let to = match state.companies[company].fleets[n].at {
        Location::Docked(s) => s,
        Location::Travelling { to, .. } | Location::Flying { to } => to,
    };
    lose_one_ship(state, company, n, to).0
}

/// One ship of fleet `n`, bound for `to`, is gone with its share of the
/// cargo (shot down, or taken by pirates). Returns that share, and whether
/// any of the fleet is left.
pub(crate) fn lose_one_ship(
    state: &mut GalaxyState,
    company: CompanyId,
    n: usize,
    to: SettlementId,
) -> (Option<Cargo>, bool) {
    let f = &state.companies[company].fleets[n];
    let lost = f.cargo.map(|c| Cargo {
        good: c.good,
        amount: c.amount / f64::from(f.ships.max(1)),
    });
    if f.ships <= 1 {
        lose_fleet(state, company, n, to);
        return (lost, false);
    }
    let f = &mut state.companies[company].fleets[n];
    f.ships -= 1;
    if let (Some(c), Some(l)) = (f.cargo.as_mut(), lost) {
        c.amount -= l.amount;
        state.traffic.add(to, l.good, -l.amount);
    }
    (lost, true)
}

/// A fleet bound for `to` is lost with everything aboard.
pub(crate) fn lose_fleet(state: &mut GalaxyState, key: CompanyId, n: usize, to: SettlementId) {
    if let Some(cargo) = state.companies[key].fleets[n].cargo {
        state.traffic.add(to, cargo.good, -cargo.amount);
    }
    remove_fleet(state, key, n);
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
    let fleet = state.companies[key].fleets.remove(n);
    state.bubble.flights.remove(&(key, fleet.id));
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
    let fleet = &mut state.companies[key].fleets[n];
    // Carrying something now, or loaded since last time, is work.
    let working = fleet.loaded || fleet.cargo.is_some();
    fleet.slack = if working { 0 } else { fleet.slack + 1 };
    fleet.loaded = false;
    match state.companies[key].fleets[n].at {
        Location::Travelling { .. } | Location::Flying { .. } => {
            state.companies[key].fleets[n].review_due = true;
        }
        Location::Docked(here) => {
            if retire(state, key, n, here) {
                return;
            }
            if crate::harvest::is_harvester(state, key, n) {
                crate::harvest::review(state, key, n, here);
                crate::harvest::run(state, key, n, here);
                return;
            }
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
    crate::decisions::time(crate::decisions::Decision::Route, || {
        choose_route_timed(state, contracts, board, key, n, here);
    });
}

fn choose_route_timed(
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
    // Cargo still aboard comes first: the route is wherever it sells
    // best, and what it cost is already spent.
    let aboard = fleet.cargo;
    // What it could load, as (good, amount, price): on the stack, since
    // there are few goods.
    let mut offers = [(Good::Ore, 0.0, 0.0); Good::COUNT];
    let mut offered = 0;
    match aboard {
        Some(cargo) => {
            offers[0] = (cargo.good, cargo.amount, 0.0);
            offered = 1;
        }
        None => {
            for good in Good::ALL {
                let price = market.price[good];
                let amount = capacity
                    .min(market.surplus(good))
                    .min(company.credits.max(0.0) * MAX_STAKE_SHARE / price);
                if amount >= 1.0 {
                    offers[offered] = (good, amount, price);
                    offered += 1;
                }
            }
        }
    }
    let offers = &offers[..offered];

    let mut best: Option<(f64, Route)> = None;
    let mut fallback: Option<(f64, SettlementId, u32, bool)> = None;
    let lists = board.lists(state, origin.system);
    let range_ly = MAX_TRIP_DAYS * class.jump_laden;
    for &(good, amount, buy) in offers {
        for listed in lists.wanted[good].iter().flatten() {
            let to = listed.at;
            if to == here || listed.ly > range_ly {
                continue;
            }
            let Some(dest) = state.settlements.get(to) else {
                continue;
            };
            // No leg there is shorter than its jump plus handling.
            let shortest = listed.ly / class.jump_laden + HANDLING_DAYS;
            // Cheap bound first: today's price is at least what it will be
            // once more cargo arrives, and the premium is at most its cap.
            let price = dest.economy.market.price[good];
            let premium_cap = crate::fiscal::max_premium(good)
                * contracts.get(dest.owner).copied().unwrap_or(0.0);
            let bound = amount * (price + premium_cap - buy) / (2.0 * shortest);
            if bound <= MIN_PROFIT_PER_DAY || best.as_ref().is_some_and(|(b, _)| bound <= *b) {
                continue;
            }
            let leg = leg_days(state, class, here, to, listed.ly, listed.jumps);
            if leg > MAX_TRIP_DAYS {
                continue;
            }
            let sell = expected_sale(state, contracts, to, good, amount);
            let treasury = dest.economy.treasury.max(0.0);
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
                    jumps: dest.system != origin.system,
                };
                best = Some((per_day, route));
            }
        }
    }
    // Nothing worth carrying from here: somewhere better to load? Rough
    // guide: what the surplus there would fetch at base prices today.
    if best.is_none() && aboard.is_none() {
        for listed in lists.sources.iter().flatten() {
            let to = listed.at;
            if to == here || listed.ly > range_ly {
                continue;
            }
            let Some(at) = state.settlements.get(to) else {
                continue;
            };
            let value = Good::ALL
                .into_iter()
                .map(|g| {
                    let m = &at.economy.market;
                    capacity.min(m.surplus(g)) * (g.base_price() - m.price[g]).max(0.0)
                })
                .fold(0.0, f64::max);
            let shortest = listed.ly / class.jump_laden + HANDLING_DAYS;
            if fallback
                .as_ref()
                .is_some_and(|(b, ..)| value / (2.0 * shortest) <= *b)
            {
                continue;
            }
            let leg = leg_days(state, class, here, to, listed.ly, listed.jumps);
            let per_day = (value - leg * fuel) / (2.0 * leg);
            if leg <= MAX_TRIP_DAYS
                && per_day > MIN_PROFIT_PER_DAY
                && fallback.as_ref().is_none_or(|(b, ..)| per_day > *b)
            {
                fallback = Some((per_day, to, leg.ceil() as u32, at.system != origin.system));
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
            // Nobody in reach can buy what's aboard: leave it at the dock
            // for nothing rather than carry it about for ever.
            if let Some(cargo) = aboard {
                let left = ledger::sell(
                    state,
                    Account::Company(key),
                    here,
                    cargo.good,
                    cargo.amount,
                    0.0,
                    1.0,
                );
                debug_assert!((left - cargo.amount).abs() < 1e-9);
                state.companies[key].fleets[n].cargo = None;
            }
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
    let fleet = &mut state.companies[key].fleets[n];
    fleet.cargo = Some(Cargo {
        good: route.good,
        amount: bought,
    });
    fleet.loaded = true;
    depart(state, key, n, here, route.to, route.leg_days, route.jumps);
}

/// Buy fuel for the trip and set off; schedule the arrival.
pub(crate) fn depart(
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
    let fleet = &state.companies[key].fleets[n];
    let id = fleet.id;
    if let Some(cargo) = fleet.cargo {
        state.traffic.add(to, cargo.good, cargo.amount);
    }
    // Setting out from inside the bubble: fly it.
    if crate::bubble::contains(state, state.settlements[here].system)
        && crate::bubble::launch(state, (key, id), here, to, jumps)
    {
        return;
    }
    let arrive = GalaxyTime::from_days(today.days() + u64::from(days.max(1)));
    state.companies[key].fleets[n].at = Location::Travelling {
        from: here,
        depart: today,
        to,
        arrive,
        jumps,
    };
    // A trip that passes through the bubble enters it on the way; that
    // event goes first, so it runs before an arrival on the same day.
    crate::bubble::watch(state, (key, id));
    let event = Event {
        company: key,
        fleet: id,
        kind: EventKind::Arrive,
    };
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

/// A prosperous company with no idle fleets, whose fleet came in full, adds
/// a ship to it if the port has the hull; if not, the port hears that hull
/// is wanted, which is what shipyards build to.
fn buy_ship(
    state: &mut GalaxyState,
    board: &Board,
    key: CompanyId,
    n: usize,
    here: SettlementId,
    full: bool,
) {
    let company = &state.companies[key];
    if !full || company.fleets.iter().any(|f| f.idle || f.slack > 0) {
        return;
    }
    let mass = state.ship_classes[company.fleets[n].class].mass_t;
    let price = mass * crate::economy::HULL_PRICE_PER_T;
    if company.credits < price * BUY_SHIP_AT_PRICES {
        return;
    }
    if state.settlements[here].economy.hull_t < mass {
        let class = company.fleets[n].class;
        let order = hull_source(state, board, here, class).unwrap_or_else(|o| o);
        state.settlements[order].economy.hull_wanted += mass;
        return;
    }
    let paid = ledger::pay(
        state,
        Account::Company(key),
        Account::Settlement(here),
        price,
    );
    debug_assert_eq!(paid, price);
    let economy = &mut state.settlements[here].economy;
    economy.hull_t -= mass;
    economy.hull_wanted = (economy.hull_wanted - mass).max(0.0);
    state.companies[key].fleets[n].ships += 1;
}

/// A fleet that has carried nothing for [`SLACK_REVIEWS`] reviews sells half
/// its ships (at least one) here for scrap: the port buys the metal as far
/// as it can pay, and takes the rest for nothing. Returns whether the
/// whole fleet is gone.
fn retire(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) -> bool {
    let fleet = &state.companies[key].fleets[n];
    if fleet.slack < SLACK_REVIEWS {
        return false;
    }
    let sold = fleet.ships.div_ceil(2);
    let metal = state.ship_classes[fleet.class].mass_t * f64::from(sold) * SCRAP_METAL_SHARE;
    let gone = sold >= fleet.ships;
    let price = state.settlements[here].economy.market.price[Good::Metal];
    let company = Account::Company(key);
    let bought = ledger::sell(
        state,
        company,
        here,
        Good::Metal,
        metal,
        price,
        MAX_SPEND_SHARE,
    );
    ledger::sell(state, company, here, Good::Metal, metal - bought, 0.0, 1.0);
    let fleet = &mut state.companies[key].fleets[n];
    fleet.slack = 0;
    if !gone {
        fleet.ships -= sold;
        return false;
    }
    // Whatever was aboard stays at the port.
    if let Some(cargo) = fleet.cargo.take() {
        ledger::sell(state, company, here, cargo.good, cargo.amount, 0.0, 1.0);
    }
    remove_fleet(state, key, n);
    true
}

/// Crew upkeep for a fleet, in credits a day.
pub(crate) fn upkeep_per_day(state: &GalaxyState, fleet: &Fleet) -> f64 {
    let class = &state.ship_classes[fleet.class];
    f64::from(fleet.ships) * (UPKEEP_PER_SHIP_DAY + class.cargo_t * UPKEEP_PER_CARGO_T_DAY)
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

/// At their monthly review, settlements with hull to sell, money to spare
/// and a hold's worth of freight may found a company with one ship of the
/// biggest class they can build, unless ships sat idle in their system
/// at the start of the month.
fn found_companies(state: &mut GalaxyState) {
    let today = state.time;
    if today.days().is_multiple_of(REVIEW_DAYS) || state.idle_ports.is_none() {
        state.idle_ports = Some(IdlePorts::new(state));
    }
    for i in crate::settlement::review_slots(today.days(), 0, state.settlements.slot_count()) {
        let Some(id) = state.settlements.key_at(i) else {
            continue;
        };
        crate::decisions::time(crate::decisions::Decision::Founding, || {
            found_one(state, id, today);
        });
    }
}

/// Systems where ships sat docked with nothing to carry at the start of
/// the month: what founding a company, or a harvesting crew, checks
/// first. Made in one scan of every fleet on the first day of each month
/// (by [`found_companies`], which runs first), not by each settlement that
/// asks.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdlePorts {
    /// Where any fleet sat idle, sorted.
    any: Vec<SystemId>,
    /// Where a harvester sat idle, sorted.
    harvesters: Vec<SystemId>,
}

impl IdlePorts {
    fn new(state: &GalaxyState) -> Self {
        let mut ports = Self::default();
        for f in state.companies.values().flat_map(|c| c.fleets.iter()) {
            let Location::Docked(at) = f.at else {
                continue;
            };
            if !(f.idle || f.slack > 0) {
                continue;
            }
            let Some(s) = state.settlements.get(at) else {
                continue;
            };
            ports.any.push(s.system);
            if f.harvest.is_some() {
                ports.harvesters.push(s.system);
            }
        }
        for list in [&mut ports.any, &mut ports.harvesters] {
            list.sort_unstable();
            list.dedup();
        }
        ports
    }

    /// Whether any fleet sat idle in `system`.
    pub(crate) fn any(&self, system: SystemId) -> bool {
        self.any.binary_search(&system).is_ok()
    }

    /// Whether a harvester sat idle in `system`.
    pub(crate) fn harvesters(&self, system: SystemId) -> bool {
        self.harvesters.binary_search(&system).is_ok()
    }
}

/// One settlement's monthly review of whether to found a company.
fn found_one(state: &mut GalaxyState, id: SettlementId, today: GalaxyTime) {
    let entity = slotmap::Key::data(&id).as_ffi();
    let economy = &state.settlements[id].economy;
    if economy.population <= 0.0
        || !Rng::stream(state.seed, "found-company", today.days(), entity)
            .chance(FOUND_COMPANY_CHANCE)
    {
        return;
    }
    let cost = |c: &ShipClass| {
        c.mass_t * crate::economy::HULL_PRICE_PER_T
            + c.cargo_t * FOUNDING_HOLDS * FOUNDING_CREDITS_PER_T
    };
    // The biggest class it can afford that there's a hold's worth of
    // freight for.
    let market = &economy.market;
    let freight = Good::ALL
        .iter()
        .map(|&g| market.surplus(g))
        .fold(0.0, f64::max);
    let Some((class, founding)) = state
        .ship_classes
        .iter()
        .filter(|(_, c)| c.cargo_t <= freight && cost(c) * FOUNDING_WEALTH <= economy.treasury)
        .max_by(|a, b| a.1.cargo_t.total_cmp(&b.1.cargo_t))
        .map(|(cid, c)| (cid, cost(c)))
    else {
        return;
    };
    let system = state.settlements[id].system;
    let idle = state.idle_ports.as_ref().is_some_and(|p| p.any(system));
    if idle {
        return;
    }
    // The crew fetches the hull from the nearest yard that has it;
    // with none in reach, the order goes to the nearest yard (or here),
    // which is what shipyards build to.
    let mass = state.ship_classes[class].mass_t;
    let yard = match hull_source(state, &state.board, id, class) {
        Ok(yard) => yard,
        Err(order) => {
            state.settlements[order].economy.hull_wanted += mass;
            return;
        }
    };
    let base = state.settlements[id]
        .name
        .split(' ')
        .next()
        .unwrap_or("Free")
        .to_owned();
    state.settlements[yard].economy.hull_t -= mass;
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
                at: Location::Docked(yard),
                cargo: None,
                route: None,
                review_due: false,
                idle: false,
                loaded: false,
                slack: 0,
                harvest: None,
            }],
            next_fleet: 0,
        },
    );
    // The settlement stakes the company, which buys its hull from the
    // yard.
    let (home, company) = (Account::Settlement(id), Account::Company(company));
    ledger::pay(state, home, company, founding);
    ledger::pay(
        state,
        company,
        Account::Settlement(yard),
        mass * crate::economy::HULL_PRICE_PER_T,
    );
}

/// Where a ship of `class` for a company founded at `id` comes from: `id`
/// itself or the nearest listed yard in trading reach with the hull for it
/// (`Ok`), or, if none has, where to order it (`Err`): the nearest listed
/// yard in reach, or `id` itself. Read from `id`'s system's trade lists
/// ([`Board`]), not searched for.
pub(crate) fn hull_source(
    state: &GalaxyState,
    board: &Board,
    id: SettlementId,
    class: ShipClassId,
) -> Result<SettlementId, SettlementId> {
    let mass = state.ship_classes[class].mass_t;
    let range_ly = MAX_TRIP_DAYS * state.ship_classes[class].jump_laden;
    let here = &state.settlements[id];
    if here.economy.hull_t >= mass {
        return Ok(id);
    }
    let lists = board.lists(state, here.system);
    let mut order = None;
    for yard in lists.yards.iter().flatten() {
        if yard.ly > range_ly {
            continue;
        }
        let Some(s) = state.settlements.get(yard.at) else {
            continue;
        };
        if s.economy.hull_t >= mass {
            return Ok(yard.at);
        }
        order.get_or_insert(yard.at);
    }
    Err(order.unwrap_or(id))
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
