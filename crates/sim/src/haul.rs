//! Hauling (DESIGN §5.4, issue #33): goods only move when a ship carries
//! them.
//!
//! Companies own fleets: groups of ships of one class, tracked as a count.
//! A fleet docked at a settlement with an empty hold picks the trip with
//! the best expected profit per day: buy a good here, sell it somewhere
//! within reach. The expected sale price counts cargo already on its way
//! to that market, so fleets don't all chase the same gap. Trips are real
//! flights: inside a system the fleet flies under the [`crate::flight`]
//! autopilot at the class's laden acceleration, out to the star's jump
//! point if the destination is elsewhere, then along the hyperlanes at the
//! jump drive's speed, and in again from the far jump point.
//!
//! Companies pay their crews' upkeep to their home settlement, which is
//! how money gets back to settlements that don't export.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use rayon::prelude::*;

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::economy::{Good, GoodMap};
use crate::flight::Vec3;
use crate::flight::{self, Goal, Kinematics};
use crate::ledger::{self, Account, Supply};
use crate::settlement::{SettlementId, Site};
use crate::{EconomyModel, GalaxyState, GalaxyTime, Id, Rng, SystemId};

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
        /// Hyperlane time to the destination's system, zero inside one.
        jump_days: f64,
        stage: Stage,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    /// Under autopilot in `system`: to the destination if it's here,
    /// otherwise to the jump point toward it.
    Flying { system: SystemId, ship: Kinematics },
    /// In the hyperlanes from `from`, coming out at the destination
    /// system's jump point facing it.
    Jumping { from: SystemId, arrive: GalaxyTime },
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
/// A company buys another ship when it holds this many times the hull's
/// price.
const BUY_SHIP_AT_PRICES: f64 = 4.0;
/// Daily chance that a settlement able to fund one founds a new company.
const FOUND_COMPANY_CHANCE: f64 = 0.01;
/// A new company starts with enough to fill its hold this many times, at
/// this much per tonne.
const FOUNDING_HOLDS: f64 = 2.0;
const FOUNDING_CREDITS_PER_T: f64 = 20.0;
/// A settlement founds companies only from treasury above this many times
/// the cost of founding one.
const FOUNDING_WEALTH: f64 = 3.0;
/// Chance a ship is lost on any one trip.
const LOSS_PER_SHIP_TRIP: f64 = 0.001;

const METRES_PER_AU: f64 = 1.496e11;
/// Jump points sit this far from a star, toward the system on the far end.
pub const JUMP_POINT_AU: f64 = 0.3;
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

/// A settlement's position and velocity at `t_days`, in metres from its
/// star: where a ship docking there has to be, and how fast.
pub fn settlement_goal(state: &GalaxyState, id: SettlementId, t_days: f64) -> Goal {
    const H_DAYS: f64 = 1e-3;
    let at = |t| flight::scale(settlement_position(state, id, t), METRES_PER_AU);
    let (a, b) = (at(t_days - H_DAYS), at(t_days + H_DAYS));
    Goal {
        pos: at(t_days),
        vel: flight::scale(flight::sub(b, a), 1.0 / (2.0 * H_DAYS * SECONDS_PER_DAY)),
    }
}

/// The jump point in `here` toward `there`, in metres from the star.
pub fn jump_point(state: &GalaxyState, here: SystemId, there: SystemId) -> Vec3 {
    let (a, b) = (state.systems[here].position, state.systems[there].position);
    let d = [b.x - a.x, b.y - a.y, b.z - a.z];
    let len = norm(d);
    let dir = if len > 0.0 {
        flight::scale(d, 1.0 / len)
    } else {
        [1.0, 0.0, 0.0]
    };
    flight::scale(dir, JUMP_POINT_AU * METRES_PER_AU)
}

fn norm(p: [f64; 3]) -> f64 {
    (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
}

/// Days to fly `au` from rest to rest under autopilot at `accel` m/s²: a
/// planning estimate of what [`flight::fly`] will take.
pub fn cruise_days(au: f64, accel: f64) -> f64 {
    flight::trip_seconds(au * METRES_PER_AU, accel) / SECONDS_PER_DAY
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
    /// Factions posting delivery contracts this tick.
    contracts: slotmap::SecondaryMap<crate::FactionId, bool>,
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
            contracts: crate::fiscal::open_contracts(state),
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
        cruise_days((norm(pa) - JUMP_POINT_AU).abs(), class.accel_laden)
            + lane_ly / class.jump_laden
            + cruise_days((norm(pb) - JUMP_POINT_AU).abs(), class.accel_laden)
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
    jump_days: f64,
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
            let jump_days = if system == origin.system {
                0.0
            } else {
                ly / class.jump_laden
            };
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
                            jump_days,
                        },
                    ));
                }
            }
            let dest = &state.settlements[to].economy;
            let dest_model = model(state, to);
            for &(good, amount, buy) in &offers {
                let coming = ctx.inbound.get(&(to, good)).copied().unwrap_or(0.0);
                let expected_stock = dest.market.stock[good] + coming + amount / 2.0;
                let price = dest.market.price_at(good, expected_stock, dest_model);
                let premium = crate::fiscal::contract_premium(
                    state,
                    &ctx.contracts,
                    to,
                    good,
                    expected_stock,
                );
                let amount = amount.min(dest.treasury.max(0.0) * MAX_SPEND_SHARE / price);
                if amount < 1.0 {
                    continue;
                }
                let profit = amount * (price + premium - buy) - fuel_per_day * days * fuel_price;
                let per_day = profit / (days + HANDLING_DAYS);
                if per_day > MIN_PROFIT_PER_DAY && best.as_ref().is_none_or(|(b, _)| per_day > *b) {
                    best = Some((
                        per_day,
                        Plan {
                            to,
                            good,
                            amount,
                            days,
                            jump_days,
                        },
                    ));
                }
            }
        }
    }
    // Repositioning is only a rough guess, so any paying loaded trip wins.
    best.or(reposition).map(|(_, plan)| plan)
}

/// Movement phase: every travelling fleet flies one day of its trip.
/// Fleets that reach their destination dock there, and some ships don't
/// make it.
pub fn movement(state: &mut GalaxyState) {
    use rayon::prelude::*;

    let now = state.time;
    let seed = state.seed;
    let jobs: Vec<(CompanyId, usize, Location, f64)> = state
        .companies
        .iter()
        .flat_map(|(key, company)| {
            company.fleets.iter().enumerate().filter_map(move |(n, f)| {
                matches!(f.at, Location::Travelling { .. }).then_some((key, n, f.at, f.class))
            })
        })
        .map(|(key, n, at, class)| (key, n, at, state.ship_classes[class].accel_laden))
        .collect();
    let moved: Vec<(CompanyId, usize, Location)> = jobs
        .into_par_iter()
        .map(|(key, n, at, accel)| (key, n, advance(state, at, accel)))
        .collect();

    for (key, n, at) in moved {
        let company_bits = slotmap::Key::data(&key).as_ffi();
        let fleet = &mut state.companies[key].fleets[n];
        fleet.at = at;
        if !matches!(at, Location::Docked(_)) {
            continue;
        }
        let mut rng = Rng::stream(
            seed,
            "ship-loss",
            now.days(),
            company_bits ^ ((n as u64) << 48),
        );
        let survivors = (0..fleet.ships)
            .filter(|_| !rng.chance(LOSS_PER_SHIP_TRIP))
            .count() as u32;
        if survivors < fleet.ships {
            // Lost ships take their share of the cargo with them.
            if let Some(cargo) = fleet.cargo.as_mut() {
                cargo.amount *= f64::from(survivors) / f64::from(fleet.ships);
            }
            fleet.ships = survivors;
        }
        // Unloading and loading before it can set off again.
        fleet.idle_until = GalaxyTime::from_days(now.days() + HANDLING_DAYS as u64);
    }
    for company in state.companies.values_mut() {
        company.fleets.retain(|f| f.ships > 0);
    }
    // A company with no ships left winds up; its money goes home.
    let gone: Vec<CompanyId> = state
        .companies
        .iter()
        .filter(|(_, c)| c.fleets.is_empty())
        .map(|(k, _)| k)
        .collect();
    for key in gone {
        let home = state.companies[key].home;
        let credits = state.companies[key].credits;
        let heir = if state.settlements.contains_key(home) {
            Some(Account::Settlement(home))
        } else {
            // Home is gone: any faction keeps the money in the galaxy.
            state.factions.keys().next().map(Account::Faction)
        };
        if let Some(heir) = heir {
            ledger::pay(state, Account::Company(key), heir, credits);
        }
        state.companies.remove(key);
    }
}

/// One day of a trip: fly, jump, or come out of the lanes.
fn advance(state: &GalaxyState, at: Location, accel: f64) -> Location {
    let Location::Travelling {
        to,
        jump_days,
        stage,
    } = at
    else {
        return at;
    };
    let now = state.time;
    let Some(dest) = state.settlements.get(to) else {
        // Nowhere left to go: the trip just ends.
        return Location::Docked(to);
    };
    let dest_system = dest.system;
    match stage {
        Stage::Jumping { arrive, .. } if arrive > now => at,
        Stage::Jumping { from, .. } => {
            // Out of the lanes at rest, on the side facing where we came
            // from; the autopilot takes it from here tomorrow.
            let entry = jump_point(state, dest_system, from);
            Location::Travelling {
                to,
                jump_days,
                stage: Stage::Flying {
                    system: dest_system,
                    ship: Kinematics {
                        pos: entry,
                        vel: [0.0; 3],
                    },
                },
            }
        }
        Stage::Flying { system, mut ship } => {
            let t0 = now.days() as f64;
            let arrived = if system == dest_system {
                flight::fly(&mut ship, accel, SECONDS_PER_DAY, |t, _| {
                    settlement_goal(state, to, t0 + t / SECONDS_PER_DAY)
                })
            } else {
                let goal = Goal {
                    pos: jump_point(state, system, dest_system),
                    vel: [0.0; 3],
                };
                flight::fly(&mut ship, accel, SECONDS_PER_DAY, |_, _| goal)
            };
            match arrived {
                None => Location::Travelling {
                    to,
                    jump_days,
                    stage: Stage::Flying { system, ship },
                },
                Some(_) if system == dest_system => Location::Docked(to),
                Some(_) => Location::Travelling {
                    to,
                    jump_days,
                    stage: Stage::Jumping {
                        from: system,
                        arrive: GalaxyTime::from_days(
                            now.days() + jump_days.ceil().max(1.0) as u64,
                        ),
                    },
                },
            }
        }
    }
}

/// Trade phase: docked fleets sell what they carry, then choose and start
/// their next trip. Companies pay upkeep. Prices update at the end.
pub fn trade(state: &mut GalaxyState) {
    let now = state.time;
    let mut ctx = Ctx::new(state);

    // Docked fleets sell, and maybe buy a ship; note who needs a plan.
    let mut deciding: Vec<(CompanyId, usize, SettlementId)> = Vec::new();
    let keys: Vec<CompanyId> = state.companies.keys().collect();
    for &key in &keys {
        for n in 0..state.companies[key].fleets.len() {
            let Location::Docked(here) = state.companies[key].fleets[n].at else {
                continue;
            };
            if !state.settlements.contains_key(here) {
                continue;
            }
            sell(state, &ctx, key, n, here);
            buy_ship(state, key, n, here);
            if state.companies[key].fleets[n].idle_until <= now {
                deciding.push((key, n, here));
            }
        }
    }

    // Plan every trip in parallel from the same snapshot...
    let plans: Vec<Option<Plan>> = {
        let state = &*state;
        let ctx = &ctx;
        deciding
            .par_iter()
            .map(|&(key, n, here)| {
                let company = &state.companies[key];
                best_trip(state, ctx, company, &company.fleets[n], here)
            })
            .collect()
    };

    // ...then carry them out in order. A plan that an earlier departure
    // this tick could have changed (same port, same destination and good,
    // or same company's money) is made again from the current state.
    let mut ports: HashSet<SettlementId> = HashSet::new();
    let mut deliveries: HashSet<(SettlementId, Good)> = HashSet::new();
    let mut claims: HashSet<SettlementId> = HashSet::new();
    let mut spenders: HashSet<CompanyId> = HashSet::new();
    for (&(key, n, here), plan) in deciding.iter().zip(plans) {
        let loaded = state.companies[key].fleets[n].cargo.is_some();
        let stale = spenders.contains(&key)
            || (!loaded && ports.contains(&here))
            || plan.as_ref().is_some_and(|p| {
                if p.amount > 0.0 || loaded {
                    deliveries.contains(&(p.to, p.good))
                } else {
                    claims.contains(&p.to)
                }
            });
        let plan = if stale {
            let company = &state.companies[key];
            best_trip(state, &ctx, company, &company.fleets[n], here)
        } else {
            plan
        };
        match plan {
            Some(plan) => {
                if plan.amount > 0.0 || loaded {
                    deliveries.insert((plan.to, plan.good));
                } else {
                    claims.insert(plan.to);
                }
                if !loaded && plan.amount > 0.0 {
                    ports.insert(here);
                }
                spenders.insert(key);
                depart(state, &mut ctx, key, n, here, plan);
            }
            None => {
                state.companies[key].fleets[n].idle_until =
                    GalaxyTime::from_days(now.days() + IDLE_DAYS);
            }
        }
    }

    for key in keys {
        pay_upkeep(state, key);
    }
    found_companies(state);

    crate::economy::update_all_prices(state);
}

/// Sell as much of a docked fleet's cargo as the settlement can afford.
/// The settlement pays the price; its faction pays any contract premium.
fn sell(state: &mut GalaxyState, ctx: &Ctx, key: CompanyId, n: usize, here: SettlementId) {
    let Some(cargo) = state.companies[key].fleets[n].cargo else {
        return;
    };
    let model = model(state, here);
    let stock = state.settlements[here].economy.market.stock[cargo.good] + cargo.amount / 2.0;
    let premium = crate::fiscal::contract_premium(state, &ctx.contracts, here, cargo.good, stock);
    let owner = state.settlements[here].owner;
    let price = state.settlements[here]
        .economy
        .market
        .price_at(cargo.good, stock, model);
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

    let company = Account::Company(key);
    if loading {
        let price = state.settlements[here].economy.market.price[plan.good];
        let affordable = ledger::available(state, here, plan.good, Supply::Surplus)
            .min(ledger::spendable(state, company) / price);
        if affordable < 1.0 {
            // Someone else got there first, or the money ran short.
            state.companies[key].fleets[n].idle_until =
                GalaxyTime::from_days(state.time.days() + IDLE_DAYS);
            return;
        }
        let bought = ledger::buy(
            state,
            company,
            here,
            plan.good,
            plan.amount,
            price,
            Supply::Surplus,
            1.0,
        );
        state.companies[key].fleets[n].cargo = Some(Cargo {
            good: plan.good,
            amount: bought,
        });
    }
    // Fuel comes out of the port's stock, as much as the company can pay
    // for; the rest is burned from the ships' own reserves, free.
    let fuel_price = state.settlements[here].economy.market.price[Good::Fuel];
    ledger::buy(
        state,
        company,
        here,
        Good::Fuel,
        fuel,
        fuel_price,
        Supply::Stock,
        1.0,
    );

    let fleet = &mut state.companies[key].fleets[n];
    match fleet.cargo {
        Some(cargo) => *ctx.inbound.entry((plan.to, cargo.good)).or_default() += cargo.amount,
        None => *ctx.claimed.entry(plan.to).or_default() += class_capacity,
    }
    // Cast off from the port, moving with it.
    let port = settlement_goal(state, here, state.time.days() as f64);
    let system = state.settlements[here].system;
    state.companies[key].fleets[n].at = Location::Travelling {
        to: plan.to,
        jump_days: plan.jump_days,
        stage: Stage::Flying {
            system,
            ship: Kinematics {
                pos: port.pos,
                vel: port.vel,
            },
        },
    };
}

/// A prosperous company docked at a shipyard adds a ship to its fleet.
fn buy_ship(state: &mut GalaxyState, key: CompanyId, n: usize, here: SettlementId) {
    let fleet = &state.companies[key].fleets[n];
    if fleet.cargo.is_some() {
        return;
    }
    let mass = state.ship_classes[fleet.class].mass_t;
    let price = mass * crate::economy::HULL_PRICE_PER_T;
    if state.companies[key].credits < price * BUY_SHIP_AT_PRICES
        || state.settlements[here].economy.hull_t < mass
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

/// Settlements with hull to sell and money to spare found new companies,
/// with one ship of the biggest class they can build.
fn found_companies(state: &mut GalaxyState) {
    let now = state.time;
    let ids: Vec<SettlementId> = state.settlements.keys().collect();
    for id in ids {
        let economy = &state.settlements[id].economy;
        if economy.hull_t <= 0.0
            || !Rng::stream(
                state.seed,
                "found-company",
                now.days(),
                slotmap::Key::data(&id).as_ffi(),
            )
            .chance(FOUND_COMPANY_CHANCE)
        {
            continue;
        }
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
            .map(|(id, c)| (id, cost(c)))
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
        let hull_price = mass * crate::economy::HULL_PRICE_PER_T;
        let company = state.companies.insert(Company {
            name: format!("{base} Haulers {}", now.year()),
            home: id,
            credits: 0.0,
            fleets: vec![Fleet {
                class,
                ships: 1,
                at: Location::Docked(id),
                cargo: None,
                idle_until: now,
            }],
        });
        // The settlement stakes the company, which buys its hull from the
        // same yard.
        let (home, company) = (Account::Settlement(id), Account::Company(company));
        ledger::pay(state, home, company, founding);
        ledger::pay(state, company, home, hull_price);
    }
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
    let home = company.home;
    ledger::pay(
        state,
        Account::Company(key),
        Account::Settlement(home),
        upkeep,
    );
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
