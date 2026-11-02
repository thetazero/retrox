//! Resources, population and facilities at the start of history.
//!
//! Facilities are placed where they make sense (mines on ore, factories in
//! populous settlements), then each kind is scaled so the galaxy as a whole
//! produces a little more than it consumes. Whether a given settlement is
//! short is then up to the trade network.

use retrox_sim::economy::{Facility, FacilityKind, Good, GoodMap, PER_CAPITA_DEMAND};
use retrox_sim::{FactionId, GalaxyState, Resources, SettlementId, SystemId};
use slotmap::SecondaryMap;

use crate::factions::Territory;
use crate::rng::Rng;

/// Galaxy-wide production as a multiple of what consumption needs.
const SURPLUS: f64 = 1.15;
/// Settlements start with enough credits to buy this many days of their own
/// demand at base prices.
const STARTING_TREASURY_DAYS: f64 = 60.0;
/// Daily metal and components per unit of population set aside for
/// construction and colony ships.
const BUILD_METAL_PER_CAPITA: f64 = 0.05;
const BUILD_COMPONENTS_PER_CAPITA: f64 = 0.01;
/// Settlements above this population with a factory get a shipyard.
const SHIPYARD_MIN_POPULATION: f64 = 30.0;
/// Tonnes of hull a day per unit of population.
const SHIPYARD_SCALE_PER_POPULATION: f64 = 0.05;

/// Population per system: capitals large, claimed systems log-normal,
/// a share of unclaimed systems small frontier outposts, the rest empty.
pub fn population(rng: &mut Rng, state: &GalaxyState, territory: &Territory) -> Vec<f64> {
    (0..state.systems.len())
        .map(|i| {
            let mut r = rng.fork(i as u64);
            let system = SystemId::from_index(i as u32);
            if territory.capitals.contains(&system) {
                r.range(150.0, 300.0)
            } else if territory.claim[i].is_some() {
                // Log-normal-ish: most systems small, a few large.
                8.0 * (r.f64() + r.f64() + r.f64() - 1.5).mul_add(1.6, 0.0).exp()
            } else if r.chance(0.3) {
                r.range(0.3, 2.0) // frontier outposts
            } else {
                0.0
            }
        })
        .collect()
}

/// What a settlement can reach: the richest of each resource among its
/// system's bodies.
fn system_resources(state: &GalaxyState, system: SystemId) -> Resources {
    state.systems[system]
        .bodies
        .iter()
        .fold(Resources::default(), |acc, &b| {
            let r = state.bodies[b].resources;
            Resources {
                ore: acc.ore.max(r.ore),
                ice: acc.ice.max(r.ice),
                gas: acc.gas.max(r.gas),
            }
        })
}

/// Facilities and starting markets for every settlement.
pub fn seed(rng: &mut Rng, state: &mut GalaxyState) {
    let mut holdings: SecondaryMap<FactionId, usize> = SecondaryMap::new();
    for s in state.settlements.values().filter(|s| !state.is_port(s)) {
        *holdings.entry(s.owner).unwrap().or_default() += 1;
    }
    let ids: Vec<SettlementId> = state.settlements.keys().collect();

    // Candidate facilities with relative weights; scaled below.
    let mut placed: Vec<(usize, FacilityKind, f64)> = Vec::new();
    for (i, &id) in ids.iter().enumerate() {
        let settlement = &state.settlements[id];
        // A highport's rolls are its own, and the planet below does the
        // mining.
        let port = state.is_port(settlement);
        let mut r = rng.fork(settlement.system.index() as u64 | u64::from(port) << 40);
        let res = if port {
            Resources {
                gas: system_resources(state, settlement.system).gas,
                ..Resources::default()
            }
        } else {
            system_resources(state, settlement.system)
        };
        let pop = settlement.economy.population;
        // Independent outposts mine at half weight, as unclaimed space did.
        let owned = holdings[settlement.owner] > 1;
        if res.ore > 0.25 {
            placed.push((
                i,
                FacilityKind::Mine,
                res.ore * if owned { 1.0 } else { 0.5 },
            ));
        }
        if res.ice > 0.25 {
            placed.push((
                i,
                FacilityKind::IceHarvester,
                res.ice * if owned { 1.0 } else { 0.5 },
            ));
        }
        // Big settlements always grow some of their own food.
        if pop > 0.0 && (pop > 20.0 || r.chance(0.75)) {
            placed.push((i, FacilityKind::Hydroponics, pop));
        }
        // Fuel from a giant's gas where there is one to skim, else from
        // ice. The ice test's roll is drawn either way, so the rolls after
        // it are the same as before gas.
        let fuel = pop > 2.0 && (res.ice > 0.3 || r.chance(0.3));
        if pop > 2.0 && res.gas > 0.25 {
            placed.push((i, FacilityKind::GasPlant, pop.sqrt()));
        } else if fuel {
            placed.push((i, FacilityKind::FuelPlant, pop.sqrt()));
        }
        if pop > 3.0 && (res.ore > 0.2 || r.chance(0.3)) {
            placed.push((i, FacilityKind::Refinery, pop.sqrt()));
        }
        if pop > 15.0 || (pop > 5.0 && r.chance(0.2)) {
            placed.push((i, FacilityKind::Factory, pop));
        }
    }

    // Shipyards at big industrial settlements. They make hulls, not goods,
    // so they're sized by population and only feed into how much metal and
    // components the galaxy must make.
    for &(i, kind, _) in &placed {
        let pop = state.settlements[ids[i]].economy.population;
        if kind == FacilityKind::Factory && pop > SHIPYARD_MIN_POPULATION {
            state.settlements[ids[i]]
                .economy
                .facilities
                .push(Facility::new(
                    FacilityKind::Shipyard,
                    pop * SHIPYARD_SCALE_PER_POPULATION,
                ));
        }
    }

    // Where two kinds make one good (fuel from gas or ice), each makes its
    // share by placed weight.
    let weight =
        |kind: FacilityKind| -> f64 { placed.iter().filter(|p| p.1 == kind).map(|p| p.2).sum() };
    let share = |kind: FacilityKind| -> f64 {
        let Some(&(good, _)) = kind.recipe().outputs.first() else {
            return 1.0;
        };
        let all: f64 = FacilityKind::ALL
            .into_iter()
            .filter(|k| k.recipe().outputs.first().is_some_and(|o| o.0 == good))
            .map(weight)
            .sum();
        if all > 0.0 { weight(kind) / all } else { 0.0 }
    };
    let required = required_output(state, &share);
    for kind in FacilityKind::ALL {
        let total = weight(kind);
        if total <= 0.0 {
            continue;
        }
        let (good, per_scale) = kind.recipe().outputs[0];
        let scale_per_weight = required[good] * share(kind) / per_scale / total;
        for &(i, k, weight) in placed.iter().filter(|p| p.1 == kind) {
            state.settlements[ids[i]]
                .economy
                .facilities
                .push(Facility::new(k, weight * scale_per_weight));
        }
    }

    // Start every market at its target stock and price.
    for settlement in state.settlements.values_mut() {
        let model = state.factions[settlement.owner].economy;
        let economy = &mut settlement.economy;
        economy.refresh_demand();
        for good in Good::ALL {
            economy.market.stock[good] = economy.market.target(good);
        }
        economy.update_prices(model);
        economy.treasury = STARTING_TREASURY_DAYS
            * Good::ALL
                .iter()
                .map(|&g| economy.market.demand[g] * g.base_price())
                .sum::<f64>();
    }
}
/// Daily output of each good the galaxy needs, working back up the supply
/// chain from what the population consumes.
fn required_output(state: &GalaxyState, share: &dyn Fn(FacilityKind) -> f64) -> GoodMap<f64> {
    let population: f64 = state
        .settlements
        .values()
        .map(|s| s.economy.population)
        .sum();
    let mut need = GoodMap::splat(0.0);
    for (good, per_capita) in PER_CAPITA_DEMAND {
        need[good] += per_capita * population;
    }
    // Headroom for building: construction and colony ships need metal and
    // components beyond what people consume.
    need[Good::Metal] += BUILD_METAL_PER_CAPITA * population;
    need[Good::Components] += BUILD_COMPONENTS_PER_CAPITA * population;
    for s in state.settlements.values() {
        for f in s
            .economy
            .facilities
            .iter()
            .filter(|f| f.kind == FacilityKind::Shipyard)
        {
            for &(good, amount) in f.kind.recipe().inputs {
                need[good] += amount * f.scale;
            }
        }
    }
    // Walk the chain from finished goods down to raw materials, adding each
    // stage's inputs to what its suppliers must make.
    let order = [
        FacilityKind::Factory,
        FacilityKind::Refinery,
        FacilityKind::Hydroponics,
        FacilityKind::FuelPlant,
        FacilityKind::GasPlant,
    ];
    for kind in order {
        let recipe = kind.recipe();
        let (out_good, per_scale) = recipe.outputs[0];
        let runs = need[out_good] * SURPLUS * share(kind) / per_scale;
        for &(good, amount) in recipe.inputs {
            need[good] += amount * runs;
        }
    }
    for good in Good::ALL {
        need[good] *= SURPLUS;
    }
    need
}
