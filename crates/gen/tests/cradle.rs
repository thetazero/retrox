use std::collections::HashSet;

use retrox_gen::{GalaxyConfig, generate_cradle};
use retrox_sim::{Site, tick};

fn config() -> GalaxyConfig {
    GalaxyConfig {
        systems: 800,
        radius_ly: 110.0,
        ..GalaxyConfig::default()
    }
}

#[test]
fn rival_nations_share_one_world() {
    for seed in 0..5 {
        let state = generate_cradle(seed, &config());
        let nations = state.factions.len();
        assert!((2..=4).contains(&nations), "seed {seed}: {nations} nations");
        assert_eq!(
            state.settlements.len(),
            nations,
            "only the cradle is settled"
        );

        let mut bodies = HashSet::new();
        for (id, faction) in &state.factions {
            let capital = &state.settlements[faction.capital];
            assert_eq!(capital.owner, id);
            let Site::Region(region) = capital.site else {
                panic!("seed {seed}: a nation isn't on a region");
            };
            bodies.insert(state.regions[region].body);
            assert!(capital.economy.population > 0.0);
            assert!(!capital.economy.facilities.is_empty());
        }
        assert_eq!(bodies.len(), 1, "seed {seed}: the nations share one world");
        assert!(!state.companies.is_empty(), "seed {seed}: no haulers");
    }
}

#[test]
fn the_cradle_grows() {
    let mut state = generate_cradle(1, &config());
    let start: f64 = state
        .settlements
        .values()
        .map(|s| s.economy.population)
        .sum();
    for _ in 0..360 * 15 {
        tick(&mut state);
    }
    let end: f64 = state
        .settlements
        .values()
        .map(|s| s.economy.population)
        .sum();
    assert!(end > start * 1.2, "population {start:.0} -> {end:.0}");
}
