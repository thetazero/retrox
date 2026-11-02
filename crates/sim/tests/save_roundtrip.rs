use retrox_sim::economy::LocalEconomy;
use retrox_sim::{
    Body, BodyKind, EconomyModel, Faction, GalaxyPos, GalaxyState, GalaxyTime, HistoryKind, Orbit,
    Orbiting, PlanetClass, Settlement, SettlementId, Site, StarClass, StarSystem, tick,
};

fn small_galaxy() -> GalaxyState {
    let mut state = GalaxyState::empty(42);
    let faction = state.factions.insert(Faction {
        name: "Union of Free Stations".into(),
        economy: EconomyModel::Planned,
        capital: SettlementId::default(),
        color: [200, 60, 50],
        founded: GalaxyTime::ZERO,
        treasury: 0.0,
    });
    state
        .history
        .record(state.time, HistoryKind::FactionFounded { faction });
    let system = |name: &str, position, star| StarSystem {
        name: name.into(),
        position,
        star,
        lanes: Vec::new(),
        bodies: Vec::new(),
        settlements: Vec::new(),
    };
    let a = state
        .systems
        .push(system("Zarya", GalaxyPos::new(0.0, 0.0, 0.0), StarClass::G));
    let b = state.systems.push(system(
        "Polaris Gate",
        GalaxyPos::new(3.0, 4.0, 0.0),
        StarClass::M,
    ));
    state.connect(a, b);

    let world = state.add_body(Body {
        system: a,
        name: "Zarya II".into(),
        kind: BodyKind::Planet(PlanetClass::Terran),
        orbit: Orbit::circular(Orbiting::Star, 1.0, 0.0),
        mass: 1.0,
        radius_km: 6371.0,
        resources: Default::default(),
        habitability: 0.9,
        regions: Vec::new(),
    });
    let capital = state.add_settlement(Settlement {
        system: a,
        site: Site::Surface(world),
        owner: faction,
        name: "Zarya City".into(),
        founded: GalaxyTime::ZERO,
        economy: LocalEconomy {
            population: 10.0,
            ..LocalEconomy::default()
        },
    });
    state.factions[faction].capital = capital;
    state.add_settlement(Settlement {
        system: b,
        site: Site::Space(Orbit::circular(Orbiting::Star, 0.3, 1.0)),
        owner: faction,
        name: "Polaris Station".into(),
        founded: GalaxyTime::ZERO,
        economy: LocalEconomy::default(),
    });
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
    assert_eq!(loaded.bodies.len(), 1);
    assert_eq!(loaded.settlements.len(), 2);
    assert_eq!(loaded.factions.len(), 1);
    assert_eq!(loaded.history.events(), state.history.events());

    let lane = &loaded.lanes.as_slice()[0];
    assert_eq!(loaded.systems[lane.b].name, "Polaris Gate");
    assert_eq!(lane.length_ly, 5.0);
    assert_eq!(loaded.neighbours(lane.a).collect::<Vec<_>>(), vec![lane.b]);
    let (_, faction) = loaded.factions.iter().next().unwrap();
    assert_eq!(faction.economy, EconomyModel::Planned);
    assert_eq!(loaded.settlements[faction.capital].name, "Zarya City");
    assert_eq!(
        loaded.largest_faction_in(lane.a),
        loaded.largest_faction_in(lane.b)
    );
}
