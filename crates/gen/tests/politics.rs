use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::politics;

#[test]
fn unstable_settlements_break_away_and_remember_their_parent() {
    let mut state = generate(
        2,
        &GalaxyConfig {
            systems: 800,
            radius_ly: 110.0,
            factions: 6,
            ..GalaxyConfig::default()
        },
    );
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
        assert!(held >= 30.0, "{} is too small to stand alone", child.name);
    }
    let names: std::collections::HashSet<_> = state.factions.values().map(|f| &f.name).collect();
    assert_eq!(
        names.len(),
        state.factions.len(),
        "faction names must be unique"
    );
}
