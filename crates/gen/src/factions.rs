//! The static starting civilization: factions, their territory, and one
//! settlement in every populated system. Prehistory will replace this
//! (DESIGN §4.2); until then it gives tools and tests a populated galaxy.
//!
//! Capitals are spread out with best-candidate sampling. Each faction then
//! grows outward along hyperlanes (a multi-source Dijkstra over lane
//! length, scaled by the faction's strength) until it reaches its share of
//! `claimed_fraction`. Nobody owns a system, so the territory is only used
//! to decide who owns each system's settlement. Populated systems outside
//! every faction's territory become independent, one-settlement factions.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use retrox_sim::economy::LocalEconomy;
use retrox_sim::{
    BodyKind, EconomyModel, Faction, FactionId, GalaxyState, GalaxyTime, HistoryKind, Orbit,
    Orbiting, Settlement, SettlementId, Site, SystemId,
};

use crate::GalaxyConfig;
use crate::bodies::AU_KM;
use crate::names;
use crate::rng::Rng;

/// The major factions' plan before anything is founded.
pub struct Territory {
    /// Capital system of each major faction.
    pub capitals: Vec<SystemId>,
    /// Which major faction (by index) claims each system.
    pub claim: Vec<Option<usize>>,
}

/// Choose capitals and grow each major faction's territory.
pub fn claim(rng: &mut Rng, state: &GalaxyState, config: &GalaxyConfig) -> Territory {
    let mut claim = vec![None; state.systems.len()];
    let count = config.factions.min(state.systems.len());
    if count == 0 {
        return Territory {
            capitals: Vec::new(),
            claim,
        };
    }
    let capitals = pick_capitals(rng, state, count, config.radius_ly);

    let strengths: Vec<f64> = (0..count).map(|_| rng.range(0.5, 1.5)).collect();
    let total_strength: f64 = strengths.iter().sum();
    let claim_total = config.claimed_fraction * state.systems.len() as f64;

    // Multi-source Dijkstra. Keys are (cost, faction, system) so ties break
    // deterministically; costs are non-negative so f64 bits order correctly.
    let mut sizes = vec![0usize; count];
    let targets: Vec<usize> = strengths
        .iter()
        .map(|s| ((claim_total * s / total_strength).round() as usize).max(1))
        .collect();
    let mut heap = BinaryHeap::new();
    for (f, &capital) in capitals.iter().enumerate() {
        heap.push(Reverse((0.0f64.to_bits(), f, capital)));
    }
    while let Some(Reverse((cost_bits, f, system))) = heap.pop() {
        if claim[system.index()].is_some() || sizes[f] >= targets[f] {
            continue;
        }
        claim[system.index()] = Some(f);
        sizes[f] += 1;
        let cost = f64::from_bits(cost_bits);
        for &lane in &state.systems[system].lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            if claim[next.index()].is_none() {
                let step = lane.length_ly / strengths[f];
                heap.push(Reverse(((cost + step).to_bits(), f, next)));
            }
        }
    }
    Territory { capitals, claim }
}

/// Found the factions and one settlement in every system with population.
pub fn found(rng: &mut Rng, state: &mut GalaxyState, territory: &Territory, population: &[f64]) {
    let mut taken: HashSet<String> = HashSet::new();

    let mut majors: Vec<FactionId> = Vec::with_capacity(territory.capitals.len());
    for (i, &capital) in territory.capitals.iter().enumerate() {
        let economy = if rng.chance(0.5) {
            EconomyModel::Planned
        } else {
            EconomyModel::Market
        };
        let capital_name = state.systems[capital].name.clone();
        let mut name = faction_name(rng, &capital_name, economy, false);
        while taken.contains(&name) {
            name = faction_name(rng, &capital_name, economy, true);
        }
        taken.insert(name.clone());
        let id = state.factions.insert(Faction {
            name,
            economy,
            // Set once the capital settlement exists.
            capital: SettlementId::default(),
            // Spread hues around the wheel by the golden angle.
            color: hsl((i as f64 * 137.508) % 360.0, 0.65, 0.6),
            founded: GalaxyTime::ZERO,
            treasury: 0.0,
            population: 0.0,
            welfare: Default::default(),
        });
        state.history.record(
            GalaxyTime::ZERO,
            HistoryKind::FactionFounded { faction: id },
        );
        majors.push(id);
    }

    for (i, &population) in population.iter().enumerate() {
        if population <= 0.0 {
            continue;
        }
        let system = SystemId::from_index(i as u32);
        let mut r = rng.fork(i as u64);
        let (site, settlement_name) = main_site(&mut r, state, system);
        let owner = match territory.claim[i] {
            Some(f) => majors[f],
            None => independent(&mut r, state, &settlement_name, &mut taken),
        };
        let settlement = state.add_settlement(Settlement {
            system,
            site,
            owner,
            name: settlement_name,
            founded: GalaxyTime::ZERO,
            economy: LocalEconomy {
                population,
                ..LocalEconomy::default()
            },
            to_capital: None,
        });
        let faction = &mut state.factions[owner];
        // An independent's only settlement is its capital.
        if territory.claim[i].is_none_or(|f| territory.capitals[f] == system) {
            faction.capital = settlement;
        }
        state.history.record(
            GalaxyTime::ZERO,
            HistoryKind::SettlementFounded {
                settlement,
                faction: owner,
            },
        );
    }

    crate::factions::refresh_populations(state);
    // Capitals always have population, so every faction holds something.
    debug_assert!(
        state
            .factions
            .values()
            .all(|f| state.settlements.contains_key(f.capital))
    );
}

/// Where a system's main settlement goes: the most habitable region of any
/// world, else the most habitable undivided body, else a station around
/// the richest body (or the star, in a system with no bodies).
fn main_site(r: &mut Rng, state: &GalaxyState, system: SystemId) -> (Site, String) {
    let bodies = &state.systems[system].bodies;
    let best_region = bodies
        .iter()
        .flat_map(|&b| state.bodies[b].regions.iter().copied())
        .max_by(|&a, &b| {
            state.regions[a]
                .habitability
                .total_cmp(&state.regions[b].habitability)
                .then(b.cmp(&a))
        });
    if let Some(region) = best_region {
        return (Site::Region(region), state.regions[region].name.clone());
    }
    let best_body = bodies
        .iter()
        .copied()
        .filter(|&b| !matches!(state.bodies[b].kind, BodyKind::GasGiant | BodyKind::Belt))
        .max_by(|&a, &b| {
            state.bodies[a]
                .habitability
                .total_cmp(&state.bodies[b].habitability)
                .then(b.cmp(&a))
        });
    if let Some(body) = best_body
        && state.bodies[body].habitability > 0.02
    {
        return (Site::Surface(body), state.bodies[body].name.clone());
    }
    let richest = bodies.iter().copied().max_by(|&a, &b| {
        let worth = |x: retrox_sim::BodyId| {
            let res = state.bodies[x].resources;
            res.ore + res.ice
        };
        worth(a).total_cmp(&worth(b)).then(b.cmp(&a))
    });
    let phase = r.range(0.0, std::f64::consts::TAU);
    match richest {
        Some(body) => {
            let b = &state.bodies[body];
            let distance_km = match b.kind {
                // Inside the belt itself.
                BodyKind::Belt => 0.0,
                _ => b.radius_km * r.range(2.0, 6.0),
            };
            let orbit = if b.kind == BodyKind::Belt {
                Orbit {
                    mean_anomaly_epoch: phase,
                    ..b.orbit
                }
            } else {
                Orbit::circular(Orbiting::Body(body), distance_km / AU_KM, phase)
            };
            (Site::Space(orbit), format!("{} Station", b.name))
        }
        None => (
            Site::Space(Orbit::circular(Orbiting::Star, r.range(0.5, 3.0), phase)),
            format!("{} Station", state.systems[system].name),
        ),
    }
}

/// A one-settlement faction for an independent settlement.
fn independent(
    r: &mut Rng,
    state: &mut GalaxyState,
    settlement_name: &str,
    taken: &mut HashSet<String>,
) -> FactionId {
    const TEMPLATES: &[&str] = &[
        "Free {}",
        "{} Independents",
        "{} Free Port",
        "Autonomous {}",
        "{} Commune",
    ];
    let base = settlement_name.split(' ').next().unwrap_or(settlement_name);
    let mut name = r.pick(TEMPLATES).replace("{}", base);
    while taken.contains(&name) {
        name = r.pick(TEMPLATES).replace("{}", &names::word(r));
    }
    taken.insert(name.clone());
    let economy = if r.chance(0.7) {
        EconomyModel::Market
    } else {
        EconomyModel::Planned
    };
    let id = state.factions.insert(Faction {
        name,
        economy,
        capital: SettlementId::default(),
        // Muted, so independents don't drown out the major powers.
        color: hsl(r.range(0.0, 360.0), 0.12, 0.42),
        founded: GalaxyTime::ZERO,
        treasury: 0.0,
        population: 0.0,
        welfare: Default::default(),
    });
    state.history.record(
        GalaxyTime::ZERO,
        HistoryKind::FactionFounded { faction: id },
    );
    id
}

/// Spread capitals out: each new capital is the best of several random
/// candidates, "best" meaning far from every existing capital, weighted
/// toward the inner galaxy.
fn pick_capitals(
    rng: &mut Rng,
    state: &GalaxyState,
    count: usize,
    radius_ly: f64,
) -> Vec<SystemId> {
    // Prefer well-connected systems away from the rim.
    let candidates: Vec<SystemId> = state
        .systems
        .iter()
        .filter(|(_, s)| {
            s.lanes.len() >= 3 && s.position.distance(Default::default()) < 0.9 * radius_ly
        })
        .map(|(id, _)| id)
        .collect();
    let pool: Vec<SystemId> = if candidates.len() >= count {
        candidates
    } else {
        state.systems.ids().collect()
    };

    let mut capitals: Vec<SystemId> = vec![*rng.pick(&pool)];
    while capitals.len() < count {
        let best = (0..24)
            .map(|_| *rng.pick(&pool))
            .filter(|c| !capitals.contains(c))
            .max_by(|&a, &b| {
                let score = |c: SystemId| {
                    // Spread out, but favour the populous inner galaxy.
                    let r = state.systems[c].position.distance(Default::default()) / radius_ly;
                    nearest_capital(state, &capitals, c) * (1.4 - r)
                };
                score(a).total_cmp(&score(b)).then(a.cmp(&b))
            });
        if let Some(best) = best {
            capitals.push(best);
        }
    }
    capitals
}

fn nearest_capital(state: &GalaxyState, capitals: &[SystemId], system: SystemId) -> f64 {
    let pos = state.systems[system].position;
    capitals
        .iter()
        .map(|&c| state.systems[c].position.distance(pos))
        .fold(f64::INFINITY, f64::min)
}

/// Faction names from the capital's name and the faction's character.
pub(crate) fn faction_name(
    rng: &mut Rng,
    capital: &str,
    economy: EconomyModel,
    fresh: bool,
) -> String {
    // The capital's base name, without suffixes like " Prime".
    let base = capital.split(' ').next().unwrap_or(capital);
    let base = if fresh || rng.chance(0.25) {
        names::word(rng)
    } else {
        base.to_owned()
    };
    let templates: &[&str] = match economy {
        EconomyModel::Planned => &[
            "{} Commonwealth",
            "Union of {}",
            "{} Cooperative",
            "{} Directorate",
            "Collective of {}",
            "{} Concord",
        ],
        EconomyModel::Market => &[
            "{} Mercantile League",
            "{} Compact",
            "Free Ports of {}",
            "{} Trade Federation",
            "{} Consortium",
            "{} Exchange",
        ],
    };
    rng.pick(templates).replace("{}", &base)
}

pub(crate) fn hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f64| ((v + m) * 255.0).round() as u8;
    [to(r), to(g), to(b)]
}

/// Each faction's population, as the ledger's debt limits need it.
pub fn refresh_populations(state: &mut GalaxyState) {
    retrox_sim::demography::refresh_faction_populations(state);
}
