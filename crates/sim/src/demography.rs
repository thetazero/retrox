//! Population (DESIGN §5.7, issue #16): settlements grow when fed, toward a
//! carrying capacity set by where they are, and shrink when they go
//! hungry.
//!
//! Growth is logistic. Capacity comes from the site: a region of a
//! habitable world holds the most, an undivided body less, a station least.
//! Food decides whether growth happens at all: a settlement that couldn't
//! buy all its food shrinks in proportion to the shortfall.

use crate::economy::Good;
use crate::economy::LocalEconomy;
use crate::{Arena, Body, GalaxyState, Region, SettlementId, Site};

/// Yearly growth rate of a fed population far below capacity.
const GROWTH_PER_YEAR: f64 = 0.03;
/// Yearly loss at a complete famine; scales with the share of food missing.
const STARVATION_PER_YEAR: f64 = 0.5;
/// Population a region of habitability 1.0 can hold.
const REGION_CAPACITY: f64 = 600.0;
/// Population an undivided body can hold: domes and burrows.
const SURFACE_CAPACITY: f64 = 150.0;
const SURFACE_BASE: f64 = 20.0;
/// Population a station can hold.
const STATION_CAPACITY: f64 = 40.0;
/// Famine starts when average hunger rises above this, and ends when it
/// falls below `FAMINE_ENDS`, so day-to-day delivery noise doesn't flicker.
const FAMINE: f64 = 0.5;
const FAMINE_ENDS: f64 = 0.2;
/// Days over which hunger is averaged.
const HUNGER_DAYS: f64 = 30.0;

const DAYS_PER_YEAR: f64 = crate::time::DAYS_PER_YEAR as f64;

/// How many people a settlement's site can hold.
pub fn capacity(state: &GalaxyState, id: SettlementId) -> f64 {
    site_capacity(state.settlements[id].site, &state.regions, &state.bodies)
}

/// How many people `site` can hold: two lookups and a multiply, cheap
/// enough for the daily books.
pub fn site_capacity(site: Site, regions: &Arena<Region>, bodies: &Arena<Body>) -> f64 {
    match site {
        Site::Region(r) => REGION_CAPACITY * regions[r].habitability,
        Site::Surface(b) => SURFACE_BASE + SURFACE_CAPACITY * bodies[b].habitability,
        Site::Space(_) => STATION_CAPACITY,
    }
}

/// One day of change for a population of `population` at `capacity`,
/// missing `hunger` (0..1) of its food.
pub fn change(population: f64, capacity: f64, hunger: f64) -> f64 {
    if hunger > 0.0 {
        return -population * (STARVATION_PER_YEAR / DAYS_PER_YEAR) * hunger;
    }
    let capacity = capacity.max(1e-6);
    population * (GROWTH_PER_YEAR / DAYS_PER_YEAR) * (1.0 - population / capacity)
}

/// Recompute every faction's total population.
pub fn refresh_faction_populations(state: &mut GalaxyState) {
    for f in state.factions.values_mut() {
        f.population = 0.0;
    }
    for s in state.settlements.values() {
        if let Some(f) = state.factions.get_mut(s.owner) {
            f.population += s.economy.population;
        }
    }
}

/// One day of population for a settlement that has just produced and
/// eaten: grow or shrink, refresh demand, and track hunger. Returns
/// whether a famine began today.
pub fn live(economy: &mut LocalEconomy, capacity: f64) -> bool {
    let population = economy.population;
    if population <= 0.0 {
        return false;
    }
    let wanted = crate::economy::per_capita(Good::Food) * population;
    let hunger = if wanted > 0.0 {
        (economy.market.shortfall[Good::Food] / wanted).clamp(0.0, 1.0)
    } else {
        0.0
    };
    economy.population = (population + change(population, capacity, hunger)).max(0.0);
    economy.refresh_demand();
    economy.hunger += (hunger - economy.hunger) * (1.0 / HUNGER_DAYS);
    let famine = if economy.famine {
        economy.hunger > FAMINE_ENDS
    } else {
        economy.hunger > FAMINE
    };
    let began = famine && !economy.famine;
    economy.famine = famine;
    began
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fed_populations_grow_toward_capacity() {
        let mut p = 10.0;
        for _ in 0..360 * 200 {
            p += change(p, 100.0, 0.0);
        }
        assert!((95.0..=100.0).contains(&p), "{p}");
        // Over capacity, they shrink back.
        let mut q = 150.0;
        for _ in 0..360 * 100 {
            q += change(q, 100.0, 0.0);
        }
        assert!((100.0..110.0).contains(&q), "{q}");
    }

    #[test]
    fn hungry_populations_shrink() {
        let mut p = 100.0;
        for _ in 0..360 {
            p += change(p, 1000.0, 1.0);
        }
        assert!(
            (55.0..65.0).contains(&p),
            "a year of total famine leaves {p}"
        );
        assert!(change(100.0, 1000.0, 0.2) < 0.0);
    }
}
