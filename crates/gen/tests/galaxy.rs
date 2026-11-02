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
