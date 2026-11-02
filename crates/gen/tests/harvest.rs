//! Harvesting (issue #68): ships gather ice and gas from space and sell it,
//! in the coarse tier and flown for real in the bubble.

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::bubble;
use retrox_sim::economy::{FacilityKind, Good};
use retrox_sim::harvest::{self, Source};
use retrox_sim::{BodyKind, GalaxyState, flight, tick};

fn small() -> GalaxyState {
    generate(
        9,
        &GalaxyConfig {
            systems: 600,
            radius_ly: 100.0,
            factions: 5,
            ..GalaxyConfig::default()
        },
    )
}

fn harvester_ships(state: &GalaxyState, good: Good) -> u32 {
    state
        .companies
        .values()
        .flat_map(|c| &c.fleets)
        .filter(|f| {
            f.harvest
                .and_then(|h| h.plan)
                .is_some_and(|p| p.good == good)
        })
        .map(|f| f.ships)
        .sum()
}

#[test]
fn gas_giants_have_gas_and_nothing_else_does() {
    let state = small();
    for body in state.bodies.as_slice() {
        if body.kind == BodyKind::GasGiant {
            assert!((0.2..=1.0).contains(&body.resources.gas), "{}", body.name);
        } else {
            assert_eq!(body.resources.gas, 0.0, "{}", body.name);
        }
    }
}

#[test]
fn harvesters_feed_gas_plants_and_icy_markets() {
    let mut state = small();
    let plants = state
        .settlements
        .values()
        .filter(|s| {
            s.economy
                .facilities
                .iter()
                .any(|f| f.kind == FacilityKind::GasPlant)
        })
        .count();
    assert!(plants > 0, "no gas plants placed");
    for _ in 0..120 {
        tick(&mut state);
    }
    assert!(harvester_ships(&state, Good::Gas) > 0, "nobody skims gas");
    assert!(
        harvester_ships(&state, Good::Ice) > 0,
        "nobody harvests ice"
    );
    // Gas reaches the plants, and they run.
    let (mut demand, mut short) = (0.0, 0.0);
    for s in state.settlements.values() {
        demand += s.economy.market.demand[Good::Gas];
        short += s.economy.market.shortfall[Good::Gas];
    }
    assert!(demand > 0.0);
    assert!(
        short < 0.25 * demand,
        "gas plants starved: {short:.0} of {demand:.0} a day unmet"
    );
    // Sources worked lately carry strain.
    assert!(harvest::mean_strain(&state) > 0.0);
}

#[test]
fn strain_slows_work_and_fades() {
    let mut state = small();
    let source = state
        .bodies
        .iter()
        .find(|(_, b)| b.kind == BodyKind::GasGiant)
        .map(|(id, _)| Source::Body(id))
        .expect("a gas giant");
    let fresh = harvest::work_days(&state, source);
    harvest::strain(&mut state, source, 80_000.0);
    let strained = harvest::work_days(&state, source);
    assert!(strained > fresh * 1.5, "{fresh} -> {strained}");
    for _ in 0..360 {
        state.time = retrox_sim::GalaxyTime::from_days(state.time.days() + 1);
    }
    let recovered = harvest::work_days(&state, source);
    assert!(recovered < strained, "{strained} -> {recovered}");
    assert!(recovered < fresh * 1.1, "{fresh} -> {recovered}");
}

/// In the bubble, harvesters fly to their source, hold station there while
/// they work, and come back with a full hold.
#[test]
fn harvesters_work_their_source_in_the_bubble() {
    let mut state = small();
    for _ in 0..20 {
        tick(&mut state);
    }
    // The system with the most harvester ships.
    let mut counts = std::collections::BTreeMap::new();
    for c in state.companies.values() {
        for f in c.fleets.iter().filter(|f| f.harvest.is_some()) {
            *counts.entry(state.settlements[c.home].system).or_insert(0) += f.ships;
        }
    }
    let centre = counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(s, _)| *s)
        .expect("harvesters somewhere");
    bubble::set_centre(&mut state, Some(centre));
    let (mut worked, mut delivered) = (0, 0);
    for _ in 0..60 {
        tick(&mut state);
        let t = state.time.days() as f64;
        for (key, f, ship) in bubble::flights_in(&state, centre) {
            let Some(work) = f.work else { continue };
            let fleet = state.companies[key.0]
                .fleets
                .iter()
                .find(|x| x.id == key.1)
                .expect("fleet");
            if work.done {
                assert!(fleet.cargo.is_some(), "a finished run with an empty hold");
                delivered += 1;
                continue;
            }
            let at = harvest::work_goal(&state, work.source, work.slot, t).pos;
            if flight::length(flight::sub(at, ship.pos)) < 1_000.0 {
                worked += 1;
            }
        }
    }
    assert!(worked > 0, "no harvester was ever seen at its source");
    assert!(delivered > 0, "no harvester filled its hold");
}

/// A work goal's velocity is how its position actually moves, offset
/// turning with the source included: the autopilot only counts a ship
/// arrived within a few m/s of it, so a goal that lies about its speed is
/// never reached (issue #115).
#[test]
fn work_goals_move_as_they_say() {
    let state = small();
    let h = 20.0;
    let mut checked = 0;
    for (system, _) in state.systems.iter() {
        for source in harvest::sources_in(&state, system) {
            for slot in [0.0, 0.3, 0.7] {
                let t = 100.0;
                let a = harvest::work_goal(&state, source, slot, t);
                let before = harvest::work_goal(&state, source, slot, t - h / 86_400.0);
                let after = harvest::work_goal(&state, source, slot, t + h / 86_400.0);
                let moved = flight::scale(flight::sub(after.pos, before.pos), 0.5 / h);
                let off = flight::length(flight::sub(moved, a.vel));
                assert!(
                    off < 0.5,
                    "{}: goal moves {off:.2} m/s off its stated velocity",
                    harvest::name(&state, source)
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "only {checked} goals checked");
}
