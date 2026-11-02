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
        to_capital: None,
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

/// Lane distance from `from` to `to`, searched for.
fn lane_ly(state: &retrox_sim::GalaxyState, from: SystemId, to: SystemId) -> Option<f64> {
    let mut found = None;
    retrox_sim::routes::search(
        state,
        from,
        |lane| lane.length_ly,
        |step| {
            if step.system == to {
                found = Some(step.cost);
                return retrox_sim::routes::Visit::Stop;
            }
            retrox_sim::routes::Visit::Expand
        },
    );
    found
}

#[test]
fn settlements_know_how_far_their_capital_is() {
    let mut state = generate(3, &config());
    let mut far = 0;
    for (id, s) in state.settlements.iter() {
        let capital = state.settlements[state.factions[s.owner].capital].system;
        let stored = s.to_capital.map(|d| d.ly);
        assert_eq!(stored, lane_ly(&state, capital, s.system), "{id:?}");
        if stored.is_some_and(|ly| ly > 0.0) {
            far += 1;
        }
    }
    assert!(far > 10, "only {far} settlements away from their capital");

    // Changing hands, a settlement measures again from its new capital.
    let (a, b) = {
        let mut owners = state
            .factions
            .iter()
            .filter(|(_, f)| state.settlements.contains_key(f.capital));
        (owners.next().unwrap().0, owners.next().unwrap().0)
    };
    let city = state
        .settlements
        .iter()
        .find(|(id, s)| s.owner == a && *id != state.factions[a].capital)
        .map(|(id, _)| id)
        .expect("a city that isn't a capital");
    retrox_sim::politics::set_owner(&mut state, city, b);
    let capital = state.settlements[state.factions[b].capital].system;
    let system = state.settlements[city].system;
    assert_eq!(
        state.settlements[city].to_capital.map(|d| d.ly),
        lane_ly(&state, capital, system)
    );

    // A new capital, and every one of its faction's settlements measures
    // from there.
    retrox_sim::politics::set_capital(&mut state, b, city);
    for (_, s) in state.settlements.iter().filter(|(_, s)| s.owner == b) {
        assert_eq!(
            s.to_capital.map(|d| d.ly),
            lane_ly(&state, system, s.system)
        );
    }
    assert_eq!(state.settlements[city].to_capital.map(|d| d.ly), Some(0.0));
}
