//! Highports (#112): a planet with a city also gets a station in orbit
//! above it, so the hop from a station down to the city (DESIGN §7.1.1)
//! happens in play. The port is its own small settlement, owned by the
//! planet's biggest city, with a share of that city's people.
//!
//! Ports orbit at 2.6–3.2 radii: above the 2-radii landing range, and far
//! enough inside the well's edge (4 radii) that a station's arrival point
//! is inside it too.

use std::collections::HashSet;

use retrox_sim::economy::LocalEconomy;
use retrox_sim::{
    BodyId, GalaxyState, HistoryKind, Orbit, Orbiting, Settlement, SettlementId, Site,
};

use crate::bodies::AU_KM;
use crate::rng::Rng;

/// Cities smaller than this get no port: frontier outposts can't keep one.
const MIN_CITY_POPULATION: f64 = 1.0;
/// The port's share of its city's people.
const PORT_SHARE: f64 = 0.08;
/// Orbit radius range, in planet radii.
const ORBIT_RADII: (f64, f64) = (2.6, 3.2);

/// Whether a settlement is a station above a body that has a city on it.
pub fn is_port(state: &GalaxyState, settlement: &Settlement) -> bool {
    state.is_port(settlement)
}

/// Put a port above every city planet that has nothing in orbit yet: one
/// per planet, owned by its biggest city.
pub fn found(rng: &mut Rng, state: &mut GalaxyState) {
    let served: HashSet<BodyId> = state
        .settlements
        .values()
        .filter_map(|s| match s.site {
            Site::Space(Orbit {
                parent: Orbiting::Body(b),
                ..
            }) => Some(b),
            _ => None,
        })
        .collect();
    // The biggest city on each body, in settlement order.
    let mut biggest: Vec<(BodyId, SettlementId)> = Vec::new();
    for (id, s) in state.settlements.iter() {
        let Site::Region(region) = s.site else {
            continue;
        };
        let body = state.regions[region].body;
        if served.contains(&body) {
            continue;
        }
        match biggest.iter_mut().find(|(b, _)| *b == body) {
            Some(entry) => {
                if s.economy.population > state.settlements[entry.1].economy.population {
                    entry.1 = id;
                }
            }
            None => biggest.push((body, id)),
        }
    }

    for (body, city) in biggest {
        let population = state.settlements[city].economy.population;
        if population < MIN_CITY_POPULATION {
            continue;
        }
        let mut r = rng.fork(body.index() as u64);
        let radius_km = state.bodies[body].radius_km * r.range(ORBIT_RADII.0, ORBIT_RADII.1);
        let phase = r.range(0.0, std::f64::consts::TAU);
        let port_population = population * PORT_SHARE;
        state.settlements[city].economy.population -= port_population;
        let city = &state.settlements[city];
        let (system, owner, founded) = (city.system, city.owner, city.founded);
        let name = format!("{} Highport", state.bodies[body].name);
        let port = state.add_settlement(Settlement {
            system,
            site: Site::Space(Orbit::circular(
                Orbiting::Body(body),
                radius_km / AU_KM,
                phase,
            )),
            owner,
            name,
            founded,
            economy: LocalEconomy {
                population: port_population,
                ..LocalEconomy::default()
            },
        });
        state.history.record(
            founded,
            HistoryKind::SettlementFounded {
                settlement: port,
                faction: owner,
            },
        );
    }
}
