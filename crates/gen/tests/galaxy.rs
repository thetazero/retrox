use std::collections::{HashSet, VecDeque};

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{BodyKind, FactionId, GalaxyState, Orbit, Orbiting, Settlement, Site, SystemId};

fn reachable(state: &GalaxyState) -> usize {
    let mut seen = vec![false; state.systems.len()];
    let mut queue = VecDeque::from([SystemId::from_index(0)]);
    seen[0] = true;
    let mut count = 1;
    while let Some(s) = queue.pop_front() {
        for n in state.neighbours(s) {
            if !seen[n.index()] {
                seen[n.index()] = true;
                count += 1;
                queue.push_back(n);
            }
        }
    }
    count
}

#[test]
fn default_galaxy_is_sane() {
    let config = GalaxyConfig::default();
    let state = generate(1, &config);

    assert_eq!(state.systems.len(), config.systems);
    assert_eq!(
        reachable(&state),
        config.systems,
        "lane graph must be connected"
    );

    let names: HashSet<&str> = state
        .systems
        .as_slice()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names.len(), config.systems, "names must be unique");

    for (_, s) in state.systems.iter() {
        assert!(s.position.distance(Default::default()) <= config.radius_ly + config.thickness_ly);
    }

    let mean_degree = 2.0 * state.lanes.len() as f64 / state.systems.len() as f64;
    assert!(
        (2.0..4.0).contains(&mean_degree),
        "mean lane degree {mean_degree:.2} outside the intended sparse range"
    );
}

#[test]
fn same_seed_same_galaxy() {
    let config = GalaxyConfig {
        systems: 800,
        radius_ly: 110.0,
        ..GalaxyConfig::default()
    };
    let a = serde_json::to_string(&generate(7, &config)).unwrap();
    let b = serde_json::to_string(&generate(7, &config)).unwrap();
    let c = serde_json::to_string(&generate(8, &config)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

/// Settlements held by `faction`.
fn holdings(state: &GalaxyState, faction: FactionId) -> Vec<&Settlement> {
    state
        .settlements
        .values()
        .filter(|s| s.owner == faction)
        .collect()
}

#[test]
fn factions_hold_contiguous_territory() {
    let config = GalaxyConfig::default();
    let state = generate(3, &config);

    let majors: Vec<FactionId> = state
        .factions
        .keys()
        .filter(|&f| holdings(&state, f).len() > 1)
        .collect();
    assert_eq!(majors.len(), config.factions);

    // Every faction holds something, starting with its capital.
    for (id, faction) in &state.factions {
        assert_eq!(state.settlements[faction.capital].owner, id);
    }
    let names: HashSet<&str> = state.factions.values().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names.len(),
        state.factions.len(),
        "faction names must be unique"
    );

    let held: usize = majors.iter().map(|&f| holdings(&state, f).len()).sum();
    let share = held as f64 / state.systems.len() as f64;
    assert!(
        (config.claimed_fraction - 0.1..=config.claimed_fraction + 0.02).contains(&share),
        "claimed share {share:.2}"
    );

    for &id in &majors {
        let systems: HashSet<SystemId> = holdings(&state, id).iter().map(|s| s.system).collect();
        let capital = state.settlements[state.factions[id].capital].system;
        // Every system the faction has settled is reachable from its
        // capital through its own systems.
        let mut seen = HashSet::from([capital]);
        let mut queue = VecDeque::from([capital]);
        while let Some(s) = queue.pop_front() {
            for n in state.neighbours(s) {
                if systems.contains(&n) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        assert_eq!(
            seen.len(),
            systems.len(),
            "{} territory is not contiguous",
            state.factions[id].name
        );
    }
}

#[test]
fn bodies_and_settlements_are_consistent() {
    let config = GalaxyConfig {
        systems: 800,
        radius_ly: 110.0,
        factions: 6,
        ..GalaxyConfig::default()
    };
    let state = generate(11, &config);
    assert!(
        state.bodies.len() > state.systems.len(),
        "systems should hold several bodies"
    );

    for (id, system) in state.systems.iter() {
        for &b in &system.bodies {
            let body = &state.bodies[b];
            assert_eq!(body.system, id);
            assert!((0.0..=1.0).contains(&body.habitability), "{}", body.name);
            if let Orbiting::Body(parent) = body.orbit.parent {
                assert_eq!(body.kind, BodyKind::Moon);
                assert_eq!(state.bodies[parent].system, id);
                assert!(matches!(
                    state.bodies[parent].kind,
                    BodyKind::Planet(_) | BodyKind::GasGiant
                ));
            }
            for &r in &body.regions {
                assert_eq!(state.regions[r].body, b);
            }
            let p = state.body_position(b, 1234.5);
            assert!(p.iter().all(|x| x.is_finite()), "{} position", body.name);
        }
        for (sid, s) in state.settlements_in(id) {
            assert_eq!(s.system, id);
            assert!(state.factions.contains_key(s.owner));
            if let Some(body) = state.settlement_body(sid) {
                assert_eq!(state.bodies[body].system, id);
            }
            if let Site::Region(r) = s.site {
                assert!(state.bodies[state.regions[r].body].regions.contains(&r));
            }
        }
    }
}

#[test]
fn circular_orbit_keeps_its_radius_and_period() {
    let orbit = Orbit::circular(Orbiting::Star, 1.0, 0.0);
    let start = orbit.position_at(0.0, 1.0);
    let quarter = orbit.position_at(365.25 / 4.0, 1.0);
    let full = orbit.position_at(365.25, 1.0);
    let r = |p: [f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    assert!((r(start) - 1.0).abs() < 1e-9 && (r(quarter) - 1.0).abs() < 1e-9);
    assert!(
        (quarter[1] - 1.0).abs() < 1e-9,
        "a quarter orbit is 90 degrees on"
    );
    assert!((full[0] - start[0]).abs() < 1e-9 && (full[1] - start[1]).abs() < 1e-9);
}
