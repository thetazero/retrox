//! The coarse economy (§5.4): goods, recipes, facilities, markets.
//!
//! Every settlement has a [`LocalEconomy`]: a population that consumes, a
//! set of facilities that turn inputs into outputs, and a market holding
//! stock. Prices come from stock relative to a target. Factions with a
//! [`EconomyModel::Planned`] economy hold prices at their official base
//! value and move goods by plan instead of by price.
//!
//! Goods only move between settlements in ships; see [`crate::haul`].

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
    /// Builds ship hulls: its output goes to [`LocalEconomy::hull_t`], not
    /// the market.
    Shipyard,
}

impl FacilityKind {
    pub const ALL: [FacilityKind; 7] = [
        FacilityKind::Mine,
        FacilityKind::IceHarvester,
        FacilityKind::Refinery,
        FacilityKind::Hydroponics,
        FacilityKind::FuelPlant,
        FacilityKind::Factory,
        FacilityKind::Shipyard,
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
            // One tonne of hull per run; see `HULL_T_PER_RUN`.
            FacilityKind::Shipyard => &Recipe {
                inputs: &[(Metal, 1.0), (Components, 0.3), (Fuel, 0.1)],
                outputs: &[],
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Facility {
    pub kind: FacilityKind,
    /// Units of the recipe run per day at full capacity.
    pub scale: f64,
    /// How much of capacity it has been running at, averaged over about a
    /// season. Long-idle facilities decay.
    pub utilization: f64,
}

impl Facility {
    pub const fn new(kind: FacilityKind, scale: f64) -> Self {
        Self {
            kind,
            scale,
            utilization: 1.0,
        }
    }
}

/// Days over which a facility's utilization is averaged.
const UTILIZATION_DAYS: f64 = 90.0;

/// Daily consumption per unit of population.
pub const PER_CAPITA_DEMAND: [(Good, f64); 3] = [
    (Good::Food, 1.0),
    (Good::Fuel, 0.3),
    (Good::Components, 0.05),
];

/// Tonnes of hull a shipyard builds per run of its recipe.
pub const HULL_T_PER_RUN: f64 = 1.0;
/// Shipyards stop once they hold this many days of their own output.
pub const HULL_STORAGE_DAYS: f64 = 120.0;
/// What a tonne of finished hull sells for, in credits.
pub const HULL_PRICE_PER_T: f64 = 60.0;

/// A facility under construction: what it will be, and the materials it
/// still needs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub kind: FacilityKind,
    /// Scale added when finished.
    pub scale: f64,
    pub needs: GoodMap<f64>,
    /// When it was started. A project that can't get its materials is
    /// eventually cut short; see [`crate::construction`].
    #[serde(default)]
    pub started: crate::GalaxyTime,
}

/// Daily consumption of `good` per unit of population.
pub fn per_capita(good: Good) -> f64 {
    PER_CAPITA_DEMAND
        .iter()
        .find(|(g, _)| *g == good)
        .map_or(0.0, |&(_, amount)| amount)
}

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

    /// What this market would charge for `good` if it held `stock` of it.
    /// Planned economies always charge the official base price.
    pub fn price_at(&self, good: Good, stock: f64, model: EconomyModel) -> f64 {
        match model {
            EconomyModel::Planned => good.base_price(),
            EconomyModel::Market => {
                let scarcity = self.target(good) / (stock.max(0.0) + 1.0);
                good.base_price() * scarcity.powf(0.6).clamp(0.25, 5.0)
            }
        }
    }

    /// Stock above target: what the market is willing to sell.
    pub fn surplus(&self, good: Good) -> f64 {
        (self.stock[good] - self.target(good)).max(0.0)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalEconomy {
    /// Abstract population units (roughly: thousands of people).
    pub population: f64,
    pub facilities: Vec<Facility>,
    pub market: Market,
    /// Share of food demand going unmet, averaged over about a month; see
    /// [`crate::demography`].
    pub hunger: f64,
    /// Whether the settlement is in famine.
    pub famine: bool,
    /// The facility being built here, if any.
    pub project: Option<Project>,
    /// Finished ship hull ready for sale, in tonnes. Built by shipyards.
    pub hull_t: f64,
    /// The settlement's money, in credits. Trade only moves money; nothing
    /// creates or destroys it yet.
    pub treasury: f64,
}

impl LocalEconomy {
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
        // A project wants its materials over the next month or so.
        if let Some(project) = &self.project {
            for good in Good::ALL {
                demand[good] += project.needs[good] / crate::construction::BUILD_DAYS;
            }
        }
        self.market.demand = demand;
    }

    /// One day of production and consumption.
    fn produce(&mut self) {
        let market = &mut self.market;
        market.shortfall = GoodMap::splat(0.0);

        for facility in &mut self.facilities {
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
            let shipyard = facility.kind == FacilityKind::Shipyard;
            if shipyard && self.hull_t >= facility.scale * HULL_T_PER_RUN * HULL_STORAGE_DAYS {
                rate = 0.0;
            }
            let rate = rate.clamp(0.0, 1.0);
            facility.utilization += (rate - facility.utilization) / UTILIZATION_DAYS;
            for &(good, amount) in recipe.inputs {
                let need = amount * facility.scale;
                market.stock[good] -= need * rate;
                market.shortfall[good] += need * (1.0 - rate);
            }
            for &(good, amount) in recipe.outputs {
                market.stock[good] += amount * facility.scale * rate;
            }
            if shipyard {
                self.hull_t += facility.scale * HULL_T_PER_RUN * rate;
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
            market.price[good] = market.price_at(good, market.stock[good], model);
        }
    }
}

/// The economic model in each settlement, in `settlements` iteration order.
fn models(state: &GalaxyState) -> Vec<EconomyModel> {
    state
        .settlements
        .values()
        .map(|s| state.factions[s.owner].economy)
        .collect()
}

/// Production phase: every settlement produces and consumes, in parallel.
pub fn production(state: &mut GalaxyState) {
    let mut settlements: Vec<_> = state.settlements.values_mut().collect();
    if settlements.len() < PARALLEL_MIN {
        settlements.iter_mut().for_each(|s| s.economy.produce());
    } else {
        settlements.par_iter_mut().for_each(|s| s.economy.produce());
    }
}

/// Below this many settlements, starting threads costs more than the work.
const PARALLEL_MIN: usize = 1024;

/// Recompute every settlement's prices from its stock.
pub fn update_all_prices(state: &mut GalaxyState) {
    let models = models(state);
    let mut settlements: Vec<_> = state.settlements.values_mut().collect();
    if settlements.len() < PARALLEL_MIN {
        for (s, &model) in settlements.iter_mut().zip(&models) {
            s.economy.update_prices(model);
        }
    } else {
        settlements
            .par_iter_mut()
            .zip(models.par_iter())
            .for_each(|(s, &model)| s.economy.update_prices(model));
    }
}

/// Galaxy-wide totals, for tools and tests.
#[derive(Clone, Debug, Default)]
pub struct EconomySummary {
    pub population: f64,
    /// Credits held by all settlements, companies and colony ships.
    pub money: f64,
    /// Sum of faction balances; negative means money factions have issued.
    pub faction_balance: f64,
    pub companies: usize,
    pub ships: u32,
    pub ships_travelling: u32,
    /// Goods aboard ships, by good.
    pub cargo: GoodMap<f64>,
    pub stock: GoodMap<f64>,
    pub demand: GoodMap<f64>,
    pub shortfall: GoodMap<f64>,
    /// Mean price across markets with any demand or stock of the good.
    pub mean_price: GoodMap<f64>,
    /// Markets where some of this good's demand went unmet.
    pub markets_short: GoodMap<usize>,
}

pub fn summarize(state: &GalaxyState) -> EconomySummary {
    let mut out = EconomySummary::default();
    let mut price_n = GoodMap::splat(0usize);
    for settlement in state.settlements.values() {
        let e = &settlement.economy;
        out.population += e.population;
        out.money += e.treasury;
        for good in Good::ALL {
            let m = &e.market;
            out.stock[good] += m.stock[good];
            out.demand[good] += m.demand[good];
            out.shortfall[good] += m.shortfall[good];
            if m.shortfall[good] > 1e-6 {
                out.markets_short[good] += 1;
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
    out.faction_balance = state.factions.values().map(|f| f.treasury).sum();
    out.money += state.colony_ships.values().map(|c| c.credits).sum::<f64>();
    out.companies = state.companies.len();
    for company in state.companies.values() {
        out.money += company.credits;
        for fleet in &company.fleets {
            out.ships += fleet.ships;
            if !matches!(fleet.at, crate::haul::Location::Docked(_)) {
                out.ships_travelling += fleet.ships;
            }
            if let Some(cargo) = fleet.cargo {
                out.cargo[cargo.good] += cargo.amount;
            }
        }
    }
    out
}
