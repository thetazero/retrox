//! `retrox system`: print a star system's layout.

use retrox_gen::system_layout;
use retrox_sim::layout::{AU, Parent, SECONDS_PER_DAY, StationKind};
use retrox_sim::{GalaxyState, SystemId};

pub fn print(state: &GalaxyState, query: &str) -> Result<(), String> {
    let id = find(state, query).ok_or_else(|| format!("no system matching {query:?}"))?;
    let system = &state.systems[id];
    let layout = system_layout(state, id);

    let owner = system
        .owner
        .map_or("unclaimed".to_owned(), |f| state.factions[f].name.clone());
    println!(
        "{} (#{}) — {:?} star, {owner}",
        system.name,
        id.index(),
        system.star
    );
    println!(
        "  star: {:.2} M☉, radius {:.0} km, luminosity {:.3} L☉",
        layout.star.mass_kg / retrox_sim::layout::SOLAR_MASS,
        layout.star.radius_m / 1e3,
        layout.star.luminosity
    );
    for (i, body) in layout.bodies.iter().enumerate() {
        let indent = if body.is_moon() { "      " } else { "  " };
        let (dist, unit) = match body.orbit.parent {
            Parent::Star => (body.orbit.radius_m / AU, "AU"),
            _ => (body.orbit.radius_m / 1e3, "km"),
        };
        println!(
            "{indent}{:<22} {:?}, r {:.0} km, orbit {dist:.2} {unit}, period {:.1} d{}",
            body.name,
            body.kind,
            body.radius_m / 1e3,
            body.orbit.period_s / SECONDS_PER_DAY,
            if body.landing_points.is_empty() {
                String::new()
            } else {
                let names: Vec<&str> = body
                    .landing_points
                    .iter()
                    .map(|l| l.name.as_str())
                    .collect();
                format!(" — landing: {}", names.join(", "))
            }
        );
        let _ = i;
    }
    for belt in &layout.belts {
        println!(
            "  {:<22} {:?} belt, {:.2}–{:.2} {}",
            belt.name,
            belt.kind,
            belt.inner_m / if belt.parent == Parent::Star { AU } else { 1e3 },
            belt.outer_m / if belt.parent == Parent::Star { AU } else { 1e3 },
            if belt.parent == Parent::Star {
                "AU"
            } else {
                "km"
            },
        );
    }
    for field in &layout.fields {
        println!(
            "      {:<30} radius {:.0} km, richness {:.2}, {} rocks",
            field.name,
            field.radius_m / 1e3,
            field.richness,
            retrox_gen::rocks(field).len()
        );
    }
    for station in &layout.stations {
        let kind = match station.kind {
            StationKind::Habitat => "habitat".to_owned(),
            StationKind::Outpost => "outpost".to_owned(),
            StationKind::Facility(f) => format!("{f:?}").to_lowercase(),
        };
        println!("  ◇ {:<34} {kind}, {:.0} m", station.name, station.size_m);
    }
    for gate in &layout.gates {
        println!(
            "  ⟶ {:<34} at {:.1} AU",
            gate.name,
            gate.position.length() / AU
        );
    }
    Ok(())
}

fn find(state: &GalaxyState, query: &str) -> Option<SystemId> {
    if let Ok(i) = query.parse::<u32>() {
        return ((i as usize) < state.systems.len()).then(|| SystemId::from_index(i));
    }
    let q = query.to_lowercase();
    state
        .systems
        .iter()
        .find(|(_, s)| s.name.to_lowercase() == q)
        .or_else(|| {
            state
                .systems
                .iter()
                .find(|(_, s)| s.name.to_lowercase().starts_with(&q))
        })
        .map(|(id, _)| id)
}
