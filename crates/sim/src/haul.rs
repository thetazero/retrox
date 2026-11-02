//! Hauling (DESIGN §5.4, issue #33): goods only move when a ship carries
//! them.
//!
//! Companies own fleets: groups of ships of one class, tracked as a count.
//! A fleet docked at a settlement with an empty hold picks the trip with
//! the best expected profit per day: buy a good here, sell it somewhere
//! within reach. The expected sale price counts cargo already on its way
//! to that market, so fleets don't all chase the same gap. Trips take real
//! time: accelerate-and-brake inside systems, at the class's laden
//! acceleration, and the jump drive's speed along hyperlanes.
//!
//! Companies pay their crews' upkeep to their home settlement, which is
//! how money gets back to settlements that don't export.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::economy::{Good, GoodMap};
use crate::settlement::{SettlementId, Site};
use crate::{EconomyModel, GalaxyState, GalaxyTime, Id, SystemId};

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
}

/// Ships of one class that travel together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fleet {
    pub class: ShipClassId,
    pub ships: u32,
    pub at: Location,
    pub cargo: Option<Cargo>,
    /// A fleet with nothing worth doing waits until this day.
    pub idle_until: GalaxyTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Location {
    Docked(SettlementId),
    Travelling {
        to: SettlementId,
        arrive: GalaxyTime,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cargo {
    pub good: Good,
    pub amount: f64,
}

/// Trips longer than this aren't considered.
const MAX_TRIP_DAYS: f64 = 40.0;
/// Loading and unloading, added to every trip.
const HANDLING_DAYS: f64 = 1.0;
/// How long a fleet with nothing worth doing waits before looking again.
const IDLE_DAYS: u64 = 5;
/// A trip has to earn at least this many credits per day to be worth it.
const MIN_PROFIT_PER_DAY: f64 = 1.0;
/// Fuel burned per ship per day of travel: a base plus a share of the hold.
const FUEL_PER_SHIP_DAY: f64 = 0.5;
const FUEL_PER_CARGO_T_DAY: f64 = 0.002;
/// Crew upkeep per ship per day, in credits: a base plus a share of the hold.
const UPKEEP_PER_SHIP_DAY: f64 = 2.0;
const UPKEEP_PER_CARGO_T_DAY: f64 = 0.02;
/// A buyer spends at most this share of its treasury on one delivery.
const MAX_SPEND_SHARE: f64 = 0.5;
/// A company spends at most this share of its credits on one cargo.
const MAX_STAKE_SHARE: f64 = 0.9;
/// Chance a ship is lost on any one trip.
const LOSS_PER_SHIP_TRIP: f64 = 0.001;

const METRES_PER_AU: f64 = 1.496e11;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Where a settlement is at time `t_days`, in AU from its star.
pub fn settlement_position(state: &GalaxyState, id: SettlementId, t_days: f64) -> [f64; 3] {
    let s = &state.settlements[id];
    match s.site {
        Site::Region(r) => state.body_position(state.regions[r].body, t_days),
        Site::Surface(b) => state.body_position(b, t_days),
        Site::Space(orbit) => state.orbit_position(s.system, &orbit, t_days),
    }
}

fn norm(p: [f64; 3]) -> f64 {
    (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
}

/// Days to cover `au` accelerating halfway and braking the rest, at
/// `accel` m/s².
pub fn cruise_days(au: f64, accel: f64) -> f64 {
    if accel <= 0.0 {
        return f64::INFINITY;
    }
    2.0 * (au * METRES_PER_AU / accel).sqrt() / SECONDS_PER_DAY
}

/// Shortest lane distance from `origin` to every system within `max_ly`.
fn reach(state: &GalaxyState, origin: SystemId, max_ly: f64) -> Vec<(SystemId, f64)> {
    let mut best: HashMap<SystemId, f64> = HashMap::from([(origin, 0.0)]);
    let mut heap = BinaryHeap::from([Reverse((0.0f64.to_bits(), origin))]);
    let mut out = Vec::new();
    while let Some(Reverse((bits, system))) = heap.pop() {
        let ly = f64::from_bits(bits);
        if best.get(&system).is_some_and(|&b| b < ly) {
            continue;
        }
        out.push((system, ly));
        for &lane in &state.systems[system].lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            let d = ly + lane.length_ly;
            if d <= max_ly && best.get(&next).is_none_or(|&b| d < b) {
                best.insert(next, d);
                heap.push(Reverse((d.to_bits(), next)));
            }
        }
    }
    out
}

/// Per-tick lookups shared by every fleet's decision.
struct Ctx {
    /// Where each settlement is today, AU from its star.
    positions: slotmap::SecondaryMap<SettlementId, [f64; 3]>,
    /// Mean price of each good across all markets: a rough guide to what a
    /// cargo is worth somewhere else.
    mean_price: GoodMap<f64>,
    inbound: HashMap<(SettlementId, Good), f64>,
    /// Empty hold capacity already flying to each settlement to load.
    claimed: HashMap<SettlementId, f64>,
}

impl Ctx {
    fn new(state: &GalaxyState) -> Self {
        let t = state.time.days() as f64;
        let mut positions = slotmap::SecondaryMap::with_capacity(state.settlements.len());
        let mut mean_price = GoodMap::splat(0.0);
        for id in state.settlements.keys() {
            positions.insert(id, settlement_position(state, id, t));
            for good in Good::ALL {
                mean_price[good] += state.settlements[id].economy.market.price[good];
            }
        }
        let n = state.settlements.len().max(1) as f64;
        for good in Good::ALL {
            mean_price[good] /= n;
        }
        let mut inbound = HashMap::new();
        let mut claimed = HashMap::new();
        for company in state.companies.values() {
            for fleet in &company.fleets {
                let Location::Travelling { to, .. } = fleet.at else {
                    continue;
                };
                match fleet.cargo {
                    Some(cargo) => *inbound.entry((to, cargo.good)).or_default() += cargo.amount,
                    None => {
                        let capacity =
                            state.ship_classes[fleet.class].cargo_t * f64::from(fleet.ships);
                        *claimed.entry(to).or_default() += capacity;
                    }
                }
            }
        }
        Self {
            positions,
            mean_price,
            inbound,
            claimed,
        }
    }

    /// Days between two settlements for a laden ship of `class`, given the
    /// lane distance between their systems.
    fn trip_days(
        &self,
        state: &GalaxyState,
        class: &ShipClass,
        from: SettlementId,
        to: SettlementId,
        lane_ly: f64,
    ) -> f64 {
        let (pa, pb) = (self.positions[from], self.positions[to]);
        if state.settlements[from].system == state.settlements[to].system {
            let d = [pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]];
            return cruise_days(norm(d), class.accel_laden);
        }
        // Out to the star's jump point, along the lanes, and in again.
        cruise_days(norm(pa), class.accel_laden)
            + lane_ly / class.jump_laden
            + cruise_days(norm(pb), class.accel_laden)
    }
}

fn model(state: &GalaxyState, id: SettlementId) -> EconomyModel {
    state.factions[state.settlements[id].owner].economy
}

/// What a fleet decided to do. With `amount` zero, it travels empty.
struct Plan {
    to: SettlementId,
    good: Good,
    amount: f64,
    days: f64,
}

/// The most profitable trip for a fleet docked at `here`, if any beats
/// [`MIN_PROFIT_PER_DAY`]. With cargo aboard, only places to sell it count.
fn best_trip(
    state: &GalaxyState,
    ctx: &Ctx,
    company: &Company,
    fleet: &Fleet,
    here: SettlementId,
) -> Option<Plan> {
    let class = &state.ship_classes[fleet.class];
    let capacity = class.cargo_t * f64::from(fleet.ships);
    let origin = &state.settlements[here];
    let market = &origin.economy.market;
    let fuel_price = market.price[Good::Fuel];
    let fuel_per_day = f64::from(fleet.ships) * FUEL_PER_SHIP_DAY + capacity * FUEL_PER_CARGO_T_DAY;

    // What could go in the hold, and at what cost per unit.
    let offers: Vec<(Good, f64, f64)> = match fleet.cargo {
        Some(cargo) => vec![(cargo.good, cargo.amount, 0.0)],
        None => Good::ALL
            .into_iter()
            .filter_map(|good| {
                let price = market.price[good];
                let amount = capacity
                    .min(market.surplus(good))
                    .min(company.credits.max(0.0) * MAX_STAKE_SHARE / price);
                (amount >= 1.0).then_some((good, amount, price))
            })
            .collect(),
    };
    let mut best: Option<(f64, Plan)> = None;
    let mut reposition: Option<(f64, Plan)> = None;
    for (system, ly) in reach(state, origin.system, MAX_TRIP_DAYS * class.jump_laden) {
        for &to in &state.systems[system].settlements {
            if to == here {
                continue;
            }
            let days = ctx.trip_days(state, class, here, to, ly);
            if days > MAX_TRIP_DAYS {
                continue;
            }
            if fleet.cargo.is_none() {
                // Worth flying there empty? Rough guide: what its surplus
                // would fetch at the galaxy's mean price, less what other
                // empty ships already on their way will take.
                let there = &state.settlements[to].economy.market;
                let claimed = ctx.claimed.get(&to).copied().unwrap_or(0.0);
                let value: f64 = Good::ALL
                    .into_iter()
                    .map(|g| {
                        capacity.min((there.surplus(g) - claimed).max(0.0))
                            * (ctx.mean_price[g] - there.price[g]).max(0.0)
                    })
                    .fold(0.0, f64::max);
                // Two legs: this one empty, the next one loaded.
                let per_day =
                    (value - fuel_per_day * days * fuel_price) / (2.0 * days + HANDLING_DAYS);
                if per_day > MIN_PROFIT_PER_DAY
                    && reposition.as_ref().is_none_or(|(b, _)| per_day > *b)
                {
                    reposition = Some((
                        per_day,
                        Plan {
                            to,
                            good: Good::Ore,
                            amount: 0.0,
                            days,
                        },
                    ));
                }
            }
            let dest = &state.settlements[to].economy;
            let dest_model = model(state, to);
            for &(good, amount, buy) in &offers {
                let coming = ctx.inbound.get(&(to, good)).copied().unwrap_or(0.0);
                let expected_stock = dest.market.stock[good] + coming + amount / 2.0;
                let sell = dest.market.price_at(good, expected_stock, dest_model);
                let amount = amount.min(dest.treasury.max(0.0) * MAX_SPEND_SHARE / sell);
                if amount < 1.0 {
                    continue;
                }
                let profit = amount * (sell - buy) - fuel_per_day * days * fuel_price;
                let per_day = profit / (days + HANDLING_DAYS);
                if per_day > MIN_PROFIT_PER_DAY && best.as_ref().is_none_or(|(b, _)| per_day > *b) {
                    best = Some((
                        per_day,
                        Plan {
                            to,
                            good,
                            amount,
                            days,
                        },
                    ));
                }
            }
        }
    }
    // Repositioning is only a rough guess, so any paying loaded trip wins.
    best.or(reposition).map(|(_, plan)| plan)
}

/// A small deterministic hash, for loss rolls until the sim has proper
/// RNG streams (#11).
fn roll(seed: u64, a: u64, b: u64) -> f64 {
    let mut z =
        seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// Movement phase: fleets that reach their destination dock there. Some
/// ships don't make it.
pub fn movement(state: &mut GalaxyState) {
    let now = state.time;
    let seed = state.seed;
    for (key, company) in state.companies.iter_mut() {
        let company_bits = slotmap::Key::data(&key).as_ffi();
        for (n, fleet) in company.fleets.iter_mut().enumerate() {
            let Location::Travelling { to, arrive } = fleet.at else {
                continue;
            };
            if arrive > now {
                continue;
            }
            let survivors = (0..fleet.ships)
                .filter(|&i| {
                    roll(
                        seed,
                        company_bits ^ now.days(),
                        (n as u64) << 32 | u64::from(i),
                    ) >= LOSS_PER_SHIP_TRIP
                })
                .count() as u32;
            if survivors < fleet.ships {
                // Lost ships take their share of the cargo with them.
                if let Some(cargo) = fleet.cargo.as_mut() {
                    cargo.amount *= f64::from(survivors) / f64::from(fleet.ships);
                }
                fleet.ships = survivors;
            }
            fleet.at = Location::Docked(to);
        }
        company.fleets.retain(|f| f.ships > 0);
    }
}

/// Trade phase: docked fleets sell what they carry, then choose and start
/// their next trip. Companies pay upkeep. Prices update at the end.
pub fn trade(state: &mut GalaxyState) {
    let now = state.time;
    let mut ctx = Ctx::new(state);

    let keys: Vec<CompanyId> = state.companies.keys().collect();
    for key in keys {
        let fleets = state.companies[key].fleets.len();
        for n in 0..fleets {
            let Location::Docked(here) = state.companies[key].fleets[n].at else {
                continue;
            };
            if !state.settlements.contains_key(here) {
                continue;
            }
            sell(state, key, n, here);
            if state.companies[key].fleets[n].idle_until > now {
                continue;
            }
            let plan = best_trip(
                state,
                &ctx,
                &state.companies[key],
                &state.companies[key].fleets[n],
                here,
            );
            match plan {
                Some(plan) => depart(state, &mut ctx, key, n, here, plan),
                None => {
                    state.companies[key].fleets[n].idle_until =
                        GalaxyTime::from_days(now.days() + IDLE_DAYS);
                }
            }
        }
        pay_upkeep(state, key);
    }

    crate::economy::update_all_prices(state);
}

/// Sell as much of a docked fleet's cargo as the settlement can afford.
fn sell(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) {
    let Some(cargo) = state.companies[key].fleets[n].cargo else {
        return;
    };
    let model = model(state, here);
    let settlement = &mut state.settlements[here];
    let market = &settlement.economy.market;
    let price = market.price_at(
        cargo.good,
        market.stock[cargo.good] + cargo.amount / 2.0,
        model,
    );
    let amount = cargo
        .amount
        .min(settlement.economy.treasury.max(0.0) * MAX_SPEND_SHARE / price);
    if amount <= 0.0 {
        return;
    }
    settlement.economy.market.stock[cargo.good] += amount;
    settlement.economy.treasury -= amount * price;
    let company = &mut state.companies[key];
    company.credits += amount * price;
    let left = cargo.amount - amount;
    company.fleets[n].cargo = (left > 1e-6).then_some(Cargo {
        good: cargo.good,
        amount: left,
    });
}

/// Load up (if the hold is empty), buy fuel, and set off.
fn depart(
    state: &mut GalaxyState,
    ctx: &mut Ctx,
    key: CompanyId,
    n: usize,
    here: SettlementId,
    plan: Plan,
) {
    let fleet = &state.companies[key].fleets[n];
    let class = &state.ship_classes[fleet.class];
    let fuel = (f64::from(fleet.ships) * FUEL_PER_SHIP_DAY
        + class.cargo_t * f64::from(fleet.ships) * FUEL_PER_CARGO_T_DAY)
        * plan.days;
    let loading = fleet.cargo.is_none() && plan.amount > 0.0;
    let class_capacity = class.cargo_t * f64::from(fleet.ships);

    let origin = &mut state.settlements[here].economy;
    let mut cost = 0.0;
    if loading {
        cost += plan.amount * origin.market.price[plan.good];
        origin.market.stock[plan.good] -= plan.amount;
    }
    // Fuel comes out of the port's stock; what it can't supply is burned
    // from the ships' own reserves, free.
    let fuel_bought = fuel.min(origin.market.stock[Good::Fuel]);
    origin.market.stock[Good::Fuel] -= fuel_bought;
    cost += fuel_bought * origin.market.price[Good::Fuel];
    origin.treasury += cost;

    let company = &mut state.companies[key];
    company.credits -= cost;
    let fleet = &mut company.fleets[n];
    if loading {
        fleet.cargo = Some(Cargo {
            good: plan.good,
            amount: plan.amount,
        });
    }
    match fleet.cargo {
        Some(cargo) => *ctx.inbound.entry((plan.to, cargo.good)).or_default() += cargo.amount,
        None => *ctx.claimed.entry(plan.to).or_default() += class_capacity,
    }
    let days = (plan.days + HANDLING_DAYS).ceil() as u64;
    fleet.at = Location::Travelling {
        to: plan.to,
        arrive: GalaxyTime::from_days(state.time.days() + days),
    };
}

/// Crews are paid at home, while the company can afford it.
fn pay_upkeep(state: &mut GalaxyState, key: CompanyId) {
    let company = &state.companies[key];
    let upkeep: f64 = company
        .fleets
        .iter()
        .map(|f| {
            let class = &state.ship_classes[f.class];
            f64::from(f.ships) * (UPKEEP_PER_SHIP_DAY + class.cargo_t * UPKEEP_PER_CARGO_T_DAY)
        })
        .sum();
    let paid = upkeep.min(company.credits.max(0.0));
    let home = company.home;
    if paid <= 0.0 || !state.settlements.contains_key(home) {
        return;
    }
    state.companies[key].credits -= paid;
    state.settlements[home].economy.treasury += paid;
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
