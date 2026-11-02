use std::collections::{HashSet, VecDeque};

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{GalaxyState, SystemId};

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

#[test]
fn factions_hold_contiguous_territory() {
    let config = GalaxyConfig::default();
    let state = generate(3, &config);
    assert_eq!(state.factions.len(), config.factions);

    let owned = state
        .systems
        .as_slice()
        .iter()
        .filter(|s| s.owner.is_some())
        .count();
    let share = owned as f64 / state.systems.len() as f64;
    assert!(
        (config.claimed_fraction - 0.1..=config.claimed_fraction + 0.02).contains(&share),
        "claimed share {share:.2}"
    );

    let names: HashSet<&str> = state.factions.values().map(|f| f.name.as_str()).collect();
    assert_eq!(names.len(), config.factions, "faction names must be unique");

    for (id, faction) in &state.factions {
        assert_eq!(state.systems[faction.capital].owner, Some(id));
        // Everything the faction owns is reachable from its capital
        // through its own systems.
        let mut seen = HashSet::from([faction.capital]);
        let mut queue = VecDeque::from([faction.capital]);
        while let Some(s) = queue.pop_front() {
            for n in state.neighbours(s) {
                if state.systems[n].owner == Some(id) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        let total = state
            .systems
            .as_slice()
            .iter()
            .filter(|s| s.owner == Some(id))
            .count();
        assert_eq!(
            seen.len(),
            total,
            "{} territory is not contiguous",
            faction.name
        );
    }
}
