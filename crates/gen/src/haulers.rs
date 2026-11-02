//! Trade ship classes and the companies that run them, for the static
//! starting galaxy. Prehistory will grow companies from the cradle
//! instead; until then a few per populated settlement keep goods moving.

use retrox_content::ships::{HULLS, stock};
use retrox_sim::haul::{Company, Fleet, Location, ShipClass, ShipClassId};
use retrox_sim::{GalaxyState, SettlementId};

use crate::names;
use crate::rng::Rng;

/// Every stock hull that can carry cargo becomes a trade ship class.
pub fn classes(state: &mut GalaxyState) -> Vec<ShipClassId> {
    let mut ids = Vec::new();
    for hull in HULLS {
        let stats = stock(hull.id).expect("every hull has a stock fit").stats();
        if stats.cargo_t <= 0.0 {
            continue;
        }
        let laden = stats.laden();
        ids.push(state.ship_classes.push(ShipClass {
            hull: hull.id.to_owned(),
            cargo_t: stats.cargo_t,
            mass_t: stats.mass_t,
            accel_laden: laden.acceleration(),
            jump_laden: laden.jump_speed(),
            drive_tier: stats.drive_tier,
        }));
    }
    ids
}

/// A few companies at every populated settlement, more and bigger at
/// large ones, each starting docked at home with one fleet.
pub fn seed(rng: &mut Rng, state: &mut GalaxyState, classes: &[ShipClassId]) {
    const SUFFIXES: &[&str] = &[
        "Haulage",
        "Transport",
        "Shipping",
        "Freight",
        "Carriers",
        "Logistics Cooperative",
    ];
    let homes: Vec<(SettlementId, f64, String)> = state
        .settlements
        .iter()
        .map(|(id, s)| (id, s.economy.population, s.name.clone()))
        .collect();
    for (i, (home, population, name)) in homes.into_iter().enumerate() {
        let mut r = rng.fork(i as u64);
        let count = (population / 10.0).sqrt().round() as usize;
        let count = if population >= 1.0 { count.max(1) } else { 0 };
        for _ in 0..count {
            // Big freighters only at big ports.
            let weights: Vec<f64> = classes
                .iter()
                .map(|&c| {
                    let cargo = state.ship_classes[c].cargo_t;
                    if cargo > 1000.0 && population < 50.0 {
                        0.0
                    } else {
                        1.0
                    }
                })
                .collect();
            let class = classes[r.weighted(&weights)];
            let ships = 1 + r.below(3) as u32;
            let capacity = state.ship_classes[class].cargo_t * f64::from(ships);
            let base = if r.chance(0.5) {
                name.split(' ').next().unwrap_or(&name).to_owned()
            } else {
                names::word(&mut r)
            };
            retrox_sim::haul::add_company(
                state,
                Company {
                    name: format!("{base} {}", r.pick(SUFFIXES)),
                    home,
                    // Enough to fill the holds a couple of times over.
                    credits: capacity * 40.0,
                    fleets: vec![Fleet {
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
                        harvest: None,
                    }],
                    next_fleet: 0,
                },
            );
        }
    }
}

/// Harvesting companies (issue #68) wherever a settlement wants more gas or
/// ice than it makes and its system has a source of it: enough ships to
/// cover the gap with a little to spare, in companies of a few fleets.
pub fn seed_harvesters(rng: &mut Rng, state: &mut GalaxyState, classes: &[ShipClassId]) {
    use retrox_sim::economy::{FacilityKind, Good};
    use retrox_sim::harvest::{self, Plan};

    const MARGIN: f64 = 1.2;
    const SHIPS_PER_COMPANY: u32 = 6;
    const MAX_SHIPS: u32 = 48;
    // Small holds for small gaps, haulers for the rest.
    let by_cargo = |small: bool| {
        classes
            .iter()
            .copied()
            .filter(|&c| {
                let cargo = state.ship_classes[c].cargo_t;
                if small { cargo < 100.0 } else { (100.0..1000.0).contains(&cargo) }
            })
            .max_by(|&a, &b| {
                state.ship_classes[a]
                    .cargo_t
                    .total_cmp(&state.ship_classes[b].cargo_t)
            })
    };
    let (Some(small), Some(big)) = (by_cargo(true), by_cargo(false)) else {
        return;
    };
    let homes: Vec<SettlementId> = state.settlements.keys().collect();
    for (i, home) in homes.into_iter().enumerate() {
        let mut r = rng.fork(i as u64);
        let system = state.settlements[home].system;
        let sources = harvest::sources_in(state, system);
        // Ice from space only where nobody in the system mines it on the
        // ground; generation already sizes those facilities to the galaxy.
        let ice_mined = state
            .settlements_in(system)
            .any(|(_, s)| {
                s.economy
                    .facilities
                    .iter()
                    .any(|f| f.kind == FacilityKind::IceHarvester)
            });
        for good in [Good::Gas, Good::Ice] {
            if good == Good::Ice && ice_mined {
                continue;
            }
            let gap = state.settlements[home].economy.market.demand[good] * MARGIN;
            if gap <= 0.0 {
                continue;
            }
            let Some(&source) = sources
                .iter()
                .filter(|&&s| harvest::yields(state, s).is_some_and(|y| y.0 == good))
                .max_by(|&&a, &&b| {
                    let ra = harvest::yields(state, a).map_or(0.0, |y| y.1);
                    let rb = harvest::yields(state, b).map_or(0.0, |y| y.1);
                    ra.total_cmp(&rb)
                })
            else {
                continue;
            };
            let class = if gap < 10.0 { small } else { big };
            let c = &state.ship_classes[class];
            let plan = Plan {
                source,
                good,
                market: home,
            };
            let (out, work, back) = harvest::run_days(state, c.accel_laden, home, &plan);
            let per_ship = c.cargo_t / (out + work + back);
            let mut ships = ((gap / per_ship).ceil() as u32).clamp(1, MAX_SHIPS);
            let base = state.settlements[home]
                .name
                .split(' ')
                .next()
                .unwrap_or("Free")
                .to_owned();
            while ships > 0 {
                let here = ships.min(SHIPS_PER_COMPANY);
                ships -= here;
                let name = if r.chance(0.5) {
                    format!("{base} {}", harvest::trade_name(good))
                } else {
                    format!("{} {}", names::word(&mut r), harvest::trade_name(good))
                };
                let mut company = harvest::company(name, home, class, here);
                company.credits = state.ship_classes[class].cargo_t * f64::from(here) * 10.0;
                retrox_sim::haul::add_company(state, company);
            }
        }
    }
}
