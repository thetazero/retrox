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

/// The hulls pirate gangs fly (DESIGN §7.2.2): light fighters, and a
/// gunship for a gang big enough to crew one.
pub fn raider_hulls(state: &mut GalaxyState) {
    for (id, heavy) in [("interceptor", false), ("heavy-fighter", false), ("gunship", true)] {
        let Some(fit) = stock(id) else {
            continue;
        };
        state.pirates.hulls.push(retrox_sim::pirates::RaiderHull {
            id: id.to_owned(),
            accel: fit.stats().acceleration(),
            heavy,
        });
    }
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
                    }],
                    next_fleet: 0,
                },
            );
        }
    }
}
