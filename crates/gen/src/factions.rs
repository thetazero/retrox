//! The factions history starts with, and the territory they hold.
//!
//! Capitals are spread out with best-candidate sampling. Each faction then
//! grows outward along hyperlanes (a multi-source Dijkstra over lane
//! length, scaled by the faction's strength) until it reaches its share of
//! `claimed_fraction`. Territory is contiguous by construction, and what's
//! left over is unclaimed frontier.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use retrox_sim::{
    EconomyModel, Faction, FactionId, GalaxyState, GalaxyTime, HistoryKind, SystemId,
};

use crate::GalaxyConfig;
use crate::names;
use crate::rng::Rng;

pub fn seed(rng: &mut Rng, state: &mut GalaxyState, config: &GalaxyConfig) {
    let count = config.factions.min(state.systems.len());
    if count == 0 {
        return;
    }
    let capitals = pick_capitals(rng, state, count, config.radius_ly);

    let strengths: Vec<f64> = (0..count).map(|_| rng.range(0.5, 1.5)).collect();
    let total_strength: f64 = strengths.iter().sum();
    let claim_total = config.claimed_fraction * state.systems.len() as f64;

    let mut ids: Vec<FactionId> = Vec::with_capacity(count);
    for (i, &capital) in capitals.iter().enumerate() {
        let economy = if rng.chance(0.5) {
            EconomyModel::Planned
        } else {
            EconomyModel::Market
        };
        let mut name = faction_name(rng, &state.systems[capital].name, economy, false);
        while state.factions.values().any(|f| f.name == name) {
            name = faction_name(rng, &state.systems[capital].name, economy, true);
        }
        let id = state.factions.insert(Faction {
            name,
            economy,
            capital,
            // Spread hues around the wheel by the golden angle.
            color: hsl((i as f64 * 137.508) % 360.0, 0.65, 0.6),
            founded: GalaxyTime::ZERO,
        });
        state.history.record(
            GalaxyTime::ZERO,
            HistoryKind::FactionFounded { faction: id },
        );
        ids.push(id);
    }

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
        if state.systems[system].owner.is_some() || sizes[f] >= targets[f] {
            continue;
        }
        state.systems[system].owner = Some(ids[f]);
        sizes[f] += 1;
        let cost = f64::from_bits(cost_bits);
        let lanes = state.systems[system].lanes.clone();
        for lane in lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            if state.systems[next].owner.is_none() {
                let step = lane.length_ly / strengths[f];
                heap.push(Reverse(((cost + step).to_bits(), f, next)));
            }
        }
    }
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
fn faction_name(rng: &mut Rng, capital: &str, economy: EconomyModel, fresh: bool) -> String {
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

fn hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
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
