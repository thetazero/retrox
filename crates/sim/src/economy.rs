//! The coarse economy (§5.4): goods, recipes, facilities, markets.
//!
//! Every system has a [`SystemEconomy`]: a population that consumes, a set
//! of facilities that turn inputs into outputs, and a market holding stock.
//! Prices come from stock relative to a target. Factions with a
//! [`EconomyModel::Planned`] economy hold prices at their official base
//! value and move goods by plan instead of by price.

use std::ops::{Index, IndexMut};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{EconomyModel, GalaxyState};

/// Tradeable goods. A small starting set; the full 15–30 goods will come
/// from content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Good {
    Ore,
    Ice,
    Metal,
    Food,
    Fuel,
    Components,
}

impl Good {
    pub const COUNT: usize = 6;
    pub const ALL: [Good; Self::COUNT] = [
        Good::Ore,
        Good::Ice,
        Good::Metal,
        Good::Food,
        Good::Fuel,
        Good::Components,
    ];

    /// The reference price, in credits per unit. Planned economies sell at
    /// this price; markets float around it.
    pub const fn base_price(self) -> f64 {
        match self {
            Good::Ore => 10.0,
            Good::Ice => 6.0,
            Good::Metal => 30.0,
            Good::Food => 15.0,
            Good::Fuel => 20.0,
            Good::Components => 80.0,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Good::Ore => "ore",
            Good::Ice => "ice",
            Good::Metal => "metal",
            Good::Food => "food",
            Good::Fuel => "fuel",
            Good::Components => "components",
        }
    }
}

/// One value per [`Good`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GoodMap<T>(pub [T; Good::COUNT]);

impl<T: Copy> GoodMap<T> {
    pub const fn splat(value: T) -> Self {
        Self([value; Good::COUNT])
    }

    pub fn from_fn(f: impl FnMut(Good) -> T) -> Self {
        Self(Good::ALL.map(f))
    }
}

impl<T> Index<Good> for GoodMap<T> {
    type Output = T;

    fn index(&self, good: Good) -> &T {
        &self.0[good as usize]
    }
}

impl<T> IndexMut<Good> for GoodMap<T> {
    fn index_mut(&mut self, good: Good) -> &mut T {
        &mut self.0[good as usize]
    }
}

/// What a facility does: per unit of scale per day, consume `inputs` and
/// produce `outputs`.
pub struct Recipe {
    pub inputs: &'static [(Good, f64)],
    pub outputs: &'static [(Good, f64)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FacilityKind {
    Mine,
    IceHarvester,
    Refinery,
    Hydroponics,
    FuelPlant,
    Factory,
}

impl FacilityKind {
    pub const ALL: [FacilityKind; 6] = [
        FacilityKind::Mine,
        FacilityKind::IceHarvester,
        FacilityKind::Refinery,
        FacilityKind::Hydroponics,
        FacilityKind::FuelPlant,
        FacilityKind::Factory,
    ];

    pub const fn recipe(self) -> &'static Recipe {
        use Good::*;
        match self {
            FacilityKind::Mine => &Recipe {
                inputs: &[],
                outputs: &[(Ore, 1.0)],
            },
            FacilityKind::IceHarvester => &Recipe {
                inputs: &[],
                outputs: &[(Ice, 1.0)],
            },
            FacilityKind::Refinery => &Recipe {
                inputs: &[(Ore, 2.0), (Fuel, 0.2)],
                outputs: &[(Metal, 1.0)],
            },
            FacilityKind::Hydroponics => &Recipe {
                inputs: &[(Ice, 0.5)],
                outputs: &[(Food, 1.0)],
            },
            FacilityKind::FuelPlant => &Recipe {
                inputs: &[(Ice, 1.0)],
                outputs: &[(Fuel, 1.0)],
            },
            FacilityKind::Factory => &Recipe {
                inputs: &[(Metal, 1.0), (Fuel, 0.5)],
                outputs: &[(Components, 1.0)],
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Facility {
    pub kind: FacilityKind,
    /// Units of the recipe run per day at full capacity.
    pub scale: f64,
}

/// Daily consumption per unit of population.
pub const PER_CAPITA_DEMAND: [(Good, f64); 3] = [
    (Good::Food, 1.0),
    (Good::Fuel, 0.3),
    (Good::Components, 0.05),
];

/// Markets aim to hold this many days of their own demand.
pub const TARGET_DAYS: f64 = 30.0;
/// Facilities stop producing a good once stock reaches this multiple of target.
pub const STORAGE_MULTIPLE: f64 = 4.0;
/// Every market keeps at least this much target stock of everything, so
/// pure producers still price their output sensibly.
pub const MIN_TARGET: f64 = 20.0;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Market {
    pub stock: GoodMap<f64>,
    pub price: GoodMap<f64>,
    /// Daily demand at full rate: population plus facility inputs.
    pub demand: GoodMap<f64>,
    /// How much demand went unmet in the last production phase.
    pub shortfall: GoodMap<f64>,
}

impl Market {
    pub fn target(&self, good: Good) -> f64 {
        (self.demand[good] * TARGET_DAYS).max(MIN_TARGET)
    }

    /// Stock relative to target: 1.0 is exactly on target.
    pub fn fill(&self, good: Good) -> f64 {
        self.stock[good] / self.target(good)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SystemEconomy {
    /// Abstract population units (roughly: thousands of people).
    pub population: f64,
    pub facilities: Vec<Facility>,
    pub market: Market,
}

impl SystemEconomy {
    /// Recompute full-rate demand from population and facilities. Call after
    /// changing either.
    pub fn refresh_demand(&mut self) {
        let mut demand = GoodMap::splat(0.0);
        for (good, per_capita) in PER_CAPITA_DEMAND {
            demand[good] += per_capita * self.population;
        }
        for facility in &self.facilities {
            for &(good, amount) in facility.kind.recipe().inputs {
                demand[good] += amount * facility.scale;
            }
        }
        self.market.demand = demand;
    }

    /// One day of production and consumption.
    fn produce(&mut self) {
        let market = &mut self.market;
        market.shortfall = GoodMap::splat(0.0);

        for facility in &self.facilities {
            let recipe = facility.kind.recipe();
            // Run as much of the recipe as inputs and storage allow.
            let mut rate: f64 = 1.0;
            for &(good, amount) in recipe.inputs {
                let need = amount * facility.scale;
                if need > 0.0 {
                    rate = rate.min(market.stock[good] / need);
                }
            }
            for &(good, _) in recipe.outputs {
                let cap = market.target(good) * STORAGE_MULTIPLE;
                if market.stock[good] >= cap {
                    rate = 0.0;
                }
            }
            let rate = rate.clamp(0.0, 1.0);
            for &(good, amount) in recipe.inputs {
                let need = amount * facility.scale;
                market.stock[good] -= need * rate;
                market.shortfall[good] += need * (1.0 - rate);
            }
            for &(good, amount) in recipe.outputs {
                market.stock[good] += amount * facility.scale * rate;
            }
        }

        for (good, per_capita) in PER_CAPITA_DEMAND {
            let want = per_capita * self.population;
            let got = want.min(market.stock[good]);
            market.stock[good] -= got;
            market.shortfall[good] += want - got;
        }
    }

    pub fn update_prices(&mut self, model: EconomyModel) {
        let market = &mut self.market;
        for good in Good::ALL {
            market.price[good] = match model {
                EconomyModel::Planned => good.base_price(),
                EconomyModel::Market => {
                    let scarcity = market.target(good) / (market.stock[good] + 1.0);
                    good.base_price() * scarcity.powf(0.6).clamp(0.25, 5.0)
                }
            };
        }
    }
}

/// The economic model that applies in a system: its owner's, or a free
/// market in unclaimed space.
fn models(state: &GalaxyState) -> Vec<EconomyModel> {
    state
        .systems
        .as_slice()
        .iter()
        .map(|s| {
            s.owner
                .map_or(EconomyModel::Market, |f| state.factions[f].economy)
        })
        .collect()
}

/// Production phase: every system produces and consumes, in parallel.
pub fn production(state: &mut GalaxyState) {
    state
        .systems
        .as_mut_slice()
        .par_iter_mut()
        .for_each(|s| s.economy.produce());
}

/// Fraction of a price (or fill) gap closed per lane per day.
const TRADE_RATE: f64 = 0.15;
/// Transport cost per light-year, as a fraction of the good's base price.
const TRANSPORT_COST_PER_LY: f64 = 0.01;
/// A system never ships out more than this share of a good's stock per day,
/// split across its lanes.
const MAX_EXPORT_SHARE: f64 = 0.5;

/// Trade phase: goods move one lane at a time.
///
/// - Within a planned faction, the plan moves goods from systems above
///   target toward systems below it.
/// - Everywhere else, goods move toward the higher price when the gap beats
///   transport cost.
///
/// Flows are computed in parallel from a snapshot, then applied, so the
/// result doesn't depend on lane order and total stock is conserved.
pub fn trade(state: &mut GalaxyState) {
    let models = models(state);
    let systems = state.systems.as_slice();

    let flows: Vec<(usize, usize, Good, f64)> = state
        .lanes
        .as_slice()
        .par_iter()
        .flat_map_iter(|lane| {
            let (ia, ib) = (lane.a.index(), lane.b.index());
            let (a, b) = (&systems[ia], &systems[ib]);
            let same_plan =
                a.owner.is_some() && a.owner == b.owner && models[ia] == EconomyModel::Planned;
            Good::ALL.into_iter().filter_map(move |good| {
                let (ma, mb) = (&a.economy.market, &b.economy.market);
                // Positive signal: flow a → b.
                let signal = if same_plan {
                    ma.fill(good) - mb.fill(good)
                } else {
                    let cost = good.base_price() * TRANSPORT_COST_PER_LY * lane.length_ly;
                    let gap = mb.price[good] - ma.price[good];
                    (gap.abs() - cost).max(0.0).copysign(gap) / good.base_price()
                };
                let (from, to, from_sys, from_market) = if signal > 0.0 {
                    (ia, ib, a, ma)
                } else {
                    (ib, ia, b, mb)
                };
                let available =
                    from_market.stock[good] * MAX_EXPORT_SHARE / from_sys.lanes.len().max(1) as f64;
                let wanted = signal.abs() * TRADE_RATE * from_market.target(good);
                let amount = wanted.min(available);
                (amount > 1e-6).then_some((from, to, good, amount))
            })
        })
        .collect();

    let systems = state.systems.as_mut_slice();
    for (from, to, good, amount) in flows {
        systems[from].economy.market.stock[good] -= amount;
        systems[to].economy.market.stock[good] += amount;
    }

    systems
        .par_iter_mut()
        .zip(models.par_iter())
        .for_each(|(s, &model)| s.economy.update_prices(model));
}

/// Galaxy-wide totals, for tools and tests.
#[derive(Clone, Debug, Default)]
pub struct EconomySummary {
    pub population: f64,
    pub stock: GoodMap<f64>,
    pub demand: GoodMap<f64>,
    pub shortfall: GoodMap<f64>,
    /// Mean price across systems with any demand or stock of the good.
    pub mean_price: GoodMap<f64>,
    /// Systems where some of this good's demand went unmet.
    pub systems_short: GoodMap<usize>,
}

pub fn summarize(state: &GalaxyState) -> EconomySummary {
    let mut out = EconomySummary::default();
    let mut price_n = GoodMap::splat(0usize);
    for system in state.systems.as_slice() {
        let e = &system.economy;
        out.population += e.population;
        for good in Good::ALL {
            let m = &e.market;
            out.stock[good] += m.stock[good];
            out.demand[good] += m.demand[good];
            out.shortfall[good] += m.shortfall[good];
            if m.shortfall[good] > 1e-6 {
                out.systems_short[good] += 1;
            }
            if m.demand[good] > 0.0 || m.stock[good] > 0.0 {
                out.mean_price[good] += m.price[good];
                price_n[good] += 1;
            }
        }
    }
    for good in Good::ALL {
        if price_n[good] > 0 {
            out.mean_price[good] /= price_n[good] as f64;
        }
    }
    out
}
