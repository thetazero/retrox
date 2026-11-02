//! The whole game is deterministic (DESIGN §5.1): the same seed gives the
//! same history, however many threads run it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::tick;

fn run(seed: u64, days: u32, threads: usize) -> u64 {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let config = GalaxyConfig {
            systems: 600,
            radius_ly: 100.0,
            factions: 5,
            ..GalaxyConfig::default()
        };
        let mut state = generate(seed, &config);
        for _ in 0..days {
            tick(&mut state);
        }
        let mut hasher = DefaultHasher::new();
        serde_json::to_string(&state).unwrap().hash(&mut hasher);
        hasher.finish()
    })
}

#[test]
fn same_seed_same_history() {
    assert_eq!(run(3, 200, 4), run(3, 200, 4));
}

#[test]
fn thread_count_doesnt_change_history() {
    assert_eq!(run(4, 200, 1), run(4, 200, 6));
}

#[test]
fn different_seeds_differ() {
    assert_ne!(run(5, 30, 2), run(6, 30, 2));
}

fn small() -> retrox_sim::GalaxyState {
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

#[test]
fn every_fleet_has_its_events() {
    use retrox_sim::haul::{EventKind, Location};
    let mut state = small();
    for _ in 0..200 {
        tick(&mut state);
    }
    for (key, company) in &state.companies {
        for fleet in &company.fleets {
            let count = |kind| {
                state
                    .calendar
                    .events()
                    .filter(|e| e.company == key && e.fleet == fleet.id && e.kind == kind)
                    .count()
            };
            assert_eq!(
                count(EventKind::Review),
                1,
                "{} fleet {}",
                company.name,
                fleet.id
            );
            let travelling = matches!(fleet.at, Location::Travelling { .. });
            assert_eq!(
                count(EventKind::Arrive),
                usize::from(travelling),
                "{} fleet {}",
                company.name,
                fleet.id
            );
        }
    }
}

#[test]
#[ignore = "slotmap free lists aren't saved: #62"]
fn a_saved_game_resumes_the_same_history() {
    let mut state = small();
    for _ in 0..100 {
        tick(&mut state);
    }
    let saved = serde_json::to_string(&state).unwrap();
    let mut loaded: retrox_sim::GalaxyState = serde_json::from_str(&saved).unwrap();
    for _ in 0..100 {
        tick(&mut state);
        tick(&mut loaded);
    }
    assert_eq!(
        serde_json::to_string(&state).unwrap(),
        serde_json::to_string(&loaded).unwrap()
    );
}
