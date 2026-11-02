//! #112: a planet with a city also gets a station in orbit above it, inside
//! the planet's gravity well.

use std::collections::HashMap;

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{BodyId, Orbiting, Site};

#[test]
fn most_city_planets_have_a_highport_inside_their_well() {
    let state = generate(1, &GalaxyConfig::default());
    // Each city planet, and the radius (in planet radii) of its port.
    let mut planets: HashMap<BodyId, Option<f64>> = HashMap::new();
    for s in state.settlements.values() {
        if let Site::Region(r) = s.site {
            planets.entry(state.regions[r].body).or_insert(None);
        }
    }
    let mut ports = 0;
    for s in state.settlements.values() {
        if let Site::Space(orbit) = s.site
            && let Orbiting::Body(b) = orbit.parent
            && let Some(port) = planets.get_mut(&b)
        {
            assert!(state.is_port(s));
            *port = Some(orbit.semi_major_au * 1.496e8 / state.bodies[b].radius_km);
            ports += 1;
        }
    }
    let served = planets.values().filter(|p| p.is_some()).count();
    eprintln!(
        "{} city planets, {served} with a port, {} settlements",
        planets.len(),
        state.settlements.len()
    );
    assert_eq!(ports, served, "one port per planet");
    assert!(
        served * 4 >= planets.len() * 3,
        "only {served} of {} city planets have a port",
        planets.len()
    );
    for radii in planets.values().flatten() {
        // Above the 2-radii landing range, well inside the 4-radii well.
        assert!((2.5..3.3).contains(radii), "port at {radii:.2} radii");
    }
}

#[test]
fn a_highport_belongs_to_the_city_below() {
    let state = generate(1, &GalaxyConfig::default());
    let port = state
        .settlements
        .values()
        .find(|s| state.is_port(s))
        .expect("a port");
    let Site::Space(orbit) = port.site else {
        unreachable!()
    };
    let Orbiting::Body(body) = orbit.parent else {
        unreachable!()
    };
    let city = state
        .settlements
        .values()
        .find(|s| matches!(s.site, Site::Region(r) if state.regions[r].body == body))
        .expect("a city below");
    assert_eq!(port.owner, city.owner);
    assert_eq!(port.system, city.system);
    assert!(port.name.ends_with("Highport"));
    assert!(port.economy.population > 0.0);
    assert!(port.economy.population < city.economy.population);
}
