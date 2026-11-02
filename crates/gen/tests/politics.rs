use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{HistoryKind, politics};

fn small() -> retrox_sim::GalaxyState {
    generate(
        2,
        &GalaxyConfig {
            systems: 800,
            radius_ly: 110.0,
            factions: 6,
            ..GalaxyConfig::default()
        },
    )
}

#[test]
fn unstable_settlements_break_away_and_remember_their_parent() {
    let mut state = small();
    let before = state.factions.len();
    // Everyone but the capitals turns against their rulers.
    let capitals: Vec<_> = state.factions.values().map(|f| f.capital).collect();
    for (id, s) in state.settlements.iter_mut() {
        if !capitals.contains(&id) {
            s.stability = 0.0;
        }
    }
    politics::secessions(&mut state);

    let children: Vec<_> = state
        .factions
        .iter()
        .filter(|(_, f)| f.parent.is_some())
        .collect();
    assert!(!children.is_empty(), "nobody broke away");
    assert!(state.factions.len() > before);
    for (id, child) in children {
        let parent = child.parent.unwrap();
        assert!(state.factions.contains_key(parent));
        assert_eq!(politics::lineage(&state, id), vec![id, parent]);
        let capital = &state.settlements[child.capital];
        assert_eq!(capital.owner, id, "{} doesn't hold its capital", child.name);
        let held: f64 = state
            .settlements
            .values()
            .filter(|s| s.owner == id)
            .map(|s| s.economy.population)
            .sum();
        assert!(
            held >= politics::SECESSION_POPULATION,
            "{} is too small to stand alone",
            child.name
        );
        assert!(
            !child.stem.is_empty() && child.name.contains(&child.stem),
            "{} isn't built on its stem {}",
            child.name,
            child.stem
        );
    }
    let names: std::collections::HashSet<_> = state.factions.values().map(|f| &f.name).collect();
    assert_eq!(
        names.len(),
        state.factions.len(),
        "faction names must be unique"
    );
    let recorded = state
        .history
        .events()
        .iter()
        .filter(|e| match &e.kind {
            HistoryKind::Secession {
                settlements,
                leader,
                ..
            } => !settlements.is_empty() && leader.contains(' '),
            _ => false,
        })
        .count();
    assert_eq!(recorded, state.factions.len() - before);
}

#[test]
fn loyalty_fades_with_distance_from_the_capital() {
    let state = small();
    for (_, f) in state.factions.iter() {
        assert_eq!(politics::target(&state, f.capital, Some(0.0)), 1.0);
    }
    let (id, _) = state
        .settlements
        .iter()
        .find(|(id, s)| state.factions[s.owner].capital != *id)
        .expect("a settlement that isn't a capital");
    let near = politics::target(&state, id, Some(0.0));
    let far = politics::target(&state, id, Some(3.0 * politics::DISTANCE_SCALE_LY));
    assert!(
        near > politics::UNSTABLE,
        "even nearby settlements are restless"
    );
    assert!(
        far < politics::UNSTABLE,
        "distance doesn't wear loyalty down"
    );
    assert_eq!(politics::target(&state, id, None), 0.0);
}
