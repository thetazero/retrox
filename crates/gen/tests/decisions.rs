//! Monthly decisions pick from prepared data (DESIGN §5.1.1, issue #58),
//! and choose what a search would have.

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::colonize::{best_free_site, search_free_site};
use retrox_sim::{GalaxyTime, Settlement, SystemId};

fn config() -> GalaxyConfig {
    GalaxyConfig {
        systems: 400,
        radius_ly: 80.0,
        ..GalaxyConfig::default()
    }
}

/// Settle `site` from `origin`'s first settlement, as colonists would.
fn settle(state: &mut retrox_sim::GalaxyState, system: SystemId, site: retrox_sim::Site) {
    let owner = state.settlements.values().next().expect("settled").owner;
    state.add_settlement(Settlement {
        system,
        site,
        owner,
        name: "Test".into(),
        founded: GalaxyTime::ZERO,
        economy: Default::default(),
        stability: 1.0,
    });
}

#[test]
fn free_site_lists_match_a_search_as_sites_are_taken() {
    let mut state = generate(3, &config());
    let origins: Vec<SystemId> = state
        .settlements
        .values()
        .map(|s| s.system)
        .step_by(7)
        .take(6)
        .collect();
    assert!(!origins.is_empty());
    let mut settled = 0;
    // Take sites one after another, from origins whose lists overlap, so
    // lists run out and are searched again.
    for round in 0..60 {
        let origin = origins[round % origins.len()];
        let listed = best_free_site(&mut state, origin);
        assert_eq!(listed, search_free_site(&state, origin), "round {round}");
        let Some((system, site)) = listed else {
            continue;
        };
        settle(&mut state, system, site);
        settled += 1;
    }
    assert!(settled > 30, "only {settled} sites settled");
}
