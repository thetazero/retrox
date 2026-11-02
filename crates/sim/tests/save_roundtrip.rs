use retrox_sim::{
    EconomyModel, Faction, GalaxyPos, GalaxyState, GalaxyTime, HistoryKind, StarClass, StarSystem,
    tick,
};

fn small_galaxy() -> GalaxyState {
    let mut state = GalaxyState::empty(42);
    let faction = state.factions.insert(Faction {
        name: "Union of Free Stations".into(),
        economy: EconomyModel::Planned,
        capital: retrox_sim::SystemId::from_index(0),
        color: [200, 60, 50],
        founded: GalaxyTime::ZERO,
    });
    state
        .history
        .record(state.time, HistoryKind::FactionFounded { faction });
    let a = state.systems.push(StarSystem {
        name: "Zarya".into(),
        position: GalaxyPos::new(0.0, 0.0, 0.0),
        star: StarClass::G,
        lanes: Vec::new(),
        resources: Default::default(),
        economy: Default::default(),
        owner: Some(faction),
    });
    let b = state.systems.push(StarSystem {
        name: "Polaris Gate".into(),
        position: GalaxyPos::new(3.0, 4.0, 0.0),
        star: StarClass::M,
        lanes: Vec::new(),
        resources: Default::default(),
        economy: Default::default(),
        owner: None,
    });
    state.connect(a, b);
    state
}

#[test]
fn ticking_advances_time() {
    let mut state = small_galaxy();
    for _ in 0..400 {
        tick(&mut state);
    }
    assert_eq!(state.time, GalaxyTime::from_days(400));
    assert_eq!(state.time.year(), 1);
}

#[test]
fn save_roundtrips() {
    let state = small_galaxy();
    let json = serde_json::to_string(&state).unwrap();
    let loaded: GalaxyState = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.seed, state.seed);
    assert_eq!(loaded.systems.len(), 2);
    assert_eq!(loaded.lanes.len(), 1);
    assert_eq!(loaded.factions.len(), 1);
    assert_eq!(loaded.history.events(), state.history.events());

    let lane = &loaded.lanes.as_slice()[0];
    assert_eq!(loaded.systems[lane.b].name, "Polaris Gate");
    assert_eq!(lane.length_ly, 5.0);
    assert_eq!(loaded.neighbours(lane.a).collect::<Vec<_>>(), vec![lane.b]);
    let owner = loaded.systems[lane.a].owner.unwrap();
    assert_eq!(loaded.factions[owner].economy, EconomyModel::Planned);
}
