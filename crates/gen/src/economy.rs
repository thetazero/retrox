//! Resources, population and facilities at the start of history.
//!
//! Facilities are placed where they make sense (mines on ore, factories in
//! populous systems), then each kind is scaled so the galaxy as a whole
//! produces a little more than it consumes. Whether a given system is
//! short is then up to the trade network.

use retrox_sim::economy::{Facility, FacilityKind, Good, GoodMap, PER_CAPITA_DEMAND};
use retrox_sim::{EconomyModel, GalaxyState, Resources, StarClass};

use crate::rng::Rng;

/// Galaxy-wide production as a multiple of what consumption needs.
const SURPLUS: f64 = 1.15;

pub fn seed(rng: &mut Rng, state: &mut GalaxyState) {
    let capitals: Vec<_> = state.factions.values().map(|f| f.capital).collect();

    for (id, system) in state.systems.iter_mut() {
        let mut r = rng.fork(id.index() as u64);
        system.resources = resources(&mut r, system.star);
        system.economy.population = if capitals.contains(&id) {
            r.range(150.0, 300.0)
        } else if system.owner.is_some() {
            // Log-normal-ish: most systems small, a few large.
            8.0 * (r.f64() + r.f64() + r.f64() - 1.5).mul_add(1.6, 0.0).exp()
        } else if r.chance(0.3) {
            r.range(0.3, 2.0) // frontier outposts
        } else {
            0.0
        };
    }

    // Candidate facilities with relative weights; scaled below.
    let mut placed: Vec<(usize, FacilityKind, f64)> = Vec::new();
    for (id, system) in state.systems.iter() {
        let mut r = rng.fork(0x1000_0000 + id.index() as u64);
        let (res, pop, owned) = (
            system.resources,
            system.economy.population,
            system.owner.is_some(),
        );
        let i = id.index();
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
        if pop > 0.0 && r.chance(0.75) {
            placed.push((i, FacilityKind::Hydroponics, pop));
        }
        if pop > 2.0 && (res.ice > 0.3 || r.chance(0.3)) {
            placed.push((i, FacilityKind::FuelPlant, pop.sqrt()));
        }
        if pop > 3.0 && (res.ore > 0.2 || r.chance(0.3)) {
            placed.push((i, FacilityKind::Refinery, pop.sqrt()));
        }
        if pop > 15.0 || (pop > 5.0 && r.chance(0.2)) {
            placed.push((i, FacilityKind::Factory, pop));
        }
    }

    let required = required_output(state);
    for kind in FacilityKind::ALL {
        let total: f64 = placed.iter().filter(|p| p.1 == kind).map(|p| p.2).sum();
        if total <= 0.0 {
            continue;
        }
        let (good, per_scale) = kind.recipe().outputs[0];
        let scale_per_weight = required[good] / per_scale / total;
        for &(i, k, weight) in placed.iter().filter(|p| p.1 == kind) {
            state.systems.as_mut_slice()[i]
                .economy
                .facilities
                .push(Facility {
                    kind: k,
                    scale: weight * scale_per_weight,
                });
        }
    }

    // Start every market at its target stock and price.
    let models: Vec<EconomyModel> = state
        .systems
        .as_slice()
        .iter()
        .map(|s| {
            s.owner
                .map_or(EconomyModel::Market, |f| state.factions[f].economy)
        })
        .collect();
    for (system, model) in state.systems.as_mut_slice().iter_mut().zip(models) {
        let economy = &mut system.economy;
        economy.refresh_demand();
        for good in Good::ALL {
            economy.market.stock[good] = economy.market.target(good);
        }
        economy.update_prices(model);
    }
}

fn resources(r: &mut Rng, star: StarClass) -> Resources {
    use StarClass::*;
    let ore_bias = match star {
        O | B | Neutron | WhiteDwarf | BlackHole => 1.6,
        _ => 1.0,
    };
    let ice_bias = match star {
        K | M => 1.5,
        O | B => 0.5,
        _ => 1.0,
    };
    Resources {
        ore: (r.f64().powi(2) * ore_bias).min(1.0),
        ice: (r.f64().powi(2) * ice_bias).min(1.0),
    }
}

/// Daily output of each good the galaxy needs, working back up the supply
/// chain from what the population consumes.
fn required_output(state: &GalaxyState) -> GoodMap<f64> {
    let population: f64 = state
        .systems
        .as_slice()
        .iter()
        .map(|s| s.economy.population)
        .sum();
    let mut need = GoodMap::splat(0.0);
    for (good, per_capita) in PER_CAPITA_DEMAND {
        need[good] += per_capita * population;
    }
    // Walk the chain from finished goods down to raw materials, adding each
    // stage's inputs to what its suppliers must make.
    let order = [
        FacilityKind::Factory,
        FacilityKind::Refinery,
        FacilityKind::Hydroponics,
        FacilityKind::FuelPlant,
    ];
    for kind in order {
        let recipe = kind.recipe();
        let (out_good, per_scale) = recipe.outputs[0];
        let runs = need[out_good] * SURPLUS / per_scale;
        for &(good, amount) in recipe.inputs {
            need[good] += amount * runs;
        }
    }
    for good in Good::ALL {
        need[good] *= SURPLUS;
    }
    need
}
