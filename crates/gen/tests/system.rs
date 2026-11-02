use retrox_gen::{GalaxyConfig, generate, rocks, system_layout};
use retrox_sim::layout::{Place, StationKind};

#[test]
fn every_system_has_a_sane_layout() {
    let state = generate(4, &GalaxyConfig::default());
    for (id, system) in state.systems.iter() {
        let layout = system_layout(&state, id);
        assert_eq!(layout.gates.len(), system.lanes.len(), "{}", system.name);
        let facility_stations = layout
            .stations
            .iter()
            .filter(|s| matches!(s.kind, StationKind::Facility(_)))
            .count();
        assert_eq!(
            facility_stations,
            system.economy.facilities.len(),
            "{}",
            system.name
        );
        for place in layout.places() {
            let p = layout.position(place, 1.0e7);
            assert!(p.is_finite(), "{}: {place:?}", system.name);
        }
        // Nothing orbits inside the star.
        for (i, _) in layout.bodies.iter().enumerate() {
            let d = layout.position(Place::Body(i), 0.0).length();
            assert!(
                d > layout.star.radius_m,
                "{}: body inside star",
                system.name
            );
        }
    }
}

#[test]
fn layouts_are_deterministic() {
    let state = generate(4, &GalaxyConfig::default());
    let id = retrox_sim::SystemId::from_index(123);
    let (a, b) = (system_layout(&state, id), system_layout(&state, id));
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    if let Some(field) = a.fields.first() {
        assert_eq!(format!("{:?}", rocks(field)), format!("{:?}", rocks(field)));
    }
}
